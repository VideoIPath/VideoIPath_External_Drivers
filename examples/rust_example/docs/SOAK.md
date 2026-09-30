# Local native soak qualification

This Linux-only harness runs the **actual Cargo-built driver child**, a **separate
simulator child**, and a loopback TLS/WebSocket fake backend. It exercises the
composition root and Unix socket device boundary. It does not contact VideoIPath
or a physical device and does not qualify a release, media path, or production load.

Measured path (all on the same host):

```text
tests/soak.rs fake backend
  -> loopback TLS/WebSocket -> target/debug/rust-example driver
  -> private Unix socket -> target/debug/rust-example simulator
```

Independent mutations originate from the test's simulator-control client on a
second private Unix socket; they do not enter through the fake backend.

## Run

From `examples/rust_example`, with the pinned Rust toolchain on `PATH`:

```sh
cargo test --locked --test soak soak_two_cycle_regression -- --exact --nocapture
SOAK_SECONDS=120 sh scripts/soak.sh
```

`SOAK_SECONDS` is test-only, defaults to 60 seconds, and accepts integers from 2
through 86400. The elapsed test is ignored by normal `cargo test`; the script
explicitly selects it with `--ignored --exact --nocapture`. The automatic
two-cycle regression uses the same workload and performs both recovery paths in
each cycle. Neither mode adds production configuration or dependencies.

The script uses the debug profile. Run the equivalent Cargo invocation with
`--release` explicitly if collecting a separate release-build result; identify the
profile in any report. Linux `/proc`, a `kill` executable, loopback networking, and
Unix-domain sockets are required. Cargo may need dependency/toolchain downloads;
the actual fixture has no external network dependency.

## Workload and bounds

| Exercise | Evidence checked |
| --- | --- |
| Structure and keepalive | Valid local structure; correlated keepalive response |
| Dynamic writes and reads | Confirmed level write, causal event after response, fresh readback; one expected invalid-mode item error per cycle |
| Independent changes | Simulator control socket changes level; driver forwards async event and reads back independently owned state |
| Alarms | Empty/raised/cleared authoritative snapshots; exact raise, clear, and poll-trigger events |
| IPT | Sender application, match, deliberate simulator drift, mismatch, repair, and confirmed match |
| Backend reconnect | Abruptly drop fake backend socket; authenticate new TLS session and verify retained values, empty alarm state, and IPT configuration |
| Target disconnect | Simulator closes existing target connections; observe old backend loss, accept recovery, and verify retained state |
| Shutdown | Both children exit successfully on SIGTERM; readiness/liveness and simulator sockets are cleaned up |

Elapsed runs schedule one cycle per 250 ms using a monotonic interval, skipping
missed ticks rather than building a backlog. Both faults run on the first cycle
and every twentieth cycle thereafter. There are no guessed readiness sleeps:
socket/readiness conditions, frames, and process state determine readiness.
Individual asynchronous operations and each complete recovery have 10-second
deadlines, workload cycles a 40-second deadline, and the whole test a
requested-duration-plus-90-second bound.
The two-cycle regression has a 90-second overall bound. A cycle already in flight
may finish after the requested duration. This is paced sequential control-plane
traffic, not a concurrency or saturation benchmark.

Children receive an explicit cleared environment, synthetic bearer and ephemeral
test CA, fixed fixture identity, private mode-0700 scratch directory, and randomly
allocated loopback port. Each fixture has its own locks and sockets. Scratch lives
under `/tmp/opencode` when available, otherwise `/tmp`. Child logs are private,
checked for synthetic secret/certificate leakage, never printed, and removed on
cleanup. Each log has a 64 MiB checked size guard; exceeding it fails the run.
This is checked at cycle/readiness boundaries, not OS-enforced log rotation. The
fixture intentionally enables driver trace-level diagnostics to exercise
redaction; the log cap can end very long runs before their requested duration.
The fixture kills/reaps its children on assertion failure as well as normal cleanup;
an externally SIGKILLed test runner cannot execute Rust cleanup.

## Metrics and interpretation

| Metric | Definition |
| --- | --- |
| `workload_ms`, `cycles` | Monotonic elapsed workload time after initial handshake/structure; completed cycles, including injected fault recovery time; excludes shutdown |
| `requests`, `responses` | Backend request attempts and received response frames, including startup structure and post-recovery verification |
| `error_responses` | Unexpected top-level protocol error responses; these fail immediately |
| `expected_item_errors` | Deliberate invalid-mode rejection, one per cycle; not an unexpected failure |
| `test_failures` | Zero on completed qualification, one on an incomplete/failing run; includes assertions, timeouts, and transport failures rather than claiming a count of every downstream symptom |
| `controls`, `events` | Simulator control calls and received/checked async frames |
| `backend_reconnects`, `target_recoveries` | Completed injected recoveries, counted only after state readback succeeds |
| Per-operation latency | Request send through correlated response receipt; excludes deferred event reception and pacing |
| Recovery latency | Fault injection through new handshake, readiness, and all three state checks |
| RSS | Driver-only Linux `VmRSS` after startup and before shutdown, maximum sampled RSS, plus kernel `VmHWM` observed across samples |

Latency uses **25 fixed power-of-two microsecond buckets**, per fixed operation
name, with no growing sample list. Reported p50/p95/p99 are bucket **upper bounds**,
not exact quantiles. The last bucket reaches 16,777,216 µs; successful operations
and recoveries are expected well inside it (a slower recovery fails the histogram
range guard). Request/event bodies are not emitted in diagnostics. Failed runs
report partial counts and the most recently completed-cycle elapsed time.

RSS is sampled after every cycle and successful recovery. Kernel high-water RSS
also catches peaks between samples. Guards are **256 MiB absolute high-water** and
**64 MiB high-water growth over startup RSS**. These intentionally generous native
debug-build limits detect gross regressions while allowing allocator/TLS/runtime
warm-up and host variation. They do **not** demonstrate absence of memory leaks,
bound simulator/test-runner memory, or establish a production memory budget.

## Recorded local evidence

Measured on 2026-09-30 from the repository's `examples/rust_example` worktree:

| Environment | Value |
| --- | --- |
| Package/build | `rust-example` 0.2.0, native Cargo debug, unoptimized + debuginfo; in-progress local worktree, not a release artifact |
| Compiler | Rust 1.98.1 (`48a229cea`, 2026-09-01), LLVM 22.1.8 |
| Platform | `x86_64-unknown-linux-gnu`, Linux `7.0.0-34-generic` |
| CPU | Intel Core Ultra 7 165H; shared host, no CPU isolation or controlled background load |
| Runtime | Tokio multi-thread, two workers each for test runner, driver, and simulator |
| Transport | Loopback TLS with generated CA; Unix-domain target/control sockets; fake backend TCP_NODELAY |

Exact long-run command (the scratch toolchain paths are specific to this host):

```sh
env PATH="/tmp/opencode/cargo/bin:$PATH" \
  CARGO_HOME=/tmp/opencode/cargo RUSTUP_HOME=/tmp/opencode/rustup \
  SOAK_SECONDS=120 sh scripts/soak.sh
```

The two-cycle regression passed in 0.55 seconds total (533 ms workload): 37
requests/responses, 14 control calls, 10 async events, two backend reconnects,
two target recoveries, two expected invalid-mode errors, zero unexpected error
responses, and zero test failures. Driver RSS start/end/high-water was
14,520 / 14,788 / 14,788 KiB. This short run verifies the harness/recovery path;
it is not the elapsed-time result.

The final elapsed-time rerun **passed**: 120,001 ms measured workload, 120.03 seconds
total test execution (excluding compilation). This rerun follows a review fix:
nanosecond observations now round **up** to microseconds before histogram bucket
selection, so a fractional value cannot exceed its reported bucket upper bound.
The rounding boundaries have a separate automatic regression test. Its summary:

| Measurement | Result |
| --- | --- |
| Completed cycles | 480 |
| Backend requests / responses | 5,905 / 5,905 |
| Unexpected error responses / test failures | 0 / 0 |
| Expected invalid-mode item errors | 480 |
| Simulator control calls / async events | 2,904 / 2,400 |
| Forced backend reconnects / target recoveries | 24 / 24 |
| Driver RSS start / end | 14,520 / 14,972 KiB |
| Maximum sampled RSS / kernel high-water | 14,972 / 14,972 KiB |
| Observed high-water growth over startup | 452 KiB |

Per-operation histogram results; all times below are **microsecond bucket upper
bounds**, including recovery time:

| Operation | Count | p50 ≤ µs | p95 ≤ µs | p99 ≤ µs |
| --- | ---: | ---: | ---: | ---: |
| `alarms` | 1,488 | 1,024 | 2,048 | 2,048 |
| `check_ipt_configuration` | 1,968 | 1,024 | 2,048 | 4,096 |
| `get_dynamic_values` | 1,008 | 1,024 | 2,048 | 2,048 |
| `get_structure` | 1 | 4,096 | 4,096 | 4,096 |
| `keepalive` | 480 | 1,024 | 2,048 | 2,048 |
| `set_dynamic_values` | 480 | 2,048 | 4,096 | 4,096 |
| `set_ipt_configurations` | 480 | 2,048 | 4,096 | 4,096 |
| Both recovery types combined | 48 | 131,072 | 262,144 | 262,144 |

The singleton structure timing is a startup observation, not a latency
distribution. The 452 KiB RSS change is a two-minute local observation, not a
growth-rate extrapolation or proof of leak absence. The run completed both
graceful shutdowns, cleanup assertions, and synthetic-secret log checks.

### Five-minute recovery run

An additional `SOAK_SECONDS=300 sh scripts/soak.sh` run passed with 300,000 ms
workload and 300.05 seconds total execution:

| Measurement | Result |
| --- | --- |
| Cycles / exchanges | 1,200 / 14,761 |
| Async events / control calls | 6,000 / 7,260 |
| Backend reconnects / target recoveries | 60 / 60 |
| Unexpected errors / test failures | 0 / 0 |
| Expected item errors | 1,200 |
| Driver RSS start / end / high-water | 14,632 / 15,052 / 15,052 KiB |

This run began before the histogram rounding correction. Its latency percentiles
are not retained as upper-bound evidence; counts, recovery assertions and RSS are
unaffected. The final two-minute rerun above verifies the corrected measurement.
Both used shared host resources; compilation/container builds and other test runs
could overlap. Neither run extrapolates to hours, days or representative media load.

`rustfmt --edition 2024 --check tests/soak.rs`,
`cargo clippy --locked --test soak -- -D warnings`, and
`sh -n scripts/soak.sh` passed using the same toolchain environment.

## Scope remaining

Longer runs on representative hardware, concurrent real endpoint load, production
build/container resource limits, and exact VideoIPath release acceptance remain
separate qualification work. This harness adds local elapsed-time evidence only;
it does not complete the broader implementation plan.
