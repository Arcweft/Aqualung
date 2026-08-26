use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(crate) const LEADER_PROTOCOL_VERSION: u32 = 1;
pub(crate) const MAX_FRAME: usize = 64 * 1024 * 1024;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum LeaderClient {
    Register {
        client_type: String,
        mode: String,
        capabilities: ClientCapabilities,
    },
    Acp {
        payload: String,
    },
    Ping,
}

impl LeaderClient {
    pub(crate) fn register() -> Self {
        Self::Register {
            client_type: "aqualung-topside".into(),
            mode: "stdio".into(),
            capabilities: ClientCapabilities {
                terminal: false,
                fs_read: false,
                fs_write: false,
            },
        }
    }

    pub(crate) fn acp(payload: String) -> Self {
        Self::Acp { payload }
    }
}

#[derive(Serialize)]
pub(crate) struct ClientCapabilities {
    pub terminal: bool,
    pub fs_read: bool,
    pub fs_write: bool,
}

fn ready_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum LeaderServer {
    Registered {
        #[serde(default = "ready_true")]
        ready: bool,
        leader_protocol_version: u32,
    },
    LeaderReady,
    Acp {
        payload: String,
    },
    Pong,
    Error {
        message: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Error)]
pub(crate) enum WireError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("leader frame exceeds 64MB ({0} bytes)")]
    TooLarge(usize),
}

pub(crate) async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, WireError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME {
        return Err(WireError::TooLarge(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

pub(crate) async fn write_frame<W: AsyncWrite + Unpin>(
    w: &mut W,
    json: &[u8],
) -> Result<(), WireError> {
    if json.len() > MAX_FRAME {
        return Err(WireError::TooLarge(json.len()));
    }
    w.write_all(&(json.len() as u32).to_be_bytes()).await?;
    w.write_all(json).await?;
    w.flush().await?;
    Ok(())
}

#[derive(Debug, Error)]
pub(crate) enum RpcError {
    #[error("invalid JSON-RPC text")]
    Invalid,
}

pub(crate) fn parse_rpc_text(text: &str) -> Result<Value, RpcError> {
    let mut de = serde_json::Deserializer::from_str(text);
    match Value::deserialize(&mut de) {
        Ok(value) if value.is_object() => match de.end() {
            Ok(()) => Ok(value),
            Err(_) => Err(RpcError::Invalid),
        },
        _ => Err(RpcError::Invalid),
    }
}

pub(crate) fn host_away_note(away: bool) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": "aqualung/host_away",
        "params": { "away": away },
    })
    .to_string()
}

pub(crate) fn initialize_result(id: &Value) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": 1,
            "authMethods": [],
        },
    })
    .to_string()
}

pub(crate) fn follower_initialize(id: u64) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "initialize",
        "params": {
            "protocolVersion": 1,
            "clientCapabilities": {},
        },
    })
    .to_string()
}

pub(crate) fn session_load(id: u64, session: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "session/load",
        "params": { "sessionId": session },
    })
    .to_string()
}

pub(crate) fn rpc_error(id: Value, code: i64, message: &str) -> String {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    })
    .to_string()
}

pub(crate) fn method_of(method: &str) -> &str {
    method.strip_prefix('_').unwrap_or(method)
}

pub(crate) fn is_interaction(method: &str) -> bool {
    matches!(
        method_of(method),
        "session/request_permission"
            | "x.ai/ask_user_question"
            | "x.ai/exit_plan_mode"
            | "x.ai/mcp/elicit"
    )
}

pub(crate) fn session_id_in(obj: &Value) -> Option<String> {
    obj.get("params")
        .and_then(|params| params.get("sessionId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            obj.get("result")
                .and_then(|result| result.get("sessionId"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}
