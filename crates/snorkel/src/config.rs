use std::{
    fs::File,
    io::{self, BufReader},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use rustls::{
    ClientConfig,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName},
};
use thiserror::Error;

pub(crate) const DEFAULT_PORT: u16 = 1943;

pub struct Config {
    pub(crate) socket: PathBuf,
    pub(crate) endpoint: Endpoint,
    pub(crate) tls: Arc<ClientConfig>,
    pub(crate) once: bool,
}

impl Config {
    pub fn load(
        socket: impl Into<PathBuf>,
        server: &str,
        cert: &Path,
        key: &Path,
        ca: &Path,
        once: bool,
    ) -> Result<Self, ConfigError> {
        let socket = socket.into();
        if socket.as_os_str().is_empty() {
            return Err(ConfigError::EmptySocket);
        }

        crate::install_crypto_provider();
        let endpoint = Endpoint::parse(server)?;
        let roots = load_roots(ca)?;
        let certs = load_certs(cert, "--cert")?;
        let key = load_key(key)?;
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_client_auth_cert(certs, key)
            .map_err(|error| ConfigError::ClientIdentity(error.to_string()))?;

        Ok(Self {
            socket,
            endpoint,
            tls: Arc::new(tls),
            once,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Endpoint {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) server_name: ServerName<'static>,
}

impl Endpoint {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(ConfigError::BadServer("server is empty".into()));
        }

        let (host, port) = if let Ok(addr) = value.parse::<SocketAddr>() {
            (addr.ip().to_string(), addr.port())
        } else if let Ok(ip) = value.parse::<IpAddr>() {
            (ip.to_string(), DEFAULT_PORT)
        } else if value.starts_with('[') {
            let end = value
                .find(']')
                .ok_or_else(|| ConfigError::BadServer(value.into()))?;
            let host = &value[1..end];
            let port = match value.get(end + 1..) {
                Some("") => DEFAULT_PORT,
                Some(rest) if rest.starts_with(':') => parse_port(&rest[1..], value)?,
                _ => return Err(ConfigError::BadServer(value.into())),
            };
            (host.to_owned(), port)
        } else if let Some((host, port)) = value.rsplit_once(':') {
            if host.is_empty() {
                return Err(ConfigError::BadServer(value.into()));
            }
            (host.to_owned(), parse_port(port, value)?)
        } else {
            (value.to_owned(), DEFAULT_PORT)
        };

        let server_name =
            ServerName::try_from(host.clone()).map_err(|_| ConfigError::BadServer(value.into()))?;
        Ok(Self {
            host,
            port,
            server_name,
        })
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("--socket must not be empty")]
    EmptySocket,
    #[error("invalid --server value {0:?}")]
    BadServer(String),
    #[error("invalid port in --server value {0:?}")]
    BadPort(String),
    #[error("cannot read {flag} file {path}: {source}")]
    ReadPem {
        flag: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    #[error("{flag} file {path} contains no certificates")]
    NoCertificates { flag: &'static str, path: PathBuf },
    #[error("--key file {0} contains no private key")]
    NoPrivateKey(PathBuf),
    #[error("invalid client certificate or key: {0}")]
    ClientIdentity(String),
    #[error("invalid certificate in --ca file {0}")]
    InvalidCa(PathBuf),
}

fn parse_port(port: &str, original: &str) -> Result<u16, ConfigError> {
    port.parse()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(|| ConfigError::BadPort(original.into()))
}

fn reader(path: &Path, flag: &'static str) -> Result<BufReader<File>, ConfigError> {
    File::open(path)
        .map(BufReader::new)
        .map_err(|source| ConfigError::ReadPem {
            flag,
            path: path.to_owned(),
            source,
        })
}

fn load_certs(
    path: &Path,
    flag: &'static str,
) -> Result<Vec<CertificateDer<'static>>, ConfigError> {
    let certs: Result<Vec<_>, _> = rustls_pemfile::certs(&mut reader(path, flag)?).collect();
    let certs = certs.map_err(|source| ConfigError::ReadPem {
        flag,
        path: path.to_owned(),
        source,
    })?;
    if certs.is_empty() {
        return Err(ConfigError::NoCertificates {
            flag,
            path: path.to_owned(),
        });
    }
    Ok(certs)
}

fn load_key(path: &Path) -> Result<PrivateKeyDer<'static>, ConfigError> {
    rustls_pemfile::private_key(&mut reader(path, "--key")?)
        .map_err(|source| ConfigError::ReadPem {
            flag: "--key",
            path: path.to_owned(),
            source,
        })?
        .ok_or_else(|| ConfigError::NoPrivateKey(path.to_owned()))
}

fn load_roots(path: &Path) -> Result<rustls::RootCertStore, ConfigError> {
    let mut roots = rustls::RootCertStore::empty();
    for cert in load_certs(path, "--ca")? {
        roots
            .add(cert)
            .map_err(|_| ConfigError::InvalidCa(path.to_owned()))?;
    }
    Ok(roots)
}
