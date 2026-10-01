//! Replace this adapter with your vendor API implementation of [`Device`].
//!
//! This client talks to the simulator, not VideoIPath. Its private request IDs,
//! revisions and Unix socket are teaching/test choices, not SPEC-23 wire fields.
//! Preserve the useful device-I/O properties when adapting it: fresh observations,
//! confirmed writes, bounded deadlines and no replay of an ambiguous action.
//! For a stream-based vendor API, one reader must distinguish replies from events.
use super::simulator::{self, Frame, Request};
use crate::domain::{Alarm, Device, Event, Failure, Location, MediaState, PortKey, Values};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncWriteExt, BufReader},
    net::{UnixStream, unix::OwnedWriteHalf},
    sync::{Semaphore, broadcast, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, thiserror::Error)]
pub enum Error {
    #[error("target transport unavailable; operation outcome may be unknown")]
    Unavailable,
    #[error("target operation timed out; operation outcome may be unknown")]
    Timeout,
    #[error("target concurrency limit reached")]
    Busy,
    #[error("target rejected operation")]
    Rejected(Failure),
    #[error("invalid target protocol frame")]
    Protocol,
}

type Reply = Result<Value, Error>;
struct Waiter {
    sender: oneshot::Sender<Reply>,
    value_write: Option<(Location, String)>,
}
type Waiters = Mutex<HashMap<u64, Waiter>>;

/// Private southbound ordering evidence, never a SPEC field. Keep this alongside
/// a relayed event until actual dispatch; receiving it is not a freshness check.
#[derive(Clone)]
pub struct ObservedEvent {
    pub revision: u64,
    pub event: Event,
}
struct Inner {
    writer: tokio::sync::Mutex<OwnedWriteHalf>,
    waiters: Waiters,
    next_id: AtomicU64,
    healthy: AtomicBool,
    stopped: CancellationToken,
    permits: Semaphore,
    timeout: Duration,
    events: broadcast::Sender<Event>,
    observed_events: broadcast::Sender<ObservedEvent>,
    value_write_floors: Mutex<HashMap<(Location, String), u64>>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl Inner {
    fn retire(&self, reason: Error) {
        self.healthy.store(false, Ordering::Release);
        self.stopped.cancel();
        for (_, waiter) in self.waiters.lock().expect("waiters mutex").drain() {
            let _ = waiter.sender.send(Err(reason));
        }
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        self.stopped.cancel();
        if let Some(reader) = self.reader.get_mut().expect("reader mutex").take() {
            reader.abort();
        }
    }
}

/// Clones share one generation. Reconnect is an explicit composition-root decision.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

/// Runs also when a caller drops its future. Set `transmitted` BEFORE write_all:
/// cancellation during a partial write is already ambiguous. Merely removing its
/// waiter would leave the remaining stream unsafe to reuse for another operation.
struct Transaction {
    inner: Arc<Inner>,
    id: u64,
    transmitted: bool,
    complete: bool,
}
impl Drop for Transaction {
    fn drop(&mut self) {
        self.inner
            .waiters
            .lock()
            .expect("waiters mutex")
            .remove(&self.id);
        if self.transmitted && !self.complete {
            self.inner.retire(Error::Unavailable);
        }
    }
}

impl Client {
    pub async fn connect(path: &Path, operation_timeout: Duration) -> Result<Self, Error> {
        if operation_timeout.is_zero() || operation_timeout > Duration::from_secs(30) {
            return Err(Error::Protocol);
        }
        let stream = tokio::time::timeout(operation_timeout, UnixStream::connect(path))
            .await
            .map_err(|_| Error::Timeout)?
            .map_err(|_| Error::Unavailable)?;
        let (reader, writer) = stream.into_split();
        let (events, _) = broadcast::channel(128);
        let (observed_events, _) = broadcast::channel(128);
        let inner = Arc::new(Inner {
            writer: tokio::sync::Mutex::new(writer),
            waiters: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            healthy: AtomicBool::new(true),
            stopped: CancellationToken::new(),
            permits: Semaphore::new(32),
            timeout: operation_timeout,
            events,
            observed_events,
            value_write_floors: Mutex::new(HashMap::new()),
            reader: Mutex::new(None),
        });
        // Weak ownership prevents client -> task -> client reference cycles.
        let weak = Arc::downgrade(&inner);
        let stopped = inner.stopped.clone();
        let task = tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            loop {
                let line = tokio::select! {
                    biased;
                    _ = stopped.cancelled() => break,
                    line = simulator::read_line(&mut reader) => line,
                };
                let Some(inner) = weak.upgrade() else { break };
                let frame = line
                    .ok()
                    .and_then(|line| serde_json::from_slice::<Frame>(&line).ok());
                match frame {
                    Some(Frame::Reply {
                        id,
                        ok,
                        value,
                        revision,
                    }) => {
                        let waiter = inner.waiters.lock().expect("waiters mutex").remove(&id);
                        if let Some(waiter) = waiter {
                            if ok && let Some(identity) = waiter.value_write {
                                let mut floors =
                                    inner.value_write_floors.lock().expect("write floors mutex");
                                if floors.len() >= 1024 && !floors.contains_key(&identity) {
                                    drop(floors);
                                    let _ = waiter.sender.send(Err(Error::Protocol));
                                    inner.retire(Error::Protocol);
                                    break;
                                }
                                // Install before waking the handler. Missing metadata
                                // cannot prove freshness after a confirmed write.
                                let floor = floors.entry(identity).or_default();
                                *floor = (*floor).max(revision.unwrap_or(u64::MAX));
                            }
                            let result = if ok {
                                Ok(value)
                            } else {
                                serde_json::from_value(value)
                                    .map(|failure| Err(Error::Rejected(failure)))
                                    .unwrap_or(Err(Error::Protocol))
                            };
                            let _ = waiter.sender.send(result);
                        } else {
                            inner.retire(Error::Protocol);
                            break;
                        }
                    }
                    Some(Frame::Event { value, revision }) => {
                        match serde_json::from_value::<Event>(value) {
                            Ok(event) => {
                                let _ = inner.observed_events.send(ObservedEvent {
                                    revision,
                                    event: event.clone(),
                                });
                                let _ = inner.events.send(event);
                            }
                            Err(_) => {
                                inner.retire(Error::Protocol);
                                break;
                            }
                        }
                    }
                    None => {
                        inner.retire(Error::Unavailable);
                        break;
                    }
                }
            }
            if let Some(inner) = weak.upgrade() {
                inner.retire(Error::Unavailable);
                // Writer waits observe stopped, so shutdown cannot sit behind an
                // unbounded write. Close the peer even while client clones exist.
                let _ = inner.writer.lock().await.shutdown().await;
            }
        });
        *inner.reader.lock().expect("reader mutex") = Some(task);
        let client = Self { inner };
        let health = client.rpc("health", Value::Null).await?;
        if health.get("version").and_then(Value::as_u64) != Some(u64::from(simulator::VERSION)) {
            client.close().await;
            return Err(Error::Protocol);
        }
        Ok(client)
    }

    pub fn is_healthy(&self) -> bool {
        self.inner.healthy.load(Ordering::Acquire)
    }

    /// A transport-health request, not a parameter polling loop. Detects a stalled
    /// target even when no Panel is currently consuming dynamic values.
    pub async fn probe(&self) -> Result<(), Error> {
        self.rpc("health", Value::Null).await.map(|_| ())
    }

    /// Lag is explicit in broadcast::Receiver::recv; consumers must resynchronize.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn subscribe_observed(&self) -> broadcast::Receiver<ObservedEvent> {
        self.inner.observed_events.subscribe()
    }

    /// Check at actual dispatch, including after causal-response delivery and any
    /// internal relay queue. False means close/resynchronize, NOT silently drop.
    /// Reads never advance floors: older GPI observations and alarm transitions
    /// must not be erased merely because a newer snapshot was requested.
    pub fn event_is_current(&self, observation: &ObservedEvent) -> bool {
        let Event::Values { location, values } = &observation.event else {
            return true;
        };
        let floors = self
            .inner
            .value_write_floors
            .lock()
            .expect("write floors mutex");
        values.keys().all(|id| {
            floors
                .get(&(location.clone(), id.clone()))
                .is_none_or(|floor| observation.revision >= *floor)
        })
    }

    pub async fn close(&self) {
        self.inner.retire(Error::Unavailable);
        let reader = self.inner.reader.lock().expect("reader mutex").take();
        if let Some(reader) = reader {
            let _ = reader.await;
        }
    }

    async fn rpc(&self, command: &str, args: Value) -> Reply {
        let _permit = self.inner.permits.try_acquire().map_err(|_| Error::Busy)?;
        if !self.is_healthy() {
            return Err(Error::Unavailable);
        }
        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        if id == u64::MAX {
            self.inner.retire(Error::Protocol);
            return Err(Error::Protocol);
        }
        let value_write = if command == "set_value" {
            Some((
                serde_json::from_value(args.get("location").cloned().ok_or(Error::Protocol)?)
                    .map_err(|_| Error::Protocol)?,
                args.get("id")
                    .and_then(Value::as_str)
                    .ok_or(Error::Protocol)?
                    .to_owned(),
            ))
        } else {
            None
        };
        let bytes = simulator::encode(&Request {
            version: simulator::VERSION,
            id,
            command: command.into(),
            args,
        })
        .map_err(|_| Error::Protocol)?;
        let (tx, rx) = oneshot::channel();
        self.inner.waiters.lock().expect("waiters mutex").insert(
            id,
            Waiter {
                sender: tx,
                value_write,
            },
        );
        let mut transaction = Transaction {
            inner: self.inner.clone(),
            id,
            transmitted: false,
            complete: false,
        };
        let result = tokio::time::timeout(self.inner.timeout, async {
            tokio::select! {
                biased;
                _ = self.inner.stopped.cancelled() => Err(Error::Unavailable),
                result = async {
                    let mut writer = self.inner.writer.lock().await;
                    transaction.transmitted = true;
                    writer.write_all(&bytes).await.map_err(|_| Error::Unavailable)?;
                    drop(writer);
                    rx.await.map_err(|_| Error::Unavailable)?
                } => result,
            }
        })
        .await;
        match result {
            Ok(result) => {
                // Correlated rejection is definitive; transport failure is not.
                transaction.complete = result.is_ok() || matches!(result, Err(Error::Rejected(_)));
                result
            }
            Err(_) => {
                self.inner.retire(Error::Timeout);
                Err(Error::Timeout)
            }
        }
    }

    async fn operation<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        args: Value,
    ) -> Result<T, Failure> {
        let value = self.rpc(command, args).await.map_err(|error| match error {
            Error::Rejected(failure) => failure,
            Error::Busy => Failure::Overloaded,
            Error::Timeout if matches!(command, "set_value" | "configure") => Failure::Ambiguous,
            Error::Timeout => Failure::Timeout,
            Error::Unavailable if matches!(command, "set_value" | "configure") => {
                Failure::Ambiguous
            }
            Error::Unavailable => Failure::Unavailable,
            Error::Protocol => Failure::Internal,
        })?;
        serde_json::from_value(value).map_err(|_| {
            self.inner.retire(Error::Protocol);
            Failure::Internal
        })
    }
}

impl Device for Client {
    async fn read_values(&self, location: Location) -> Result<Values, Failure> {
        self.operation("read_values", serde_json::json!({"location":location}))
            .await
    }
    async fn set_value(
        &self,
        location: Location,
        id: String,
        value: Value,
    ) -> Result<Value, Failure> {
        self.operation(
            "set_value",
            serde_json::json!({"location":location,"id":id,"value":value}),
        )
        .await
    }
    async fn alarms(&self) -> Result<Vec<Alarm>, Failure> {
        self.operation("alarms", Value::Null).await
    }
    async fn sender_sdp(&self, key: PortKey) -> Result<Option<String>, Failure> {
        self.operation("sender_sdp", serde_json::json!({"key":key}))
            .await
    }
    async fn configure(&self, key: PortKey, state: Option<MediaState>) -> Result<(), Failure> {
        self.operation("configure", serde_json::json!({"key":key,"state":state}))
            .await
    }
    async fn configuration(&self, key: PortKey) -> Result<Option<MediaState>, Failure> {
        self.operation("configuration", serde_json::json!({"key":key}))
            .await
    }
}
