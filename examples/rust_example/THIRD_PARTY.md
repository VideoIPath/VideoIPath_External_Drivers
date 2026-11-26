# Source and notices for the Rust example image

The driver is GPL-3.0-only. `LICENSE` contains the application license. No vendor SDK or physical-device firmware is included. Keep the deployment bundle with the OCI archive when transferring or redistributing it.

| Component | Where to find the source and notices |
| --- | --- |
| Driver and build files | `sources/rust-example.tar.gz` contains the example directory from `source.commit` in `driver-release.json`, including the Dockerfile, scripts, `Cargo.toml` and `Cargo.lock`. |
| Rust dependencies | `sources/crates/` contains the original `.crate` archives (gzip-compressed tar files), including their upstream source and license/notice files. Each archive is checked against its SHA-256 in the bundled lockfile. The complete lockfile is included, so some packages are build-only, test-only, or for other platforms. |
| Rust standard library and toolchain notices | `notices/rust-toolchain.tar` contains `COPYRIGHT-library.html`, the Rust license texts and `rustc -Vv` from the pinned builder. The compiler revision identifies the [Rust source](https://github.com/rust-lang/rust). The compiler itself is not shipped in the runtime image. |
| musl libc | `notices/musl-*-COPYRIGHT` contains upstream attribution and license texts. [musl releases and source](https://musl.libc.org/releases/). |
| Distroless and Debian runtime files | `notices/distroless-LICENSE` is the Distroless project license. The OCI archive retains `/usr/share/doc/<package>/copyright`, `/usr/share/common-licenses/`, and installed versions under `/var/lib/dpkg/status.d/`. See the [Distroless source](https://github.com/GoogleContainerTools/distroless) and the [Debian source archive](https://snapshot.debian.org/package/) for the installed source package and version. |

For version 0.3.0, the runtime Debian source packages are [base-files 13.8+deb13u8](https://snapshot.debian.org/package/base-files/13.8%2Bdeb13u8/), [netbase 6.5](https://snapshot.debian.org/package/netbase/6.5/), [tzdata 2026f-0+deb13u1](https://snapshot.debian.org/package/tzdata/2026f-0%2Bdeb13u1/) (also `tzdata-legacy`), [media-types 13.0.0](https://snapshot.debian.org/package/media-types/13.0.0/), and [ca-certificates 20250419+deb13u1](https://snapshot.debian.org/package/ca-certificates/20250419%2Bdeb13u1/). When changing the runtime base or builder, update these references and check the new image's notices.
