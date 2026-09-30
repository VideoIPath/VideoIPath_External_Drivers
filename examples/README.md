# Examples

Use these examples to learn the VideoIPath External Driver API, then map the
operations to your own device. Choose only the capabilities your device supports.

| Example | Use it to learn |
| --- | --- |
| [`rust_example`](rust_example/) | A control from definition through read/write/readback and async update, followed by alarms, SDP and IP configuration; runnable tests and a separate simulated device |
| [`Python`](Python/) | A compact introduction to connection, structure and dynamic-value handling; not the broader media/lifecycle coverage of the Rust example |

Start with the [Rust capability map](rust_example/README.md#1-start-with-the-capability-you-need)
and [adaptation guide](rust_example/docs/EXTENDING.md). Protocol lessons transfer
across languages; the simulator transport and Rust module layout are not API
requirements. Deployment details and local qualification results are linked from
each example rather than prerequisites for learning the API.
