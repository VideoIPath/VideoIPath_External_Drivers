//! Native composition-root evidence: an actual driver child, real loopback TLS,
//! and an independently owned simulator reached through its Unix socket API.
//! Passing this fixture does not establish acceptance by a VideoIPath release.
#![cfg(target_os = "linux")]
#![cfg(feature = "device-lab")]
// Tungstenite fixes the handshake callback's error type; changing it is not an option here.
#![allow(clippy::result_large_err)]

use std::{
    collections::BTreeMap,
    future::Future,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use futures_util::{SinkExt, StreamExt};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use rust_example::{lab::simulator, registry::validate_structure};
use rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer};
use serde_json::{Value, json};
use tokio::{
    net::{TcpListener, TcpStream},
    task::JoinHandle,
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
use tokio_util::sync::CancellationToken;

const BIN: &str = env!("CARGO_BIN_EXE_device-lab");
const TOKEN: &str = "native-fixture-only-$-not-a-real-bearer";
const PAYLOAD_SECRET: &str = "synthetic-payload-secret-never-log";
const NODE: &str = "afe3cf0d-d906-4a89-a495-5f6a989a16c1";
const BOUND: Duration = Duration::from_secs(10);
type Socket = WebSocketStream<TlsStream<TcpStream>>;

async fn bounded<T>(label: &'static str, future: impl Future<Output = T>) -> T {
    timeout(BOUND, future)
        .await
        .unwrap_or_else(|_| panic!("timed out: {label}"))
}

/// Reap children on assertion failure too, so a failed test cannot retain a node lock.
struct Process {
    child: Child,
    log: tempfile::NamedTempFile,
}

impl Process {
    fn spawn(env: &BTreeMap<String, String>, directory: &Path) -> Self {
        // An inherited developer shell could contain real credentials or change timeouts.
        // Only the fixture's explicit environment crosses the process boundary.
        let log = tempfile::NamedTempFile::new_in(directory).unwrap();
        let child = Command::new(BIN)
            .arg("driver")
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            // Clones share one file offset, preventing stderr from overwriting stdout.
            .stdout(log.as_file().try_clone().unwrap())
            .stderr(log.as_file().try_clone().unwrap())
            .spawn()
            .expect("spawn the Cargo-built driver binary");
        Self { child, log }
    }

    async fn wait(&mut self) -> ExitStatus {
        bounded("driver exit", async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    return status;
                }
                // Poll process state rather than guessing how long startup/shutdown takes.
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    async fn terminate(&mut self) -> Duration {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "driver exited before SIGTERM"
        );
        let started = Instant::now();
        // Signal only the child we own; no process-name matching, shell, or unsafe libc call.
        let mut signal = Command::new("kill")
            .args(["-TERM", "--", &self.child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Linux kill executable");
        let signalled = bounded("SIGTERM helper exit", async {
            loop {
                if let Some(status) = signal.try_wait().unwrap() {
                    break status;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(signalled.success());
        assert!(
            self.wait().await.success(),
            "graceful shutdown must exit successfully"
        );
        started.elapsed()
    }

    fn sanitized_log(&self) -> String {
        // Never print captured output on failure. Only fixed assertions and timings escape.
        let log = std::fs::read_to_string(self.log.path()).unwrap();
        assert!(!log.contains(TOKEN), "driver log exposed bearer");
        assert!(
            !log.contains(PAYLOAD_SECRET),
            "driver log exposed request payload"
        );
        log
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if matches!(self.child.try_wait(), Ok(None)) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Fixture {
    directory: tempfile::TempDir,
    state: PathBuf,
    socket: PathBuf,
    control: PathBuf,
    env: BTreeMap<String, String>,
    listener: TcpListener,
    tls: Arc<ServerConfig>,
    simulator_stop: CancellationToken,
    simulator_task: Option<JoinHandle<Result<(), simulator::Error>>>,
}

impl Fixture {
    async fn start() -> Self {
        // Short names keep sockaddr_un below its platform path limit. No global paths or ports.
        let scratch = if Path::new("/tmp/opencode").is_dir() {
            Path::new("/tmp/opencode")
        } else {
            Path::new("/tmp")
        };
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .prefix("vip-native-")
            .tempdir_in(scratch)
            .unwrap();
        let socket = directory.path().join("d.sock");
        let control = directory.path().join("c.sock");
        let state = directory.path().join("state");
        let locks = directory.path().join("locks");
        std::fs::create_dir(&locks).unwrap();

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
        let port = listener.local_addr().unwrap().port();
        let env = [
            (
                "VIDEOIPATH_URL",
                format!("wss://127.0.0.1:{port}/x-driver/v1"),
            ),
            ("VIDEOIPATH_TOKEN", TOKEN.into()),
            ("VIDEOIPATH_NODE_ID", NODE.into()),
            ("VIDEOIPATH_CA_BUNDLE", ca_path.display().to_string()),
            ("DRIVER_SOCKET", socket.display().to_string()),
            ("DRIVER_CONTROL_SOCKET", control.display().to_string()),
            ("DRIVER_STATE_DIRECTORY", state.display().to_string()),
            ("DRIVER_LOCK_DIRECTORY", locks.display().to_string()),
            ("DRIVER_CONFIGURATION_ORDER", "receivers_first".into()),
            ("DRIVER_LOG_LEVEL", "trace".into()),
            ("DRIVER_RECONNECT_MIN_MS", "100".into()),
            ("DRIVER_RECONNECT_MAX_MS", "100".into()),
            ("DRIVER_SHUTDOWN_TIMEOUT_MS", "3000".into()),
            ("TOKIO_WORKER_THREADS", "2".into()),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value))
        .collect();

        let simulator_stop = CancellationToken::new();
        let stop = simulator_stop.clone();
        let target_path = socket.clone();
        let control_path = control.clone();
        let simulator_task =
            tokio::spawn(async move { simulator::serve(&target_path, &control_path, stop).await });
        let fixture = Self {
            directory,
            state,
            socket,
            control,
            env,
            listener,
            tls: Arc::new(tls),
            simulator_stop,
            simulator_task: Some(simulator_task),
        };
        bounded("simulator sockets ready", async {
            while !fixture.socket.exists() || !fixture.control.exists() {
                assert!(
                    !fixture.simulator_task.as_ref().unwrap().is_finished(),
                    "simulator stopped during startup"
                );
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert_eq!(
            fixture.control("status", &[]).await["configured_streams"],
            0
        );
        fixture
    }

    fn driver(&self) -> Process {
        Process::spawn(&self.env, self.directory.path())
    }

    async fn accept(&self) -> Socket {
        bounded("driver TLS and WebSocket handshake", async {
            let (tcp, _) = self.listener.accept().await.unwrap();
            // Avoid delayed-ACK effects in this local sequential workload, not production tuning.
            tcp.set_nodelay(true).unwrap();
            let stream = TlsAcceptor::from(self.tls.clone())
                .accept(tcp)
                .await
                .unwrap_or_else(|_| panic!("driver rejected local TLS fixture"));
            accept_hdr_async(stream, |request: &Request, response: Response| {
                assert!(
                    request.headers().get("authorization").is_some_and(
                        |value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes()
                    ),
                    "expected fixture bearer header"
                );
                assert!(
                    request.uri().to_string() == format!("/x-driver/v1?node_id={NODE}"),
                    "stable node identity must be the only URL query parameter"
                );
                Ok(response)
            })
            .await
            .unwrap_or_else(|_| panic!("driver WebSocket upgrade failed"))
        })
        .await
    }

    async fn control(&self, command: &str, args: &[String]) -> Value {
        bounded(
            "simulator control reply",
            simulator::control(&self.control, command, args),
        )
        .await
        .expect("local simulator command accepted")
    }

    async fn ready(&self) {
        bounded("driver readiness", async {
            loop {
                if std::fs::read_to_string(self.state.join("readiness"))
                    .ok()
                    .as_deref()
                    == Some("device=true\nvideoipath=true\n")
                    && self.state.join("heartbeat").exists()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
    }

    fn stopped(&self) {
        assert_eq!(
            std::fs::read_to_string(self.state.join("readiness")).unwrap(),
            "device=false\nvideoipath=false\n"
        );
        assert!(
            !self.state.join("heartbeat").exists(),
            "shutdown must clear liveness"
        );
    }

    async fn finish(&mut self) {
        self.simulator_stop.cancel();
        bounded("simulator shutdown", self.simulator_task.take().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert!(!self.socket.exists() && !self.control.exists());
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.simulator_stop.cancel();
        if let Some(task) = &self.simulator_task {
            task.abort();
        }
    }
}

async fn receive(socket: &mut Socket) -> Value {
    let frame = bounded("driver frame", socket.next())
        .await
        .expect("driver socket unexpectedly closed")
        .unwrap_or_else(|_| panic!("invalid driver WebSocket frame"));
    assert!(
        frame.is_text(),
        "expected JSON, not an unsolicited ping or close"
    );
    let value: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
    assert!(
        value.get("_driver_revision").is_none(),
        "private observation metadata leaked on wire"
    );
    value
}

async fn exchange(socket: &mut Socket, id: u64, kind: &str, data: Value) -> Value {
    bounded(
        "backend request write",
        socket.send(Message::Text(
            json!({"message_id":id,"message_type":kind,"data":data})
                .to_string()
                .into(),
        )),
    )
    .await
    .unwrap_or_else(|_| panic!("backend request write failed"));
    let response = receive(socket).await;
    // Do not skip events to find a matching ID: an event before a write response is a defect.
    assert_eq!(
        response["message_id"], id,
        "unexpected response/event ordering"
    );
    assert!(
        response["message_type"] == kind,
        "unexpected response type or operation error"
    );
    response["data"].clone()
}

async fn values_changed(socket: &mut Socket, values: Value) {
    assert_eq!(
        receive(socket).await,
        json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":values}})
    );
}

fn sender_configuration() -> Value {
    json!({"id":"tx-1","transceiver_type":"sender","enabled_config":{
        "main_ip_destination":{"udp_port":5004,"address":"239.1.1.1",
            "network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "format_info":{},"streaming":{}}})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn native_driver_tls_simulator_control_restart_and_sequential_smoke() {
    let mut fixture = Fixture::start().await;
    let mut first = fixture.driver();
    let mut backend = fixture.accept().await;
    fixture.ready().await;

    let structure = exchange(&mut backend, 1, "get_structure", json!({})).await;
    validate_structure(&structure).expect("driver advertises a valid local structure");
    assert_eq!(structure["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(structure["configuration_order"], "receivers_first");
    assert!(
        structure["modules"]
            .as_array()
            .is_some_and(|modules| !modules.is_empty())
    );
    let initial = exchange(
        &mut backend,
        2,
        "get_dynamic_values",
        json!({"location":null,"ids":["level","enabled","temperature","mode"]}),
    )
    .await;
    assert_eq!(
        initial,
        json!({"level":50,"enabled":true,"temperature":21.0,"mode":"standby"})
    );

    // Each mutation finishes, including its deferred change, before the next begins.
    // This is the normal causal trace, not a claim about concurrent history reconciliation.
    assert_eq!(
        exchange(
            &mut backend,
            3,
            "set_dynamic_values",
            json!({"location":null,"values":{"level":61}})
        )
        .await,
        json!({})
    );
    values_changed(&mut backend, json!({"level":61})).await;

    let partial = exchange(
        &mut backend,
        4,
        "set_dynamic_values",
        json!({"location":null,"values":{
        "level":62,"enabled":"not-a-boolean","temperature":22.0,"mode":PAYLOAD_SECRET}}),
    )
    .await;
    assert_eq!(
        partial,
        json!({"enabled":"Invalid value type","temperature":"Read-only parameter","mode":"Out of range"})
    );
    values_changed(&mut backend, json!({"level":62})).await;
    assert_eq!(
        exchange(
            &mut backend,
            5,
            "get_dynamic_values",
            json!({"location":null,"ids":["level","enabled","temperature","mode"]})
        )
        .await,
        json!({"level":62,"enabled":true,"temperature":21.0,"mode":"standby"})
    );

    // This change originates through the simulator's independent control socket, with no
    // backend write. The actual child adapter must forward it and strip private revision data.
    assert_eq!(
        fixture
            .control("set", &["null".into(), "level".into(), "73".into()])
            .await,
        json!({"changed":true})
    );
    values_changed(&mut backend, json!({"level":73})).await;
    assert_eq!(
        exchange(
            &mut backend,
            6,
            "get_dynamic_values",
            json!({"ids":["level"]})
        )
        .await,
        json!({"level":73})
    );

    let sender = sender_configuration();
    assert_eq!(
        exchange(
            &mut backend,
            7,
            "set_ipt_configurations",
            json!({"module_id":"module-1","sender_configs":[sender]})
        )
        .await,
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    let check = |repair| json!({"module_id":"module-1","config":sender,"set_again":repair});
    assert_eq!(
        exchange(&mut backend, 8, "check_ipt_configuration", check(false)).await,
        true
    );
    assert_eq!(
        fixture.control("status", &[]).await["configured_streams"],
        1
    );
    let key =
        json!({"module_id":"module-1","port_id":"tx-1","sub_index":null,"direction":"sender"});
    fixture.control("drift", &[key.to_string()]).await;
    assert_eq!(
        fixture.control("status", &[]).await["configured_streams"],
        0
    );
    assert_eq!(
        exchange(&mut backend, 9, "check_ipt_configuration", check(false)).await,
        false
    );
    assert_eq!(
        exchange(&mut backend, 10, "check_ipt_configuration", check(true)).await,
        true
    );
    assert_eq!(
        exchange(&mut backend, 11, "check_ipt_configuration", check(false)).await,
        true
    );
    assert_eq!(
        fixture.control("status", &[]).await["configured_streams"],
        1
    );

    // A second OS process tests the kernel lock, rather than merely opening a second object.
    let mut duplicate = fixture.driver();
    assert!(
        !duplicate.wait().await.success(),
        "simultaneous same-node process must be refused"
    );
    assert!(
        duplicate
            .sanitized_log()
            .contains("node ownership unavailable")
    );
    assert_eq!(
        exchange(&mut backend, 12, "keepalive", Value::Null).await,
        Value::Null
    );
    fixture.ready().await;

    let first_shutdown = first.terminate().await;
    fixture.stopped();
    let log = first.sanitized_log();
    assert!(log.contains("VideoIPath session established"));
    assert!(log.contains("TX delivered") && log.contains("message_id"));
    drop(backend);

    // Keep both identity and simulator state across a real process restart. The handshake
    // validates the same UUID again, and get_structure must preserve the same port catalog.
    let mut restarted = fixture.driver();
    let mut backend = fixture.accept().await;
    fixture.ready().await;
    assert_eq!(
        exchange(&mut backend, 1, "get_structure", json!({})).await,
        structure
    );
    assert_eq!(
        exchange(
            &mut backend,
            2,
            "get_dynamic_values",
            json!({"ids":["level"]})
        )
        .await,
        json!({"level":73})
    );
    assert_eq!(
        exchange(&mut backend, 3, "check_ipt_configuration", check(false)).await,
        true
    );

    // Fixed-size native smoke load: 500 keepalives plus 500 fresh device reads. Report the
    // measured local run, never a hard microsecond budget or a universal latency promise.
    let workload_start = Instant::now();
    let mut latencies = Vec::with_capacity(1000);
    bounded("1000 sequential requests", async {
        for index in 0..1000_u64 {
            let start = Instant::now();
            if index % 2 == 0 {
                assert_eq!(
                    exchange(&mut backend, 1000 + index, "keepalive", Value::Null).await,
                    Value::Null
                );
            } else {
                assert_eq!(
                    exchange(
                        &mut backend,
                        1000 + index,
                        "get_dynamic_values",
                        json!({"ids":["level"]})
                    )
                    .await,
                    json!({"level":73})
                );
            }
            latencies.push(start.elapsed().as_micros());
        }
    })
    .await;
    let workload_elapsed = workload_start.elapsed();
    latencies.sort_unstable();
    let restart_shutdown = restarted.terminate().await;
    fixture.stopped();
    restarted.sanitized_log();
    fixture.finish().await;
    println!(
        "local native smoke: requests=1000 keepalive=500 reads=500 elapsed_ms={} p50_us={} p95_us={} p99_us={} shutdown_ms={}/{}",
        workload_elapsed.as_millis(),
        latencies[499],
        latencies[949],
        latencies[989],
        first_shutdown.as_millis(),
        restart_shutdown.as_millis()
    );
}
