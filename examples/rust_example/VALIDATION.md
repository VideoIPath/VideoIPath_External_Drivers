# Validation record

## Scope of this record

This example models device control state, not media processing. No live VideoIPath
instance, release/backend SHA, implemented backend decoder or accepted target capture
has been supplied. Live commissioning is a separate work item. Neither successful
local tests nor a healthy container establish VideoIPath compatibility.

There is no published formal external JSON Schema in the reference set used here.
The Rust protocol models are derived implementation models, not an official schema.

## Reproducible dependency selection

Checked on 2026-09-30 against the official Rust stable channel manifest and crates.io
release metadata. Only non-yanked stable releases were considered. `Cargo.lock`
records the resolved graph; `rust-toolchain.toml` fixes Rust 1.98.1. Edition 2024 is
used. Minimum supported compiler is this tested toolchain, not an older guessed MSRV.

| Direct dependency | Selected release | Purpose |
| --- | --- | --- |
| Tokio | 1.53.1 | Bounded asynchronous runtime and OS I/O |
| Tokio-Tungstenite | 0.30.0 | WebSocket client and local test server |
| Tokio-util | 0.7.19 | Cancellation and framing utilities |
| Serde / serde_json | 1.0.229 / 1.0.151 | Typed models and JSON boundary |
| Rustls | 0.23.45 | Verified TLS using its ring provider |
| rustls-pemfile / rustls-native-certs | 2.2.0 / 0.8.4 | Custom CA and system trust loading |
| tracing / tracing-subscriber | 0.1.44 / 0.3.23 | Structured, sanitized diagnostics |
| thiserror | 2.0.21 | Explicit internal failure categories |
| UUID / URL | 1.26.1 / 2.5.8 | Stable identity and safe URL construction |
| futures-util | 0.3.34 | Stream/sink adapters and bounded task results |
| rand | 0.10.3 | Reconnect jitter |
| tempfile | 3.27.0 | Isolated local test state |
| rcgen / tokio-rustls | 0.14.10 / 0.26.6 | Test-only certificates and TLS servers |

Sources: [Rust release channel](https://static.rust-lang.org/dist/channel-rust-stable.toml),
[Rust releases](https://releases.rs/), and each package's [crates.io](https://crates.io/)
release history. A future reader must recheck releases rather than interpreting this
dated selection as an evergreen “latest” claim. Runtime dependencies and test-only
dependencies are separated in `Cargo.toml`.

The implementation uses standard-library file locking instead of a separate legacy
locking crate and native async trait futures rather than allocating boxed futures
for each device capability. The session handler's boxed future is deliberately a
transport/test boundary, not a generic framework.

## Local checks

Run from this directory:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo doc --locked --no-deps
```

Record completed executions and environmental limitations below as implementation
proceeds. An unchecked gate is not a passing gate.

| Gate | Current evidence |
| --- | --- |
| Native Rust compilation and tests | 82 automatic tests passed; Linux x86_64, Rust 1.98.1 |
| Formatting and static analysis | `cargo fmt --all --check`, strict all-target Clippy, and rustdoc passed |
| TLS/authentication behavior | Six real local TLS tests passed, including custom CA, hostname mismatch, untrusted CA and HTTP 401 |
| Simulator and socket lifecycle | 16 tests passed, including pre-apply cancellation, stale revisions, bounded alarm snapshots and safe socket recovery |
| Compose/environment contract | Parity/default tests passed; additional Docker Compose interpolation test explicitly run and passed (83 total tests) |
| Clean container build and runtime smoke | Static musl image built; final crash-recovery smoke recorded below |
| Runtime-image vulnerability scan | Static runtime candidate passed blocking scan; exact final image identity recorded below |
| Load and latency | 128 configured streams, 10 complete cyclic-check rounds and per-port capacity failure passed; local measurements below |
| Host reboot and boot launcher | Template validation only until an authorized host rehearsal |
| Live VideoIPath commissioning | Deferred: no instance or target evidence available |
| Physical device/media | Not claimed; simulator only |

### Security findings retained during development

The first Debian slim runtime scan failed the blocking policy: unused OpenSSL
packages carried fixable high-severity CVE-2026-75804 and CVE-2026-84782 findings.
A distroless `cc` candidate still contained the same unnecessary SSL library. These
were rejected rather than waived. The runtime was reduced to a statically
linked binary and pinned distroless static base; its final scan is recorded below.
The locked Cargo graph scan found no HIGH/CRITICAL vulnerabilities under the same
fixable-vulnerability policy. Scanning a stripped runtime alone does not inspect
its Rust dependency graph, so CI runs both checks.

### Final local container candidate

- Local image ID: `sha256:451f5298d8892ab6370804f971c0270eb4c2d389f9eb155194afd46b4da0fb78`.
- Local tag: `rust-example:local-0.1.0`; this is a development artifact, not a published release or registry manifest digest.
- Docker-reported image size: 3,232,406 bytes; fixed runtime UID/GID `10001:10001`.
- Clean source compilation in the pinned musl builder passed. Runtime has no shell, compiler or package manager.
- `scripts/runtime-smoke.sh` passed against this candidate: hardened two-service startup, local liveness, independent simulator changes, duplicate-node refusal, driver restart, simulator SIGKILL/restart with safe stale-socket recovery, and bounded graceful stop. Its temporary project/volumes were removed.
- Trivy **0.74.0**, dated database fetched 2026-09-30: zero fixable HIGH/CRITICAL findings in the exact saved runtime image. `Cargo.lock` was separately scanned under the same blocking policy. This is a scoped scan result, not a claim that no vulnerability can exist.
- No live VideoIPath traffic, registry publication, installed host service or host reboot was involved. Docker used its installed legacy builder; BuildKit/provenance generation was not available locally and is not claimed.

### Local performance observations

Measured on 2026-09-30, Linux x86_64, 22 logical CPUs, shared development host.
These are individual observations, not latency guarantees, production soak
qualification, or VideoIPath measurements. Concurrent build load can affect tails.

| Workload | Build/path | Observation |
| --- | --- | --- |
| 2,000 sequential root snapshots after 100 warmups | Native release, actual Unix socket, in-process simulator task | p50 22 µs; p95 31 µs; p99 63 µs; maximum 704 µs |
| 1,000 sequential requests: 500 keepalives + 500 parameter reads | Debug driver child, loopback TLS fake backend, independent simulator owner | 358 ms total; p50 328 µs; p95 655 µs; p99 827 µs |
| Native driver termination and restart | Same end-to-end fixture, preserved UUID | 1 ms / 2 ms shutdown observations |

Reproduce with `cargo run --locked --release --example latency` and
`cargo test --locked --test native_end_to_end -- --nocapture`. The latter also
checks real writes/events, drift repair, singleton refusal, identity continuity,
and readiness clearing. Long-duration soak and real deployment load remain
qualification work, not implied by these short repeatable workloads.

## Future VideoIPath acceptance

Record exact release/backend SHA, driver revision/image digest, simulator scenario,
and sanitized accepted/rejected frames. Validate in order; stop at the first failed
layer instead of trying to repair a Panel before its device/catalog is valid.

| Layer | Required evidence |
| --- | --- |
| Wire compatibility | Backend decoder/model accepts the selected path, models, defaults, and async tokens |
| Discovery and Inventory | One stable candidate, operator add, accepted structure, Device UP |
| Metadata catalog | Complete collector map and frontend decoding, including optional range metadata |
| Reads | All structured locations and requested-ID cases; empty-list policy confirmed |
| Writes | Safe set, independent simulator readback, restored initial state, per-item errors |
| Panel and async | Correct controls/read-only display; independent change delivered once |
| Media control | SDP, enable/disable, redundancy, IPT drift repair, routing/multiplicity cases |
| Recovery | Device/VIP loss, stable identity, saved references, stale subscriptions and reconnect |

Any later physical-device adapter needs its own firmware/API evidence, physical
readback/restoration and media validation. Simulator acceptance cannot substitute
for those checks. Production or critical-equipment use requires appropriate
target-specific qualification and explicit operational risk acceptance.
