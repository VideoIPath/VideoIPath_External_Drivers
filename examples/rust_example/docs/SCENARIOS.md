# Explore the External Driver API

Run the complete offline lesson from the example directory:

```sh
cargo run --locked --example api_walkthrough
```

No backend, token, device socket, second process or container is needed. The
walkthrough calls the real API handlers with `DemoDevice`, printing requests,
responses and changes. Structure is summarized rather than dumping the entire
catalog. Read the [walkthrough source](../examples/api_walkthrough.rs) beside the
[complete protocol frames](PROTOCOL.md).

## Choose a lesson

| Device behavior | API lesson | Where to look |
| --- | --- | --- |
| Describe a control | `get_structure` returns definitions, not readings | `Registry::new`, `Parameter::definition` |
| Read gain or temperature | `get_dynamic_values` returns observed known values at the requested location | `application::dispatch`, `DemoDevice::read_values` |
| Set gain | `set_dynamic_values` validates, applies and confirms; failed IDs form a direct map | `application::set_values`, `DemoDevice::set_value` |
| Press a write-only trigger | Each invocation is an action, not persistent readable state | `DemoDevice::action_count`; no blind retry on lost acknowledgement |
| Change a local signal | `async_values_changed` uses canonical location/local IDs | `DemoDevice::tick`; independent of a backend write |
| Raise/clear a warning | Full `alarms` snapshot plus optional async raise/clear | `DemoDevice::tick`, `DemoDevice::alarms`; omission means clear |
| Enable a media sender/receiver | Apply supported fields, read them back, return errors per port | `application::set_ipt`, `protocol::media_state` |
| Read/use SDP | Sender returns string/null/error; receiver must support the supplied SDP | `sdp.rs`, [paired SDP lesson](REDUNDANT_SDP.md) |
| Detect a front-panel configuration change | Check actual state; repair only when `set_again` is true | `check_ipt_configuration` branch and offline drift lesson |
| Disable or release an indexed resource | `enabled_config: null` removes that resource, not its siblings | `DemoDevice::configure`; `(module, port, direction, sub_index)` |
| Select an internal route | Advertised `rx-tx` choices constrain `resource_partner` | [Selection frames](PROTOCOL.md#selecting-a-receiver-resource) |

The standalone lesson is deliberately finite and deterministic. It changes model
state directly to demonstrate drift—there is no invented device-control API.
Model state is independent of the request object, but is **not physical readback**.

## Observe unsolicited updates when connected

`rust-example driver` opens the authenticated VideoIPath session. While connected,
a teaching input runs every ten seconds:

1. `signal_present` changes from true to false; an async value update and a warning
   named `Demo signal absent` are emitted. The full alarm poll contains that warning.
2. On the next tick the signal becomes true; a value update and alarm-clear message
   are emitted. The next full alarm poll omits the warning.

The warning is explicitly non-service-affecting. There is no real signal loss.
The input is read-only to backend writes, preventing a misleading same-value
write/event race. `DemoDevice::tick` makes these steps callable directly in tests;
your vendor adapter should replace the timer with real device observations.

Writes to ordinary parameters use the handler's separate response-then-change
path. Repeating an unchanged write does not generate another change; repeating a
write-only action still invokes it. Do not use the demo clock as justification for
polling demand-only physical sensors or advertising GPI without reliable events.

## Run focused tests

```sh
cargo test --locked --test documented_frames
cargo test --locked --test demo_device
cargo test --locked --test capabilities
cargo test --locked --test native_demo
```

The last test launches the actual example with a local TLS test backend; it does
not contact VideoIPath. For an actual target, follow the separate
[commissioning checklist](../OPERATIONS.md#target-commissioning--deferred).

## Optional device-transport laboratory

Transport corruption, delayed replies, stale revisions, Unix socket recovery and
long-running two-process soak tests belong to [optional lab tooling](../tools/device_lab/),
not this API learning path. That tooling is excluded from default builds and the
driver image. You do not need to read or run it to implement a new driver.
