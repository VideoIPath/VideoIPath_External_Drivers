//! Local native composition evidence, not acceptance by a VideoIPath release.
#![cfg(target_os = "linux")]
#![allow(clippy::result_large_err)] // Tungstenite's handshake callback signature.

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rust_example::registry::validate_structure;
use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use std::{
    future::Future,
    os::unix::fs::PermissionsExt,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
    },
};

const BIN: &str = env!("CARGO_BIN_EXE_rust-example");
const TOKEN: &str = "native-demo-only-$-synthetic-bearer";
const PAYLOAD: &str = "synthetic-request-payload-never-log";
const NODE: &str = "afe3cf0d-d906-4a89-a495-5f6a989a16c1";
type Socket = WebSocketStream<TlsStream<TcpStream>>;

async fn bounded<T>(label: &str, future: impl Future<Output = T>) -> T {
    timeout(Duration::from_secs(5), future)
        .await
        .unwrap_or_else(|_| panic!("timed out: {label}"))
}

/// Own and reap the actual driver, including on assertion failure or timeout.
struct Process {
    child: Child,
    log: tempfile::NamedTempFile,
}
impl Process {
    fn spawn(command: &mut Command, directory: &std::path::Path) -> Self {
        let log = tempfile::NamedTempFile::new_in(directory).unwrap();
        let child = command
            .stdin(Stdio::null())
            .stdout(log.as_file().try_clone().unwrap())
            .stderr(log.as_file().try_clone().unwrap())
            .spawn()
            .unwrap();
        Self { child, log }
    }

    async fn terminate(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "driver exited prematurely"
        );
        // Only signal the owned PID; no shell or process-name matching.
        assert!(
            Command::new("kill")
                .args(["-TERM", "--", &self.child.id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        let status = bounded("graceful driver exit", async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(status.success(), "SIGTERM must exit successfully");
        let log = std::fs::read_to_string(self.log.path()).unwrap();
        // Never print the captured logs even when these assertions fail.
        assert!(!log.contains(TOKEN), "bearer leaked to logs");
        assert!(!log.contains(PAYLOAD), "request payload leaked to logs");
        assert!(
            !log.contains("BEGIN PRIVATE KEY"),
            "private key leaked to logs"
        );
        assert!(log.contains("VideoIPath session established"));
        assert!(
            log.contains("TX delivered"),
            "trace logging must actually be exercised"
        );
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

async fn fixture() -> (tempfile::TempDir, Command, TcpListener, TlsAcceptor) {
    let scratch = if std::path::Path::new("/tmp/opencode").is_dir() {
        "/tmp/opencode"
    } else {
        "/tmp"
    };
    let directory = tempfile::Builder::new()
        .prefix("vip-demo-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir_in(scratch)
        .unwrap();
    std::fs::create_dir(directory.path().join("locks")).unwrap();
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let ca_key = KeyPair::generate().unwrap();
    let ca = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::new(ca_params, ca_key);
    let mut leaf_params = CertificateParams::new(vec!["127.0.0.1".into()]).unwrap();
    leaf_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let key = KeyPair::generate().unwrap();
    let leaf = leaf_params.signed_by(&key, &issuer).unwrap();
    let ca_path = directory.path().join("ca.pem");
    std::fs::write(&ca_path, ca.pem()).unwrap();
    let tls =
        ServerConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(
                vec![leaf.der().clone()],
                PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
            )
            .unwrap();
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let mut command = Command::new(BIN);
    // Inherited developer credentials and configuration never reach this child.
    command
        .arg("driver")
        .env_clear()
        .env(
            "VIDEOIPATH_URL",
            format!(
                "wss://127.0.0.1:{}/x-driver/v1",
                listener.local_addr().unwrap().port()
            ),
        )
        .env("VIDEOIPATH_TOKEN", TOKEN)
        .env("VIDEOIPATH_NODE_ID", NODE)
        .env("VIDEOIPATH_CA_BUNDLE", ca_path)
        .env("DRIVER_STATE_DIRECTORY", directory.path().join("state"))
        .env("DRIVER_LOCK_DIRECTORY", directory.path().join("locks"))
        .env("DRIVER_CONFIGURATION_ORDER", "receivers_first")
        .env("DRIVER_LOG_LEVEL", "trace")
        .env("TOKIO_WORKER_THREADS", "2")
        .env("DRIVER_RECONNECT_MIN_MS", "100")
        .env("DRIVER_RECONNECT_MAX_MS", "100")
        .env("DRIVER_SHUTDOWN_TIMEOUT_MS", "3000");
    (
        directory,
        command,
        listener,
        TlsAcceptor::from(Arc::new(tls)),
    )
}

async fn accept(listener: &TcpListener, tls: &TlsAcceptor) -> Socket {
    bounded("authenticated TLS upgrade", async {
        let (tcp, _) = listener.accept().await.unwrap();
        tcp.set_nodelay(true).unwrap();
        let stream = tls
            .accept(tcp)
            .await
            .unwrap_or_else(|_| panic!("local TLS rejected"));
        accept_hdr_async(stream, |request: &Request, response: Response| {
            assert!(
                request
                    .headers()
                    .get("authorization")
                    .is_some_and(|value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes()),
                "fixture bearer required"
            );
            assert!(
                request.uri().to_string() == format!("/x-driver/v1?node_id={NODE}"),
                "expected path and stable node identity, without credential query parameters"
            );
            Ok(response)
        })
        .await
        .unwrap_or_else(|_| panic!("WebSocket upgrade failed"))
    })
    .await
}

// Callers bound the whole exchange/event pair, not each successive received frame.
async fn receive(socket: &mut Socket) -> Value {
    let frame = socket
        .next()
        .await
        .expect("driver socket closed")
        .unwrap_or_else(|_| panic!("invalid WebSocket frame"));
    assert!(frame.is_text(), "expected JSON text");
    serde_json::from_str(frame.to_text().unwrap()).expect("valid JSON frame")
}

async fn exchange(socket: &mut Socket, id: u64, kind: &str, data: Value) -> Value {
    bounded("request and response", async {
        socket
            .send(Message::Text(
                json!({"message_id":id,"message_type":kind,"data":data})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let response = receive(socket).await;
        // Never skip events: an early or duplicate causal event fails this barrier.
        assert_eq!(response["message_id"], id, "response/event ordering");
        assert_eq!(response["message_type"], kind);
        response["data"].clone()
    })
    .await
}

fn changed(values: Value) -> Value {
    json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":values}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_demo_tls_events_reconnect_and_restart() {
    timeout(Duration::from_secs(40), async {
        let (directory, mut command, listener, tls) = fixture().await;
        let mut first = Process::spawn(&mut command, directory.path());
        let mut backend = accept(&listener, &tls).await;
        let structure = exchange(&mut backend, 1, "get_structure", json!({})).await;
        validate_structure(&structure).unwrap();
        assert_eq!(structure["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(structure["configuration_order"], "receivers_first");
        let read = json!({"location":null,"ids":["level","enabled","temperature","mode","signal_present"]});
        let initial = json!({"level":50,"enabled":true,"temperature":21.0,"mode":"standby","signal_present":true});
        assert_eq!(exchange(&mut backend, 2, "get_dynamic_values", read.clone()).await, initial);
        assert_eq!(exchange(&mut backend, 3, "alarms", json!({})).await, json!([]));

        assert_eq!(exchange(&mut backend, 4, "set_dynamic_values", json!({"location":null,"values":{
            "level":62,"enabled":"not-a-boolean","temperature":22.0,"mode":PAYLOAD
        }})).await, json!({"enabled":"Invalid value type","temperature":"Read-only parameter","mode":"Out of range"}));
        assert_eq!(bounded("one causal event", receive(&mut backend)).await, changed(json!({"level":62})));
        let mut updated = initial.clone();
        updated["level"] = json!(62);
        assert_eq!(exchange(&mut backend, 5, "get_dynamic_values", read.clone()).await, updated);

        // The fixed ten-second teaching clock is independent of backend writes.
        // Strict frame consumption also catches any delayed duplicate level event.
        let alarm = json!({"id":"Demo signal absent","location":null,
            "details":"Teaching clock input; not a physical fault","severity":"warning","service_affecting":false});
        timeout(Duration::from_secs(12), async {
            assert_eq!(receive(&mut backend).await, changed(json!({"signal_present":false})));
            assert_eq!(receive(&mut backend).await,
                json!({"message_id":null,"message_type":"async_alarm","data":alarm}));
        }).await.expect("teaching value and alarm events within twelve seconds");
        assert_eq!(exchange(&mut backend, 6, "alarms", json!({})).await, json!([alarm]));
        updated["signal_present"] = json!(false);
        assert_eq!(exchange(&mut backend, 7, "get_dynamic_values", read.clone()).await, updated);

        // Force loss of the backend transport while retaining the same OS process.
        drop(backend);
        let mut backend = accept(&listener, &tls).await;
        assert_eq!(exchange(&mut backend, 8, "get_structure", json!({})).await, structure);
        assert_eq!(exchange(&mut backend, 9, "get_dynamic_values", read.clone()).await, updated);
        assert_eq!(exchange(&mut backend, 10, "alarms", json!({})).await, json!([alarm]));
        first.terminate().await;
        assert_eq!(std::fs::read_to_string(directory.path().join("state/readiness")).unwrap(),
            "device=false\nvideoipath=false\n");
        assert!(!directory.path().join("state/heartbeat").exists());
        drop(backend);

        // Reuse identity and directories, but in-memory values and alarms must reset.
        let mut restarted = Process::spawn(&mut command, directory.path());
        let mut backend = accept(&listener, &tls).await;
        assert_eq!(exchange(&mut backend, 11, "get_structure", json!({})).await, structure);
        assert_eq!(exchange(&mut backend, 12, "get_dynamic_values", read).await, initial);
        assert_eq!(exchange(&mut backend, 13, "alarms", json!({})).await, json!([]));
        restarted.terminate().await;
    }).await.expect("native demo scenario must finish within forty seconds");
}
