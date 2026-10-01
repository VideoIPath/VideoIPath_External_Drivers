# Optional device-transport laboratory

This directory preserves the richer independent simulator and adapter for testing
stream I/O, fault handling and cancellation. **It is not the example driver** and
is not compiled into the default library, binary or Docker image. Start with the
[API walkthrough](../../README.md#3-run-the-lessons-without-videoipath) instead.

| File | Test-equipment role |
| --- | --- |
| `simulator.rs` | Independent volatile state, Unix framing, external controls and fault injection |
| `client.rs` | One stream reader, reply/event demultiplexing and ambiguous-write handling |
| `main.rs` | Optional `device-lab` executable with legacy driver/simulator/simctl modes |
| `config.rs` | Lab-only socket and device-operation deadline settings; shared secure backend settings |

From `examples/rust_example`:

```sh
cargo test --locked --features device-lab --all-targets
cargo run --locked --features device-lab --bin device-lab -- --help
SOAK_SECONDS=120 sh scripts/soak.sh
```

The elapsed soak starts isolated children and a fake TLS backend; it does not
use a live VideoIPath target. See the [lab controls](../../docs/SIMULATOR.md) for
manual native use and [soak evidence](../../docs/SOAK.md) for scope and dated results.

The optional lab uses `DRIVER_SOCKET`, `DRIVER_CONTROL_SOCKET` and
`DRIVER_OPERATION_TIMEOUT_MS` (defaults `/run/rust-example/device.sock`,
`/run/rust-example/control.sock`, 3000 ms). These settings are **not** supported
options of the lean driver or its `.env.example`. Keep the lab native: the default
Compose contract deploys only the lean driver. No separate lab image is published.

The lab binary reuses SPEC handlers, domain models and the sender SDP formatter;
the driver does not import the lab's private protocol. Lab state survives its
adapter reconnect and restarts independently. That distinction is useful test
evidence, not a requirement that every new driver implement a simulator platform.
