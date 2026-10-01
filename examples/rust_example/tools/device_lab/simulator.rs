//! A stand-in for physical equipment so API lessons can run without hardware.
//! State lives here, separately from the driver, to demonstrate independent
//! readback, front-panel-like changes and faults. Neither this Unix protocol nor
//! its revision scheme belongs in SPEC-23. A real driver uses the vendor API
//! instead; storing modeled FEC/SRT state here does not implement those transports.
use crate::{
    domain::*,
    registry::{Access, Registry},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use std::{io, path::Path, time::Duration};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{Semaphore, broadcast, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

pub(crate) const MAX_FRAME: usize = 256 * 1024;
pub(crate) const IO_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const VERSION: u8 = 1;

#[derive(thiserror::Error)]
pub enum Error {
    #[error("simulator transport I/O failed")]
    Io(#[from] io::Error),
    #[error("invalid simulator frame")]
    Json(#[from] serde_json::Error),
    #[error("simulator operation timed out")]
    Timeout,
    #[error("simulator rejected operation")]
    Rejected,
}

// serde's error text may quote an invalid enum value from the input. Even Debug
// stays sanitized so a CLI or task panic cannot accidentally print that payload.
impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

/// IDs belong to this stream generation, not a VideoIPath session.
#[derive(Serialize, Deserialize)]
pub(crate) struct Request {
    pub version: u8,
    pub id: u64,
    pub command: String,
    pub args: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Frame {
    Reply {
        id: u64,
        ok: bool,
        value: Value,
        #[serde(default)]
        revision: Option<u64>,
    },
    Event {
        value: Value,
        #[serde(default)]
        revision: u64,
    },
}

/// Unlike unbounded read_until, cap allocation before copying buffered chunks.
pub(crate) async fn read_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    loop {
        let chunk = reader.fill_buf().await?;
        if chunk.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "closed stream",
            ));
        }
        let end = chunk.iter().position(|byte| *byte == b'\n');
        let count = end.map_or(chunk.len(), |index| index + 1);
        if bytes.len() + count > MAX_FRAME {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame limit"));
        }
        bytes.extend_from_slice(&chunk[..count]);
        reader.consume(count);
        if end.is_some() {
            return Ok(bytes);
        }
    }
}

pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>, Error> {
    let mut bytes = serde_json::to_vec(value)?;
    if bytes.len() >= MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "frame limit").into());
    }
    bytes.push(b'\n');
    Ok(bytes)
}

async fn send(writer: &mut (impl AsyncWrite + Unpin), frame: &Frame) -> Result<(), Error> {
    let bytes = encode(frame)?;
    tokio::time::timeout(IO_TIMEOUT, writer.write_all(&bytes))
        .await
        .map_err(|_| Error::Timeout)??;
    Ok(())
}

/// Bounded one-shot CLI control; errors never include secret-bearing payloads.
pub async fn control(path: &Path, command: &str, args: &[String]) -> Result<Value, Error> {
    let request = Request {
        version: VERSION,
        id: 1,
        command: command.into(),
        args: serde_json::to_value(args)?,
    };
    let bytes = encode(&request)?;
    tokio::time::timeout(IO_TIMEOUT, async {
        let mut stream = tokio::net::UnixStream::connect(path).await?;
        stream.write_all(&bytes).await?;
        let mut reader = tokio::io::BufReader::new(stream);
        match serde_json::from_slice::<Frame>(&read_line(&mut reader).await?)? {
            Frame::Reply {
                id: 1,
                ok: true,
                value,
                ..
            } => Ok(value),
            _ => Err(Error::Rejected),
        }
    })
    .await
    .map_err(|_| Error::Timeout)?
}

/// All changes are atomic under one short synchronous lock; no socket or sleep
/// is awaited while owning it. State outlives every driver connection.
struct State {
    registry: Registry,
    values: BTreeMap<Location, Values>,
    configurations: BTreeMap<PortKey, MediaState>,
    alarms: BTreeMap<(Location, String), Alarm>,
    revision: u64,
    actions: u64,
    delay_next: Duration,
    fail_next: Option<Failure>,
    pending_delays: Arc<AtomicUsize>,
    connections: CancellationToken,
}

impl State {
    fn new() -> Self {
        let registry = Registry::new();
        let values = registry
            .locations()
            .into_iter()
            .map(|location| {
                let defaults = registry.defaults(&location);
                (location, defaults)
            })
            .collect();
        Self {
            registry,
            values,
            configurations: BTreeMap::new(),
            alarms: BTreeMap::new(),
            revision: 0,
            actions: 0,
            delay_next: Duration::ZERO,
            fail_next: None,
            pending_delays: Arc::new(AtomicUsize::new(0)),
            connections: CancellationToken::new(),
        }
    }

    fn set(
        &mut self,
        location: Location,
        id: String,
        value: Value,
        external: bool,
    ) -> Result<(Value, Option<Event>), Failure> {
        let parameter = self
            .registry
            .parameter(&location, &id)
            .ok_or(Failure::Unknown)?;
        if !external && parameter.access == Access::ReadOnly {
            return Err(Failure::ReadOnly);
        }
        parameter.validate(&value)?;
        if parameter.access == Access::WriteOnly {
            // A trigger is an action, not remembered readable state. Count every
            // accepted invocation (even equal inputs) so no-replay is observable.
            self.actions = self.actions.checked_add(1).ok_or(Failure::Overloaded)?;
            self.revision += 1;
            return Ok((value, None));
        }
        let values = self.values.get_mut(&location).ok_or(Failure::Unknown)?;
        let changed = values.get(&id) != Some(&value);
        values.insert(id.clone(), value.clone());
        if changed {
            self.revision += 1;
        }
        let event = (external && changed).then(|| Event::Values {
            location,
            values: [(id, value.clone())].into(),
        });
        Ok((value, event))
    }

    fn configure(&mut self, key: PortKey, state: Option<MediaState>) -> Result<(), Failure> {
        let port = self.registry.port(&key)?;
        if port["topology_info"].is_null() {
            return Err(Failure::Unsupported);
        }
        if let Some(state) = &state {
            // Domain fields, not an opaque request cache: this is the target's
            // modeled routing/format state and is independently mutable by simctl.
            for stream in std::iter::once(&state.main).chain(state.spare.iter()) {
                if stream.udp_port == 0
                    || stream.vlan.is_some_and(|v| v > 4094)
                    || !self.registry.interface_exists(&stream.interface)
                {
                    return Err(Failure::Invalid);
                }
            }
            if let Some(partner) = &state.partner {
                self.registry.port(partner)?;
            }
            if state.format.attributes.len() > 64
                || state.format.audio_pairs.len() > 8
                || state.sdp.as_ref().is_some_and(|s| s.len() > 16_384)
                || serde_json::to_vec(state)
                    .map_err(|_| Failure::Internal)?
                    .len()
                    > 32_768
            {
                return Err(Failure::Overloaded);
            }
            if !self.configurations.contains_key(&key) && self.configurations.len() >= 128 {
                return Err(Failure::Overloaded);
            }
        }
        if self.configurations.get(&key) != state.as_ref() {
            match state {
                Some(state) => {
                    self.configurations.insert(key, state);
                }
                None => {
                    self.configurations.remove(&key);
                }
            }
            self.revision += 1;
        }
        Ok(())
    }

    fn operation(&mut self, command: &str, args: Value) -> Result<Value, Failure> {
        match command {
            "health" => Ok(serde_json::json!({"version":VERSION})),
            "read_values" => {
                let location: Location = field(&args, "location")?;
                if !self.registry.known_location(&location) {
                    return Err(Failure::Unknown);
                }
                let mut values = self.values.get(&location).cloned().unwrap_or_default();
                values.retain(|id, _| {
                    self.registry
                        .parameter(&location, id)
                        .is_some_and(|p| p.access != Access::WriteOnly)
                });
                to_value(values)
            }
            "set_value" => {
                let (value, _) = self.set(
                    field(&args, "location")?,
                    field(&args, "id")?,
                    field(&args, "value")?,
                    false,
                )?;
                // Never emit here: the SPEC handler owns response-before-event.
                Ok(value)
            }
            "alarms" => to_value(self.alarms.values().collect::<Vec<_>>()),
            "configure" => {
                self.configure(field(&args, "key")?, field(&args, "state")?)?;
                Ok(Value::Null)
            }
            "configuration" => {
                let key: PortKey = field(&args, "key")?;
                self.registry.port(&key)?;
                to_value(self.configurations.get(&key))
            }
            "sender_sdp" => {
                let key: PortKey = field(&args, "key")?;
                let port = self.registry.port(&key)?;
                if key.direction != Direction::Sender
                    || port["topology_info"]["sdp_support"] != true
                {
                    return Err(Failure::Unsupported);
                }
                let sdp = self
                    .configurations
                    .get(&key)
                    .map(crate::sdp::sender_sdp)
                    .transpose()?;
                to_value(sdp)
            }
            _ => Err(Failure::Unsupported),
        }
    }

    fn control(
        &mut self,
        command: &str,
        args: &[String],
    ) -> Result<(Value, Option<Event>), Failure> {
        let mut event = None;
        let response = match (command, args.len()) {
            ("status" | "inspect", 0) => {
                serde_json::json!({"version":VERSION,"revision":self.revision,
                "action_count":self.actions,"configured_streams":self.configurations.len(),"active_alarms":self.alarms.len(),
                "delay_pending_ms":self.delay_next.as_millis(),"failure_pending":self.fail_next.is_some(),
                "pending_delays":self.pending_delays.load(Ordering::Acquire)})
            }
            ("set", 3) => {
                let (_, changed) =
                    self.set(parse(&args[0])?, args[1].clone(), parse(&args[2])?, true)?;
                event = changed;
                serde_json::json!({"changed":event.is_some()})
            }
            ("raise", 1) => {
                let alarm: Alarm = parse(&args[0])?;
                if alarm.id.is_empty()
                    || alarm.id.len() > 256
                    || alarm.details.len() > 2048
                    || !self.registry.known_location(&alarm.location)
                {
                    return Err(Failure::Invalid);
                }
                let key = (alarm.location.clone(), alarm.id.clone());
                if !self.alarms.contains_key(&key) && self.alarms.len() >= 128 {
                    return Err(Failure::Overloaded);
                }
                let changed = self.alarms.get(&key) != Some(&alarm);
                if changed {
                    // Count actual escaped JSON and the largest reply envelope.
                    // Admission must never create a state we cannot fully poll;
                    // replacement is checked before mutating the old snapshot.
                    let candidate: Vec<_> = self
                        .alarms
                        .iter()
                        .filter(|(existing, _)| *existing != &key)
                        .map(|(_, alarm)| alarm)
                        .chain(std::iter::once(&alarm))
                        .collect();
                    let snapshot = reply(u64::MAX, to_value(candidate), Some(u64::MAX));
                    encode(&snapshot).map_err(|_| Failure::Overloaded)?;
                    self.alarms.insert(key, alarm.clone());
                    self.revision += 1;
                    event = Some(Event::Alarm { alarm });
                }
                serde_json::json!({"changed":changed})
            }
            ("clear", 2) => {
                let location: Location = parse(&args[0])?;
                let id = args[1].clone();
                let changed = self
                    .alarms
                    .remove(&(location.clone(), id.clone()))
                    .is_some();
                if changed {
                    self.revision += 1;
                    event = Some(Event::AlarmCleared { location, id });
                }
                serde_json::json!({"changed":changed})
            }
            ("poll-alarms", 0) => {
                event = Some(Event::PollAlarms);
                Value::Null
            }
            ("drift", 1 | 2) => {
                // Omitting state disables this modeled stream. Supplying a domain
                // state applies it just as a front-panel change would; checks read
                // this actual state rather than any remembered driver request.
                self.configure(
                    parse(&args[0])?,
                    if args.len() == 2 {
                        parse(&args[1])?
                    } else {
                        None
                    },
                )?;
                Value::Null
            }
            ("disconnect", 0) => {
                self.connections.cancel();
                self.connections = CancellationToken::new();
                Value::Null
            }
            ("delay" | "delay-next", 1) => {
                let millis: u64 = args[0].parse().map_err(|_| Failure::Invalid)?;
                if millis > 10_000 {
                    return Err(Failure::Range);
                }
                self.delay_next = Duration::from_millis(millis);
                Value::Null
            }
            ("fail-next", 0) => {
                self.fail_next = Some(Failure::Unavailable);
                Value::Null
            }
            _ => return Err(Failure::Invalid),
        };
        Ok((response, event))
    }
}

fn parse<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Failure> {
    serde_json::from_str(text).map_err(|_| Failure::Invalid)
}
fn field<T: serde::de::DeserializeOwned>(value: &Value, key: &str) -> Result<T, Failure> {
    serde_json::from_value(value.get(key).cloned().ok_or(Failure::Invalid)?)
        .map_err(|_| Failure::Invalid)
}
fn to_value(value: impl Serialize) -> Result<Value, Failure> {
    serde_json::to_value(value).map_err(|_| Failure::Internal)
}

struct Shared {
    state: Mutex<State>,
    events: broadcast::Sender<(u64, Event)>,
}

fn reply(id: u64, result: Result<Value, Failure>, revision: Option<u64>) -> Frame {
    match result {
        Ok(value) => Frame::Reply {
            id,
            ok: true,
            value,
            revision,
        },
        Err(error) => Frame::Reply {
            id,
            ok: false,
            value: serde_json::to_value(error).expect("failure enum"),
            revision,
        },
    }
}

async fn connection(stream: UnixStream, shared: Arc<Shared>, control: bool) -> Result<(), Error> {
    let (reader, mut writer) = stream.into_split();
    let mut events = shared.events.subscribe();
    let closed = CancellationToken::new();
    let (requests, mut incoming) = mpsc::channel(32);
    // The sole reader keeps observing EOF even while the operation owner is in a
    // pre-apply delay. Its queue is bounded, with overload closing the generation
    // rather than blocking EOF detection behind an unbounded pipeline.
    let mut readers = JoinSet::new();
    readers.spawn({
        let closed = closed.clone();
        async move {
            let mut reader = tokio::io::BufReader::new(reader);
            loop {
                let line =
                    match tokio::time::timeout(Duration::from_secs(60), read_line(&mut reader))
                        .await
                    {
                        Ok(Ok(line)) => line,
                        _ => break,
                    };
                let request = match serde_json::from_slice::<Request>(&line) {
                    Ok(request) if request.version == VERSION && request.id != 0 => request,
                    _ => break,
                };
                if requests.try_send(request).is_err() {
                    break;
                }
            }
            closed.cancel();
        }
    });
    // JoinSet abort-on-drop owns the reader on every error/cancellation exit.
    loop {
        let request = loop {
            tokio::select! {
                biased;
                _ = closed.cancelled() => return Err(Error::Rejected),
                request = incoming.recv() => break request.ok_or(Error::Rejected)?,
                event = events.recv(), if !control => {
                    // Never silently drop alarm transitions on a slow connection.
                    let (revision, event) = event.map_err(|_| Error::Rejected)?;
                    send(&mut writer,&Frame::Event{value:serde_json::to_value(event)?,revision}).await?;
                }
            }
        };
        if control {
            let result = serde_json::from_value::<Vec<String>>(request.args)
                .map_err(|_| Failure::Invalid)
                .and_then(|args| {
                    let mut state = shared.state.lock().expect("state mutex");
                    let (value, event) = state.control(&request.command, &args)?;
                    // Publish under the state lock to preserve mutation order.
                    if let Some(event) = event {
                        let _ = shared.events.send((state.revision, event));
                    }
                    Ok(value)
                });
            send(&mut writer, &reply(request.id, result, None)).await?;
            return Ok(());
        }
        let (delay, failure, pending) = {
            let mut state = shared.state.lock().expect("state mutex");
            let delay = std::mem::take(&mut state.delay_next);
            let pending = (!delay.is_zero()).then(|| {
                state.pending_delays.fetch_add(1, Ordering::AcqRel);
                PendingDelay(state.pending_delays.clone())
            });
            (delay, state.fail_next.take(), pending)
        };
        if !delay.is_zero() {
            tokio::select! {
                biased;
                _ = closed.cancelled() => return Err(Error::Rejected),
                _ = tokio::time::sleep(delay) => {},
            }
        }
        let (result, revision) = {
            let mut state = shared.state.lock().expect("state mutex");
            if closed.is_cancelled() {
                return Err(Error::Rejected);
            }
            let result = if let Some(failure) = failure {
                Err(failure)
            } else {
                state.operation(&request.command, request.args)
            };
            (result, state.revision)
        };
        drop(pending);
        // Revision and result were observed atomically under the state lock.
        send(&mut writer, &reply(request.id, result, Some(revision))).await?;
    }
}

struct PendingDelay(Arc<AtomicUsize>);
impl Drop for PendingDelay {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Cleanup only our inode, never a replacement socket or a user's existing file.
struct SocketFile {
    path: PathBuf,
    inode: u64,
    device: u64,
    uid: u32,
}
impl Drop for SocketFile {
    fn drop(&mut self) {
        if std::fs::symlink_metadata(&self.path).is_ok_and(|m| {
            m.ino() == self.inode && m.dev() == self.device && m.file_type().is_socket()
        }) {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
fn bind(path: &Path) -> Result<(UnixListener, SocketFile), Error> {
    let listener = UnixListener::bind(path)?; // Refuses pre-existing paths.
    let metadata = std::fs::symlink_metadata(path)?;
    let file = SocketFile {
        path: path.into(),
        inode: metadata.ino(),
        device: metadata.dev(),
        uid: metadata.uid(),
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, file))
}

/// A private directory plus a held OS lock permits safe recovery after hard kills.
/// File existence is never treated as ownership, and a live old-version socket is
/// protected even if its process predates this locking convention.
async fn socket_owner(path: &Path, uid: u32) -> Result<std::fs::File, Error> {
    let parent = path.parent().ok_or(Error::Rejected)?;
    let directory = std::fs::metadata(parent)?;
    if directory.uid() != uid || directory.mode() & 0o022 != 0 {
        return Err(Error::Rejected);
    }
    let lock_path = path.with_extension("owner.lock");
    if let Ok(metadata) = std::fs::symlink_metadata(&lock_path)
        && (!metadata.file_type().is_file() || metadata.uid() != uid)
    {
        return Err(Error::Rejected);
    }
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(lock_path)?;
    lock.try_lock().map_err(io::Error::other)?;
    let before = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(lock),
        Err(e) => return Err(e.into()),
    };
    if !before.file_type().is_socket() || before.uid() != uid {
        return Err(Error::Rejected);
    }
    match tokio::time::timeout(Duration::from_millis(250), UnixStream::connect(path)).await {
        Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => {}
        // Success, timeout, permission error, and all other ambiguous states are
        // NOT proof that the old owner is gone. Never replace them automatically.
        _ => return Err(Error::Rejected),
    }
    let after = std::fs::symlink_metadata(path)?;
    if after.ino() != before.ino() || after.dev() != before.dev() || !after.file_type().is_socket()
    {
        return Err(Error::Rejected);
    }
    std::fs::remove_file(path)?;
    Ok(lock)
}

/// Own both sockets, independent state, and every accepted task. No detached
/// connection survives shutdown; admission has separate target/control budgets.
pub async fn serve(
    socket: &Path,
    control_socket: &Path,
    shutdown: CancellationToken,
) -> Result<(), Error> {
    // A socketpair provides kernel identity without unsafe code or another crate.
    let (identity, _peer) = UnixStream::pair()?;
    let uid = identity.peer_cred()?.uid();
    let _target_owner = socket_owner(socket, uid).await?;
    let _control_owner = socket_owner(control_socket, uid).await?;
    let (target, target_file) = bind(socket)?;
    let (control, _control_file) = bind(control_socket)?;
    let (events, _) = broadcast::channel(128);
    let shared = Arc::new(Shared {
        state: Mutex::new(State::new()),
        events,
    });
    let target_budget = Arc::new(Semaphore::new(16));
    let control_budget = Arc::new(Semaphore::new(8));
    let mut tasks = JoinSet::new();
    let result = loop {
        let (accepted, is_control) = tokio::select! {
            biased;
            _ = shutdown.cancelled() => break Ok(()),
            Some(_) = tasks.join_next(), if !tasks.is_empty() => continue,
            accepted = target.accept() => (accepted,false),
            accepted = control.accept() => (accepted,true),
        };
        let (stream, _) = match accepted {
            Ok(pair) => pair,
            Err(error) => break Err(error.into()),
        };
        // Kernel credentials close even the tiny bind-to-chmod window. The two
        // processes deliberately share a non-root UID, not an unauthenticated API.
        if !stream
            .peer_cred()
            .is_ok_and(|peer| peer.uid() == target_file.uid)
        {
            drop(stream);
            continue;
        }
        let budget = if is_control {
            &control_budget
        } else {
            &target_budget
        };
        let Ok(permit) = budget.clone().try_acquire_owned() else {
            drop(stream);
            continue;
        };
        let shared = shared.clone();
        let generation = if is_control {
            CancellationToken::new()
        } else {
            shared
                .state
                .lock()
                .expect("state mutex")
                .connections
                .clone()
        };
        let shutdown = shutdown.clone();
        tasks.spawn(async move {
            let _permit = permit;
            tokio::select! {
                biased;
                _ = shutdown.cancelled() => {},
                _ = generation.cancelled() => {},
                _ = connection(stream,shared,is_control) => {},
            }
        });
    };
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    result
}
