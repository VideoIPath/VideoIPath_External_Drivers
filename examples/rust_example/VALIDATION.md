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
cargo test --locked --features device-lab --all-targets
cargo doc --locked --no-deps
```

Record completed executions and environmental limitations below as implementation
proceeds. An unchecked gate is not a passing gate.

### 0.3.0 — lean API example (2026-10-01)

The default program now uses in-memory `DemoDevice`; the independent simulator
and Unix adapter are optional `device-lab` tooling outside `src`. Previous image,
latency and soak measurements below **do not qualify this new execution path**.
There is still no target-release/schema/physical-device acceptance evidence.

| Check | Current local evidence |
| --- | --- |
| Default tests | 82 automatic tests passed, including the actual lean binary against a local TLS backend |
| Optional laboratory regression | 113 automatic tests passed with `--features device-lab`, including the default cases; two opt-in tests ignored |
| Compose interpolation | Explicit ignored deployment-contract test passed; now one service with no target socket settings |
| Author lesson | `cargo run --locked --example api_walkthrough` completed offline without environment settings or credentials |
| Static checks | Formatting, all-features/all-targets Clippy with warnings denied, and warnings-denied rustdoc passed |
| New lifecycle | Actual child preserves state on WebSocket reconnect, resets on process restart, emits teaching signal/alarm events, orders causal response/events, and shuts down on SIGTERM |
| Packaging | Source-only static image built; single-service Compose smoke passed (non-root/read-only, liveness, singleton, stable UUID, recreate/restart/stop); no live target |
| Security | Final image and locked Cargo graph passed Trivy 0.74.0 fixable HIGH/CRITICAL policy; source secret scan reported no findings |
| Artifact isolation | Runtime filesystem has no lab executable, source, shell/build tools or secret environment file; `--version` reports 0.3.0 |

Final local image: `rust-example:local-0.3.0`, image ID
`sha256:e4fd6a8e4937482ffe93f38e94173bfb01c76c3474880540eb02207d3b527820`,
Docker-reported 2,938,052 bytes, UID/GID `10001:10001`. This is not a published
registry digest. No accepted deployment was replaced; prior local images remain.
The optional long-duration soak was not rerun (its automatic short regression
passed); the old timing figures remain historical. Local documentation links and
shell syntax were checked. Hosted CI, real reboot and target commissioning remain
unperformed.

The first container build caught an implicit optional-example path that Cargo
could not resolve in the source-only context. Adding the explicit `latency` example
path preserves the opt-in feature without copying test tooling into the image.

Review also caught unrelated demo ticks retiring a session during writes. The
explicit independent-event mode now permits read-only signal/alarm updates to
interleave, while causal writable-value changes still follow their response.
A paused-clock barrier test holds a mutation pending, receives both tick events
and a keepalive, then verifies response/causal ordering. The lab retains its
conservative freshness/conflict policy; vendor reconciliation is not assumed.

### Historical 0.1/0.2 implementation gates

| Gate | Current evidence |
| --- | --- |
| Native Rust compilation and tests | 100 automatic tests passed after the author-guide update; Linux x86_64, Rust 1.98.1 |
| Formatting and static analysis | `cargo fmt --all --check`, strict all-target Clippy, and rustdoc passed |
| TLS/authentication behavior | Six real local TLS tests passed, including custom CA, hostname mismatch, untrusted CA and HTTP 401 |
| Simulator and socket lifecycle | 16 tests passed, including pre-apply cancellation, stale revisions, bounded alarm snapshots and safe socket recovery |
| Compose/environment contract | Parity/default tests passed; explicit Compose interpolation and elapsed soak passed for the preceding 0.2.0 implementation milestone |
| Clean container build and runtime smoke | 0.2.0 static-musl rebuild and hardened two-service singleton/restart/crash-recovery/stop checks passed |
| Runtime-image vulnerability scan | Exact 0.2.0 image and locked Cargo graph passed the blocking fixable HIGH/CRITICAL policy |
| Load and latency | 128 streams × 10 checks passed; five-minute native child-process soak completed with 14,761 exchanges and 120 injected recoveries |
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

### Preserved 0.1.0 local container baseline

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
and readiness clearing. The measurements above belong to the 0.1.0 baseline.
Version 0.2.0 adds the longer elapsed workload below; representative production
load and release-specific qualification are not implied by either result.

### 0.2.0 local coverage extension

The subsequent author-guide update adds an executable README gain-control lesson
and changes production comments only (verified against the preceding commit).
All 100 automatic tests, formatting, strict Clippy and warnings-denied rustdoc passed;
edited guide links/anchors were checked. Container and elapsed-soak results below
remain the implementation milestone's evidence; neither was rerun for prose changes.

- Actual paired H264/H265 SDP generation → receiver validation → target apply/readback,
  including IPv4/IPv6, section/group normalization, coherent concurrent snapshots,
  drift/repair and negative combinations. No media packets generated.
- `rx-alt` adds a second documented receiver-to-sender choice. Selection, indexed
  identity, rejected-partner state preservation and independent disable are tested.
- Numeric/null/default matrices cover all eight audio slots, GOP/audio integer
  limits, streaming/FEC combinations, SRT modes/key bounds and VLAN/UDP/sub-index
  neighbours. These are simulator policy tests, not target schema conformance.
- The five-minute debug child-process soak completed 1,200 cycles, 14,761 correlated
  exchanges, 6,000 async events, 60 backend reconnects and 60 target recoveries with
  zero unexpected errors. Driver RSS was 14,632 → 15,052 KiB; high-water 15,052 KiB.
  Its early histogram used truncated microseconds; latency upper-bound claims from
  that run are deliberately not retained. The ceil conversion was fixed and regression-tested.
- See [SOAK.md](docs/SOAK.md) for final timed rerun metrics, bounds and limitations.
  [WIRE_EVIDENCE.md](docs/WIRE_EVIDENCE.md) records the completed documentation search
  and the exact still-missing wire evidence; no release schema was found.

### 0.2.0 tested local image

| Item | Recorded result |
| --- | --- |
| Local image ID | `sha256:1f46fbe3fdaa31c4bdc7f17651ae2013a62ecb8ece0e5f4cd1d0e024c7221913` |
| Development tag | `rust-example:local-0.2.0` (not a published registry manifest) |
| Docker-reported size / user | 3,244,166 bytes / `10001:10001` |
| Build | Same pinned musl builder/static runtime and locked dependencies; clean source compilation passed |
| Runtime lifecycle | `scripts/runtime-smoke.sh` passed, including duplicate refusal, driver restart, simulator SIGKILL recovery and graceful stop; disposable resources removed |
| Security | Trivy 0.74.0 image + Cargo graph: zero fixable HIGH/CRITICAL findings under the dated database; not an absence-of-all-vulnerabilities claim |
| Source / image audit | Secret scan reported no findings; runtime filesystem contains no source/build tools, populated environment or private plan; read-only non-root `--version` reports 0.2.0 |
| Publication / live changes | None; old local image retained, no accepted deployment replaced |

The final corrected-histogram soak rerun completed another 5,905 exchanges and 48
recoveries in two minutes with zero unexpected errors; driver high-water 14,972 KiB.
These observations do not validate container-limit soak behavior, real media or
VideoIPath compatibility. No target-release or publication gate is marked complete.

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
| Writes | Safe set, model readback, restored initial state, per-item errors; physical readback required for a vendor adapter |
| Panel and async | Correct controls/read-only display; independent change delivered once |
| Media control | SDP, enable/disable, redundancy, IPT drift repair, routing/multiplicity cases |
| Recovery | Device/VIP loss, stable identity, saved references, stale subscriptions and reconnect |

Any later physical-device adapter needs its own firmware/API evidence, physical
readback/restoration and media validation. Simulator acceptance cannot substitute
for those checks. Production or critical-equipment use requires appropriate
target-specific qualification and explicit operational risk acceptance.
