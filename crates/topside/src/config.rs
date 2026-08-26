use std::{
    fs::File,
    io::{self, BufReader},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use clap::Parser;
use rustls::{
    DigitallySignedStruct, DistinguishedName, SignatureScheme,
    client::danger::HandshakeSignatureValid,
    pki_types::{CertificateDer, PrivateKeyDer, UnixTime},
    server::{
        WebPkiClientVerifier,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
};
use thiserror::Error;

pub const DEFAULT_SNORKEL: &str = "0.0.0.0:1943";
pub const DEFAULT_PHONE: &str = "0.0.0.0:7678";

#[derive(Parser, Clone, Debug)]
#[command(
    version,
    about = "Accept a home snorkel and speak ACP to phones",
    after_help = "The snorkel port defaults to 1943. The phone port defaults to 7678. Exit status 0 means a signal stopped topside. Bad configuration exits 2."
)]
pub struct Cli {
    #[arg(long, env = "TOPSIDE_CERT", value_name = "PEM")]
    pub cert: PathBuf,

    #[arg(long, env = "TOPSIDE_KEY", value_name = "PEM")]
    pub key: PathBuf,

    #[arg(long, env = "TOPSIDE_CA", value_name = "PEM")]
    pub ca: PathBuf,

    #[arg(long, env = "TOPSIDE_CLIENT_CERT", value_name = "PEM")]
    pub client_cert: PathBuf,

    #[arg(long, env = "TOPSIDE_TOKEN", value_name = "TOKEN")]
    pub token: String,

    #[arg(
        long,
        env = "TOPSIDE_SNORKEL",
        value_name = "ADDR",
        default_value = DEFAULT_SNORKEL
    )]
    pub snorkel: SocketAddr,

    #[arg(
        long,
        env = "TOPSIDE_PHONE",
        value_name = "ADDR",
        default_value = DEFAULT_PHONE
    )]
    pub phone: SocketAddr,
}

pub struct Config {
    pub(crate) snorkel: SocketAddr,
    pub(crate) phone: SocketAddr,
    pub(crate) tls: Arc<rustls::ServerConfig>,
    pub(crate) token: Arc<Vec<u8>>,
}

impl Config {
    pub fn load(cli: Cli) -> Result<Self, ConfigError> {
        if cli.token.is_empty() {
            return Err(ConfigError::EmptyToken);
        }

        crate::install_crypto_provider();
        let roots = load_roots(&cli.ca)?;
        let server_certs = load_certs(&cli.cert, "--cert")?;
        let server_key = load_key(&cli.key)?;
        let pin_certs = load_certs(&cli.client_cert, "--client-cert")?;
        let leaf = pin_certs[0].clone();

        let inner = WebPkiClientVerifier::builder(roots.into())
            .build()
            .map_err(|error| ConfigError::ClientVerifier(error.to_string()))?;
        let verifier = Arc::new(PinnedClientVerifier { inner, leaf });
        let mut tls = rustls::ServerConfig::builder()
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_certs, server_key)
            .map_err(|error| ConfigError::ServerIdentity(error.to_string()))?;
        tls.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
        tls.send_tls13_tickets = 0;

        Ok(Self {
            snorkel: cli.snorkel,
            phone: cli.phone,
            tls: Arc::new(tls),
            token: Arc::new(cli.token.into_bytes()),
        })
    }
}

#[derive(Debug)]
pub(crate) struct PinnedClientVerifier {
    inner: Arc<dyn ClientCertVerifier>,
    leaf: CertificateDer<'static>,
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, rustls::Error> {
        self.inner
            .verify_client_cert(end_entity, intermediates, now)?;
        if end_entity.as_ref() != self.leaf.as_ref() {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::ApplicationVerificationFailure,
            ));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }

    fn offer_client_auth(&self) -> bool {
        self.inner.offer_client_auth()
    }

    fn client_auth_mandatory(&self) -> bool {
        self.inner.client_auth_mandatory()
    }

    fn requires_raw_public_keys(&self) -> bool {
        self.inner.requires_raw_public_keys()
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("--token must not be empty")]
    EmptyToken,
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
    #[error("invalid certificate in --ca file {0}")]
    InvalidCa(PathBuf),
    #[error("invalid client certificate verifier: {0}")]
    ClientVerifier(String),
    #[error("invalid server certificate or key: {0}")]
    ServerIdentity(String),
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

pub(crate) fn token_eq(provided: &[u8], expected: &[u8]) -> bool {
    if provided.len() != expected.len() {
        return false;
    }
    let mut diff = 0u8;
    for (a, b) in provided.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}
