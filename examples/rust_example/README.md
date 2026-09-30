# VideoIPath Rust Example Driver

<img width="1940" height="520" alt="Nevion VideoIPath external drivers banner" src="https://github.com/user-attachments/assets/e236e8ea-7787-414f-a728-822985695e19" />

$\Large\color{green}{\textsf{About This Example}}$

This is a Rust reference implementation for the [VideoIPath External Drivers repository](../../README.md). It helps contributors understand, build, and test their own external drivers through documented, runnable examples of the External Driver API.

The driver connects the external-driver protocol to a **separate simulated device**. Explore device control, monitoring, and failure recovery without physical equipment. Docker Compose runs both processes; native Rust commands support local development.

Please read the [Contributing Guidelines](../../CONTRIBUTING.md) and [Code of Conduct](../../CODE_OF_CONDUCT.md) before contributing.

> [!IMPORTANT]
> **Buildable, locally tested reference.** Source, a locked dependency graph, simulator controls, tests, and container artifacts are included. The [coverage ledger](docs/COVERAGE.md) distinguishes implemented behavior, limited examples, and remaining work—this is not a claim of complete protocol or release conformance.

> [!WARNING]
> **VideoIPath compatibility is unverified.** No VideoIPath instance is currently available for validation. Integration testing against an identified release and backend SHA will be completed as separate work. Local simulator tests will not establish compatibility with VideoIPath or a physical device.

> [!NOTE]
> This content is provided “as is,” without warranty. See the repository's [license](../../LICENSE) and [disclaimer](../../README.md). This example is a learning and development reference. Validate your implementation against the intended VideoIPath release and target device before deploying it in production.

$\Large\color{green}{\textsf{What is an External Driver?}}$

An external driver is a standalone process that translates between VideoIPath's external-driver protocol and a device API. It runs outside VideoIPath and initiates its own connection to the backend.

This example uses Rust for that translation layer and a simulator in place of a physical device. It is **not** an external driverhost, which is a separate feature for running existing internal VideoIPath drivers outside the backend process.

$\Large\color{green}{\textsf{Architecture}}$

```text
VideoIPath
    |
    | SPEC-23: JSON over a TLS WebSocket
    | Connection initiated by the driver
    |
Rust example driver
    |
    | Separate simulator API: requests, readback, and events
    | Local Unix-domain socket; no published network port
    |
Simulated device
```

The Compose deployment runs the driver and simulator as separate services. The simulator owns its device state, so configuration writes can be checked through independent readback rather than merely echoing the request. Explicit scenarios demonstrate local changes, alarms, configuration drift, and failures.

$\Large\color{green}{\textsf{What This Example Demonstrates}}$

These are control-plane examples, verified locally to the extent recorded in [VALIDATION.md](VALIDATION.md). Consult the [coverage ledger](docs/COVERAGE.md) for the precise limits of each feature.

| Area | Examples |
| --- | --- |
| Identity and structure | Stable node and element IDs; modules; senders, receivers, generic ports, and network interfaces |
| Parameters | Boolean, numeric, and enumerated string values; device, module, and port scopes; read-only, read-write, and write-only access |
| GPIO and events | GPI/GPO behavior, independent simulated-device changes, and asynchronous value updates |
| Alarms | Full active-alarm snapshots; severities and locations; asynchronous raise, refresh, clear, and poll requests |
| SDP | Sender SDP reads and receiver SDP configuration, including unavailable and unsupported cases |
| IP transport | Sender/receiver enable and disable, main/spare streams, bulk configuration, and per-port failures |
| Format and transport options | Documented format attributes, FEC, and SRT configuration, modeled without generating media |
| Configuration checks | Actual simulated-state comparison, drift detection, repair, and readback |
| Advanced routing | Interface redundancy, receiver-to-sender resource links, configuration ordering, and multi-vertex/sub-index examples; unresolved sender-to-sender semantics remain documented |
| Lifecycle and failures | Authentication, keepalive, reconnect, cancellation, overload, partial failures, and graceful shutdown |

$\Large\color{green}{\textsf{Limitations and Boundaries}}$

- The simulator models **control-plane state**, not real media. Accepting an SRT or FEC configuration does not demonstrate a working SRT or FEC implementation.
- The driver does not create VideoIPath connections, modify groups, program switches, or access the VideoIPath database. Orchestration belongs outside the driver.
- Firmware upgrades, device backup/restore, and automatic active/active driver redundancy are outside this example's scope.
- Device status can be exposed through ordinary read-only parameters; there is no dedicated SPEC-23 metrics message.
- Ambiguous or release-specific protocol details are identified explicitly rather than filled in with invented wire fields. Non-null IP overrides and sender-to-sender partner selection remain blocked on evidence.
- SDP includes single-stream H.264/H.265 and a validated two-stream `DUP` example with matching receiver configuration. Single-stream MP2T video and L16/L24 audio reception are also covered. Full ST 2110 format generation remains outside the implemented subset; see the [redundant SDP walkthrough](docs/REDUNDANT_SDP.md).

## 1. Purpose and Scope

The example aims to cover every documented SPEC-23 capability and meaningful payload variant through a progression from simple monitoring to advanced configuration. The current fixed device model exposes several port types and two modules. [Scenario walkthroughs](docs/SCENARIOS.md) exercise individual features without requiring a different device identity for each lesson.

The implementation emphasizes idiomatic Rust, explicit ownership, asynchronous I/O, secure defaults, bounded resources, and measured latency. Inline comments and Rust documentation explain **why** protocol rules, security checks, and concurrency decisions exist—not simply repeat what the code does.

## 2. Communication Model

The documented connection shape is:

```text
wss://<videoipath-virtual-ip>/x-driver/v1?node_id=<stable-uuid>
```

Authentication uses an `Authorization: Bearer <token>` header, never a token in the URL. The node UUID identifies one simulated device and must remain stable across driver restarts.

The driver preserves request IDs, responds to unsupported requests with correlated errors, and never answers an incoming `error` message. Keepalives reflect device health rather than just process liveness. See the [commented frame walkthrough](docs/PROTOCOL.md).

> [!IMPORTANT]
> One node ID must have one active driver owner. A replacement connection with the same ID supersedes the old connection; scaling replicas for that ID can cause continuous reconnects.

## 3. Rust Design

| Component | Responsibility |
| --- | --- |
| Startup and configuration | Validate immutable settings, stable identity, and singleton ownership before connecting |
| Protocol and session | Encode/decode SPEC-23, correlate requests, supervise reconnects, and serialize bounded outbound traffic |
| Capability handlers | Implement parameter, alarm, SDP, and IP-configuration semantics independently of transport |
| Parameter registry | Keep advertised metadata, canonical identities, validation, and runtime behavior consistent |
| Device adapter | Translate application operations to the simulator API and confirm observed state |
| Simulated device | Own state and generate controlled events, drift, and faults independently of the driver |

Rust **1.98.1**, Edition **2024**, and direct dependency releases were checked against authoritative upstream sources on **2026-09-30**. `Cargo.lock` fixes the tested graph; container bases are digest-pinned. See [VALIDATION.md](VALIDATION.md) for the version inventory and [DESIGN.md](DESIGN.md) for ownership, retries, and dependency boundaries.

## 4. Parameters, GPIO, and Alarms

Examples show successful operations and important edge cases: invalid types, read-only writes, unknown IDs, partial failures, coherent sensor snapshots, and changes occurring outside a driver request.

Changed values caused by a request are published only after its correlated response is delivered. GPI examples use independent simulator events rather than demand reads. Full alarm polling is authoritative: an alarm omitted from the next snapshot is cleared. Private target revisions detect stale changes before transmission; concurrent conflicts retire the session rather than guessing state. Full event-history reconciliation remains outside this example's current implementation.

## 5. Endpoint Configuration and Routing

Sender and receiver examples cover configuration, disabling, the supported SDP subset, redundant streams, and format options. Configuration checks read the simulator's state and repair drift when requested; they do not return unconditional success.

Advanced scenarios explain resource links and dynamically addressed streams separately. Modeled configuration success is distinct from actual media delivery.

## 6. Security, Latency, and Resilience

| Design goal | Approach |
| --- | --- |
| Secure transport | TLS certificate and hostname verification, with explicit trusted-CA support |
| Secret handling | Deployment-time credentials, redacted diagnostics, and no secrets in URLs, command arguments, or images |
| Low latency | Event-driven updates, coherent bulk reads, minimal conflicting-operation serialization, and measured response times |
| Bounded work | Queue, frame, batch, concurrency, and deadline limits; no unbounded task creation |
| Recovery | Bounded reconnect backoff with jitter; fail fast on permanent authentication/configuration/TLS errors |
| Safe writes | Readback and explicit ambiguous-outcome handling; no blind replay of write-only actions |
| Session isolation | Cancel obsolete work and suppress late responses after reconnect or shutdown |

Low latency is a measurement goal, not a universal performance claim. Local tests cover slow-device conditions and saturation. `cargo run --locked --release --example latency` measures Unix-socket snapshot round trips; it does not measure VideoIPath or network latency.

The [opt-in soak workload](docs/SOAK.md) runs the actual driver and simulator as
separate processes, exercises fault/reconnect recovery, and records bounded
latency histograms and driver RSS. Use `SOAK_SECONDS=120 sh scripts/soak.sh`;
normal tests include a short version, while manual CI runs the timed workload.

## 7. Building and Running

From this directory, with Rustup installed:

```sh
cargo build --locked
cargo test --locked --all-targets
cargo run --locked -- --version
```

To explore without VideoIPath, follow the [native simulator setup](docs/SIMULATOR.md#native-controls), then use `simctl` to change values and alarms. No VideoIPath credentials are required for the simulator.

For Docker Compose, start with the [operator runbook](OPERATIONS.md): copy `.env.example` to a protected `.env`, supply your own node identity and connection settings, prepare the shared lock directory, and deliberately build the local image using `compose.build.yaml`. Then run the image-only deployment. An unedited `.env.example` intentionally cannot start a connected driver.

The runtime image is non-root and distroless, with no shell or compiler. No prebuilt registry image is published. Package version is **0.2.0**; Linux amd64 is the locally exercised platform, not a promise of other-platform or VideoIPath-release compatibility.

## 8. Development and Test Sequence

| Step | Milestone | Required evidence |
| --- | --- | --- |
| 1 | Foundation | Verified dependency versions, startup validation, secure deployment skeleton |
| 2 | Protocol and structure | Typed codecs, validated references, positive and negative fixtures |
| 3 | Simulator and session | Independent state/readback/events; fake-backend lifecycle and failure tests |
| 4 | Monitoring and control | Scoped parameters, GPIO, alarms, partial failures, and event ordering |
| 5 | Media configuration | SDP, sender/receiver configuration, drift checks, and advanced scenarios |
| 6 | Local delivery | Native run, clean container build, security scan, load tests, and operator documentation |
| 7 | VideoIPath acceptance | Separate future work against a recorded release/backend SHA |

## 9. Validation and Compatibility

No VideoIPath instance or target-release schema/capture is currently available. No published formal external JSON Schema is available in the reference set used for planning. Derived Rust models and local fixtures must not be presented as official schema conformance evidence.

Future integration work must verify the authenticated session, accepted structure, discovery, Inventory **Device UP**, complete parameter catalog, runtime reads/writes with simulator readback, Panel presentation, asynchronous updates, and recovery. Passing one layer does not prove the next.

## 10. Deployment Notes

- Prefer a managed host outside the VideoIPath cluster. VideoIPath does not deploy or supervise this container.
- Run non-root with a read-only root filesystem, dropped capabilities, and no published ports by default.
- Keep populated environment files and secrets out of source control and image layers.
- Use one driver instance per node ID, with a process-held lock against same-host duplicates. Cross-host ownership remains an operator responsibility.
- Distinguish local liveness from dependency readiness: a healthy container does not prove that VideoIPath is connected.
- Stop the old singleton before an upgrade or rollback and preserve its node identity.

See [OPERATIONS.md](OPERATIONS.md) for commands, CA/token rotation, singleton upgrade/rollback, health semantics, and the optional Linux boot launcher.

## 11. Extending and Contributing

The [extension guide](docs/EXTENDING.md) explains how to replace the simulator adapter with a real device API while keeping protocol and capability boundaries clear. A real driver must advertise only capabilities its target can honor and must be validated against that device's firmware and the intended VideoIPath release.

Contributions should include explanatory inline comments, positive and failure-case tests, and documentation that separates observed behavior from assumptions. Follow the repository's [contribution requirements](../../CONTRIBUTING.md), including its licensing and maintenance policy.

See [third-party and distribution notes](THIRD_PARTY.md) before redistributing a compiled artifact.

[Back to the repository guide](../../README.md) · [All examples](../README.md) · [Nevion VideoIPath](https://nevion.com/videoipath)
