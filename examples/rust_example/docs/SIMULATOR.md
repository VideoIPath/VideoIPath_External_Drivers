# Independent simulated target

The simulator is a separate process with its own volatile state. Driver restarts
preserve that state; simulator restarts reset it. It models control state, not
media generation, reception, encoding, FEC, or encryption. Local success is not
VideoIPath release acceptance or physical-device verification.

## Transport contract

The two Unix sockets are private local APIs, **not SPEC-23**. No TCP port is
opened. The target socket accepts domain operations; a separate control socket
accepts deliberate external changes and bounded fault injection. Socket files
are permissioned `0600`; run both processes with the same non-root UID in a
private shared directory. Live sockets, symlinks, and regular files are never replaced.

Each frame is one UTF-8 JSON object followed by LF, at most 256 KiB including LF.
Requests have `version: 1`, an unsigned `id`, `command`, and `args`. Replies have
`kind: "reply"`, the matching `id`, `ok`, `value`, and optional `revision`.
Events have `kind: "event"`, `value`, and `revision`. The simulator assigns state
revisions under its mutation lock and takes reply observations under that same
lock. These are private transport metadata, **never SPEC-23 fields**. Legacy
fixtures may omit revisions (reply: null, event: zero).

The adapter has exactly one reader and a serialized writer, at most 32 admitted
operations, and a 128-event broadcast ring. Registering a reply waiter precedes
the first write. A deadline includes waiting for the writer. Cancellation during
or after the first write retires the entire stream and fails all waiters; it
never retries a possibly applied action. `close()` cancels and joins the reader.
Dropping the last client cancels its task as a fallback.

Driver-originated changes produce replies and readback, **no unsolicited causal
events**. The application/session owns publication after its correlated response
has been delivered. Only control-socket changes generate independent events.
Event consumers must treat broadcast lag as lost synchronization and retire or
resynchronize their session; silently ignoring lag loses alarm transitions.

### Freshness at dispatch

Use `Client::subscribe_observed()` for the production relay. It yields
`ObservedEvent { revision, event }`. Keep that observation with the internal
session queue entry; do not serialize the revision into a SPEC frame or log the
observation. Call `client.event_is_current(&observation)` **at actual event
dispatch**, including after awaiting any causal response delivery. Checking only
when the event enters a relay queue is too early: a confirmed write can overtake
it there. A false result requires closing/resynchronizing the session, not
silently discarding the event. `subscribe()` remains available for plain-domain
consumers/tests but alone cannot make this freshness check.

The reader records successful `set_value` reply revisions **before waking the
request waiter**, keyed by canonical `(location, local_id)`. Each event value
must be at least as new as its own parameter's confirmed-write floor. Floors
are not global and reads never advance them: a newer snapshot must not discard
unrelated GPI history. Alarm raises, clears, and poll triggers always pass this
value-freshness check. Missing revision metadata after a confirmed write cannot
establish freshness, so events for that identity fail closed. Floors are bounded
to 1024 identities per client generation; overflow retires the stream.

This is **fail-closed conflict detection**, not complete revision-history
reconciliation. It neither reorders history nor repairs a stale value. The
coordinator must integrate the dispatch-time check and resynchronization path.
Already-applied physical writes remain potentially ambiguous on cancellation;
revision metadata is not an undo mechanism.

Control responses and errors are summaries, not configuration dumps. Media state
can include secret-bearing fields, so neither raw state nor request bodies belong
in diagnostic logs.

## Native controls

From this example's directory, create a private runtime directory and start the
independent process (no VideoIPath credentials required):

```sh
mkdir -p "$HOME/videoipath-files/rust-example-simulator"
chmod 700 "$HOME/videoipath-files/rust-example-simulator"
export DRIVER_SOCKET="$HOME/videoipath-files/rust-example-simulator/device.sock"
export DRIVER_CONTROL_SOCKET="$HOME/videoipath-files/rust-example-simulator/control.sock"
export DRIVER_STATE_DIRECTORY="$HOME/videoipath-files/rust-example-simulator/state"
cargo run -- simulator
```

In another terminal, set the same `DRIVER_CONTROL_SOCKET`, then run
`cargo run -- simctl status` or `cargo run -- simctl set 'null' level '60'`.
Use Ctrl-C for graceful cleanup. The examples below show arguments after
`simctl`. JSON arguments must be shell-quoted. Root location is JSON `null`.

| Command | Effect |
| --- | --- |
| `status` or `inspect` | Sanitized revision, action count, stream/alarm counts, pending faults, active `pending_delays` |
| `set 'null' level '60'` | Independently change root level; emits only if changed |
| `set '{"module_id":"module-1","port_location":null}' enabled 'false'` | Change module-scoped enabled, leaving root enabled alone |
| `set 'null' temperature '25.0'` | Model a sensor change; ordinary driver writes remain read-only |
| `raise '{"id":"Link loss","location":null,"details":"Modeled loss","severity":"major","service_affecting":true}'` | Raise/refresh a scoped alarm; equal repeats emit nothing |
| `clear 'null' 'Link loss'` | Remove active alarm and emit clear only if present |
| `poll-alarms` | Request a full authoritative alarm poll |
| `drift '{"module_id":"module-1","port_id":"tx-1","sub_index":null,"direction":"sender"}'` | Disable an independently configured stream; a later configuration check sees drift |
| `disconnect` | Close existing target connections; allow fresh connections |
| `delay-next 1000` | Delay the next target operation by 1000 ms; maximum 10000 |
| `fail-next` | Definitively reject the next target operation before applying it |

`drift` also accepts a second JSON argument containing a normalized domain
`MediaState` (or `null`), rather than a SPEC configuration. It validates known
ports/interfaces and bounds. Configuration readback is a fresh target operation;
the driver handler compares it with desired state and performs optional repair.
Disabling removes the corresponding sub-index resource. Sub-indices 1–64 are
separate resources; null is the normal single-stream identity.

Parameter defaults, scoped addresses, access, types, and validation come from
the same immutable `Registry` used for discovery. Values themselves live only
in the simulator's independent state. An accepted `trigger` invocation increments
`action_count`; it is never exposed as readable persistent state or replayed.
Driver-configured state emits no external event. All five alarm severities use
the domain model, and a full alarm snapshot reflects clears immediately.

## Bounds, lifecycle, and limitations

| Resource | Bound/policy |
| --- | --- |
| Connections | 16 target + 8 separately reserved control connections |
| Client operations | 32 immediately admitted; excess returns capacity failure |
| Server request pipeline | 32 queued requests per connection; overload closes it |
| Frame | 256 KiB including LF; serde's recursion limit also applies |
| Events | 128 per broadcast ring; lagging server connections close |
| Configured resources | 128, at most 32 KiB serialized state each |
| Alarms | Up to 128; IDs ≤256 bytes, details ≤2048 bytes; aggregate serialized snapshot plus worst-case reply envelope must fit one frame |
| Read/write | 60-second frame deadline; 5-second server write deadline |
| Control call | 5 seconds end-to-end |
| Client operation | Supplied timeout, positive and at most 30 seconds |

The sockets additionally check kernel peer UID, covering the brief bind-to-chmod
window. Both socket directories must belong to the runtime UID and must not be
writable by group/others; mode `0700` is recommended and used in the image.
Process-held per-socket OS locks protect ownership. Under these locks, startup
recovers a same-UID socket only after a bounded connection probe proves it is
unserved (`ConnectionRefused`) and its inode/device are unchanged. Live or ambiguous
paths, symlinks and regular files fail closed. This supports restart after a hard
kill without treating file existence as ownership. Graceful cleanup unlinks only
this process's socket inode/device. Simulator state remains volatile.

Fault controls affect the next operation across all target connections, including
a new client's startup health request. Delay happens before application; clients
cannot infer this from a timeout and still retire ambiguous streams. The server's
sole reader continues observing EOF while the operation waits: peer loss cancels
unapplied delayed operations and queued requests. A canceled delay drops its
`pending_delays` guard, so no delayed old operation remains to overwrite a new
connection's state. This is a simulator pre-apply behavior, **not a promise that
canceling a real device operation can undo a transmitted or applied write**.
`fail-next`
is a definitive correlated rejection and keeps the stream usable. `disconnect`
is transport loss and retires every affected client clone. An ordinary idle
connection can close after 60 seconds without a request; reconnect is owned by
the driver composition root.

Alarm admission counts actual escaped JSON bytes, not just characters. It checks
new records and replacements before mutation, reserving the maximum-width reply
ID/revision envelope. Capacity rejection preserves the previous complete pollable
snapshot and does not increment its revision.

Sender SDP supports **single-stream H.264 or H.265 RTP video**, with an explicit
dynamic `format.attributes.rtp_payload_type` from 96 through 127 and no other
format attributes. Codec names and payload numbers come from observed state;
IPv4 multicast includes its TTL. Unknown/missing codecs or payloads, additional
format attributes, audio pairs, receiver-source fields, supplied SDP overrides,
SRT, FEC, and spare streams return `Failure::Unsupported`. In particular, no
invented raw/RTP96 fallback or unsupported DUP group is emitted. A disabled sender
returns null. This intentionally narrow subset is not a complete ST 2110 SDP
generator. No SRT session, FEC packets, media, or device timing is produced.
Advanced media validation belongs
to the capability layer; the simulator enforces its own bounded domain storage
and basic target references. No random/background state changes are generated.

## Tests

Run `cargo test --test device_lifecycle`. It uses real local Unix streams for
immediate reply correlation, event/reply interleaving, cancellation after a
potentially applied action, timeout invalidation of concurrent waiters, state
survival, scoped values, alarm transitions, drift/readback, controlled failures,
concurrency saturation, oversized unterminated frames, partial-request/event
interleaving, and socket permissions/ownership. Regression cases cover old-value
11/new-value 22 across canceled pre-apply generations, non-idempotent actions,
escaped alarm snapshot capacity, truthful codec/payload SDP and unsupported
transport combinations, and stale relays rechecked after a confirmed write.
These tests do not contact VideoIPath.
