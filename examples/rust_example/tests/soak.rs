//! Opt-in elapsed-time qualification of the actual native composition root.
//! Small TLS/process helpers intentionally mirror native_end_to_end.rs; neither
//! fixture depends on the other's tests or introduces a shared test framework.
#![cfg(target_os = "linux")]
#![cfg(feature = "device-lab")]
#![allow(clippy::result_large_err)]

use std::{
    collections::BTreeMap,
    future::Future,
    io::{BufRead, BufReader},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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
    time::timeout,
};
use tokio_rustls::{TlsAcceptor, server::TlsStream};
use tokio_tungstenite::{
    WebSocketStream, accept_hdr_async_with_config,
    tungstenite::{
        Message,
        handshake::server::{Request, Response},
        protocol::WebSocketConfig,
    },
};

const BIN: &str = env!("CARGO_BIN_EXE_device-lab");
const TOKEN: &str = "soak-only-synthetic-$-bearer-never-log";
const SECRET: &str = "soak-only-synthetic-payload-never-log";
const NODE: &str = "40965121-57a7-41b8-9e03-9f3f2900c006";
const BOUND: Duration = Duration::from_secs(10);
const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;
// Deliberately generous debug-build regression guards, not a leak detector.
const MAX_RSS_KIB: u64 = 256 * 1024;
const MAX_RSS_GROWTH_KIB: u64 = 64 * 1024;
type Socket = WebSocketStream<TlsStream<TcpStream>>;

async fn bounded<T>(label: &'static str, future: impl Future<Output = T>) -> T {
    timeout(BOUND, future)
        .await
        .unwrap_or_else(|_| panic!("timed out: {label}"))
}

/// Both driver and simulator are owned OS children, killed/reaped on unwinding.
struct Process {
    child: Child,
    log: tempfile::NamedTempFile,
}

impl Process {
    fn spawn(mode: &str, env: &BTreeMap<String, String>, directory: &Path) -> Self {
        let log = tempfile::NamedTempFile::new_in(directory).unwrap();
        let child = Command::new(BIN)
            .arg(mode)
            // Ambient real credentials and developer timeout overrides cannot cross here.
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(log.as_file().try_clone().unwrap())
            .stderr(log.as_file().try_clone().unwrap())
            .spawn()
            .expect("spawn Cargo-built fixture child");
        Self { child, log }
    }

    fn running(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_none(),
            "fixture child exited early"
        );
        assert!(
            self.log.as_file().metadata().unwrap().len() <= MAX_LOG_BYTES,
            "fixture log size guard exceeded (contents suppressed)"
        );
    }

    fn check_log(&self) {
        assert!(self.log.as_file().metadata().unwrap().len() <= MAX_LOG_BYTES);
        // Stream a bounded file, never dump captured logs or arbitrary error payloads.
        let file = std::fs::File::open(self.log.path()).unwrap();
        for line in BufReader::new(file).lines() {
            let line = line.expect("read fixture log");
            assert!(
                !line.contains(TOKEN) && !line.contains(SECRET),
                "fixture log leaked synthetic secret"
            );
            assert!(
                !line.contains("-----BEGIN"),
                "fixture log leaked certificate material"
            );
        }
    }

    async fn terminate(&mut self) {
        self.running();
        let mut signal = Command::new("kill")
            .args(["-TERM", "--", &self.child.id().to_string()])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("Linux kill executable");
        bounded("SIGTERM helper", async {
            loop {
                if let Some(status) = signal.try_wait().unwrap() {
                    assert!(status.success(), "SIGTERM failed");
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        bounded("fixture child shutdown", async {
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    assert!(status.success(), "fixture child shutdown failed");
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        self.check_log();
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
    // Drop children before their private files/sockets. No fixed ports/shared locks.
    driver: Process,
    simulator: Process,
    _directory: tempfile::TempDir,
    state: PathBuf,
    socket: PathBuf,
    control: PathBuf,
    listener: TcpListener,
    tls: Arc<ServerConfig>,
}

impl Fixture {
    async fn start() -> Self {
        let scratch = if Path::new("/tmp/opencode").is_dir() {
            "/tmp/opencode"
        } else {
            "/tmp"
        };
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .prefix("vip-soak-")
            .tempdir_in(scratch)
            .unwrap();
        let socket = directory.path().join("d.sock");
        let control = directory.path().join("c.sock");
        let state = directory.path().join("driver-state");
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
        let mut env: BTreeMap<String, String> = [
            (
                "VIDEOIPATH_URL",
                format!("wss://127.0.0.1:{port}/x-driver/v1"),
            ),
            ("VIDEOIPATH_TOKEN", TOKEN.into()),
            ("VIDEOIPATH_NODE_ID", NODE.into()),
            ("VIDEOIPATH_CA_BUNDLE", ca_path.display().to_string()),
            ("DRIVER_SOCKET", socket.display().to_string()),
            ("DRIVER_CONTROL_SOCKET", control.display().to_string()),
            (
                "DRIVER_STATE_DIRECTORY",
                directory.path().join("sim-state").display().to_string(),
            ),
            ("DRIVER_LOCK_DIRECTORY", locks.display().to_string()),
            ("DRIVER_LOG_LEVEL", "trace".into()),
            ("DRIVER_RECONNECT_MIN_MS", "100".into()),
            ("DRIVER_RECONNECT_MAX_MS", "100".into()),
            ("DRIVER_SHUTDOWN_TIMEOUT_MS", "3000".into()),
            ("TOKIO_WORKER_THREADS", "2".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect();
        let mut simulator = Process::spawn("simulator", &env, directory.path());
        bounded("simulator socket readiness", async {
            while !socket.exists() || !control.exists() {
                simulator.running();
                tokio::task::yield_now().await;
            }
        })
        .await;
        env.insert("DRIVER_STATE_DIRECTORY".into(), state.display().to_string());
        let driver = Process::spawn("driver", &env, directory.path());
        Self {
            driver,
            simulator,
            _directory: directory,
            state,
            socket,
            control,
            listener,
            tls: Arc::new(tls),
        }
    }

    async fn accept(&self) -> Socket {
        bounded("TLS/WebSocket reconnect", async {
            let (tcp, _) = self.listener.accept().await.unwrap();
            tcp.set_nodelay(true).unwrap();
            let stream = TlsAcceptor::from(self.tls.clone())
                .accept(tcp)
                .await
                .unwrap_or_else(|_| panic!("local TLS handshake failed"));
            accept_hdr_async_with_config(
                stream,
                |request: &Request, response: Response| {
                    assert!(
                        request
                            .headers()
                            .get("authorization")
                            .is_some_and(
                                |value| value.as_bytes() == format!("Bearer {TOKEN}").as_bytes()
                            ),
                        "fixture auth mismatch"
                    );
                    assert!(
                        request.uri().to_string() == format!("/x-driver/v1?node_id={NODE}"),
                        "fixture identity mismatch"
                    );
                    Ok(response)
                },
                Some(
                    WebSocketConfig::default()
                        .max_message_size(Some(64 * 1024))
                        .max_frame_size(Some(64 * 1024)),
                ),
            )
            .await
            .unwrap_or_else(|_| panic!("local WebSocket upgrade failed"))
        })
        .await
    }

    async fn ready(&mut self) {
        bounded("driver readiness", async {
            loop {
                self.driver.running();
                self.simulator.running();
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

    async fn control(&self, stats: &mut Stats, command: &str, args: &[String]) -> Value {
        stats.controls += 1;
        bounded(
            "simulator control",
            simulator::control(&self.control, command, args),
        )
        .await
        .unwrap_or_else(|_| panic!("simulator control failed (details suppressed)"))
    }
}

/// Fixed power-of-two microsecond buckets; memory does not grow with elapsed time.
#[derive(Default)]
struct Histogram {
    buckets: [u64; 25],
    count: u64,
}
impl Histogram {
    fn record(&mut self, elapsed: Duration) {
        // Round up before bucketing: truncation could report 1024 µs as an
        // "upper bound" for an observation of 1024 µs + 1 ns.
        let micros = elapsed.as_nanos().div_ceil(1000).max(1);
        assert!(micros <= (1_u128 << 24), "latency exceeds histogram range");
        let bucket = self
            .buckets
            .iter()
            .enumerate()
            .position(|(i, _)| micros <= (1_u128 << i))
            .unwrap_or(24);
        self.buckets[bucket] += 1;
        self.count += 1;
    }
    fn percentile_upper_us(&self, percentile: u64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = (self.count * percentile).div_ceil(100);
        let mut count = 0;
        for (i, bucket) in self.buckets.iter().enumerate() {
            count += bucket;
            if count >= rank {
                return 1 << i;
            }
        }
        unreachable!("histogram rank");
    }
}

#[test]
fn latency_histogram_ceil_preserves_reported_upper_bounds() {
    for (nanos, expected) in [
        (0, 1),
        (1, 1),
        (1000, 1),
        (1001, 2),
        (1_024_000, 1024),
        (1_024_001, 2048),
    ] {
        let mut histogram = Histogram::default();
        histogram.record(Duration::from_nanos(nanos));
        for percentile in [50, 95, 99, 100] {
            assert_eq!(histogram.percentile_upper_us(percentile), expected);
        }
    }
}

#[derive(Default)]
struct Stats {
    cycles: u64,
    requests: u64,
    responses: u64,
    error_responses: u64,
    expected_item_errors: u64,
    controls: u64,
    events: u64,
    backend_reconnects: u64,
    target_recoveries: u64,
    latency: BTreeMap<&'static str, Histogram>,
    recovery: Histogram,
    rss_start: u64,
    rss_end: u64,
    rss_sample_max: u64,
    rss_highwater: u64,
    elapsed: Duration,
    completed: bool,
}

impl Stats {
    fn memory(&mut self, process: &Process) {
        let status =
            std::fs::read_to_string(format!("/proc/{}/status", process.child.id())).unwrap();
        let field = |name: &str| -> u64 {
            status
                .lines()
                .find_map(|line| line.strip_prefix(name))
                .and_then(|tail| tail.split_whitespace().next())
                .and_then(|number| number.parse().ok())
                .expect("Linux RSS field available")
        };
        let rss = field("VmRSS:");
        if self.rss_start == 0 {
            self.rss_start = rss;
        }
        self.rss_end = rss;
        self.rss_sample_max = self.rss_sample_max.max(rss);
        self.rss_highwater = self.rss_highwater.max(field("VmHWM:"));
        assert!(
            self.rss_highwater <= MAX_RSS_KIB,
            "driver RSS absolute guard exceeded"
        );
        assert!(
            self.rss_highwater.saturating_sub(self.rss_start) <= MAX_RSS_GROWTH_KIB,
            "driver RSS growth guard exceeded"
        );
    }
}

impl Drop for Stats {
    fn drop(&mut self) {
        // Fixed labels/counts only, including on assertion/timeout unwind. One failed
        // run is one test_failure, not an invented count of all downstream errors.
        println!(
            "local soak: completed={} workload_ms={} cycles={} requests={} responses={} error_responses={} expected_item_errors={} test_failures={} controls={} events={} backend_reconnects={} target_recoveries={} rss_start_kib={} rss_end_kib={} rss_sample_max_kib={} rss_hwm_kib={}",
            self.completed,
            self.elapsed.as_millis(),
            self.cycles,
            self.requests,
            self.responses,
            self.error_responses,
            self.expected_item_errors,
            u64::from(!self.completed),
            self.controls,
            self.events,
            self.backend_reconnects,
            self.target_recoveries,
            self.rss_start,
            self.rss_end,
            self.rss_sample_max,
            self.rss_highwater
        );
        for (kind, histogram) in &self.latency {
            println!(
                "local soak latency: operation={kind} count={} p50_upper_us={} p95_upper_us={} p99_upper_us={}",
                histogram.count,
                histogram.percentile_upper_us(50),
                histogram.percentile_upper_us(95),
                histogram.percentile_upper_us(99)
            );
        }
        println!(
            "local soak recovery: count={} p50_upper_us={} p95_upper_us={} p99_upper_us={}",
            self.recovery.count,
            self.recovery.percentile_upper_us(50),
            self.recovery.percentile_upper_us(95),
            self.recovery.percentile_upper_us(99)
        );
    }
}

async fn receive(socket: &mut Socket) -> Value {
    let frame = bounded("driver frame", socket.next())
        .await
        .expect("driver unexpectedly closed")
        .unwrap_or_else(|_| panic!("invalid driver frame"));
    assert!(frame.is_text(), "unexpected non-JSON driver frame");
    let value: Value = serde_json::from_str(frame.to_text().unwrap())
        .unwrap_or_else(|_| panic!("invalid driver JSON"));
    assert!(
        value.get("_driver_revision").is_none(),
        "private metadata on wire"
    );
    value
}

async fn exchange(
    socket: &mut Socket,
    stats: &mut Stats,
    kind: &'static str,
    data: Value,
) -> Value {
    stats.requests += 1;
    let id = stats.requests;
    let start = Instant::now();
    // One deadline includes sending AND receiving; causal events are never skipped
    // to find the response, so missing, duplicate, or reordered output fails loudly.
    let response = bounded("backend exchange", async {
        socket
            .send(Message::Text(
                json!({"message_id":id,"message_type":kind,"data":data})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap_or_else(|_| panic!("backend send failed"));
        receive(socket).await
    })
    .await;
    stats.responses += 1;
    if response["message_type"] == "error" {
        stats.error_responses += 1;
    }
    assert!(
        response["message_id"] == id && response["message_type"] == kind,
        "response correlation/type/order mismatch (payload suppressed)"
    );
    stats
        .latency
        .entry(kind)
        .or_default()
        .record(start.elapsed());
    response["data"].clone()
}

async fn event(socket: &mut Socket, stats: &mut Stats, kind: &str, data: Value) {
    let actual = receive(socket).await;
    stats.events += 1;
    assert!(
        actual == json!({"message_id":null,"message_type":kind,"data":data}),
        "async event mismatch (payload suppressed)"
    );
}

fn sender() -> Value {
    json!({"id":"tx-1","transceiver_type":"sender","enabled_config":{
        "main_ip_destination":{"udp_port":5004,"address":"239.1.1.1",
            "network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "format_info":{},"streaming":{}}})
}

async fn readback(socket: &mut Socket, stats: &mut Stats, level: u64) {
    assert!(
        exchange(
            socket,
            stats,
            "get_dynamic_values",
            json!({"ids":["level"]})
        )
        .await
            == json!({"level":level}),
        "fresh readback mismatch"
    );
}

async fn cycle(fixture: &mut Fixture, socket: &mut Socket, stats: &mut Stats) -> u64 {
    // Distinct values ensure both mutations emit an event on every cycle.
    let level = 10 + stats.cycles % 20;
    assert!(
        exchange(socket, stats, "keepalive", Value::Null)
            .await
            .is_null()
    );
    let result = exchange(
        socket,
        stats,
        "set_dynamic_values",
        json!({"values":{"level":level,"mode":SECRET}}),
    )
    .await;
    assert!(
        result == json!({"mode":"Out of range"}),
        "expected isolated invalid-mode rejection"
    );
    stats.expected_item_errors += 1;
    event(
        socket,
        stats,
        "async_values_changed",
        json!({"location":null,"values":{"level":level}}),
    )
    .await;
    readback(socket, stats, level).await;
    let external = level + 40;
    assert!(
        fixture
            .control(
                stats,
                "set",
                &["null".into(), "level".into(), external.to_string()]
            )
            .await
            == json!({"changed":true})
    );
    event(
        socket,
        stats,
        "async_values_changed",
        json!({"location":null,"values":{"level":external}}),
    )
    .await;
    readback(socket, stats, external).await;

    assert!(exchange(socket, stats, "alarms", Value::Null).await == json!([]));
    let alarm = json!({"id":"soak-fault","location":null,"details":"Synthetic local fault","severity":"major","service_affecting":true});
    fixture.control(stats, "raise", &[alarm.to_string()]).await;
    event(socket, stats, "async_alarm", alarm.clone()).await;
    assert!(exchange(socket, stats, "alarms", Value::Null).await == json!([alarm]));
    fixture
        .control(stats, "clear", &["null".into(), "soak-fault".into()])
        .await;
    event(
        socket,
        stats,
        "async_alarm_cleared",
        json!({"id":"soak-fault","location":null}),
    )
    .await;
    fixture.control(stats, "poll-alarms", &[]).await;
    event(socket, stats, "async_poll_alarms", Value::Null).await;
    assert!(exchange(socket, stats, "alarms", Value::Null).await == json!([]));

    assert!(
        exchange(
            socket,
            stats,
            "set_ipt_configurations",
            json!({"module_id":"module-1","sender_configs":[sender()]})
        )
        .await
            == json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    let check = |repair| json!({"module_id":"module-1","config":sender(),"set_again":repair});
    assert!(exchange(socket, stats, "check_ipt_configuration", check(false)).await == true);
    let key =
        json!({"module_id":"module-1","port_id":"tx-1","sub_index":null,"direction":"sender"});
    fixture.control(stats, "drift", &[key.to_string()]).await;
    assert!(exchange(socket, stats, "check_ipt_configuration", check(false)).await == false);
    assert!(exchange(socket, stats, "check_ipt_configuration", check(true)).await == true);
    assert!(exchange(socket, stats, "check_ipt_configuration", check(false)).await == true);
    assert!(fixture.control(stats, "status", &[]).await["configured_streams"] == 1);
    external
}

async fn recover(
    fixture: &mut Fixture,
    socket: Socket,
    stats: &mut Stats,
    level: u64,
    target: bool,
) -> Socket {
    let start = Instant::now();
    if target {
        fixture.control(stats, "disconnect", &[]).await;
        let mut old = socket;
        // Observe loss of the old backend generation before accepting a new one.
        let closed = bounded("backend closes on target disconnect", old.next()).await;
        assert!(
            matches!(closed, None | Some(Err(_)) | Some(Ok(Message::Close(_)))),
            "old backend emitted data after target disconnect"
        );
        drop(old);
    } else {
        // Deliberate abrupt TLS/TCP loss, not a driver process restart.
        drop(socket);
    }
    let mut next = fixture.accept().await;
    fixture.ready().await;
    readback(&mut next, stats, level).await;
    assert!(exchange(&mut next, stats, "alarms", Value::Null).await == json!([]));
    assert!(
        exchange(
            &mut next,
            stats,
            "check_ipt_configuration",
            json!({"module_id":"module-1","config":sender(),"set_again":false})
        )
        .await
            == true
    );
    if target {
        stats.target_recoveries += 1;
    } else {
        stats.backend_reconnects += 1;
    }
    stats.recovery.record(start.elapsed());
    stats.memory(&fixture.driver);
    next
}

async fn run(duration: Option<Duration>) {
    let mut stats = Stats::default();
    let mut fixture = Fixture::start().await;
    let mut socket = fixture.accept().await;
    fixture.ready().await;
    let structure = exchange(&mut socket, &mut stats, "get_structure", json!({})).await;
    assert!(
        validate_structure(&structure).is_ok(),
        "invalid advertised structure"
    );
    stats.memory(&fixture.driver);
    let start = Instant::now();
    // Monotonic pacing is workload scheduling, never a guessed readiness sleep.
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        if let Some(duration) = duration {
            tick.tick().await;
            if start.elapsed() >= duration {
                break;
            }
        } else if stats.cycles == 2 {
            break;
        }
        let level = timeout(
            Duration::from_secs(40),
            cycle(&mut fixture, &mut socket, &mut stats),
        )
        .await
        .expect("cycle deadline exceeded");
        if duration.is_none() || stats.cycles.is_multiple_of(20) {
            socket = bounded(
                "backend recovery",
                recover(&mut fixture, socket, &mut stats, level, false),
            )
            .await;
            socket = bounded(
                "target recovery",
                recover(&mut fixture, socket, &mut stats, level, true),
            )
            .await;
        }
        stats.cycles += 1;
        stats.memory(&fixture.driver);
        fixture.driver.running();
        fixture.simulator.running();
        stats.elapsed = start.elapsed();
    }
    stats.elapsed = start.elapsed();
    assert!(stats.cycles >= 2 && stats.backend_reconnects > 0 && stats.target_recoveries > 0);
    stats.memory(&fixture.driver);
    fixture.driver.terminate().await;
    assert!(
        std::fs::read_to_string(fixture.state.join("readiness")).unwrap()
            == "device=false\nvideoipath=false\n"
    );
    assert!(!fixture.state.join("heartbeat").exists());
    drop(socket);
    fixture.simulator.terminate().await;
    assert!(!fixture.socket.exists() && !fixture.control.exists());
    stats.completed = true;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn soak_two_cycle_regression() {
    timeout(Duration::from_secs(90), run(None))
        .await
        .expect("two-cycle deadline exceeded");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "opt-in elapsed-time qualification: sh scripts/soak.sh"]
async fn elapsed_soak() {
    let seconds = std::env::var("SOAK_SECONDS")
        .unwrap_or_else(|_| "60".into())
        .parse::<u64>()
        .expect("SOAK_SECONDS must be an integer");
    assert!(
        (2..=86_400).contains(&seconds),
        "SOAK_SECONDS must be 2..=86400"
    );
    timeout(
        Duration::from_secs(seconds + 90),
        run(Some(Duration::from_secs(seconds))),
    )
    .await
    .expect("elapsed soak total deadline exceeded");
}
