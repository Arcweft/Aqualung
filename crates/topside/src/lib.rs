mod config;
mod hub;
mod net;
mod shutdown;
mod wire;

use std::{io, net::SocketAddr, sync::Arc};

use rustls::ServerConfig;
use thiserror::Error;
use tokio::net::TcpListener;

pub use config::{Cli, Config, ConfigError};
pub use shutdown::Shutdown;

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

pub struct Server {
    pub snorkel: SocketAddr,
    pub phone: SocketAddr,
    snorkel_listener: TcpListener,
    phone_listener: TcpListener,
    tls: Arc<ServerConfig>,
    token: Arc<Vec<u8>>,
}

impl Server {
    pub async fn bind(config: Config) -> Result<Self, BindError> {
        let snorkel_listener = TcpListener::bind(config.snorkel)
            .await
            .map_err(BindError::Snorkel)?;
        let phone_listener = TcpListener::bind(config.phone)
            .await
            .map_err(BindError::Phone)?;
        let snorkel = snorkel_listener.local_addr().map_err(BindError::Snorkel)?;
        let phone = phone_listener.local_addr().map_err(BindError::Phone)?;
        Ok(Self {
            snorkel,
            phone,
            snorkel_listener,
            phone_listener,
            tls: config.tls,
            token: config.token,
        })
    }

    pub async fn serve(self, shutdown: Shutdown) -> Result<(), RunError> {
        net::serve(self, shutdown).await
    }
}

pub async fn run(config: Config, shutdown: Shutdown) -> Result<(), RunError> {
    Server::bind(config).await?.serve(shutdown).await
}

#[derive(Debug, Error)]
pub enum BindError {
    #[error("cannot bind snorkel listener: {0}")]
    Snorkel(io::Error),
    #[error("cannot bind phone listener: {0}")]
    Phone(io::Error),
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error(transparent)]
    Bind(#[from] BindError),
    #[error("hub task panicked")]
    Hub,
}
