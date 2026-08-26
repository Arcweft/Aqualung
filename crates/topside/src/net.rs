use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use rustls::ServerConfig;
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    time::MissedTickBehavior,
};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::{
    accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{ErrorResponse, Request, Response},
    },
};

use crate::{
    RunError, Server, Shutdown,
    config::token_eq,
    hub::{Hub, ToHub},
    wire::{self, LeaderClient, LeaderServer},
};

pub(crate) async fn serve(server: Server, mut shutdown: Shutdown) -> Result<(), RunError> {
    let (tx, rx) = mpsc::unbounded_channel();
    let hub = tokio::spawn(Hub::run(Hub::new(), rx));

    let snorkel = tokio::spawn(snorkel_accept(
        server.snorkel_listener,
        server.tls,
        tx.clone(),
        shutdown.clone(),
    ));
    let phone = tokio::spawn(phone_accept(
        server.phone_listener,
        server.token,
        tx.clone(),
        shutdown.clone(),
    ));

    shutdown.wait().await;
    snorkel.abort();
    phone.abort();
    let _ = tx.send(ToHub::Shutdown);
    hub.await.map_err(|_| RunError::Hub)?;
    Ok(())
}

async fn snorkel_accept(
    listener: TcpListener,
    tls: Arc<ServerConfig>,
    to_hub: mpsc::UnboundedSender<ToHub>,
    mut shutdown: Shutdown,
) {
    let acceptor = TlsAcceptor::from(tls);
    loop {
        tokio::select! {
            _ = shutdown.wait() => break,
            accepted = listener.accept() => {
                let Ok((tcp, _)) = accepted else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let to_hub = to_hub.clone();
                tokio::spawn(async move {
                    home_connection(tcp, acceptor, to_hub).await;
                });
            }
        }
    }
}

async fn home_connection(
    tcp: TcpStream,
    acceptor: TlsAcceptor,
    to_hub: mpsc::UnboundedSender<ToHub>,
) {
    let Ok(tls) = acceptor.accept(tcp).await else {
        return;
    };
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let (halt_tx, mut halt_rx) = oneshot::channel();
    let (bind_tx, bind_rx) = oneshot::channel();
    if to_hub
        .send(ToHub::Accepted {
            out: out_tx,
            halt: halt_tx,
            bind: bind_tx,
        })
        .is_err()
    {
        return;
    }
    let Ok(home_gen) = bind_rx.await else {
        return;
    };
    let mut tls = tls;
    let Ok(register) = serde_json::to_vec(&LeaderClient::register()) else {
        return;
    };
    if wire::write_frame(&mut tls, &register).await.is_err() {
        let _ = to_hub.send(ToHub::HomeEof { home_gen });
        return;
    }
    let (mut rd, mut wr) = tokio::io::split(tls);
    let mut ping = tokio::time::interval(Duration::from_secs(30));
    ping.set_missed_tick_behavior(MissedTickBehavior::Delay);
    ping.tick().await;
    loop {
        tokio::select! {
            frame = wire::read_frame(&mut rd) => {
                match frame {
                    Ok(bytes) => {
                        let msg = serde_json::from_slice::<LeaderServer>(&bytes)
                            .unwrap_or(LeaderServer::Unknown);
                        if to_hub.send(ToHub::HomeFrame { home_gen, msg }).is_err() {
                            break;
                        }
                    }
                    Err(_) => {
                        let _ = to_hub.send(ToHub::HomeEof { home_gen });
                        break;
                    }
                }
            }
            cmd = out_rx.recv() => {
                match cmd {
                    Some(msg) => {
                        let Ok(json) = serde_json::to_vec(&msg) else {
                            continue;
                        };
                        if wire::write_frame(&mut wr, &json).await.is_err() {
                            let _ = to_hub.send(ToHub::HomeEof { home_gen });
                            break;
                        }
                    }
                    None => break,
                }
            }
            _ = ping.tick() => {
                let Ok(json) = serde_json::to_vec(&LeaderClient::Ping) else {
                    continue;
                };
                if wire::write_frame(&mut wr, &json).await.is_err() {
                    let _ = to_hub.send(ToHub::HomeEof { home_gen });
                    break;
                }
            }
            _ = &mut halt_rx => break,
        }
    }
}

async fn phone_accept(
    listener: TcpListener,
    token: Arc<Vec<u8>>,
    to_hub: mpsc::UnboundedSender<ToHub>,
    mut shutdown: Shutdown,
) {
    loop {
        tokio::select! {
            _ = shutdown.wait() => break,
            accepted = listener.accept() => {
                let Ok((tcp, _)) = accepted else {
                    continue;
                };
                let token = Arc::clone(&token);
                let to_hub = to_hub.clone();
                tokio::spawn(async move {
                    phone_connection(tcp, token, to_hub).await;
                });
            }
        }
    }
}

async fn phone_connection(
    tcp: TcpStream,
    token: Arc<Vec<u8>>,
    to_hub: mpsc::UnboundedSender<ToHub>,
) {
    let expected = token;
    let callback = move |req: &Request, response: Response| -> Result<Response, ErrorResponse> {
        let header = req
            .headers()
            .get("Authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        match bearer_bytes(header) {
            Some(provided) if token_eq(provided, expected.as_slice()) => Ok(response),
            _ => Err(unauthorized()),
        }
    };
    let Ok(ws) = accept_hdr_async(tcp, callback).await else {
        return;
    };
    let (mut sink, mut stream) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let (bind_tx, bind_rx) = oneshot::channel();
    if to_hub
        .send(ToHub::PhoneHello {
            out: out_tx,
            bind: bind_tx,
        })
        .is_err()
    {
        return;
    }
    let Ok(phone) = bind_rx.await else {
        return;
    };
    loop {
        tokio::select! {
            incoming = stream.next() => {
                match incoming {
                    Some(Ok(Message::Text(text))) => {
                        if to_hub
                            .send(ToHub::PhoneText {
                                phone,
                                text: text.to_string(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Some(Ok(Message::Ping(payload))) => {
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Some(Ok(Message::Pong(_))) | Some(Ok(Message::Frame(_))) => {}
                    Some(Ok(Message::Binary(_))) => {}
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                        let _ = to_hub.send(ToHub::PhoneEof { phone });
                        break;
                    }
                }
            }
            outgoing = out_rx.recv() => {
                match outgoing {
                    Some(text) => {
                        if sink.send(Message::Text(text.into())).await.is_err() {
                            let _ = to_hub.send(ToHub::PhoneEof { phone });
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }
}

fn bearer_bytes(header: &str) -> Option<&[u8]> {
    let header = header.trim();
    let (scheme, rest) = header.split_once(' ')?;
    if scheme.eq_ignore_ascii_case("Bearer") {
        Some(rest.trim().as_bytes())
    } else {
        None
    }
}

fn unauthorized() -> ErrorResponse {
    Response::builder()
        .status(tokio_tungstenite::tungstenite::http::StatusCode::UNAUTHORIZED)
        .body(None)
        .expect("static unauthorized response")
}
