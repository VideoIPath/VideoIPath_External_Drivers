# VideoIPath Rust Example Driver

<img width="1940" height="520" alt="Nevion VideoIPath external drivers banner" src="https://github.com/user-attachments/assets/e236e8ea-7787-414f-a728-822985695e19" />

$\Large\color{green}{\textsf{Build Your Own External Driver}}$

Use this runnable example to learn how to expose **your device's controls, status, alarms and media endpoints** through the VideoIPath External Driver API. Start with one parameter, follow a request through to device readback, then add only the capabilities your device supports.

```text
VideoIPath  <-- SPEC-23 JSON over WebSocket -->  Your driver  <-- vendor API -->  Your device
                                              ^ replace the example's simulator adapter
```

The included Rust driver uses a separate simulated device so you can study reads, writes, local changes and failures without equipment. The simulator's Unix-socket API is **not** part of the VideoIPath API; replace it with your device's HTTP, TCP, serial or SDK interface. The example is a source reference, not a framework you must adopt wholesale.

> [!IMPORTANT]
> Source, tests and Docker Compose are implemented and locally tested. This example has **not been commissioned against a VideoIPath release or physical device**. Its local models are not an official schema; validate your driver against your intended release and firmware. Exact evidence and limitations are in [VALIDATION.md](VALIDATION.md) and the [coverage ledger](docs/COVERAGE.md).

## 1. Start with the capability you need

You do not need media routing to expose a temperature or gain control. Keep unsupported capabilities out of your structure, and return a correlated error for unsupported requests rather than pretending success.

| Your device needs to… | External Driver API | What to implement / example to study |
| --- | --- | --- |
| Describe itself and its controls | `get_structure` | Stable IDs and parameter **definitions**, not readings. Start in [`Registry::new`](src/registry.rs). |
| Report status or sensor readings | `get_dynamic_values` | Read fresh device state, resolve `(location, local_id)`, return requested known IDs. Follow that branch of `dispatch` in [application.rs](src/application.rs). |
| Change a setting or trigger an action | `set_dynamic_values` | Validate, apply and confirm; report failures per ID. Follow `set_values` in [application.rs](src/application.rs). |
| Report a front-panel/GPI change | `async_values_changed` | Translate actual device events to scoped changes. Study [event encoding](src/protocol.rs) and [value/event frames](docs/PROTOCOL.md#dynamic-values); do not advertise GPI from demand reads alone. |
| Report active faults | `alarms`, optional async alarm messages | Return the complete active set; omission clears an alarm. Follow the [alarm frames](docs/PROTOCOL.md#alarms). |
| Expose routable senders/receivers | `get_structure`, `set_ipt_configurations` | Describe ports/interfaces, apply each configuration, handle null as disable and return per-port failures. Follow `set_ipt` in [application.rs](src/application.rs). |
| Supply or consume SDP | `get_sender_sdp`, receiver configuration | Read/generate truthful sender SDP or apply receiver SDP only when advertised. See [SDP frames](docs/PROTOCOL.md#ipt-configuration-and-sdp) and the optional [main/spare walkthrough](docs/REDUNDANT_SDP.md). |
| Detect and repair configuration drift | `check_ipt_configuration` | Compare supported fields with actual device state; reapply and reread when `set_again` is true. Follow that branch of `dispatch` in [application.rs](src/application.rs). |
| Select internal processing resources | Resource links and `sub_index` | Use only for devices that need resource routing. See [receiver selection](docs/PROTOCOL.md#selecting-a-receiver-resource) and [advanced configurations](docs/PROTOCOL.md#partial-failure-and-advanced-forms). |

## 2. Follow one control from definition to device

The example's device-level `audio_gain` is a useful first lesson. Its range of −20 to +20 dB in 0.5 dB steps is **simulator policy**, not an API requirement. For your device, use its real range and conversion rules.

**Describe it:** `get_structure.data.dynamic_values.audio_gain` contains type, range and access metadata. `Registry::new` supplies that definition; `Parameter::validate` enforces it. The default initializes simulated state—it does not instruct a real device to reset on connection.

**Read it:** VideoIPath sends this request; the driver calls the device and responds with the observed value. Here the initial observation is 0 dB:

```json
{"message_id":10,"message_type":"get_dynamic_values","data":{"location":null,"ids":["audio_gain"]}}
```

```json
{"message_id":10,"message_type":"get_dynamic_values","data":{"audio_gain":0.0}}
```

**Set it:** translate the value into a vendor command and read the device back. These are three separate frames, in order, for a confirmed change from 0 to 2.5 dB:

```json
{"message_id":11,"message_type":"set_dynamic_values","data":{"location":null,"values":{"audio_gain":2.5}}}
```

```json
{"message_id":11,"message_type":"set_dynamic_values","data":{}}
```

```json
{"message_id":null,"message_type":"async_values_changed","data":{"location":null,"values":{"audio_gain":2.5}}}
```

An empty set-response map means no failed IDs; it is not a value echo. A partial failure instead returns failed IDs mapped directly to error strings. Send a causal change only **after the response is delivered**, and only for an observed change. Never replay an action just because its acknowledgement was lost.

Trace the lesson in this order:

```text
registry.rs: definition + validation
  → application.rs: dispatch (get_dynamic_values) / set_values
  → domain.rs: Device::read_values / Device::set_value
  → device.rs: simulator adapter (replace with your vendor API)
  → application.rs: readback + result
  → session.rs: response, then deferred change
```

For module/port controls, keep the same local ID but supply its structured `location`; do not flatten a UI catalog address into the value map. See the [full frame guide](docs/PROTOCOL.md#dynamic-values) for scopes, read-only status, partial errors and write-only actions.

## 3. Run the lessons without VideoIPath

From this directory, with Rustup installed (the toolchain is pinned):

```sh
cargo build --locked
cargo test --locked --test documented_frames
cargo test --locked --test capabilities
cargo test --locked --all-targets
```

The tests send protocol requests to handlers and use fake-backend/device boundaries to demonstrate the expected outcomes. `documented_frames` checks the walkthrough frames; `capabilities` includes malformed requests, partial writes, authoritative alarms and configuration drift. None of these commands needs a VideoIPath token or physical device.

For interactive changes, use the [native simulator controls](docs/SIMULATOR.md#native-controls) and [scenarios](docs/SCENARIOS.md). `simctl` changes the simulated device; it **does not send SPEC frames**. The [native end-to-end test](tests/native_end_to_end.rs) demonstrates the actual driver process with a local TLS test backend. The simulator models control state, not media transmission.

## 4. Adapt the example to your device

Start with the [worked adaptation guide](docs/EXTENDING.md), not a copy of every simulated feature.

| Keep the principle | Replace for your device | Leave out unless needed |
| --- | --- | --- |
| Definitions and runtime validation agree | Parameter IDs, units, ranges and vendor conversions | Demonstration values, hidden namespaces and unused scopes |
| Stable identity and truthful structure | Product identity, modules, ports and interface references | Media endpoints on a monitoring-only device |
| Read actual state; confirm writes | Simulator adapter with one vendor API adapter | Simulator process, control socket, private revisions and fault controls |
| Respect operation-specific response shapes | Supported media fields and normalization | SDP/FEC/SRT, SIPS and resource links the device cannot honor |
| Bound work; isolate reconnects and cancellation | Timeouts, pacing and event reconciliation for the vendor API | This example's exact module/trait layout as a supposed API requirement |

Keep vendor types and transport errors inside your adapter. The sample `Device` trait is a teaching boundary, not an SDK interface mandated by SPEC-23; trim or split it for your capability set. Do not fill unused methods with fake success.

## 5. Connect and commission your driver

The driver is the client of `wss://<virtual-ip>/x-driver/v1?node_id=<stable-uuid>`, with an `Authorization: Bearer <token>` header. Preserve the node UUID for the same physical device and run **one active owner**; a second session with that ID replaces the first. Verify TLS and never put tokens in URLs, logs or command arguments.

Preserve each request ID and successful response type. Unsupported/failed requests receive `error`; incoming `error` messages get no reply. Echo keepalives only while device contact is healthy, and keep slow device I/O from blocking them. The [transport walkthrough](docs/PROTOCOL.md#transport-and-envelope) and [session code](src/session.rs) show these rules.

For containers, follow [OPERATIONS.md](OPERATIONS.md) and the complete [.env.example](.env.example). The supplied Compose deployment runs the driver and simulator separately; replace the simulator service and private socket settings when integrating your device. No prebuilt registry image is published. VideoIPath does not deploy this process, and an external driver is not an *external driverhost*.

Commission in order: authenticated connection → accepted structure → discovery/Inventory **Device UP** → full parameter catalog → read → controlled write with device readback and restoration → independent event/Panel update → loss/reconnect recovery. Record the exact VideoIPath release/backend SHA and device firmware. A populated picker, successful local test or healthy container alone does not establish working device control.

## Reference material when you need it

| Question | Read |
| --- | --- |
| What are the complete requests, responses and failure shapes? | [Protocol frames](docs/PROTOCOL.md) |
| How do I map my device to these operations? | [Adaptation guide](docs/EXTENDING.md) |
| How can I exercise alarms, drift, GPIO and advanced configuration? | [Scenarios](docs/SCENARIOS.md), [paired SDP](docs/REDUNDANT_SDP.md) |
| Why does this Rust implementation serialize/cancel work this way? | [Design notes](DESIGN.md) |
| How do I deploy, rotate credentials, upgrade or roll back? | [Operator runbook](OPERATIONS.md) |
| Which variants are implemented and what still needs evidence? | [Coverage](docs/COVERAGE.md), [wire questions](docs/WIRE_EVIDENCE.md), [validation](VALIDATION.md) |
| How do I exercise longer-running recovery and resource bounds? | [Opt-in soak](docs/SOAK.md) |

The SDP examples cover a bounded subset, not full ST 2110; populated IP overrides and sender-to-sender partner selection still require authoritative wire evidence. Do not infer physical SRT, FEC or redundancy capability from simulated configuration storage. Keep connection orchestration and VideoIPath database access outside your driver.

See the [contribution guidelines](../../CONTRIBUTING.md), [code of conduct](../../CODE_OF_CONDUCT.md) and [distribution notes](THIRD_PARTY.md) before contributing or packaging. This example is provided “as is,” without warranty; see the [license](../../LICENSE) and repository disclaimer.

[Repository guide](../../README.md) · [All examples](../README.md) · [Nevion VideoIPath](https://nevion.com/videoipath)
