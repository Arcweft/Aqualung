use std::{
    net::{IpAddr, Ipv4Addr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::{
    ClientConfig, RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName},
    server::WebPkiClientVerifier,
};
use snorkel::{Config, Shutdown};
use tempfile::TempDir;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, TlsConnector, client, server};

#[test]
fn help_names_the_contract_flags_and_default_port() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_snorkel"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in ["--socket", "--server", "--cert", "--key", "--ca", "--once"] {
        assert!(help.contains(flag), "missing {flag} in help");
    }
    assert!(help.contains("defaults to 1943"));
}

struct Pki {
    _directory: TempDir,
    ca_path: PathBuf,
    client_cert_path: PathBuf,
    client_key_path: PathBuf,
    ca_der: CertificateDer<'static>,
    server_cert_der: CertificateDer<'static>,
    server_key_der: Vec<u8>,
    client_cert_der: CertificateDer<'static>,
    client_key_der: Vec<u8>,
}

impl Pki {
    fn generate() -> Self {
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
        server_params.subject_alt_names =
            vec![SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST))];
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_cert = server_params.signed_by(&server_key, &issuer).unwrap();

        let client_key = KeyPair::generate().unwrap();
        let mut client_params = CertificateParams::new(vec!["client.test".into()]).unwrap();
        client_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();

        let ca_path = directory.path().join("ca.pem");
        let client_cert_path = directory.path().join("client.pem");
        let client_key_path = directory.path().join("client.key");
        std::fs::write(&ca_path, ca_cert.pem()).unwrap();
        std::fs::write(&client_cert_path, client_cert.pem()).unwrap();
        std::fs::write(&client_key_path, client_key.serialize_pem()).unwrap();

        Self {
            _directory: directory,
            ca_path,
            client_cert_path,
            client_key_path,
            ca_der: ca_cert.der().clone(),
            server_cert_der: server_cert.der().clone(),
            server_key_der: server_key.serialize_der(),
            client_cert_der: client_cert.der().clone(),
            client_key_der: client_key.serialize_der(),
        }
    }

    fn server_config(&self) -> Arc<ServerConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.ca_der.clone()).unwrap();
        let verifier = WebPkiClientVerifier::builder(roots.into()).build().unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.server_key_der.clone()));
        Arc::new(
            ServerConfig::builder()
                .with_client_cert_verifier(verifier)
                .with_single_cert(vec![self.server_cert_der.clone()], key)
                .unwrap(),
        )
    }

    fn client_config(&self) -> ConfigBuilder<'_> {
        ConfigBuilder { trust: self }
    }
}

struct ConfigBuilder<'a> {
    trust: &'a Pki,
}

impl ConfigBuilder<'_> {
    fn authenticated(&self, identity: &Pki) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.trust.ca_der.clone()).unwrap();
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_client_auth_cert(
                    vec![identity.client_cert_der.clone()],
                    PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(identity.client_key_der.clone())),
                )
                .unwrap(),
        )
    }

    fn anonymous(&self) -> Arc<ClientConfig> {
        let mut roots = RootCertStore::empty();
        roots.add(self.trust.ca_der.clone()).unwrap();
        Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        )
    }
}

struct Fixture {
    _socket_directory: TempDir,
    socket_path: PathBuf,
    local: UnixListener,
    remote: TcpListener,
    acceptor: TlsAcceptor,
}

impl Fixture {
    async fn new(pki: &Pki) -> Self {
        let socket_directory = tempfile::tempdir().unwrap();
        let socket_path = socket_directory.path().join("agent.sock");
        let local = UnixListener::bind(&socket_path).unwrap();
        let remote = TcpListener::bind("127.0.0.1:0").await.unwrap();
        Self {
            _socket_directory: socket_directory,
            socket_path,
            local,
            remote,
            acceptor: TlsAcceptor::from(pki.server_config()),
        }
    }

    fn config(&self, pki: &Pki, once: bool) -> Config {
        Config::load(
            &self.socket_path,
            &self.remote.local_addr().unwrap().to_string(),
            &pki.client_cert_path,
            &pki.client_key_path,
            &pki.ca_path,
            once,
        )
        .unwrap()
    }

    async fn accept_pair(&self) -> (UnixStream, server::TlsStream<TcpStream>) {
        let (tcp, _) = timeout(Duration::from_secs(2), self.remote.accept())
            .await
            .expect("timed out waiting for TLS dial")
            .unwrap();
        let remote = timeout(Duration::from_secs(2), self.acceptor.accept(tcp))
            .await
            .expect("timed out during TLS handshake")
            .unwrap();
        let (local, _) = timeout(Duration::from_secs(2), self.local.accept())
            .await
            .expect("timed out waiting for unix dial")
            .unwrap();
        (local, remote)
    }
}

#[tokio::test]
async fn copies_binary_split_writes_both_ways_and_stops_after_once() {
    let pki = Pki::generate();
    let fixture = Fixture::new(&pki).await;
    let (shutdown, _stop) = Shutdown::manual();
    let run = tokio::spawn(snorkel::run(fixture.config(&pki, true), shutdown));
    let (mut local, mut remote) = fixture.accept_pair().await;

    let to_server = [0, 0xff, 1, 2, 0x80, 3];
    local.write_all(&to_server[..2]).await.unwrap();
    local.write_all(&to_server[2..]).await.unwrap();
    let mut received = [0; 6];
    remote.read_exact(&mut received).await.unwrap();
    assert_eq!(received, to_server);

    let to_socket = [0xfe, 0, 4, 5, 6];
    remote.write_all(&to_socket[..1]).await.unwrap();
    remote.write_all(&to_socket[1..]).await.unwrap();
    let mut returned = [0; 5];
    local.read_exact(&mut returned).await.unwrap();
    assert_eq!(returned, to_socket);

    timeout(Duration::from_secs(2), remote.shutdown())
        .await
        .unwrap()
        .unwrap();
    drop(remote);
    let report = timeout(Duration::from_secs(2), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(report.sessions, 1);
    assert_eq!(local.read(&mut [0]).await.unwrap(), 0);
}

#[tokio::test]
async fn remote_eof_closes_local_and_reconnects_with_a_new_pair() {
    let pki = Pki::generate();
    let fixture = Fixture::new(&pki).await;
    let (shutdown, stop) = Shutdown::manual();
    let run = tokio::spawn(snorkel::run(fixture.config(&pki, false), shutdown));

    let (mut first_local, mut first_remote) = fixture.accept_pair().await;
    let _ = timeout(Duration::from_secs(1), first_remote.shutdown()).await;
    drop(first_remote);
    assert_eq!(
        timeout(Duration::from_secs(1), first_local.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );

    let (mut second_local, mut second_remote) =
        timeout(Duration::from_secs(2), fixture.accept_pair())
            .await
            .unwrap();
    second_remote.write_all(&[1]).await.unwrap();
    second_local.read_exact(&mut [0]).await.unwrap();
    stop();
    let report = timeout(Duration::from_secs(1), run)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(report.sessions, 2);
}

#[tokio::test]
async fn missing_socket_never_dials_tls() {
    let pki = Pki::generate();
    let directory = tempfile::tempdir().unwrap();
    let remote = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = Config::load(
        directory.path().join("missing.sock"),
        &remote.local_addr().unwrap().to_string(),
        &pki.client_cert_path,
        &pki.client_key_path,
        &pki.ca_path,
        false,
    )
    .unwrap();
    let (shutdown, stop) = Shutdown::manual();
    let run = tokio::spawn(snorkel::run(config, shutdown));

    tokio::time::sleep(Duration::from_millis(700)).await;
    stop();
    assert!(
        timeout(Duration::from_millis(100), remote.accept())
            .await
            .is_err()
    );
    assert_eq!(run.await.unwrap().unwrap().sessions, 0);
}

#[tokio::test]
async fn shutdown_mid_session_closes_both_sides() {
    let pki = Pki::generate();
    let fixture = Fixture::new(&pki).await;
    let (shutdown, stop) = Shutdown::manual();
    let run = tokio::spawn(snorkel::run(fixture.config(&pki, false), shutdown));
    let (mut local, mut remote) = fixture.accept_pair().await;

    remote.write_all(&[1]).await.unwrap();
    local.read_exact(&mut [0]).await.unwrap();
    stop();
    let local_closed = timeout(Duration::from_secs(1), local.read(&mut [0]))
        .await
        .unwrap();
    assert!(matches!(local_closed, Ok(0) | Err(_)));
    let remote_closed = timeout(Duration::from_secs(1), remote.read(&mut [0]))
        .await
        .unwrap();
    assert!(matches!(remote_closed, Ok(0) | Err(_)));
    assert_eq!(run.await.unwrap().unwrap().sessions, 1);
}

#[tokio::test]
async fn mtls_rejects_anonymous_and_wrong_ca_clients() {
    let trusted = Pki::generate();
    let wrong = Pki::generate();
    assert!(!handshake(trusted.server_config(), trusted.client_config().anonymous()).await);
    assert!(
        !handshake(
            trusted.server_config(),
            trusted.client_config().authenticated(&wrong)
        )
        .await
    );
}

async fn handshake(server: Arc<ServerConfig>, client: Arc<ClientConfig>) -> bool {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server_task = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        TlsAcceptor::from(server).accept(tcp).await.is_ok()
    });
    let tcp = TcpStream::connect(address).await.unwrap();
    let name = ServerName::try_from("127.0.0.1").unwrap();
    let client_result: Result<client::TlsStream<TcpStream>, _> =
        TlsConnector::from(client).connect(name, tcp).await;
    let server_result = server_task.await.unwrap();
    client_result.is_ok() && server_result
}
