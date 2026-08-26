mod common;

use std::{net::TcpListener as StdListener, time::Duration};

use common::*;
use rustls::pki_types::ServerName;
use serde_json::json;
use tokio::{io::AsyncReadExt, net::TcpStream, time::timeout};
use tokio_rustls::TlsConnector;

#[test]
fn help_names_flags() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_topside"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--cert",
        "--key",
        "--ca",
        "--client-cert",
        "--token",
        "--snorkel",
        "--phone",
    ] {
        assert!(help.contains(flag), "missing {flag} in help");
    }
}

#[tokio::test]
async fn bad_token_no_101() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    assert!(
        upgrade_with_token(running.phone, "wrong").await.is_err(),
        "wrong token must fail handshake"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn initialize_without_home() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut phone = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    phone.initialize().await;
    running.shutdown().await;
}

#[tokio::test]
async fn other_cert_rejected() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut first = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    first.handshake().await;

    assert!(
        pki.client_ders_differ(),
        "fixture certs must not be identical"
    );
    let tcp = TcpStream::connect(running.snorkel).await.unwrap();
    let connect = TlsConnector::from(pki.other_client())
        .connect(ServerName::try_from("127.0.0.1").unwrap(), tcp)
        .await;
    match connect {
        Err(_) => {}
        Ok(mut stream) => {
            let mut buf = [0u8; 1];
            let io = timeout(Duration::from_secs(1), stream.read(&mut buf)).await;
            assert!(
                matches!(io, Ok(Ok(0)) | Ok(Err(_)) | Err(_)),
                "other client cert must not stay live: {io:?}"
            );
        }
    }

    first.send_json(&json!({"type": "ping"}));
    assert!(first.still_open());
    running.shutdown().await;
}

#[tokio::test]
async fn register_is_first_write() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.expect_register("aqualung-topside", "stdio").await;
    running.shutdown().await;
}

#[tokio::test]
async fn ready_false_waits() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.expect_register("aqualung-topside", "stdio").await;
    home.reply_registered(false);
    assert!(
        home.try_recv_acp(Duration::from_millis(200))
            .await
            .is_none(),
        "no ACP until leader_ready"
    );
    home.send_json(&json!({"type": "leader_ready"}));
    let id = home.expect_initialize().await;
    home.reply_initialize(id);
    running.shutdown().await;
}

#[tokio::test]
async fn host_away_holds_socket() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.handshake().await;

    let mut phone = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    phone.initialize().await;

    drop(home);
    let note = timeout(Duration::from_secs(2), phone.recv_json())
        .await
        .expect("phone must stay up and receive host_away");
    assert_eq!(note["method"], "aqualung/host_away");
    assert_eq!(note["params"]["away"], true);

    phone
        .send_text(r#"{"jsonrpc":"2.0","id":3,"method":"session/new","params":{}}"#)
        .await;
    let err = phone.recv_json().await;
    assert_eq!(err["id"], 3);
    assert!(err["error"]["message"].as_str().unwrap().contains("away"));
    running.shutdown().await;
}

#[tokio::test]
async fn replace_load_before_drop() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut first = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    first.handshake().await;

    let mut phone = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    phone.initialize().await;
    phone
        .send_text(&json_rpc(7, "session/new", json!({})))
        .await;

    let new_msg = first.recv_json().await;
    assert_eq!(new_msg["type"], "acp");
    let payload: serde_json::Value =
        serde_json::from_str(new_msg["payload"].as_str().unwrap()).unwrap();
    assert_eq!(payload["method"], "session/new");
    let home_id = payload["id"].as_u64().unwrap();
    first.reply_acp(&json!({
        "jsonrpc": "2.0",
        "id": home_id,
        "result": { "sessionId": "sess_1" },
    }));
    let created = phone.recv_json().await;
    assert_eq!(created["id"], 7);
    assert_eq!(created["result"]["sessionId"], "sess_1");

    let mut second = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    second.expect_register("aqualung-topside", "stdio").await;
    second.reply_registered(true);
    let init_id = second.expect_initialize().await;
    second.reply_initialize(init_id);
    let load_id = second.expect_session_load("sess_1").await;
    assert!(
        first.still_open(),
        "old TCP must still be open when session/load arrives"
    );
    second.reply_load_result(load_id, "sess_1");
    first.expect_closed().await;
    running.shutdown().await;
}

#[tokio::test]
async fn two_phones_same_id() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.handshake().await;

    let mut a = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    let mut b = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    a.initialize().await;
    b.initialize().await;

    a.send_text(&json_rpc(1, "session/new", json!({"cwd": "/a"})))
        .await;
    b.send_text(&json_rpc(1, "session/new", json!({"cwd": "/b"})))
        .await;

    let mut seen = Vec::new();
    for _ in 0..2 {
        let frame = home.recv_json().await;
        let payload: serde_json::Value =
            serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap();
        assert_eq!(payload["method"], "session/new");
        seen.push((
            payload["id"].as_u64().unwrap(),
            payload["params"]["cwd"].as_str().unwrap().to_owned(),
        ));
    }
    assert_ne!(seen[0].0, seen[1].0, "home ids must be unique");

    for (id, cwd) in &seen {
        let session = if cwd == "/a" { "sess_a" } else { "sess_b" };
        home.reply_acp(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "sessionId": session },
        }));
    }

    let ra = a.recv_json().await;
    let rb = b.recv_json().await;
    assert_eq!(ra["id"], 1);
    assert_eq!(rb["id"], 1);
    assert_eq!(ra["result"]["sessionId"], "sess_a");
    assert_eq!(rb["result"]["sessionId"], "sess_b");
    running.shutdown().await;
}

#[tokio::test]
async fn fanout_watchers_only() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.handshake().await;

    let mut watcher = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    let mut other = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    watcher.initialize().await;
    other.initialize().await;

    watcher
        .send_text(&json_rpc(2, "session/new", json!({})))
        .await;
    let frame = home.recv_json().await;
    let payload: serde_json::Value =
        serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap();
    home.reply_acp(&json!({
        "jsonrpc": "2.0",
        "id": payload["id"],
        "result": { "sessionId": "sess_watch" },
    }));
    let created = watcher.recv_json().await;
    assert_eq!(created["result"]["sessionId"], "sess_watch");

    home.reply_acp(&json!({
        "jsonrpc": "2.0",
        "method": "session/update",
        "params": {
            "sessionId": "sess_watch",
            "update": { "sessionUpdate": "agent_message_chunk" },
        },
    }));

    let update = watcher.recv_json().await;
    assert_eq!(update["method"], "session/update");
    assert!(
        other.try_recv(Duration::from_millis(200)).await.is_none(),
        "unrelated phone must not see session/update"
    );
    running.shutdown().await;
}

#[tokio::test]
async fn first_answer_wins() {
    let pki = Pki::generate();
    let running = spawn_topside(&pki, TOKEN).await;
    let mut home = FakeLeader::dial(running.snorkel, pki.pinned_client())
        .await
        .unwrap();
    home.handshake().await;

    let mut a = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    let mut b = upgrade_with_token(running.phone, TOKEN).await.unwrap();
    a.initialize().await;
    b.initialize().await;

    a.send_text(&json_rpc(4, "session/new", json!({}))).await;
    let frame = home.recv_json().await;
    let payload: serde_json::Value =
        serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap();
    home.reply_acp(&json!({
        "jsonrpc": "2.0",
        "id": payload["id"],
        "result": { "sessionId": "sess_perm" },
    }));
    assert_eq!(a.recv_json().await["result"]["sessionId"], "sess_perm");

    b.send_text(&json_rpc(
        5,
        "session/load",
        json!({ "sessionId": "sess_perm" }),
    ))
    .await;
    let load = home.recv_json().await;
    let load_payload: serde_json::Value =
        serde_json::from_str(load["payload"].as_str().unwrap()).unwrap();
    assert_eq!(load_payload["method"], "session/load");
    home.reply_acp(&json!({
        "jsonrpc": "2.0",
        "id": load_payload["id"],
        "result": { "sessionId": "sess_perm" },
    }));
    assert_eq!(b.recv_json().await["id"], 5);

    home.reply_acp(&json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "session/request_permission",
        "params": { "sessionId": "sess_perm", "options": [] },
    }));

    let qa = a.recv_json().await;
    let qb = b.recv_json().await;
    assert_eq!(qa["method"], "session/request_permission");
    assert_eq!(qb["method"], "session/request_permission");
    let id_a = qa["id"].clone();
    let id_b = qb["id"].clone();
    assert_ne!(id_a, id_b);

    a.send_text(
        &json!({
            "jsonrpc": "2.0",
            "id": id_a,
            "result": { "outcome": "selected", "optionId": "allow" },
        })
        .to_string(),
    )
    .await;
    b.send_text(
        &json!({
            "jsonrpc": "2.0",
            "id": id_b,
            "result": { "outcome": "selected", "optionId": "deny" },
        })
        .to_string(),
    )
    .await;

    let answered = home.recv_json().await;
    let answered_payload: serde_json::Value =
        serde_json::from_str(answered["payload"].as_str().unwrap()).unwrap();
    assert_eq!(answered_payload["id"], 99);
    assert_eq!(answered_payload["result"]["optionId"], "allow");
    assert!(
        home.try_recv_acp(Duration::from_millis(200))
            .await
            .is_none(),
        "second answer must not go home"
    );
    running.shutdown().await;
}

#[test]
fn sigint_exit_0() {
    let pki = Pki::generate();
    let snorkel = free_port();
    let phone = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_topside"))
        .args([
            "--cert",
            pki.server_cert_path.to_str().unwrap(),
            "--key",
            pki.server_key_path.to_str().unwrap(),
            "--ca",
            pki.ca_path.to_str().unwrap(),
            "--client-cert",
            pki.client_cert_path.to_str().unwrap(),
            "--token",
            TOKEN,
            "--snorkel",
            &snorkel,
            "--phone",
            &phone,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    let phone_addr: std::net::SocketAddr = phone.parse().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::net::TcpStream::connect(phone_addr).is_err() && std::time::Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        std::net::TcpStream::connect(phone_addr).is_ok(),
        "topside did not listen on {phone}"
    );

    let pid = child.id();
    let status = std::process::Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let exit = child.wait().unwrap();
    assert_eq!(exit.code(), Some(0));
    std::thread::sleep(Duration::from_millis(50));
    assert!(std::net::TcpStream::connect(phone_addr).is_err());
}

fn free_port() -> String {
    let listener = StdListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr.to_string()
}
