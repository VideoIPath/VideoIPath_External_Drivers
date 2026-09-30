# Build a driver for your device

Start with a device and firmware you can observe and control. Implement only its
real capabilities, and make structure metadata match those capabilities. The
existing simulator provides a learning and local-test boundary; it is not proof
of a physical device's behavior or VideoIPath release acceptance.

Start with the [README's gain-control lesson](../README.md#2-follow-one-control-from-definition-to-device).
Use [PROTOCOL.md](PROTOCOL.md) for complete frames as you add each capability;
you do not need to learn the simulator's internals first.

## First increment: one real parameter

Suppose the vendor API has a gain read and an absolute gain set. Establish its
units, range, rounding, acknowledgement and readback behavior from vendor evidence
before mapping it. The example's `audio_gain` is a worked pattern, not a required
name or range.

| Step | Change in your driver | What the test must prove |
| --- | --- | --- |
| 1. Choose the identity | Give the physical target a stable UUID; choose a root, module or port `Location` and stable local parameter ID | Restarts and relabeling do not create a different control |
| 2. Describe the control | Adapt `Registry::new` and `Parameter` to the real type/range/unit/access; remove unrelated demonstration entries | `get_structure` describes the same values the runtime accepts; it contains no current reading |
| 3. Read the vendor API | Implement `Device::read_values` in a new vendor adapter; convert device units to advertised units | `get_dynamic_values` returns the vendor observation, not `Parameter.default`; failed reads are errors |
| 4. Set and confirm | Implement `Device::set_value`; convert units, apply, then query the actual value | A valid change succeeds, an invalid value never reaches the device, and normalization/partial apply is reported truthfully |
| 5. Connect the adapter | Construct it in `main.rs` where `Client::connect` currently creates the simulator client; replace the simulator-specific health/event wiring | Device contact is established before the backend session; device loss changes readiness and keepalive behavior |
| 6. Exercise the whole request | Adapt the frame tests to a fake vendor server, then use approved safe physical values | Response correlation, failed-ID map, readback and changed-only notification order survive the new transport |

For a read-only sensor, stop before the setter: use `read_only`, disable GPIO
flags, and reject writes. For a write-only command, use the action path instead
of inventing a readable value. If the device has events, add them after reads and
writes work; do not turn a demand-only sensor into GPI by changing a metadata flag.

The example tests make useful starting points:

```sh
cargo test --locked --test documented_frames readme_gain_frames_follow_observed_device_state
cargo test --locked --test capabilities partial_set_types_access_steps_changed_only_and_action
cargo test --locked --test capabilities scope_identity_and_uncertain_write_readback
```

Replace the simulator process/control-socket deployment settings with your vendor
endpoint, authentication and transport settings. Update `.env.example`, Compose
forwarding and configuration tests together; don't retain dummy simulator options.

## Add only the next needed API capability

| Need | Vendor behavior to supply | Protocol lesson |
| --- | --- | --- |
| Local/front-panel changes | Reliable observed events; reconcile them with concurrent writes | `async_values_changed` uses the same location and local IDs as reads |
| Alarms | Full active fault list, optionally event raise/clear | `alarms` omission means clear; an unavailable list is not an empty one |
| Routed media ports | Enable, configure, disable and query each actual sender/receiver | `set_ipt_configurations` reports per-port failures; `enabled_config: null` disables |
| SDP | Obtain/generate actual sender SDP or apply supported receiver SDP | Advertise `sdp_support` only for the corresponding working operation |
| Drift repair | Independent normalized readback and safe reapply | `check_ipt_configuration` compares supported fields and honors `set_again` |
| Resource routing | Real internal receiver-to-sender choices and resource lifetime | Add links/indices only when needed; see the [selection walkthrough](PROTOCOL.md#selecting-a-receiver-resource) |

Do not port the entire media parser for a sensor-only device. The sample's FEC,
SRT and redundancy storage demonstrates payload handling, not the vendor operations
needed to provide those capabilities. The [coverage ledger](COVERAGE.md) lists
the exact implemented variants; unresolved wire details are not extension points
for guessed fields.

## Keep the translation boundary

| Layer | Put here | Keep out |
| --- | --- | --- |
| `registry.rs` | Stable scoped definitions, access, GPIO, truthful ranges and topology | Device credentials and transient readings |
| `protocol.rs` | SPEC decoding and conversion to supported domain fields | Vendor endpoints and vendor DTOs |
| `application.rs` | Read/set/check semantics and per-item outcomes | Socket handling and concrete adapter imports |
| `domain.rs` | Small capabilities and observed device semantics | Original SPEC frames as cached “state” |
| New vendor adapter | Authentication, vendor DTOs, transport, normalization, physical readback | VideoIPath connection management and database access |
| `main.rs` | Construct the concrete adapter and supervise its lifetime | Capability-specific behavior |

The sample `Device` trait groups six operations to make these lessons runnable.
It is not part of SPEC-23 or a requirement to implement unused capabilities.
Trim or split it to match your device; keep unsupported requests as explicit errors.
Retain the separation between protocol semantics and vendor I/O, not necessarily
this project's file layout or exact types.

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
- Replace the demonstration structure rather than appending your product to it.
  Test clean consumers after structural changes, then plan Inventory synchronization
  and migration of saved Panel bindings. Changing a label should not rename an ID.

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

Use bounded buffers, requests and tasks. Serialize conflicting operations through
read/apply/readback without blocking keepalive handling. A handler returns causal
changes to the session, which delivers the response first; the adapter must not
publish the same change independently before that response.

Design event reconciliation from what the vendor actually supplies: transaction
IDs, revisions, readback or other evidence. The simulator's private revisions and
the sample's retire/resynchronize policy are **not SPEC fields or a universal device
algorithm**. Do not fabricate equivalent vendor guarantees. Preserve meaningful
alarm transitions and action counts; suppress only attributable echoes, and recover
authoritative state visibly on event loss. See [DESIGN.md](../DESIGN.md) for how the
Rust example enforces these invariants if you reuse its session implementation.

Make health reflect real device contact, subscribe early enough not to lose startup
events, and clear readiness before reconnect backoff. Do not add a second parameter
poll solely to keep a Panel value current.

## Error, security and teaching contract

Map vendor failures to bounded sanitized failures. Never interpolate raw HTTP
bodies, SDK errors, credentials, SDP or SRT keys into protocol errors or logs.
Keep TLS/hostname verification on, scope permissions to the device API and place
secrets outside source and command-line arguments. Do not introduce a generic
URL/OID/file resolver for unadvertised parameters; define a validated namespace.

Keep singleton ownership across reconnects and cancel/drain old device work on
session replacement. Preserve these properties when simplifying the sample;
the exact lock/task implementation is a design choice, not a wire requirement.

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
