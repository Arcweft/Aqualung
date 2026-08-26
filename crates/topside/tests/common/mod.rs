use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, watch},
    time::timeout,
};
use tokio_rustls::TlsConnector;
use tokio_tungstenite::{
    client_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use topside::{Cli, Config, Server, Shutdown};

pub const TOKEN: &str = "test-token";

pub struct Pki {
    _directory: TempDir,
    pub ca_path: PathBuf,
    pub server_cert_path: PathBuf,
    pub server_key_path: PathBuf,
    pub client_cert_path: PathBuf,
    ca_der: CertificateDer<'static>,
    client_cert_der: CertificateDer<'static>,
    client_key_der: Vec<u8>,
    other_cert_der: CertificateDer<'static>,
    other_key_der: Vec<u8>,
}

impl Pki {
    pub fn generate() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let mut ca_params = CertificateParams::default();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca_params.self_signed(&ca_key).unwrap();
        let issuer = Issuer::new(ca_params, ca_key);

        let server_key = KeyPair::generate().unwrap();
        let mut server_params = CertificateParams::new(Vec::new()).unwrap();
        server_params.subject_alt_names = vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

        let client_key = KeyPair::generate().unwrap();
        let mut client_params = CertificateParams::new(vec!["client.test".into()]).unwrap();
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();

        let other_key = KeyPair::generate().unwrap();
        let mut other_params = CertificateParams::new(vec!["other.test".into()]).unwrap();
        other_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let other_cert = other_params.signed_by(&other_key, &issuer).unwrap();

        let ca_path = directory.path().join("ca.pem");
        let server_cert_path = directory.path().join("server.pem");
        let server_key_path = directory.path().join("server.key");
        let client_cert_path = directory.path().join("client.pem");
        std::fs::write(&ca_path, ca_cert.pem()).unwrap();
        std::fs::write(&server_cert_path, server_cert.pem()).unwrap();
        std::fs::write(&server_key_path, server_key.serialize_pem()).unwrap();
        std::fs::write(&client_cert_path, client_cert.pem()).unwrap();

        Self {
            _directory: directory,
            ca_path,
            server_cert_path,
            server_key_path,
            client_cert_path,
            ca_der: ca_cert.der().clone(),
            client_cert_der: client_cert.der().clone(),
            client_key_der: client_key.serialize_der(),
            other_cert_der: other_cert.der().clone(),
            other_key_der: other_key.serialize_der(),
        }
    }

    pub fn cli(&self, token: &str) -> Cli {
        Cli {
            cert: self.server_cert_path.clone(),
            key: self.server_key_path.clone(),
            ca: self.ca_path.clone(),
            client_cert: self.client_cert_path.clone(),
            token: token.to_owned(),
            snorkel: "127.0.0.1:0".parse().unwrap(),
            phone: "127.0.0.1:0".parse().unwrap(),
        }
    }

    fn client_tls(&self, cert: CertificateDer<'static>, key: Vec<u8>) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.ca_der.clone()).unwrap();
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_client_auth_cert(
                    vec![cert],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key)),
                )
                .unwrap(),
        )
    }

    pub fn pinned_client(&self) -> Arc<ClientConfig> {
        self.client_tls(self.client_cert_der.clone(), self.client_key_der.clone())
    }

    pub fn other_client(&self) -> Arc<ClientConfig> {
        self.client_tls(self.other_cert_der.clone(), self.other_key_der.clone())
    }

    pub fn client_ders_differ(&self) -> bool {
        self.client_cert_der.as_ref() != self.other_cert_der.as_ref()
    }
}

pub struct Running {
    pub snorkel: SocketAddr,
    pub phone: SocketAddr,
    stop: Box<dyn Fn() + Send>,
    join: tokio::task::JoinHandle<Result<(), topside::RunError>>,
}

impl Running {
    pub async fn shutdown(self) {
        (self.stop)();
        let _ = timeout(Duration::from_secs(2), self.join).await;
    }
}

pub async fn spawn_topside(pki: &Pki, token: &str) -> Running {
    let config = Config::load(pki.cli(token)).unwrap();
    let server = Server::bind(config).await.unwrap();
    let snorkel = server.snorkel;
    let phone = server.phone;
    let (shutdown, stop) = Shutdown::manual();
    let join = tokio::spawn(server.serve(shutdown));
    Running {
        snorkel,
        phone,
        stop: Box::new(stop),
        join,
    }
}

pub struct FakeLeader {
    inbound: mpsc::UnboundedReceiver<Vec<u8>>,
    outbound: mpsc::UnboundedSender<Vec<u8>>,
    closed: Arc<AtomicBool>,
    stop: watch::Sender<bool>,
}

impl Drop for FakeLeader {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}

impl FakeLeader {
    pub async fn dial(addr: SocketAddr, tls: Arc<ClientConfig>) -> Result<Self, String> {
        let tcp = TcpStream::connect(addr)
            .await
            .map_err(|error| error.to_string())?;
        let stream = TlsConnector::from(tls)
            .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
            .await
            .map_err(|error| error.to_string())?;
        let (mut rd, mut wr) = tokio::io::split(stream);
        let closed = Arc::new(AtomicBool::new(false));
        let (in_tx, inbound) = mpsc::unbounded_channel();
        let (outbound, mut out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
        let (stop, mut stop_read) = watch::channel(false);
        let mut stop_write = stop.subscribe();
        let closed_read = Arc::clone(&closed);
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_read.changed() => {
                        closed_read.store(true, Ordering::SeqCst);
                        break;
                    }
                    frame = read_frame(&mut rd) => {
                        match frame {
                            Ok(frame) => {
                                if in_tx.send(frame).is_err() {
                                    break;
                                }
                            }
                            Err(_) => {
                                closed_read.store(true, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                }
            }
        });
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = stop_write.changed() => break,
                    frame = out_rx.recv() => {
                        match frame {
                            Some(frame) => {
                                if write_frame(&mut wr, &frame).await.is_err() {
                                    break;
                                }
                            }
                            None => break,
                        }
                    }
                }
            }
        });
        Ok(Self {
            inbound,
            outbound,
            closed,
            stop,
        })
    }

    pub fn still_open(&self) -> bool {
        !self.closed.load(Ordering::SeqCst)
    }

    pub async fn expect_closed(&self) {
        let start = tokio::time::Instant::now();
        while self.still_open() {
            if start.elapsed() > Duration::from_secs(2) {
                panic!("old stream still open");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    pub async fn recv_json(&mut self) -> Value {
        loop {
            let frame = timeout(Duration::from_secs(2), self.inbound.recv())
                .await
                .expect("timed out waiting for leader frame")
                .expect("leader frame channel closed");
            let value: Value = serde_json::from_slice(&frame).unwrap();
            if value["type"] == "ping" {
                self.send_json(&json!({"type": "pong"}));
                continue;
            }
            return value;
        }
    }

    pub fn send_json(&self, value: &Value) {
        self.outbound
            .send(serde_json::to_vec(value).unwrap())
            .unwrap();
    }

    pub async fn expect_register(&mut self, client_type: &str, mode: &str) {
        let value = self.recv_json().await;
        assert_eq!(value["type"], "register");
        assert_eq!(value["client_type"], client_type);
        assert_eq!(value["mode"], mode);
        assert_eq!(value["capabilities"]["terminal"], false);
        assert_eq!(value["capabilities"]["fs_read"], false);
        assert_eq!(value["capabilities"]["fs_write"], false);
        assert!(value["capabilities"].get("yolo_mode").is_none());
    }

    pub fn reply_registered(&self, ready: bool) {
        self.send_json(&json!({
            "type": "registered",
            "client_id": 1,
            "ready": ready,
            "leader_protocol_version": 1,
            "leader_binary_version": "test",
        }));
    }

    pub async fn expect_initialize(&mut self) -> u64 {
        let value = self.recv_json().await;
        assert_eq!(value["type"], "acp");
        let payload: Value = serde_json::from_str(value["payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["method"], "initialize");
        payload["id"].as_u64().unwrap()
    }

    pub fn reply_initialize(&self, id: u64) {
        self.reply_acp(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": 1,
                "agentCapabilities": {},
            },
        }));
    }

    pub async fn expect_session_load(&mut self, session: &str) -> u64 {
        let value = self.recv_json().await;
        assert_eq!(value["type"], "acp");
        let payload: Value = serde_json::from_str(value["payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["method"], "session/load");
        assert_eq!(payload["params"]["sessionId"], session);
        payload["id"].as_u64().unwrap()
    }

    pub fn reply_load_result(&self, id: u64, session: &str) {
        self.reply_acp(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "sessionId": session },
        }));
    }

    pub fn reply_acp(&self, payload: &Value) {
        self.send_json(&json!({
            "type": "acp",
            "payload": payload.to_string(),
        }));
    }

    pub async fn handshake(&mut self) {
        self.expect_register("aqualung-topside", "stdio").await;
        self.reply_registered(true);
        let id = self.expect_initialize().await;
        self.reply_initialize(id);
    }

    pub async fn try_recv_acp(&mut self, wait: Duration) -> Option<Value> {
        let start = tokio::time::Instant::now();
        loop {
            let remaining = wait.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                return None;
            }
            match timeout(remaining, self.inbound.recv()).await {
                Ok(Some(frame)) => {
                    let value: Value = serde_json::from_slice(&frame).unwrap();
                    if value["type"] == "ping" {
                        self.send_json(&json!({"type": "pong"}));
                        continue;
                    }
                    return Some(value);
                }
                _ => return None,
            }
        }
    }
}

pub struct Phone {
    sink: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    stream: futures_util::stream::SplitStream<tokio_tungstenite::WebSocketStream<TcpStream>>,
}

pub async fn upgrade_with_token(addr: SocketAddr, token: &str) -> Result<Phone, String> {
    let stream = TcpStream::connect(addr)
        .await
        .map_err(|error| error.to_string())?;
    let mut request = format!("ws://{addr}/")
        .into_client_request()
        .map_err(|error| error.to_string())?;
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {token}").parse().map_err(
            |error: tokio_tungstenite::tungstenite::http::header::InvalidHeaderValue| {
                error.to_string()
            },
        )?,
    );
    let (ws, _) = client_async(request, stream)
        .await
        .map_err(|error| error.to_string())?;
    let (sink, stream) = ws.split();
    Ok(Phone { sink, stream })
}

impl Phone {
    pub async fn send_text(&mut self, text: &str) {
        self.sink
            .send(Message::Text(text.to_owned().into()))
            .await
            .unwrap();
    }

    pub async fn recv_text(&mut self) -> String {
        loop {
            let msg = timeout(Duration::from_secs(2), self.stream.next())
                .await
                .expect("timed out waiting for phone text")
                .expect("phone socket closed")
                .expect("phone websocket error");
            match msg {
                Message::Text(text) => return text.to_string(),
                Message::Ping(payload) => {
                    let _ = self.sink.send(Message::Pong(payload)).await;
                }
                Message::Close(_) => panic!("phone socket closed"),
                _ => {}
            }
        }
    }

    pub async fn recv_json(&mut self) -> Value {
        serde_json::from_str(&self.recv_text().await).unwrap()
    }

    pub async fn initialize(&mut self) -> Value {
        self.send_text(r#"{"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}"#)
            .await;
        let reply = self.recv_json().await;
        assert_eq!(reply["id"], 0);
        assert_eq!(reply["result"]["protocolVersion"], 1);
        assert_eq!(reply["result"]["authMethods"], json!([]));
        let caps = reply["result"].get("agentCapabilities");
        assert!(caps.is_none() || caps == Some(&json!({})));
        reply
    }

    pub async fn try_recv(&mut self, wait: Duration) -> Option<Value> {
        match timeout(wait, self.stream.next()).await {
            Ok(Some(Ok(Message::Text(text)))) => serde_json::from_str(&text.to_string()).ok(),
            _ => None,
        }
    }
}

pub fn json_rpc(id: u64, method: &str, params: Value) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
    .to_string()
}

async fn read_frame<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> Result<Vec<u8>, std::io::Error> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn write_frame<W: tokio::io::AsyncWrite + Unpin>(
    w: &mut W,
    json: &[u8],
) -> Result<(), std::io::Error> {
    w.write_all(&(json.len() as u32).to_be_bytes()).await?;
    w.write_all(json).await?;
    w.flush().await?;
    Ok(())
}
