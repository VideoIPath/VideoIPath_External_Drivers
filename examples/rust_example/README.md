# VideoIPath Rust Example Driver

<img width="1940" height="520" alt="Nevion VideoIPath external drivers banner" src="https://github.com/user-attachments/assets/e236e8ea-7787-414f-a728-822985695e19" />

$\Large\color{green}{\textsf{About This Example}}$

This example is being developed as a Rust reference for the [VideoIPath External Drivers repository](../../README.md). Its goal is to help contributors understand, build, and test their own external drivers through documented, runnable examples of the External Driver API.

The planned driver will connect VideoIPath to a **separate simulated device**. This lets developers explore device control, monitoring, and failure recovery without requiring physical equipment. Docker Compose will provide the primary way to run the example.

Please read the [Contributing Guidelines](../../CONTRIBUTING.md) and [Code of Conduct](../../CODE_OF_CONDUCT.md) before contributing.

> [!IMPORTANT]
> **Documentation scaffold — implementation has not started.** The capabilities and architecture below describe the intended example, not currently working functionality. There is no runnable Rust package or container yet.

> [!WARNING]
> **VideoIPath compatibility is unverified.** No VideoIPath instance is currently available for validation. Integration testing against an identified release and backend SHA will be completed as separate work. Local simulator tests will not establish compatibility with VideoIPath or a physical device.

> [!NOTE]
> This content is provided “as is,” without warranty. See the repository's [license](../../LICENSE) and [disclaimer](../../README.md). Treat the external-driver feature and this example as experimental, not as a recommendation for critical equipment.

$\Large\color{green}{\textsf{What is an External Driver?}}$

An external driver is a standalone process that translates between VideoIPath's external-driver protocol and a device API. It runs outside VideoIPath and initiates its own connection to the backend.

This example will use Rust for that translation layer and a simulator in place of a physical device. It is **not** an external driverhost, which is a separate feature for running existing internal VideoIPath drivers outside the backend process.

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

The planned Compose deployment will run the driver and simulator as separate services. The simulator will own its device state, so a driver write can be checked through an independent readback rather than merely echoing the request. Explicit scenarios will demonstrate local changes, alarms, configuration drift, and failures.

$\Large\color{green}{\textsf{What This Example Will Demonstrate}}$

**All entries below are planned.** Coverage means documented protocol behavior with meaningful examples and tests—not just types that deserialize successfully.

| Area | Planned examples |
| --- | --- |
| Identity and structure | Stable node and element IDs; modules; senders, receivers, generic ports, and network interfaces |
| Parameters | Boolean, numeric, and enumerated string values; device, module, and port scopes; read-only, read-write, and write-only access |
| GPIO and events | GPI/GPO behavior, independent simulated-device changes, and asynchronous value updates |
| Alarms | Full active-alarm snapshots; severities and locations; asynchronous raise, refresh, clear, and poll requests |
| SDP | Sender SDP reads and receiver SDP configuration, including unavailable and unsupported cases |
| IP transport | Sender/receiver enable and disable, main/spare streams, bulk configuration, and per-port failures |
| Format and transport options | Documented format attributes, FEC, and SRT configuration, modeled without generating media |
| Configuration checks | Actual simulated-state comparison, drift detection, repair, and readback |
| Advanced routing | Interface redundancy, resource links, configuration ordering, and multi-vertex/sub-index examples |
| Lifecycle and failures | Authentication, keepalive, reconnect, cancellation, overload, partial failures, and graceful shutdown |

$\Large\color{green}{\textsf{Limitations and Boundaries}}$

- The simulator will model **control-plane state**, not transmit or receive real media. Accepting an SRT or FEC configuration does not demonstrate a working SRT or FEC implementation.
- The driver will not create VideoIPath connections, modify groups, program switches, or access the VideoIPath database. Orchestration belongs outside the driver.
- Firmware upgrades, device backup/restore, and automatic active/active driver redundancy are outside this example's scope.
- Device status can be exposed through ordinary read-only parameters; there is no dedicated SPEC-23 metrics message.
- Ambiguous or release-specific protocol details will be identified explicitly rather than filled in with invented wire fields.

## 1. Purpose and Scope

The example aims to cover every documented SPEC-23 capability and meaningful payload variant through a progression from simple monitoring to advanced configuration. Smaller scenarios will keep individual features understandable instead of presenting one contradictory “everything enabled” device.

The implementation will emphasize idiomatic Rust, explicit ownership, asynchronous I/O, secure defaults, bounded resources, and measured latency. Inline comments and Rust documentation will explain **why** protocol rules, security checks, and concurrency decisions exist—not simply repeat what the code does.

## 2. Communication Model

The documented connection shape is:

```text
wss://<videoipath-virtual-ip>/x-driver/v1?node_id=<stable-uuid>
```

Authentication uses an `Authorization: Bearer <token>` header, never a token in the URL. The node UUID identifies one simulated device and must remain stable across driver restarts.

The driver will preserve request IDs, respond to unsupported requests with correlated errors, and never answer an incoming `error` message. Keepalives will reflect device health rather than just process liveness.

> [!IMPORTANT]
> One node ID must have one active driver owner. A replacement connection with the same ID supersedes the old connection; scaling replicas for that ID can cause continuous reconnects.

## 3. Rust Design

| Component | Planned responsibility |
| --- | --- |
| Startup and configuration | Validate immutable settings, stable identity, and singleton ownership before connecting |
| Protocol and session | Encode/decode SPEC-23, correlate requests, supervise reconnects, and serialize bounded outbound traffic |
| Capability handlers | Implement parameter, alarm, SDP, and IP-configuration semantics independently of transport |
| Parameter registry | Keep advertised metadata, canonical identities, validation, and runtime behavior consistent |
| Device adapter | Translate application operations to the simulator API and confirm observed state |
| Simulated device | Own state and generate controlled events, drift, and faults independently of the driver |

At implementation time, stable Rust and direct dependency releases will be checked against authoritative upstream sources. The tested dependency graph will be locked, and container builds will use pinned versions rather than floating `latest` tags. Exact versions have not yet been selected.

## 4. Parameters, GPIO, and Alarms

Examples will show both successful operations and important edge cases: invalid types, read-only writes, unknown IDs, partial failures, coherent sensor snapshots, and changes occurring outside a driver request.

Changed values caused by a request will be published only after its correlated response is delivered. GPI examples will include reliable event updates rather than relying on demand reads. Full alarm polling will remain authoritative: an alarm omitted from the next snapshot is cleared.

## 5. Endpoint Configuration and Routing

Sender and receiver examples will cover configuration, disabling, SDP, redundant streams, and supported format options. Configuration checks will read the simulator's state and repair drift when requested; they will not return unconditional success.

Advanced scenarios will explain resource links and dynamically addressed streams separately. Modeled configuration success will always be distinguished from actual media delivery.

## 6. Security, Latency, and Resilience

| Design goal | Planned approach |
| --- | --- |
| Secure transport | TLS certificate and hostname verification, with explicit trusted-CA support |
| Secret handling | Deployment-time credentials, redacted diagnostics, and no secrets in URLs, command arguments, or images |
| Low latency | Event-driven updates, coherent bulk reads, minimal conflicting-operation serialization, and measured response times |
| Bounded work | Queue, frame, batch, concurrency, and deadline limits; no unbounded task creation |
| Recovery | Bounded reconnect backoff with jitter; fail fast on permanent authentication/configuration/TLS errors |
| Safe writes | Readback and explicit ambiguous-outcome handling; no blind replay of write-only actions |
| Session isolation | Cancel obsolete work and suppress late responses after reconnect or shutdown |

Low latency is a measurement goal, not a current performance claim. Tests will measure request and event latency under normal load, slow-device conditions, and queue saturation without weakening validation or safety checks.

## 7. Building and Running

**Not available yet.** Build and launch commands will be added alongside the working, tested implementation.

The intended delivery includes a locked Rust package, multi-stage Dockerfile, image-only Compose configuration, deliberate local-build override, and a complete `.env.example`. Both a native development workflow and the container workflow will be documented.

No prebuilt image, package version, minimum toolchain version, or release compatibility is claimed at this stage.

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

These are implementation requirements; container artifacts and verified operator commands will follow with the code.

## 11. Extending and Contributing

The eventual extension guide will show how to replace the simulator adapter with a real device API while keeping the protocol and capability boundaries clear. A real driver must advertise only capabilities its target can honor and must be validated against that device's firmware and the intended VideoIPath release.

Contributions should include explanatory inline comments, positive and failure-case tests, and documentation that separates observed behavior from assumptions. Follow the repository's [contribution requirements](../../CONTRIBUTING.md), including its licensing and maintenance policy.

[Back to the repository guide](../../README.md) · [All examples](../README.md) · [Nevion VideoIPath](https://nevion.com/videoipath)
