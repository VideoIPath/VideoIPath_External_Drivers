# Dependency and distribution notes

The example's source is covered by the repository's [GPL version 3 license](../../LICENSE).
Dependencies retain their own licenses. No vendor SDK or proprietary device code is included.

Direct-package expressions below were checked with `cargo tree --locked --depth 1
--format '{p} | {l}'` on 2026-09-30; exact versions are in [VALIDATION.md](VALIDATION.md)
and `Cargo.lock`. This inventory is not a legal opinion or a replacement for upstream notices.

| Packages | Upstream license expression |
| --- | --- |
| Tokio, tokio-util, tokio-tungstenite, tracing, tracing-subscriber | MIT |
| futures-util, rand, Serde, serde_json, thiserror, URL | MIT OR Apache-2.0 |
| UUID | Apache-2.0 OR MIT |
| Rustls, rustls-native-certs, rustls-pemfile | Apache-2.0 OR ISC OR MIT |
| Test-only rcgen, tempfile, tokio-rustls | MIT OR Apache-2.0 |

The transitive graph additionally contains Apache-2.0 AND ISC (`ring`), BSD-3-Clause,
ISC, Unicode-3.0 and Unlicense OR MIT expressions. Rust's standard library, musl and
the pinned container base also carry their own licenses. Obtain actual notices from
the exact upstream source packages used by the build, not guessed license templates.

Before distributing a prebuilt image or native binary, package the application license,
required third-party notices and appropriate corresponding source/source information;
review the complete locked graph and base-image obligations. Image publication is not
implemented or authorized by this example's CI. Its local image tests and vulnerability
checks do not constitute a completed distribution/license review. Dependency-update PRs
must rerun the same checks and review any license changes.

For an inventory of the current resolved runtime graph:

```sh
cargo tree --locked -e normal --format '{p} | {l}' --prefix none
```
