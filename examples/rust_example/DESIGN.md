# Design: API lessons, not a device platform

The default executable is one external driver. `DemoDevice` supplies a small
in-memory implementation of the device operations so every API lesson can run
without vendor hardware, a second service, sockets or a simulator-control CLI.
It is explicitly demonstration state, not physical readback or media delivery.

```text
VideoIPath → session → protocol/application → Device → DemoDevice
                                                    replace with your vendor API
```

## Read the API, then replace the stand-in

| File | Lesson |
| --- | --- |
| `registry.rs` | Describe controls, stable scoped IDs, ports and truthful capabilities |
| `application.rs` | Respond to get/set/alarms/SDP/IPT/check, including partial failures and readback |
| `protocol.rs` | Preserve wire identities and response shapes; normalize supported configuration |
| `domain.rs` | Small device operations to implement; no prescribed vendor transport |
| `demo.rs` | In-memory reads, writes, actions, alarm snapshots and independent configuration state |
| `sdp.rs` | Generate the supported single/paired H264/H265 SDP subset from observed state |
| `main.rs`, `session.rs` | Authenticated connection, bounded dispatch, correlation, causal events and recovery |

`DemoDevice::tick` is a deliberate teaching input: every ten seconds **while
connected**, the read-only `signal_present` alternates and a non-service-affecting
warning is raised/cleared. It is not a physical sensor poll. Tests call `tick`
directly without waiting. Replace this clock with real vendor events in a driver;
do not copy it as a production monitoring strategy.

## Ownership and failure rules

| State or operation | Owner and bound | Failure / retry rule |
| --- | --- | --- |
| Node identity | Externally configured UUID; main holds the OS lock before connecting | Same-host duplicate fails without network I/O; no active/active owners |
| Values, one demo alarm, actions, configurations | DemoDevice; one short mutex, no await while held; at most 128 configured resources | Exact validation; overflow/error instead of unbounded growth; no automatic write retry |
| Structure and parameter metadata | Immutable Registry | Validate before opening a session; IDs do not move with array ordering |
| Requests / outbound messages | Session; configured finite limits and local 5/20-second default deadlines | Correlated operation-specific errors, overload rejection, cancel/drain retired work |
| Request-caused changes | Handler returns observations; session awaits response delivery | No adapter-side echo; unchanged writes emit no change; ambiguous hardware outcomes need reconciliation |
| Teaching events | One session-owned timer/producer; bounded queue | On loss/overflow retire the session, not silent edge dropping; cancel/join on closure |
| WebSocket establishment | Main supervisor | Jittered bounded backoff for transient loss; fail fast for permanent auth/config/TLS failures |
| Local health | Heartbeat file owned by process | Reports liveness only; readiness separates model availability from backend connection |

Demo state survives a **WebSocket reconnect** because the model is constructed
once. It deliberately resets on process restart; only node identity and diagnostic
files survive container recreation. These are sample choices, not persistence
instructions for a physical device.

Only the read-only signal changes independently in the connected demo. There is
no race between that signal and a backend write to the same identity. A vendor
adapter with writable front-panel controls needs its own documented observation,
normalization and echo-reconciliation rules. Do not invent revisions or return a
cached request as physical state. The default session permits these declared
read-only/alarm events to interleave with unrelated writes; request-caused changes
still wait for their own response. The optional lab retains conflict detection
and retirement for events that can overlap writable state.

## Optional transport laboratory

The previous two-process simulator, Unix adapter, private revision scheme and
fault-injection CLI are retained under [`tools/device_lab`](tools/device_lab/).
They compile only with `--features device-lab`. The default driver library, binary
and Docker image do not include them; `src` has no dependency on their private
protocol. They are **test equipment**, not steps in implementing the API.

The lab shares API models and the SDP formatter with the example, but does not
define its public configuration. Legacy transport/lifecycle/soak tests are optional.
Default tests cover API capability semantics and the in-memory example; the lab
adds independent-process failure evidence. Neither is live VideoIPath acceptance.

## Verification boundaries

The later documented forms (`/x-driver/v1`, array IPT errors, `port_id`,
`async_values_changed`) are used. No formal target schema or live release has been
provided. Preserve [wire evidence limits](docs/WIRE_EVIDENCE.md) when adapting them.
Full ST 2110 and physical FEC/SRT/redundancy are not implemented by memory storage.
See [EXTENDING.md](docs/EXTENDING.md) for vendor adaptation and
[VALIDATION.md](VALIDATION.md) for dated local versus target-release evidence.
