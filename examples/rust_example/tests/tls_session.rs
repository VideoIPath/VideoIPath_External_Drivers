//! Real loopback TCP/TLS/WebSocket handshakes, using ephemeral test-only certificates.
//! These fixtures verify local transport behavior, not acceptance by a VideoIPath release.

// The handshake callback's error type is fixed by Tungstenite. `err().expect()`
// deliberately avoids Debug-printing a successful transport/configuration on failure.
#![allow(clippy::result_large_err, clippy::err_expect)]

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rust_example::{config::Config, session, tls};
use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};
use tokio_rustls::TlsAcceptor;
use tokio_tungstenite::{
    Connector, accept_hdr_async, connect_async_tls_with_config,
    tungstenite::{
        Error, Message,
        handshake::server::{Request, Response},
        http::StatusCode,
    },
};

const TOKEN: &str = "synthetic-test-bearer-$-never-a-real-credential";
const NODE: &str = "afe3cf0d-d906-4a89-a495-5f6a989a16c1";
const BOUND: Duration = Duration::from_secs(5);

struct Certificates {
    // Keeping the directory alive also keeps the custom trust file available to Config.
    directory: tempfile::TempDir,
    server: Arc<ServerConfig>,
}

impl Certificates {
    fn new(host: &str) -> Self {
        let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca = ca_params.self_signed(&ca_key).unwrap();
        let issuer = Issuer::new(ca_params, ca_key);
        let mut leaf_params = CertificateParams::new(vec![host.to_owned()]).unwrap();
        leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let leaf_key = KeyPair::generate().unwrap();
        let leaf = leaf_params.signed_by(&leaf_key, &issuer).unwrap();
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("ca.pem"), ca.pem()).unwrap();
        // Explicit ring selection matches production and avoids a process-global provider race.
        let server =
            ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                .with_safe_default_protocol_versions()
                .unwrap()
                .with_no_client_auth()
                .with_single_cert(
                    vec![leaf.der().clone()],
                    PrivatePkcs8KeyDer::from(leaf_key.serialize_der()).into(),
                )
                .unwrap();
        Self {
            directory,
            server: Arc::new(server),
        }
    }

    fn config(&self, port: u16, custom_ca: bool) -> Config {
        let mut env: BTreeMap<String, String> = [
            (
                "VIDEOIPATH_URL".into(),
                format!("wss://127.0.0.1:{port}/x-driver/v1"),
            ),
            ("VIDEOIPATH_TOKEN".into(), TOKEN.into()),
            ("VIDEOIPATH_NODE_ID".into(), NODE.into()),
        ]
        .into();
        if custom_ca {
            env.insert(
                "VIDEOIPATH_CA_BUNDLE".into(),
                self.directory
                    .path()
                    .join("ca.pem")
                    .to_str()
                    .unwrap()
                    .into(),
            );
        }
        Config::parse(&env, true).unwrap()
    }
}

#[tokio::test]
async fn custom_ca_authenticates_real_tls_and_bearer_stays_in_header() {
    let certificates = Certificates::new("127.0.0.1");
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let config = certificates.config(listener.local_addr().unwrap().port(), true);
    let (observed_tx, observed_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let stream = TlsAcceptor::from(certificates.server)
            .accept(tcp)
            .await
            .unwrap();
        let mut websocket =
            accept_hdr_async(stream, move |request: &Request, response: Response| {
                // Return only booleans: test assertion failures must not dump an Authorization value.
                let correct_header = request
                    .headers()
                    .get("authorization")
                    .is_some_and(|value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes());
                let correct_uri =
                    request.uri().to_string() == format!("/x-driver/v1?node_id={NODE}");
                let _ = observed_tx.send((correct_header, correct_uri));
                Ok(response)
            })
            .await
            .unwrap();
        let frame = websocket.next().await.unwrap().unwrap();
        websocket.send(frame).await.unwrap();
    });
    let request = tls::request(&config).unwrap();
    assert!(request.headers()["authorization"].is_sensitive());
    assert!(
        !format!("{request:?}").contains(TOKEN),
        "request debug exposed bearer"
    );
    assert!(
        !request.uri().to_string().contains(TOKEN),
        "credential leaked into URL"
    );
    let (mut socket, response) = timeout(
        BOUND,
        connect_async_tls_with_config(
            request,
            None,
            false,
            Some(Connector::Rustls(tls::connector(&config).unwrap())),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(
        timeout(BOUND, observed_rx).await.unwrap().unwrap(),
        (true, true)
    );
    // Carry a real application frame after the handshake, not merely a TCP connect probe.
    let frame = Message::Text(
        r#"{"message_id":9007199254740993,"message_type":"keepalive","data":null}"#.into(),
    );
    socket.send(frame.clone()).await.unwrap();
    assert_eq!(
        timeout(BOUND, socket.next())
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        frame
    );
    timeout(BOUND, server).await.unwrap().unwrap();
}

async fn rejected_tls(host: &str, custom_ca: bool) {
    let certificates = Certificates::new(host);
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let config = certificates.config(listener.local_addr().unwrap().port(), custom_ca);
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        TlsAcceptor::from(certificates.server)
            .accept(tcp)
            .await
            .is_err()
    });
    let result = timeout(
        BOUND,
        connect_async_tls_with_config(
            tls::request(&config).unwrap(),
            None,
            false,
            Some(Connector::Rustls(tls::connector(&config).unwrap())),
        ),
    )
    .await
    .unwrap();
    let error = result
        .err()
        .expect("certificate verification must reject handshake");
    assert!(
        session::permanent(&error),
        "TLS verification must not enter transient retries"
    );
    // The server must observe rejection too; an HTTP rejection would not prove TLS verification.
    assert!(timeout(BOUND, server).await.unwrap().unwrap());
}

#[tokio::test]
async fn native_roots_reject_an_untrusted_ephemeral_ca() {
    rejected_tls("127.0.0.1", false).await;
}

#[tokio::test]
async fn trusted_ca_does_not_disable_hostname_verification() {
    rejected_tls("different-host.example.test", true).await;
}

#[tokio::test]
async fn authentication_401_over_verified_tls_is_permanent() {
    let certificates = Certificates::new("127.0.0.1");
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let config = certificates.config(listener.local_addr().unwrap().port(), true);
    let server = tokio::spawn(async move {
        let (tcp, _) = listener.accept().await.unwrap();
        let stream = TlsAcceptor::from(certificates.server)
            .accept(tcp)
            .await
            .unwrap();
        accept_hdr_async(stream, |_request: &Request, _response: Response| {
            Err(tokio_tungstenite::tungstenite::http::Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Some("synthetic rejection".into()))
                .unwrap())
        })
        .await
        .is_err()
    });
    let result = timeout(
        BOUND,
        connect_async_tls_with_config(
            tls::request(&config).unwrap(),
            None,
            false,
            Some(Connector::Rustls(tls::connector(&config).unwrap())),
        ),
    )
    .await
    .unwrap();
    let error = result.err().expect("backend refused bearer");
    assert!(
        matches!(&error, Error::Http(response) if response.status() == StatusCode::UNAUTHORIZED)
    );
    assert!(session::permanent(&error));
    assert!(timeout(BOUND, server).await.unwrap().unwrap());
}

#[test]
fn transport_loss_and_server_503_remain_transient() {
    // A permanent-auth fixture alone could pass if every error were misclassified as permanent.
    let temporary = Error::Http(Box::new(
        tokio_tungstenite::tungstenite::http::Response::builder()
            .status(StatusCode::SERVICE_UNAVAILABLE)
            .body(None)
            .unwrap(),
    ));
    assert!(!session::permanent(&temporary));
    assert!(!session::permanent(&Error::Io(
        std::io::ErrorKind::ConnectionReset.into()
    )));
}

#[test]
fn invalid_ca_material_reports_only_the_setting_name() {
    let certificates = Certificates::new("127.0.0.1");
    let config = certificates.config(443, true);
    std::fs::write(certificates.directory.path().join("ca.pem"), TOKEN).unwrap();
    let error = tls::connector(&config)
        .err()
        .expect("non-certificate file must fail")
        .to_string();
    assert!(error.contains("VIDEOIPATH_CA_BUNDLE"));
    assert!(!error.contains(TOKEN));
}
