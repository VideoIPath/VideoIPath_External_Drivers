# Design: a driver with an independent target

This example teaches the boundary between VideoIPath requests and device state.
The simulator is a separate process. It models parameters, alarms and media
configuration; it does not transmit, receive, encode, encrypt or protect media.

The implementation follows the later SPEC-23 v1 forms: `/x-driver/v1`, integer
request IDs, array-based IPT batches and `async_values_changed`. No live VideoIPath
instance, release-specific schema, backend SHA or accepted release capture was
available for this example. Local execution is evidence about this implementation,
not commissioning of a VideoIPath release. See [coverage](docs/COVERAGE.md).

## Boundaries and dependency direction

```text
VideoIPath (WebSocket server)
        |
    session.rs  -- one socket generation, one writer, bounded handlers
        |
    protocol.rs -- SPEC envelope and supported-field translation
        |
 application.rs -- capability rules, readback, deferred changes
        |
    domain.rs   -- Device contract and modeled state
        ^
    device.rs   -- Unix-stream client adapter
        |
 simulator.rs  -- separate process, independent volatile state

main.rs assembles the concrete pieces; registry.rs supplies immutable definitions.
```

| File | Responsibility | Important boundary |
| --- | --- | --- |
| [domain.rs](src/domain.rs) | Locations, failures, values, alarms, normalized media state and `Device` | No WebSocket or Unix-socket dependency |
| [registry.rs](src/registry.rs) | Definitions, defaults, lookup, validators and structure | Identity is `(location, local_id)`, not a flattened UI address |
| [protocol.rs](src/protocol.rs) | SPEC DTO decoding, response/event frames, media validation | Discards unknown/irrelevant fields before comparison |
| [application.rs](src/application.rs) | Fresh reads, sets, alarms, SDP, IPT application/checks | Calls `Device`; returns response and deferred causal changes |
| [device.rs](src/device.rs) | Private simulator RPC, waiters, events and transport retirement | Exactly one reader; no write replay |
| [simulator.rs](src/simulator.rs) | Target and control sockets; modeled values/configuration/alarms | Does not accept SPEC frames or contact VideoIPath |
| [session.rs](src/session.rs) | Keepalive, dispatch, outbound delivery, generation shutdown | Only its writer task writes the WebSocket |
| [main.rs](src/main.rs) | Startup, supervision, reconnect and concrete wiring | Owns the node lock across reconnects |

`Registry::new()` currently builds **one comprehensive fixture**. There is no
scenario-selection flag, plugin loader or runtime personality switch. Scenarios
in [SCENARIOS.md](docs/SCENARIOS.md) change that fixture's state. Structural changes
are code changes and require deliberate identity/Inventory migration. The startup
setting `DRIVER_CONFIGURATION_ORDER` selects `senders_first` (default),
`receivers_first` or `all` within that same fixture; it does not select a different
device personality.

## Ownership, concurrency and lifetime

| Owner | Owned state | Rule |
| --- | --- | --- |
| `Config` / composition root | Validated settings, TLS connector, registry, `NodeLock` | Resolve before opening the backend connection |
| `NodeLock` | Open `std::fs::File` with `try_lock()` | Keep the guard alive for the process; file existence is not ownership |
| Supervisor | Current target client, WebSocket session, reconnect attempt | Close/drain the old generation before replacement |
| Session writer | Socket sink, bounded normal/urgent queues | A successful queue send is not delivery; handlers await a oneshot acknowledgement |
| Session dispatcher | Bounded `JoinSet` and device `RwLock` | Reads share a lane; mutations hold exclusive ownership through response and causal-event delivery |
| Adapter `Inner` | One reader, serialized writer, bounded waiters, event rings and per-identity write revisions | Register waiter before writing; install confirmed-write revision before waking caller; retirement fails all waiters |
| Adapter `Transaction` | Request ID and potentially-transmitted state | Cancellation after transmission begins retires the stream |
| Simulator `State` | Values, configurations, alarms, action count and revision | Short synchronous mutex sections; no I/O or `.await` while locked |

The adapter reader holds a weak reference to shared state to avoid an ownership
cycle. Explicit `close()` cancels and joins it; dropping the last client is a
fallback. The simulator owns accepted connection tasks and drains them at shutdown.

The session's exclusive lane covers dynamic writes, IPT batches and checks with
`set_again: true`, including response and deferred-change delivery. Other requests
share the read lane; keepalive bypasses it. Waiting for the lane consumes the
handler budget. This prevents two backend mutation transactions from interleaving
their baseline, apply, readback and causal publication.

Independent control-socket changes still happen outside that lane. Simulator
replies/events carry private revisions. The adapter records the latest successful
value-write revision for each `(location, local_id)` before waking its waiter.
`subscribe_observed()` retains event revisions through the relay queue, and
`run_guarded()` checks them at actual dispatch. A value event older than its own
confirmed-write floor retires the session. Reads never advance floors; unrelated
GPI observations and alarm transitions are not erased by a newer snapshot.

The relay's `_driver_revision` is removed before WebSocket serialization. If an
independent event cannot acquire the read lane while a mutation owns or is waiting
for it, the session also retires rather than guessing event order. The supervisor
reconnects so the backend can recover authoritative state. This is **fail-closed
conflict detection**, not complete event-history reconciliation, replay or an
exactly-once guarantee. Revisions cannot undo an already-applied write.

## Trace one write

For the complete frames see [PROTOCOL.md](docs/PROTOCOL.md#dynamic-values).

1. The session accepts an integer-ID `set_dynamic_values` frame within its size
   and admission limits and acquires the mutation lane. It keeps keepalive handling
   separate from device I/O. An `i128` DTO preserves both i64 and u64 JSON IDs.
2. The handler decodes `location` and `values`, validates the location and obtains
   one fresh baseline snapshot from the target.
3. Registry lookup checks each `(location, id)`, access mode, JSON type, bounds
   and numeric step. Invalid entries become direct failed-ID map entries.
4. The adapter registers a waiter, then sends a private `set_value` operation.
   The simulator updates its independent state and replies with observed state.
   The adapter installs the reply's per-parameter revision floor before waking
   the handler. Driver-originated writes do not also generate a simulator causal event.
5. The handler compares observed and requested values. A normalized mismatch is
   reported as failure while the actual observed change can still be published.
   A write-only action has acknowledgement, but no fabricated readable value.
6. `Outcome` carries the correlated response plus changed values. The session
   awaits the writer's response-delivery acknowledgement before sending
   `async_values_changed` with a null ID, then releases the mutation lane.

An independent `simctl set` follows a different route: simulator event → adapter
reader → revision-preserving bounded relay → dispatch freshness/lane check →
session writer. Repeating an equal modeled value does not generate another event.
The supervisor subscribes before startup reads and the backend handshake. During
an established session it sends a lightweight target `health` probe every five
seconds; this checks transport progress, not parameters, and adds no sensor-poll loop.

## Budgets and failure behavior

These are implementation defaults, not benchmark results or latency guarantees.
Environment bounds are defined in [config.rs](src/config.rs).

| Resource | Default/current bound | Failure behavior |
| --- | --- | --- |
| WebSocket frame/message | 1 MiB configurable | Oversize closes the session |
| Ordinary admitted handlers | 16 configurable | Correlated overload error if deliverable |
| Normal and urgent outbound queues | 128 each configurable | Saturation/failed delivery retires the generation |
| Handler work, including lane wait | 5 s ordinary; 20 s IPT batch | Correlated timeout; no blind write retry |
| Response/event delivery | 1 s per delivery; socket send also bounded | Failed delivery closes the generation |
| Target operation | 3 s, including writer wait | Timeout retires the target stream |
| Target client concurrency | 32 operations | Admission fails with capacity error |
| Dynamic map/IPT batch | 256 total items | Whole-request capacity error |
| Simulator frame | 256 KiB including LF | Connection rejects oversize input |
| Simulator connections | 16 target, 8 control | Additional connections close |
| Simulator state | 128 configured streams; up to 128 alarms whose escaped snapshot fits one frame | Capacity rejection preserves the previous state |
| Event rings | 128 entries | Lag is lost synchronization, triggering recovery |
| Value-write revision floors | 1024 scoped identities per client generation | Overflow retires the stream |
| Simulator request pipeline | 32 queued requests per connection | Overflow closes the connection |
| Inbound backend idle | 60 s | Session closes; transport pings do not refresh it |

Handler work and outbound delivery currently have separate budgets; this is not
one end-to-end absolute deadline. IPT loops are sequential within a batch and
confirm each successful configuration through a separate readback. The advertised
startup order controls sender/receiver processing. `all` accepts both directions
in one batch but still uses sequential adapter calls; it is not atomic or parallel
device application. Cross-module scheduling belongs to VideoIPath.

| Failure | Handling | State implication |
| --- | --- | --- |
| Invalid field with usable ID | Correlated `error`, or documented item failure | No success claim for the invalid operation |
| Invalid JSON/unusable request ID | Close | Never invent a correlation ID |
| Incoming `error` | Sanitized log, no reply | Prevents an error-response loop |
| Unknown dynamic ID on read/write | Read omits it; write reports `Unknown parameter` | Other valid writes can succeed |
| Lost target / ambiguous write | Retire stream; supervisor recovers | Applied state may survive; no rollback or action replay implied |
| IPT drift | Compare actual state; optionally reapply and reread | `true` requires observed agreement |
| Authentication/URL/TLS failure | Permanent classification exits supervisor | No reconnect loop for known permanent failure |
| Transient connection failure | Capped exponential backoff with jitter | New session generation; no queued write replay |
| Stale or concurrent independent event | Close and reconnect for resynchronization | No silent drop, guessed ordering or fabricated rollback |

## Rust choices and teaching requirements

- Use the stable toolchain pinned by this package and Rust 2024 idioms. The
  `Device` trait uses return-position `impl Future + Send`; implementations use
  native async methods. No `async-trait` dependency or per-device dynamic dispatch
  is needed. The session callback uses a boxed future at its explicit test seam.
- Use standard-library file locking for same-host singleton ownership. It does
  not coordinate independent hosts; all local instances need the same lock mount.
- Use bounded Tokio channels, semaphores and owned task sets. A task-per-frame
  design without admission control is not an acceptable extension.
- Keep brief state-mutex sections synchronous. Use async synchronization where
  ownership spans I/O: the adapter writer mutex and session transaction `RwLock`.
  Cancellation and ambiguous outcomes are part of the contract, not afterthoughts.
- Keep inline comments and rustdoc close to non-obvious rules. Contributions
  must teach ownership, security, concurrency, cancellation, protocol ordering
  and validation decisions. Comments that merely narrate syntax are less useful
  than explaining why a waiter precedes a write or why an action cannot be retried.

## Security and state

TLS certificate and hostname verification remain enabled; custom CA roots are
supported. Credentials enter through configuration, not CLI arguments or URL
userinfo. Logs use allowlisted metadata and fixed failure text; raw frames, SDP,
keys and dependency debug dumps are not diagnostic output. Encryption's custom
`Debug` redacts its key, while private transport serialization necessarily carries
it; never use serialization as a logging shortcut.

Unix sockets are local, permissioned `0600`, and check the kernel peer UID, with
no simulator TCP listener. Run
driver and simulator under the intended shared non-root UID in a private directory.
The simulator owns only its own socket inode/device during cleanup. Process-held
per-socket OS locks protect startup; stale same-UID sockets are recovered only
after a refused-connection probe and unchanged inode/device checks. Live sockets,
symlinks, regular files and ambiguous paths are never replaced. Socket directories
must be owned by the runtime UID and not group/other writable.

Simulator operational state survives driver reconnection and resets when the
simulator restarts. The externally provisioned node UUID must persist separately.
Health is local event-loop progress; readiness tracks dependencies and is cleared
before reconnect backoff. Neither proves
VideoIPath discovery, accepted collector metadata, Panel operation or media flow.
Deployment details are in [OPERATIONS.md](OPERATIONS.md).

The [Dockerfile](Dockerfile) builds a static musl binary with the digest-pinned
Rust 1.98.1 Alpine builder and a pinned distroless static non-root runtime. The
runtime has CA roots, but no shell, package manager or unused OpenSSL libraries.
Native Linux amd64 and container evidence are recorded separately in
[VALIDATION.md](VALIDATION.md); no additional platform coverage is implied.
