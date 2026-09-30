# Local control-plane scenarios

These exercises use one fixed registry and deliberately change simulator state.
There is no `--scenario` flag or automatic random-change mode. The startup setting
`DRIVER_CONFIGURATION_ORDER` accepts `senders_first`, `receivers_first` or `all`;
it changes scheduling within the same registry, not the device personality. The
simulator models control state only; it creates no media. A live VideoIPath instance was
not available for this example. Expected SPEC exchanges are documented separately
in [PROTOCOL.md](PROTOCOL.md), not presented as target-verified captures.

## Run without VideoIPath

Use Linux, the package's pinned Rust toolchain and a private directory. Run these
commands from `examples/rust_example` in a terminal:

```sh
cargo build --locked
export RUST_EXAMPLE_RUN="$(mktemp -d "${TMPDIR:-/tmp}/rust-example.XXXXXX")"
chmod 700 "$RUST_EXAMPLE_RUN"
export DRIVER_SOCKET="$RUST_EXAMPLE_RUN/device.sock"
export DRIVER_CONTROL_SOCKET="$RUST_EXAMPLE_RUN/control.sock"
export DRIVER_STATE_DIRECTORY="$RUST_EXAMPLE_RUN/state"
echo "$RUST_EXAMPLE_RUN"
./target/debug/rust-example simulator
```

The simulator stays in the foreground. In another terminal, set `RUST_EXAMPLE_RUN`
to the directory just printed, then export the same socket variables below.
No VideoIPath URL, token or node UUID is needed for `simulator` or `simctl`.

```sh
export DRIVER_SOCKET="$RUST_EXAMPLE_RUN/device.sock"
export DRIVER_CONTROL_SOCKET="$RUST_EXAMPLE_RUN/control.sock"
./target/debug/rust-example simctl status
```

A fresh process reports `version: 1`, `revision: 0`, `action_count: 0`,
`configured_streams: 0`, `active_alarms: 0`, `delay_pending_ms: 0`,
`failure_pending: false` and `pending_delays: 0`. JSON property order is not significant.

Stop the foreground simulator with Ctrl-C. Its state resets at its next start;
the driver alone can restart without resetting simulator state. Existing socket
paths are not silently overwritten. Keep the private directory for inspection
until you are finished.

For an already configured Compose deployment, the equivalent command is:

```sh
docker compose exec simulator /usr/local/bin/rust-example simctl status
```

Use [README.md](../README.md) and [OPERATIONS.md](../OPERATIONS.md) for Compose
configuration. Compose interpolates required driver settings even when selecting
the simulator service; native mode is the credential-free local exercise here.

## The current fixture

| Identity | Purpose | Initial behavior |
| --- | --- | --- |
| Root `level` | Integer 0–100, step 1, read/write | 50 |
| Root `audio_gain` | Number −20–20, step 0.5 dB, read/write | 0.0 |
| Root `enabled` | Boolean read/write, GPI/GPO metadata | true |
| Root `signal_present` | Read-only boolean, GPI capable | true; independent control changes are observable |
| Root `mute` | Read/write boolean, GPO capable, not GPI | false |
| Root `mode` | String choice `standby` / `active` | `standby` |
| Root `temperature` | Read-only number −40–125, step 0.1 | 21.0 |
| Root `unbounded` | Finite numeric read/write value, null range | 0 |
| Root `trigger` | Boolean write-only action | Invocation counter only |
| Root `sim.user.1`, `sim.user.2` | Unadvertised integer scratch values 0–100 | 0; explicit read selection required |
| `module-1` / `tx-1` | Video sender, limited H.264/H.265 SDP and SIPS metadata | IPT disabled; `format_id: raw-video` |
| `module-1` / `tx-audio`, `tx-asi`, `tx-anc`, `tx-unknown` | Other topology format types | IPT disabled; no sender SDP |
| `module-1` / `tx-monitor` | Transceiver with null topology | IPT unsupported |
| `module-1` / `rx-1` | Video receiver with H264/H265/MP2T SDP subset and separate spare-path state | IPT disabled; `format_id: raw-video` |
| `module-1` / `rx-alt` | Alternative video resource receiver; non-SDP configuration | IPT disabled; second receiver-to-tx-1 resource choice |
| `module-1` / `rx-audio` | Audio receiver with L16/L24 PCM SDP subset | IPT disabled |
| `module-1` / `fan-1` | Non-routable generic component | Scoped `enabled: true` |
| `module-1` / `net-main`, `net-spare` | Redundant pair | 192.0.2.10 / 198.51.100.10 |
| `module-2` / `net-aux` | Cross-module interface option; module label `Auxiliary Console 42` | Null known address |

The two modules, `tx-1`, both receivers, `fan-1`, and all three interfaces each have an
independent `enabled` parameter initially true. A null known interface address
needs externally supplied topology addressing or a confirmed override form for
real routing; local acceptance does not establish that prerequisite.

`raw-video` is the fixture's device-defined resource-format identifier, not a
promise of raw-video SDP support. `all` batches accept both directions and apply
sequentially; `receivers_first` reverses the direction order. Set the driver
environment before startup, for example `export DRIVER_CONFIGURATION_ORDER=receivers_first`,
and restart deliberately when changing advertised metadata.

## Basic values and actions

```sh
./target/debug/rust-example simctl set null level 60
./target/debug/rust-example simctl set null level 60
./target/debug/rust-example simctl set null audio_gain 2.5
./target/debug/rust-example simctl set null mode '"active"'
./target/debug/rust-example simctl set '{"module_id":"module-1","port_location":{"type":"sender","id":"tx-1"}}' enabled false
```

The first level change returns `{"changed":true}`, the repeated value returns
`{"changed":false}`. Each genuine change increments the revision and, when a
driver is connected, produces an independent `async_values_changed` event.
Root `enabled` stays true after the scoped sender change. If an independent
change races a backend mutation, the session may close and reconnect rather than
publish an uncertain ordering; revision checks are fail-closed conflict detection.

```sh
./target/debug/rust-example simctl set null level true
./target/debug/rust-example simctl set null audio_gain 0.25
./target/debug/rust-example simctl set null missing 1
```

Each command fails with a nonzero exit and a sanitized CLI error. They exercise
type, step and lookup rejection. `simctl` is a front-panel-like control surface:
it can change read-only sensor state, whereas a SPEC write to that sensor fails.

```sh
./target/debug/rust-example simctl set null temperature 22.5
./target/debug/rust-example simctl set null signal_present false
./target/debug/rust-example simctl set null mute true
./target/debug/rust-example simctl set null trigger true
./target/debug/rust-example simctl set null trigger true
./target/debug/rust-example simctl status
```

The two action invocations increase `action_count` by two. They return
`changed: false` because no readable value event exists, not because the action
was ignored. `status` is a bounded summary, not a parameter snapshot command.
To exercise SPEC reads and writes, use the [full frames](PROTOCOL.md#dynamic-values)
through a backend/test harness. `simctl set` is not a substitute for testing
`set_dynamic_values` correlation and partial-failure behavior.

## Alarms

```sh
./target/debug/rust-example simctl raise '{"id":"Link Down","location":{"module_id":"module-1","port_location":{"type":"network_interface","id":"net-main"}},"details":"Modeled carrier loss","severity":"major","service_affecting":true}'
./target/debug/rust-example simctl status
./target/debug/rust-example simctl clear '{"module_id":"module-1","port_location":{"type":"network_interface","id":"net-main"}}' 'Link Down'
./target/debug/rust-example simctl poll-alarms
```

Raise sets one active alarm and produces `async_alarm` for connected clients.
Repeating the identical alarm is deduplicated; changing details/severity refreshes
it. Clear removes that identity and emits `async_alarm_cleared`; clearing it again
returns `changed: false`. `poll-alarms` emits only the poll trigger and returns null.
An actual backend then decides when to poll. Full alarm polls remain authoritative.

Use each documented severity and test both service-affecting values when extending
the scenario. Omitting `service_affecting` means true. These are model variants,
not a claim that all severity/scope combinations have automated coverage.

## IPT, SDP and drift

The full enable, read-SDP, receive, check and disable sequence is in
[PROTOCOL.md](PROTOCOL.md#ipt-configuration-and-sdp). It requires SPEC dispatch in
a test harness or backend. The CLI has no `configure-ipt` or `send-frame` command.
After applying that sender configuration, this genuine local command removes its
modeled target state independently of the driver's requested state:

```sh
./target/debug/rust-example simctl drift '{"module_id":"module-1","port_id":"tx-1","sub_index":null,"direction":"sender"}'
./target/debug/rust-example simctl status
```

`configured_streams` decreases if the stream existed. A SPEC check against the
earlier enabled configuration returns false; repair with `set_again: true` must
apply and reread before true. Sender SDP is null while disabled. A stream already
absent is an idempotent no-change drift operation.

The optional second `drift` argument is a serialized **domain `MediaState`**, not
a SPEC `enabled_config`; see [domain.rs](../src/domain.rs). Prefer removal for a
simple drift exercise. Never paste real SRT keys into CLI arguments. The modeled
SDP and format limits are described explicitly in [PROTOCOL.md](PROTOCOL.md).

## Failure injection

For documented resource-routing choices, run
`cargo test --locked --test resource_choices`. It initializes indexed `rx-1` and
`rx-alt`, selects each for `tx-1`, verifies target readback/check, rejects an
unlinked partner without changing state, and disables the sender independently.
The [complete selection frame](PROTOCOL.md#selecting-a-receiver-resource) shows
the documented receiver-partner shape. No `tx-tx` selection extension is guessed.

| Command suffix after `simctl` | Effect | Observation |
| --- | --- | --- |
| `delay-next 1000` (also `delay 1000`) | Delay the next target request by 1000 ms; maximum 10000 | `delay_pending_ms` then returns to zero |
| `fail-next` | Reject the next target operation as unavailable | `failure_pending` then returns to false |
| `disconnect` | Close current target connections; keep simulator state | Driver session recovers through supervision |
| `inspect` | Alias of `status` | Summary only, no secret-bearing configuration dump |

Faults affect whichever target request arrives next, including a health/read
request or the driver's five-second health probe; they are not bound to a
particular SPEC message ID. Without an attached target client, delay/failure
remains pending. Deterministic timeout and no-replay
tests should coordinate the next operation explicitly rather than race a CLI
command against a busy driver. `pending_delays` reports operations already in the
simulator's pre-apply delay. Peer loss cancels those unapplied operations and
queued requests; this simulator behavior does not undo an already-applied physical
write. Simulated faults do not prove token revocation, TLS behavior or backend takeover.

## Local test entry points

```sh
cargo test --locked
cargo test --locked session::tests::keepalive_is_exact_and_independent_of_hung_handler
cargo test --locked session::tests::correlated_response_precedes_causal_event_and_error_is_silent
cargo test --locked --test native_end_to_end -- --nocapture
cargo test --locked --test load
```

The session tests use an in-memory WebSocket peer and a controlled callback; they
do not require credentials. Their scope is session behavior, not complete device
or VideoIPath acceptance. The native integration test runs the actual driver
binary against an ephemeral verified-TLS backend and independent simulator. It
covers read/write/events, drift/repair, duplicate-node process rejection, restart
and a sequential workload of 500 keepalives plus 500 fresh reads, with sanitized
trace logging. The load test applies 128 resources, checks all of them for ten
cycles and verifies capacity rejection preserves existing state. These are bounded
local workloads, not throughput or full event-history guarantees. Execution results
remain in [VALIDATION.md](../VALIDATION.md). See [COVERAGE.md](COVERAGE.md) for
implementation and test gaps rather than inferring coverage from a passing command alone.
