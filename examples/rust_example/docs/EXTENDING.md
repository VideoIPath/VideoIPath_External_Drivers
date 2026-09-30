# Replace the simulator with one device API

Start with a device and firmware you can observe and control. Implement only its
real capabilities, and make structure metadata match those capabilities. The
existing simulator provides a learning and local-test boundary; it is not proof
of a physical device's behavior or VideoIPath release acceptance.

Read [DESIGN.md](../DESIGN.md), [PROTOCOL.md](PROTOCOL.md) and the
[coverage ledger](COVERAGE.md) before changing the adapter.

## Keep the translation boundary

| Layer | Put here | Keep out |
| --- | --- | --- |
| `registry.rs` | Stable scoped definitions, access, GPIO, truthful ranges and topology | Device credentials and transient readings |
| `protocol.rs` | SPEC decoding and conversion to supported domain fields | Vendor endpoints and vendor DTOs |
| `application.rs` | Read/set/check semantics and per-item outcomes | Socket handling and concrete adapter imports |
| `domain.rs` | Small capabilities and observed device semantics | Original SPEC frames as cached “state” |
| New vendor adapter | Authentication, vendor DTOs, transport, normalization, physical readback | VideoIPath connection management and database access |
| `main.rs` | Construct the concrete adapter and supervise its lifetime | Capability-specific behavior |

The existing `Device` trait groups six related operations. It is statically
dispatched with native Rust futures; do not add `async-trait`, a plugin framework
or an all-vendor abstraction to implement one device. Split the trait into smaller
capabilities only when a real implementation boundary benefits from that split.

## Define identity and advertised behavior first

- Provision a stable UUID for the physical target. Preserve it through process
  replacement; do not generate a new identity on every startup.
- Preserve module, interface and port IDs. Local parameter IDs may repeat at
  different structured locations. Never substitute a UI-projected address for
  `(location, local_id)` on the wire.
- Remove unsupported advertisements: no sender SDP flag without meaningful SDP,
  no receiver SDP flag without supported application/validation, no SIPS claim
  from merely accepting a spare-field object, and no GPI without reliable events.
- A sensor can be read-only without GPIO. Read it from a fresh coherent snapshot
  when requested; do not add a duplicate background poll just to create events.
- Update the one current registry deliberately. There is no runtime structure
  selection; only configuration order is a startup setting within the fixed
  fixture. Test clean consumers after structural changes, then plan Inventory
  synchronization and migration of saved Panel bindings.

## Implement observed state, not request echo

`configure` must apply device state. `configuration` must query the device and
return the supported normalized fields. A dictionary of the last requested SPEC
payload cannot detect front-panel changes, reboot, partial apply or drift.

| Operation | Required result | Retry/readback rule |
| --- | --- | --- |
| Snapshot read | Fresh coherent values or failure | A cached stale value is not a fresh success |
| Absolute set | Confirmed observed value | Reconcile ambiguity through readback; never automatically replay a possibly applied write |
| Write-only action | Definitive acknowledgement where available | Never replay without a device-supported idempotency mechanism |
| IPT apply | Applied supported fields, followed by readback | Independent failures use per-port error arrays |
| IPT check | Comparison against observed state | Repair only for `set_again: true`; reread before returning `true` |
| Alarm poll | Complete authoritative active set | Missing entries clear; a failed poll is an error, not `[]` |
| Independent event | Canonical identity and validated observed change | Preserve meaningful transitions and handle lag explicitly |

Classify normalization intentionally. Convert irrelevant profile metadata away
before comparing, but do not silently round supported capabilities to make a test
green. If a requested value is not achieved, report the mismatch and retain the
actual observed change. If one physical operation updates several fields, read
and reconcile them together.

`source_meta` is informational in the SPEC model; this example ignores it. Do not
promote it to configuration identity. Likewise, IP override wire shape and
`tx-tx` partner selection remain blocked evidence questions, not opportunities
to invent local wire extensions.

## Own transport and conflicting operations

For a stream protocol, one reader classifies replies, unsolicited events and
auxiliary messages. Register a waiter before transmission. Mark an operation
potentially transmitted before the first write/flush/drain can suspend. Timeout
or cancellation after that point must retire an ambiguous stream and fail its
waiters so late replies cannot satisfy a later command.

Use bounded buffers, requests and tasks. The session already holds an exclusive
`RwLock` lane for backend mutations through read/apply/readback, response and
causal-event delivery. Reads share the lane; keepalive bypasses it. A real device
may need additional resource-specific coordination outside that session boundary.

The simulator's private transport attaches revisions to observations, and the
adapter tracks confirmed value-write floors by `(location, local_id)`. Preserve
this metadata through internal relay queues and validate freshness at actual
session dispatch. Do not add it to SPEC frames: `_driver_revision` is an internal
relay key removed before wire serialization. Reads must not advance write floors
or suppress unrelated GPIO/alarm history.

A handler returns causal changes to the session. It must not also publish them
from the adapter. The session delivers the response before those changes.
For independent events, the current policy closes/resynchronizes on a stale
observation or a conflict with the mutation lane. This detects conflicts without
claiming full event-history reconciliation. To support richer vendor concurrency,
add device-backed transaction/revision evidence and tests before replacing that
policy; suppress only attributable echoes, not every event with the same ID.
Preserve alarm transitions and action counts; coalesce only replaceable current
state. On overflow, fail visibly and recover authoritative state.

The five-second `health` probe is transport liveness, not a second parameter poll.
Subscribe before startup reads and backend establishment so early event overflow
is detectable, and clear readiness before reconnect backoff.

## Error, security and teaching contract

Map vendor failures to bounded sanitized failures. Never interpolate raw HTTP
bodies, SDK errors, credentials, SDP or SRT keys into protocol errors or logs.
Keep TLS/hostname verification on, scope permissions to the device API and place
secrets outside source and command-line arguments. Do not introduce a generic
URL/OID/file resolver for unadvertised parameters; define a validated namespace.

Keep the node's `std::fs` lock guard alive across reconnects. Use Tokio task
ownership and cancellation explicitly. Modern stable Rust already supports the
native future-based capability contract; additional machinery needs a concrete
reason.

Inline teaching comments are required for non-obvious ownership, concurrency,
security and protocol rules. Explain why retries are unsafe, what an observation
means, when a mutex is released, and who joins a task. Add worked tests that expose
these invariants rather than comments that restate a function's name.

## Evidence to add with an extension

| Layer | Positive case | Failure case |
| --- | --- | --- |
| Codec/registry | Complete parameter maps and every advertised scope | Duplicates, missing references, wrong types, unsupported enums |
| Handler | Full SPEC request → physical-model change → readback → response/event | Partial writes, unknown IDs, read-only and write-only policies |
| Device transport | Interleaved reply/event with one reader | Timeout, cancellation during transmission, late reply and no action replay |
| Session | Correlation and response-before-causal-event | Saturation, stalled device, target loss, old-generation suppression |
| Media configuration | Main/spare, disable, supported SDP, drift and repair | Inconsistent SDP, failed repair/readback and bounded batch |
| Native integration/load | Actual binary with test TLS backend and simulator; bounded full-capacity work | Duplicate process, restart, capacity overflow and secret-free trace |
| Target release | Accepted structure, catalog, runtime and consumer behavior | Rejected frames retained as sanitized release-scoped fixtures |

Run the package checks from `examples/rust_example`:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

Record the actual commands and results in the project's validation record and
update the relevant stable ledger rows. A test that parses a model does not prove
application, target acceptance or media operation. Commission the precise
VideoIPath release/backend SHA and device firmware separately, through discovery,
Inventory, full parameter catalog, active reads, writes, async events and recovery.
Physical media tests remain an additional evidence layer.
