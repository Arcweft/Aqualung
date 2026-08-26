use std::{io, sync::Arc, time::Duration};

use socket2::{SockRef, TcpKeepalive};
use tokio::net::{TcpStream, UnixStream};
use tokio_rustls::{TlsConnector, client::TlsStream};

use crate::config::{Config, Endpoint};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE: Duration = Duration::from_secs(30);

pub(crate) struct RemoteDialer {
    endpoint: Endpoint,
    tls: Arc<rustls::ClientConfig>,
}

impl RemoteDialer {
    pub(crate) fn new(config: &Config) -> Self {
        Self {
            endpoint: config.endpoint.clone(),
            tls: Arc::clone(&config.tls),
        }
    }

    pub(crate) async fn dial(&self) -> Result<TlsStream<TcpStream>, DialError> {
        let address = (self.endpoint.host.as_str(), self.endpoint.port);
        let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
            .await
            .map_err(|_| DialError::new("TLS server", "connection timed out"))?
            .map_err(|error| DialError::io("TLS server", error))?;

        SockRef::from(&tcp)
            .set_tcp_keepalive(&TcpKeepalive::new().with_time(KEEPALIVE))
            .map_err(|error| DialError::io("TLS server keepalive", error))?;

        tokio::time::timeout(
            CONNECT_TIMEOUT,
            TlsConnector::from(Arc::clone(&self.tls))
                .connect(self.endpoint.server_name.clone(), tcp),
        )
        .await
        .map_err(|_| DialError::new("TLS handshake", "timed out"))?
        .map_err(|error| DialError::io("TLS handshake", error))
    }
}

pub(crate) async fn connect_local(config: &Config) -> Result<UnixStream, DialError> {
    UnixStream::connect(&config.socket)
        .await
        .map_err(|error| DialError::io("unix socket", error))
}

#[derive(Debug)]
pub(crate) struct DialError {
    at: &'static str,
    detail: String,
}

impl DialError {
    fn io(at: &'static str, error: io::Error) -> Self {
        Self::new(at, error.to_string())
    }

    fn new(at: &'static str, detail: impl Into<String>) -> Self {
        Self {
            at,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for DialError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.at, self.detail)
    }
}
