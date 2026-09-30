//! External Driver API session rules, independent of the vendor's commands.
//!
//! Preserve request correlation, response-before-causal-event delivery and healthy
//! keepalives even when device I/O is slow. Bounds and generation cancellation keep
//! old work from answering a new connection. These are reusable lessons; the exact
//! queues/locks are this Rust example's design, not additional SPEC-23 messages.
//! Vendor operations belong behind the handler, never inside the socket writer.
use futures_util::{SinkExt, StreamExt, future::BoxFuture};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::{mpsc, oneshot},
    task::JoinSet,
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{self, Message},
};
use tokio_util::sync::CancellationToken;

pub struct Completion {
    pub response: Value,
    pub changes: Vec<Value>,
}
pub type Handler = Arc<dyn Fn(Value) -> BoxFuture<'static, Completion> + Send + Sync>;

#[derive(Clone)]
pub struct Limits {
    pub requests: usize,
    pub outbound: usize,
    pub max_frame: usize,
    pub ordinary: Duration,
    pub ipt: Duration,
}

struct Write {
    frame: Message,
    delivered: oneshot::Sender<bool>,
}

// Guards are held for their RAII lifetime, not inspected after acquisition.
struct Lane {
    _read: Option<tokio::sync::OwnedRwLockReadGuard<()>>,
    _write: Option<tokio::sync::OwnedRwLockWriteGuard<()>>,
}

async fn deliver(queue: &mpsc::Sender<Write>, frame: Value) -> Result<(), ()> {
    let (delivered, ack) = oneshot::channel();
    queue
        .send(Write {
            frame: Message::Text(frame.to_string().into()),
            delivered,
        })
        .await
        .map_err(|_| ())?;
    if ack.await.map_err(|_| ())? {
        Ok(())
    } else {
        Err(())
    }
}

fn correlated_error(id: Value, detail: &'static str) -> Value {
    json!({"message_id":id,"message_type":"error","data":detail})
}

/// Returns only after generation-owned handlers and writer have been drained.
pub async fn run<S>(
    socket: WebSocketStream<S>,
    handler: Handler,
    events: mpsc::Receiver<Value>,
    healthy: Arc<dyn Fn() -> bool + Send + Sync>,
    limits: Limits,
    shutdown: CancellationToken,
) -> &'static str
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    run_guarded(
        socket,
        handler,
        events,
        healthy,
        limits,
        shutdown,
        Arc::new(|_| true),
    )
    .await
}

/// The target adapter can attach an internal revision for a final freshness check.
/// Internal metadata is removed here and never becomes a SPEC-23 wire field.
pub async fn run_guarded<S>(
    socket: WebSocketStream<S>,
    handler: Handler,
    mut events: mpsc::Receiver<Value>,
    healthy: Arc<dyn Fn() -> bool + Send + Sync>,
    limits: Limits,
    shutdown: CancellationToken,
    current: Arc<dyn Fn(&Value) -> bool + Send + Sync>,
) -> &'static str
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (mut sink, mut source) = socket.split();
    let (out, out_rx) = mpsc::channel::<Write>(limits.outbound);
    let (urgent, urgent_rx) = mpsc::channel::<Write>(limits.outbound);
    let (mut out_rx, mut urgent_rx) = (out_rx, urgent_rx);
    let writer_stop = shutdown.child_token();
    let stop = writer_stop.clone();
    let mut writer = tokio::spawn(async move {
        loop {
            // Priority is useful for keepalive, but NOT our causal-ordering guarantee:
            // handlers explicitly wait for the response's delivery acknowledgement below.
            let item = tokio::select! { biased;
                _=stop.cancelled()=>break,
                item=urgent_rx.recv()=>item,
                item=out_rx.recv()=>item,
            };
            let Some(item) = item else { break };
            let ok = matches!(
                timeout(Duration::from_secs(1), sink.send(item.frame)).await,
                Ok(Ok(()))
            );
            let _ = item.delivered.send(ok);
            if !ok {
                break;
            }
        }
        let _ = timeout(Duration::from_secs(1), sink.close()).await;
    });
    let mut work = JoinSet::new();
    // The dispatcher serializes device mutations for their complete read/apply/readback
    // transaction. A transport write mutex alone cannot prevent two requests' observed
    // changes from racing. Reads share the lane; keepalive never waits for this lock.
    let device_lane = Arc::new(tokio::sync::RwLock::new(()));
    let mut last_inbound = Instant::now();
    let mut health_tick = tokio::time::interval(Duration::from_millis(100));
    health_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let reason = loop {
        tokio::select! { biased;
            _=shutdown.cancelled()=>break "shutdown",
            _=&mut writer=>break "writer unavailable",
            _=health_tick.tick()=> {
                if !healthy() {break "device unavailable";}
                if last_inbound.elapsed()>=Duration::from_secs(60) {break "inbound idle timeout";}
            }
            Some(result)=work.join_next(), if !work.is_empty()=> {
                if result.is_err() {break "handler task failed";}
            }
            frame=source.next()=> {
                let Some(Ok(frame))=frame else {break "socket closed";};
                match frame {
                    Message::Text(text)=> {
                        last_inbound=Instant::now();
                        if text.len()>limits.max_frame {break "frame too large";}
                        let Ok(value)=serde_json::from_str::<Value>(&text) else {break "invalid JSON";};
                        if !value.is_object() {break "invalid envelope";}
                        let kind=value.get("message_type").and_then(Value::as_str);
                        // Do not print incoming error payloads: they can echo secret-bearing input.
                        if kind==Some("error") {tracing::warn!("received backend error (payload omitted)");continue;}
                        let Some(id)=value.get("message_id").filter(|id|id.is_i64()||id.is_u64()).cloned() else {break "unusable message ID";};
                        // Unknown type strings are untrusted too; log only an allowlisted tag.
                        let tag=safe_type(kind);
                        tracing::debug!(message_id=%id,message_type=tag,"RX");
                        let (ack,_) = oneshot::channel();
                        if kind.is_none() || value.get("data").is_none() {
                            if out.try_send(Write{frame:Message::Text(correlated_error(id,"invalid envelope").to_string().into()),delivered:ack}).is_err(){break "response overload";}
                            continue;
                        }
                        if kind==Some("keepalive") {
                            if !healthy() {break "device unavailable";}
                            if urgent.try_send(Write{frame:Message::Text(text),delivered:ack}).is_err() {break "keepalive overload";}
                            tracing::debug!(message_id=%id,message_type=tag,"TX queued");
                            continue;
                        }
                        if work.len()>=limits.requests {
                            if urgent.try_send(Write{frame:Message::Text(correlated_error(id,"driver overloaded").to_string().into()),delivered:ack}).is_err(){break "response overload";}
                            continue;
                        }
                        let budget=if kind==Some("set_ipt_configurations"){limits.ipt}else{limits.ordinary};
                        let handler=handler.clone(); let out=out.clone(); let fail_generation=writer_stop.clone();
                        let lane=device_lane.clone();
                        let mutating=matches!(kind,Some("set_dynamic_values"|"set_ipt_configurations"))
                            || (kind==Some("check_ipt_configuration") && value["data"]["set_again"]==true);
                        work.spawn(async move {
                            let start=Instant::now();
                            // Catch a handler panic at this boundary so a usable-ID request is
                            // never silently abandoned while its generation is still alive.
                            let called=std::panic::AssertUnwindSafe(async {
                                // Keep the owned guard through delivery below, not merely device I/O:
                                // later writes must not publish before an earlier causal update.
                                let guard=if mutating { Lane{_write:Some(lane.write_owned().await),_read:None} }
                                    else { Lane{_read:Some(lane.read_owned().await),_write:None} };
                                (handler(value).await,guard)
                            });
                            use futures_util::FutureExt;
                            let result=timeout(budget,called.catch_unwind()).await;
                            let (completion,_guard)=match result {
                                Ok(Ok((done,guard)))=>(done,Some(guard)),
                                Ok(Err(_))=>(Completion{response:correlated_error(id.clone(),"internal handler failure"),changes:vec![]},None),
                                Err(_)=>(Completion{response:correlated_error(id.clone(),"request deadline exceeded"),changes:vec![]},None),
                            };
                            if matches!(timeout(Duration::from_secs(1),deliver(&out,completion.response)).await,Ok(Ok(()))) {
                                tracing::debug!(message_id=%id,message_type=tag,latency_us=start.elapsed().as_micros() as u64,"TX delivered");
                                for event in completion.changes {
                                    if !matches!(timeout(Duration::from_secs(1),deliver(&out,event)).await,Ok(Ok(()))) {
                                        fail_generation.cancel(); break;
                                    }
                                }
                            } else {
                                // A full writer queue cannot extend a request indefinitely. Closing
                                // the generation is safer than a late or silently lost response.
                                fail_generation.cancel();
                            }
                        });
                    }
                    Message::Close(_)=>break "peer close",
                    Message::Binary(_)=>break "expected JSON text",
                    // No autonomous Ping is sent. Transport control frames do not reset the
                    // SPEC idle watchdog, which requires backend application traffic.
                    Message::Ping(_)|Message::Pong(_)|Message::Frame(_)=>{}
                }
            }
            event=events.recv()=> {
                let Some(mut event)=event else {break "event stream unavailable";};
                if !current(&event) {break "stale target event requires resynchronization";}
                if let Some(object)=event.as_object_mut() {object.remove("_driver_revision");}
                // Without a target transaction/revision token we cannot attribute an
                // independent event to an in-flight write safely. Fail closed and
                // resynchronize rather than publish a possibly stale causal value.
                let Ok(_event_guard)=device_lane.try_read() else {break "concurrent target change requires resynchronization";};
                let (delivered,_)=oneshot::channel();
                if out.try_send(Write{frame:Message::Text(event.to_string().into()),delivered}).is_err() {
                    // Never silently discard an alarm edge. Reconnect and re-read authoritative state.
                    break "event backpressure";
                }
            }
        }
    };
    work.abort_all();
    while work.join_next().await.is_some() {}
    writer_stop.cancel();
    if !writer.is_finished() {
        let _ = writer.await;
    }
    reason
}

fn safe_type(kind: Option<&str>) -> &'static str {
    match kind {
        Some("keepalive") => "keepalive",
        Some("get_structure") => "get_structure",
        Some("get_dynamic_values") => "get_dynamic_values",
        Some("set_dynamic_values") => "set_dynamic_values",
        Some("alarms") => "alarms",
        Some("get_sender_sdp") => "get_sender_sdp",
        Some("set_ipt_configurations") => "set_ipt_configurations",
        Some("check_ipt_configuration") => "check_ipt_configuration",
        _ => "unknown",
    }
}

/// Pure jitter seam: production supplies randomness; tests supply fixed entropy.
pub fn backoff(attempt: u32, minimum: Duration, maximum: Duration, entropy: u64) -> Duration {
    let cap = minimum.saturating_mul(1u32 << attempt.min(16)).min(maximum);
    let upper = cap.as_millis().min(u64::MAX as u128) as u64;
    let lower = (upper / 2).max(1);
    Duration::from_millis(lower + entropy % (upper.saturating_sub(lower) + 1))
}

/// Classification deliberately avoids formatting the error, which may carry HTTP headers.
pub fn permanent(error: &tungstenite::Error) -> bool {
    match error {
        tungstenite::Error::Http(response) => response.status().is_client_error(),
        tungstenite::Error::Tls(_)
        | tungstenite::Error::Url(_)
        | tungstenite::Error::HttpFormat(_) => true,
        tungstenite::Error::Io(e) => e.kind() == std::io::ErrorKind::InvalidData,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::protocol::Role;

    #[test]
    fn reconnect_jitter_is_bounded_and_saturates() {
        let min = Duration::from_millis(500);
        let max = Duration::from_secs(30);
        for attempt in [0, 1, 5, 16, u32::MAX] {
            for entropy in [0, 1, 42, u64::MAX] {
                let delay = backoff(attempt, min, max, entropy);
                assert!(delay >= min / 2 && delay <= max);
            }
        }
        assert_eq!(backoff(0, min, max, 0), min / 2);
        assert_eq!(backoff(u32::MAX, min, max, 0), max / 2);
    }
    async fn pair() -> (
        WebSocketStream<tokio::io::DuplexStream>,
        WebSocketStream<tokio::io::DuplexStream>,
    ) {
        let (a, b) = tokio::io::duplex(65536);
        (
            WebSocketStream::from_raw_socket(a, Role::Client, None).await,
            WebSocketStream::from_raw_socket(b, Role::Server, None).await,
        )
    }
    fn limits() -> Limits {
        Limits {
            requests: 2,
            outbound: 8,
            max_frame: 65536,
            ordinary: Duration::from_secs(5),
            ipt: Duration::from_secs(20),
        }
    }

    #[tokio::test]
    async fn private_revision_never_leaks_onto_spec_wire() {
        let (client, mut server) = pair().await;
        let (tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|_| Box::pin(std::future::pending()));
        let task = tokio::spawn(run_guarded(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel.clone(),
            Arc::new(|v| v["_driver_revision"] == 42),
        ));
        tx.send(json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"enabled":false}},"_driver_revision":42})).await.unwrap();
        let frame = server.next().await.unwrap().unwrap().into_text().unwrap();
        let frame: Value = serde_json::from_str(&frame).unwrap();
        assert!(frame.get("_driver_revision").is_none());
        assert_eq!(frame["message_type"], "async_values_changed");
        cancel.cancel();
        task.await.unwrap();
    }

    #[tokio::test]
    async fn stale_delayed_event_retires_instead_of_overwriting_confirmed_state() {
        let (client, mut server) = pair().await;
        let (tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|_| Box::pin(std::future::pending()));
        let task = tokio::spawn(run_guarded(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel,
            Arc::new(|_| false),
        ));
        tx.send(json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"enabled":false}},"_driver_revision":1})).await.unwrap();
        assert!(matches!(
            server.next().await.unwrap().unwrap(),
            Message::Close(_)
        ));
        assert_eq!(
            task.await.unwrap(),
            "stale target event requires resynchronization"
        );
    }

    #[tokio::test]
    async fn mutating_requests_hold_the_lane_through_causal_delivery() {
        let (client, mut server) = pair().await;
        let (_tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|v| {
            Box::pin(async move {
                Completion {
                    response: json!({"message_id":v["message_id"],"message_type":"set_dynamic_values","data":{}}),
                    changes: vec![
                        json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"level":v["message_id"]}}}),
                    ],
                }
            })
        });
        let task = tokio::spawn(run(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel.clone(),
        ));
        for id in [1, 2] {
            server
                .send(Message::Text(
                    json!({"message_id":id,"message_type":"set_dynamic_values","data":{}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        }
        for _ in 0..2 {
            let reply: Value =
                serde_json::from_str(&server.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            let event: Value =
                serde_json::from_str(&server.next().await.unwrap().unwrap().into_text().unwrap())
                    .unwrap();
            assert_eq!(reply["message_type"], "set_dynamic_values");
            assert_eq!(event["message_type"], "async_values_changed");
            assert_eq!(event["data"]["values"]["level"], reply["message_id"]);
        }
        cancel.cancel();
        task.await.unwrap();
    }
    #[tokio::test]
    async fn correlated_response_precedes_causal_event_and_error_is_silent() {
        let (client, mut server) = pair().await;
        let (tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|v| {
            Box::pin(async move {
                Completion {
                    response: json!({"message_id":v["message_id"],"message_type":"set_dynamic_values","data":{}}),
                    changes: vec![
                        json!({"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"x":1}}}),
                    ],
                }
            })
        });
        let task = tokio::spawn(run(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel.clone(),
        ));
        server
            .send(Message::Text(
                json!({"message_id":9,"message_type":"error","data":"SECRET"})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        server
            .send(Message::Text(
                json!({"message_id":10,"message_type":"set_dynamic_values","data":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let a = server.next().await.unwrap().unwrap().into_text().unwrap();
        let b = server.next().await.unwrap().unwrap().into_text().unwrap();
        assert_eq!(serde_json::from_str::<Value>(&a).unwrap()["message_id"], 10);
        assert_eq!(
            serde_json::from_str::<Value>(&b).unwrap()["message_type"],
            "async_values_changed"
        );
        cancel.cancel();
        let _ = task.await.unwrap();
        drop(tx);
    }
    #[tokio::test]
    async fn keepalive_is_exact_and_independent_of_hung_handler() {
        let (client, mut server) = pair().await;
        let (_tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|_| Box::pin(std::future::pending()));
        let task = tokio::spawn(run(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel.clone(),
        ));
        server
            .send(Message::Text(
                r#"{"message_id":1,"message_type":"alarms","data":null}"#.into(),
            ))
            .await
            .unwrap();
        let keep = r#"{ "message_id":2,"message_type":"keepalive","data":null,"future":42 }"#;
        server.send(Message::Text(keep.into())).await.unwrap();
        assert_eq!(
            server.next().await.unwrap().unwrap().into_text().unwrap(),
            keep
        );
        cancel.cancel();
        let _ = task.await.unwrap();
    }
    #[tokio::test]
    async fn unknown_type_and_invalid_data_keep_correlation() {
        let (client, mut server) = pair().await;
        let (_tx, rx) = mpsc::channel(2);
        let cancel = CancellationToken::new();
        let h: Handler = Arc::new(|v| {
            Box::pin(async move {
                Completion {
                    response: correlated_error(v["message_id"].clone(), "unsupported request"),
                    changes: vec![],
                }
            })
        });
        let task = tokio::spawn(run(
            client,
            h,
            rx,
            Arc::new(|| true),
            limits(),
            cancel.clone(),
        ));
        for input in [
            json!({"message_id":1,"message_type":"new","data":null}),
            json!({"message_id":2,"data":null}),
        ] {
            server
                .send(Message::Text(input.to_string().into()))
                .await
                .unwrap();
            let result = server.next().await.unwrap().unwrap().into_text().unwrap();
            let v: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(v["message_id"], input["message_id"]);
            assert_eq!(v["message_type"], "error");
        }
        cancel.cancel();
        let _ = task.await.unwrap();
    }
}
