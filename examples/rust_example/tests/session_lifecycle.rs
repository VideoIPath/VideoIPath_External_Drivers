//! Device-free socket-generation tests: real WebSocket framing over bounded memory sockets.
//! Paused Tokio time and explicit barriers make deadlines/cancellation reproducible locally.

use futures_util::{FutureExt, SinkExt, StreamExt};
use rust_example::session::{self, Completion, Handler, Limits};
use serde_json::{Value, json};
use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::DuplexStream,
    sync::{Notify, mpsc, oneshot},
    task::JoinHandle,
    time::{advance, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};
use tokio_util::sync::CancellationToken;

type Socket = WebSocketStream<DuplexStream>;
const BOUND: Duration = Duration::from_secs(2);

fn limits() -> Limits {
    Limits {
        requests: 2,
        outbound: 8,
        max_frame: 65_536,
        ordinary: Duration::from_secs(5),
        ipt: Duration::from_secs(20),
    }
}

fn echo_handler() -> Handler {
    Arc::new(|request| {
        Box::pin(async move {
            Completion {
                response: request,
                changes: vec![],
            }
        })
    })
}

#[tokio::test(start_paused = true)]
async fn independent_demo_tick_interleaves_with_write_without_retiring_session() {
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let (begin, finish) = (entered.clone(), release.clone());
    let causal = json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"audio_gain":2.5}}});
    let expected_causal = causal.clone();
    let handler: Handler = Arc::new(move |request| {
        let (begin, finish, causal) = (begin.clone(), finish.clone(), causal.clone());
        Box::pin(async move {
            begin.notify_one();
            finish.notified().await;
            Completion {
                response: json!({"message_id":request["message_id"],"message_type":"set_dynamic_values","data":{}}),
                changes: vec![causal],
            }
        })
    });
    let (client, server) = tokio::io::duplex(65_536);
    let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
    let peer = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
    let (events, incoming) = mpsc::channel(4);
    let stop = CancellationToken::new();
    let task = tokio::spawn(session::run_independent(
        client,
        handler,
        incoming,
        Arc::new(|| true),
        limits(),
        stop.clone(),
    ));
    let mut generation = Generation {
        peer,
        events,
        stop,
        task,
    };
    generation.send(json!({"message_id":1,"message_type":"set_dynamic_values","data":{"values":{"audio_gain":2.5}}})).await;
    bounded(entered.notified()).await; // The mutation owns its lane until response delivery.
    let demo =
        rust_example::demo::DemoDevice::new(Arc::new(rust_example::registry::Registry::new()));
    for event in demo.tick().unwrap() {
        let frame = rust_example::protocol::event_frame(&event);
        generation.events.send(frame.clone()).await.unwrap();
        assert_eq!(generation.response().await, frame);
    }
    let keepalive = json!({"message_id":2,"message_type":"keepalive","data":null});
    generation.send(keepalive.clone()).await;
    assert_eq!(generation.response().await, keepalive);
    release.notify_one();
    assert_eq!(
        generation.response().await,
        json!({"message_id":1,"message_type":"set_dynamic_values","data":{}})
    );
    assert_eq!(generation.response().await, expected_causal);
    generation.stop.cancel();
    bounded(generation.task).await.unwrap();
}

struct Generation {
    peer: Socket,
    events: mpsc::Sender<Value>,
    stop: CancellationToken,
    task: JoinHandle<&'static str>,
}

impl Generation {
    async fn new(
        handler: Handler,
        limits: Limits,
        healthy: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        let (client, server) = tokio::io::duplex(65_536);
        let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
        let peer = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let (events, incoming) = mpsc::channel(32);
        let stop = CancellationToken::new();
        let task = tokio::spawn(session::run(
            client,
            handler,
            incoming,
            healthy,
            limits,
            stop.clone(),
        ));
        Self {
            peer,
            events,
            stop,
            task,
        }
    }

    async fn ordinary(handler: Handler) -> Self {
        Self::new(handler, limits(), Arc::new(|| true)).await
    }

    async fn send(&mut self, request: Value) {
        bounded(self.peer.send(Message::Text(request.to_string().into())))
            .await
            .unwrap();
    }

    async fn response(&mut self) -> Value {
        let frame = bounded(self.peer.next())
            .await
            .expect("session frame")
            .expect("valid frame");
        assert!(frame.is_text(), "expected a correlated JSON response");
        serde_json::from_str(frame.to_text().unwrap()).unwrap()
    }

    async fn synchronize(&mut self, id: i64) {
        // An echoed keepalive proves the reader and writer have both run before clock changes.
        let request = json!({"message_id":id,"message_type":"keepalive","data":null});
        self.send(request.clone()).await;
        assert_eq!(self.response().await, request);
    }

    async fn shutdown(self) {
        self.stop.cancel();
        assert_eq!(bounded(self.task).await.unwrap(), "shutdown");
    }
}

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    timeout(BOUND, future)
        .await
        .expect("bounded synchronization exceeded")
}

async fn settle() {
    // A fixed yield budget lets ready tasks progress without wall-clock sleeps or timer auto-advance.
    for _ in 0..24 {
        tokio::task::yield_now().await;
    }
}

fn request(id: Value, kind: &str) -> Value {
    json!({"message_id":id,"message_type":kind,"data":{}})
}

#[tokio::test(start_paused = true)]
async fn inbound_idle_closes_at_sixty_seconds_without_autonomous_pings() {
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation.synchronize(1).await;
    advance(Duration::from_secs(59)).await;
    settle().await;
    assert!(!generation.task.is_finished());
    assert!(
        generation.peer.next().now_or_never().is_none(),
        "idle driver emitted an unsolicited frame"
    );
    advance(Duration::from_secs(1)).await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "inbound idle timeout"
    );
    assert!(matches!(
        bounded(generation.peer.next()).await,
        Some(Ok(Message::Close(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn application_traffic_resets_idle_but_outbound_events_do_not() {
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation.synchronize(1).await;
    advance(Duration::from_secs(40)).await;
    generation.synchronize(2).await;
    advance(Duration::from_secs(40)).await;
    let event = json!({"message_id":null,"message_type":"async_alarms","data":[]});
    generation.events.send(event.clone()).await.unwrap();
    assert_eq!(generation.response().await, event);
    advance(Duration::from_secs(19)).await;
    settle().await;
    assert!(!generation.task.is_finished());
    advance(Duration::from_secs(1)).await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "inbound idle timeout"
    );
}

#[tokio::test(start_paused = true)]
async fn transport_pong_does_not_reset_application_watchdog() {
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation.synchronize(1).await;
    advance(Duration::from_secs(59)).await;
    generation
        .peer
        .send(Message::Pong(vec![1, 2, 3].into()))
        .await
        .unwrap();
    settle().await;
    advance(Duration::from_secs(1)).await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "inbound idle timeout"
    );
}

#[tokio::test(start_paused = true)]
async fn keepalive_is_byte_exact_while_handler_is_busy() {
    let started = Arc::new(Notify::new());
    let signal = started.clone();
    let handler: Handler = Arc::new(move |_| {
        let signal = signal.clone();
        Box::pin(async move {
            signal.notify_one();
            std::future::pending().await
        })
    });
    let mut generation = Generation::ordinary(handler).await;
    generation.send(request(json!(1), "alarms")).await;
    bounded(started.notified()).await;
    let raw = r#"{ "message_id":2, "message_type":"keepalive", "data":null, "future":{"x":42} }"#;
    generation
        .peer
        .send(Message::Text(raw.into()))
        .await
        .unwrap();
    let frame = bounded(generation.peer.next()).await.unwrap().unwrap();
    assert_eq!(frame.to_text().unwrap(), raw);
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn unhealthy_device_closes_instead_of_acknowledging_keepalive() {
    let healthy = Arc::new(AtomicBool::new(true));
    let observed = healthy.clone();
    let mut generation = Generation::new(
        echo_handler(),
        limits(),
        Arc::new(move || observed.load(Ordering::SeqCst)),
    )
    .await;
    generation.synchronize(1).await;
    healthy.store(false, Ordering::SeqCst);
    generation.send(request(json!(2), "keepalive")).await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "device unavailable"
    );
    assert!(matches!(
        bounded(generation.peer.next()).await,
        Some(Ok(Message::Close(_)))
    ));
}

#[tokio::test(start_paused = true)]
async fn dispatcher_rejects_overload_with_correlation_and_preserves_keepalive() {
    let started = Arc::new(Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let signal = started.clone();
    let count = calls.clone();
    let handler: Handler = Arc::new(move |_| {
        count.fetch_add(1, Ordering::SeqCst);
        let signal = signal.clone();
        Box::pin(async move {
            signal.notify_one();
            std::future::pending().await
        })
    });
    let mut bounded_limits = limits();
    bounded_limits.requests = 1;
    let mut generation = Generation::new(handler, bounded_limits, Arc::new(|| true)).await;
    generation.send(request(json!(100), "alarms")).await;
    bounded(started.notified()).await;
    generation.send(request(json!(101), "alarms")).await;
    assert_eq!(
        generation.response().await,
        json!({"message_id":101,"message_type":"error","data":"driver overloaded"})
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "overloaded request must never start a handler"
    );
    generation.synchronize(102).await;
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn significant_event_queue_overload_retires_generation() {
    let mut bounded_limits = limits();
    bounded_limits.outbound = 2;
    let generation = Generation::new(echo_handler(), bounded_limits, Arc::new(|| true)).await;
    // Prefill before the session is scheduled. The biased event loop cannot yield to its
    // writer between these ready receives, so the third event deterministically saturates it.
    for sequence in 0..3 {
        generation.events.try_send(json!({"message_id":null,"message_type":"async_alarms","data":{"sequence":sequence}})).unwrap();
    }
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "event backpressure"
    );
}

#[tokio::test(start_paused = true)]
async fn slow_writer_is_bounded_and_retires_generation() {
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation.synchronize(1).await;
    // Larger than the memory socket's capacity: without a peer reader, send cannot complete.
    generation.events.send(json!({"message_id":null,"message_type":"async_values_changed","data":"x".repeat(131_072)})).await.unwrap();
    settle().await;
    advance(Duration::from_secs(1)).await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "writer unavailable"
    );
}

struct OnDrop(Arc<AtomicBool>);
impl Drop for OnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn cancellation_drains_inflight_handler_and_suppresses_stale_completion() {
    let (late_tx, late_rx) = oneshot::channel::<()>();
    let receiver = Arc::new(Mutex::new(Some(late_rx)));
    let started = Arc::new(Notify::new());
    let signal = started.clone();
    let dropped = Arc::new(AtomicBool::new(false));
    let observed_drop = dropped.clone();
    let handler: Handler = Arc::new(move |request| {
        let receiver = receiver.lock().unwrap().take().unwrap();
        let signal = signal.clone();
        let guard = OnDrop(observed_drop.clone());
        Box::pin(async move {
            let _guard = guard;
            signal.notify_one();
            let _ = receiver.await;
            Completion {
                response: request,
                changes: vec![
                    json!({"message_id":null,"message_type":"async_values_changed","data":"stale"}),
                ],
            }
        })
    });
    let mut old = Generation::ordinary(handler).await;
    old.send(request(json!(42), "set_dynamic_values")).await;
    bounded(started.notified()).await;
    old.stop.cancel();
    assert_eq!(bounded(old.task).await.unwrap(), "shutdown");
    assert!(
        dropped.load(Ordering::SeqCst),
        "run must drain owned work before returning"
    );
    assert!(
        late_tx.send(()).is_err(),
        "retired handler must not accept a late result"
    );
    assert!(matches!(
        bounded(old.peer.next()).await,
        Some(Ok(Message::Close(_)))
    ));
    // Reusing the request ID on another socket cannot resurrect the old response or event.
    let mut fresh = Generation::ordinary(echo_handler()).await;
    let current = request(json!(42), "get_structure");
    fresh.send(current.clone()).await;
    assert_eq!(fresh.response().await, current);
    settle().await;
    assert!(fresh.peer.next().now_or_never().is_none());
    fresh.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn response_is_delivered_before_deferred_causal_changes() {
    let handler: Handler = Arc::new(|request| {
        Box::pin(async move {
            Completion {
                response: request,
                changes: vec![
                    json!({"message_id":null,"message_type":"async_values_changed","data":{"revision":1}}),
                    json!({"message_id":null,"message_type":"async_values_changed","data":{"revision":2}}),
                ],
            }
        })
    });
    let mut generation = Generation::ordinary(handler).await;
    let input = request(json!(7), "set_dynamic_values");
    generation.send(input.clone()).await;
    assert_eq!(generation.response().await, input);
    for revision in [1, 2] {
        let change = generation.response().await;
        assert!(change["message_id"].is_null());
        assert_eq!(change["data"]["revision"], revision);
    }
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn handler_future_panic_returns_one_correlated_sanitized_error() {
    let handler: Handler = Arc::new(|_| Box::pin(async { panic!("synthetic handler panic") }));
    let mut generation = Generation::ordinary(handler).await;
    generation.send(request(json!(-9), "alarms")).await;
    assert_eq!(
        generation.response().await,
        json!({"message_id":-9,"message_type":"error","data":"internal handler failure"})
    );
    generation.synchronize(10).await;
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn handler_construction_panic_also_preserves_correlation() {
    // Fn can panic before returning a future; the containment boundary must cover both phases.
    let handler: Handler = Arc::new(|_| panic!("synthetic closure construction panic"));
    let mut generation = Generation::ordinary(handler).await;
    generation.send(request(json!(19), "alarms")).await;
    assert_eq!(
        generation.response().await,
        json!({"message_id":19,"message_type":"error","data":"internal handler failure"})
    );
    generation.synchronize(20).await;
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn deadlines_are_operation_specific_and_cancel_the_timed_out_future() {
    for (kind, budget) in [("alarms", 5), ("set_ipt_configurations", 20)] {
        let started = Arc::new(Notify::new());
        let signal = started.clone();
        let dropped = Arc::new(AtomicBool::new(false));
        let observed = dropped.clone();
        let handler: Handler = Arc::new(move |_| {
            let signal = signal.clone();
            let guard = OnDrop(observed.clone());
            Box::pin(async move {
                let _guard = guard;
                signal.notify_one();
                std::future::pending().await
            })
        });
        let mut generation = Generation::ordinary(handler).await;
        generation.send(request(json!(88), kind)).await;
        bounded(started.notified()).await;
        advance(Duration::from_secs(budget - 1)).await;
        settle().await;
        assert!(!dropped.load(Ordering::SeqCst));
        assert!(generation.peer.next().now_or_never().is_none());
        advance(Duration::from_secs(1)).await;
        assert_eq!(
            generation.response().await,
            json!({"message_id":88,"message_type":"error","data":"request deadline exceeded"})
        );
        assert!(dropped.load(Ordering::SeqCst));
        generation.shutdown().await;
    }
}

#[tokio::test(start_paused = true)]
async fn unknown_fields_and_full_width_integer_ids_survive_dispatch() {
    let mut generation = Generation::ordinary(echo_handler()).await;
    for id in [
        json!(i64::MIN),
        json!(u64::MAX),
        json!(9_007_199_254_740_993_u64),
    ] {
        let mut input = request(id, "get_structure");
        input["future_field"] = json!({"nested":[1, null, true]});
        generation.send(input.clone()).await;
        assert_eq!(generation.response().await, input);
    }
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn unusable_ids_close_without_fabricating_correlated_responses() {
    for id in [
        Value::Null,
        json!("1"),
        json!(1.5),
        json!(true),
        json!({}),
        json!([]),
    ] {
        let mut generation = Generation::ordinary(echo_handler()).await;
        generation.send(request(id, "alarms")).await;
        assert_eq!(
            bounded(generation.task).await.unwrap(),
            "unusable message ID"
        );
        assert!(matches!(
            bounded(generation.peer.next()).await,
            Some(Ok(Message::Close(_)))
        ));
    }
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation
        .send(json!({"message_type":"alarms","data":{}}))
        .await;
    assert_eq!(
        bounded(generation.task).await.unwrap(),
        "unusable message ID"
    );
}

#[tokio::test(start_paused = true)]
async fn malformed_envelopes_with_usable_ids_get_errors_without_dispatch() {
    let handler: Handler = Arc::new(|_| panic!("invalid envelope reached the handler"));
    let mut generation = Generation::ordinary(handler).await;
    for input in [
        json!({"message_id":1,"data":{}}),
        json!({"message_id":2,"message_type":"alarms"}),
        json!({"message_id":3,"message_type":17,"data":{}}),
    ] {
        generation.send(input.clone()).await;
        assert_eq!(
            generation.response().await,
            json!({"message_id":input["message_id"],"message_type":"error","data":"invalid envelope"})
        );
    }
    generation.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn malformed_json_binary_and_oversized_text_retire_socket() {
    for (frame, reason) in [
        (Message::Text("{".into()), "invalid JSON"),
        (Message::Text("[]".into()), "invalid envelope"),
        (Message::Binary(vec![1].into()), "expected JSON text"),
        (Message::Text("x".repeat(257).into()), "frame too large"),
    ] {
        let mut small = limits();
        small.max_frame = 256;
        let mut generation = Generation::new(echo_handler(), small, Arc::new(|| true)).await;
        generation.peer.send(frame).await.unwrap();
        assert_eq!(bounded(generation.task).await.unwrap(), reason);
    }
}

#[derive(Clone, Default)]
struct CapturedLog(Arc<Mutex<Vec<u8>>>);
impl std::io::Write for CapturedLog {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
async fn logs_keep_request_correlation_without_untrusted_types_or_payload_secrets() {
    let log = CapturedLog::default();
    let output = log.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter("rust_example=trace")
        .with_ansi(false)
        .without_time()
        .with_target(false)
        .with_writer(move || output.clone())
        .finish();
    // This integration-test binary owns one subscriber. A global dispatcher also captures
    // spawned work consistently when Rust runs the other test runtimes on parallel threads.
    // Use production's crate-only filter, since dependency TRACE can expose raw frames.
    tracing::subscriber::set_global_default(subscriber).unwrap();
    const SECRET: &str = "synthetic-SRT-key-bearer-password-do-not-log";
    let mut generation = Generation::ordinary(echo_handler()).await;
    generation
        .send(json!({"message_id":null,"message_type":"error","data":SECRET}))
        .await;
    let input =
        json!({"message_id":7341,"message_type":SECRET,"data":{"srt_key":SECRET,"sdp":SECRET}});
    generation.send(input.clone()).await;
    assert_eq!(generation.response().await, input);
    // An error reply loop would appear before this echo; none is allowed for incoming errors.
    generation.synchronize(7342).await;
    generation.shutdown().await;
    let text = String::from_utf8(log.0.lock().unwrap().clone()).unwrap();
    assert!(text.contains("message_id=7341"));
    assert!(text.contains("TX delivered"));
    assert!(text.contains("payload omitted"));
    assert!(
        !text.contains(SECRET),
        "diagnostics exposed untrusted input"
    );
}
