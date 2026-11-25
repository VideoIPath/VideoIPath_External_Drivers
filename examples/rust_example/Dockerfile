# A version plus digest prevents a rebuild from silently selecting a different base.
FROM rust:1.100.0-alpine@sha256:8d0e82abfdf9a38071e07cabc5afba4ddbe077631a80e4e742a760c38beadc33 AS builder
WORKDIR /build
# Explicit inputs and the context allowlist keep local credentials and the private plan out.
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
# The native musl toolchain produces a static binary. The committed lockfile is
# mandatory; a build must never silently resolve new dependencies.
RUN cargo build --locked --release --bin rust-example
# Prepare owned writable mount points in the builder; the runtime has no shell.
RUN install -d -o 10001 -g 10001 -m 0700 /runtime/var/lib/rust-example \
       /runtime/var/lock/videoipath-drivers \
    && install -d -m 0755 /runtime/etc/rust-example/certificates

FROM gcr.io/distroless/static-debian13:nonroot@sha256:aafe993665bf6200db38a02c11a9351e782a84b6a45e7f33b6695638c88e3931 AS runtime
# The pinned runtime supplies CA roots without unused OpenSSL libraries or
# a shell, package manager, compiler, source tree, or mutable package installation.
COPY --from=builder /build/target/release/rust-example /usr/local/bin/rust-example
# Fresh Docker volumes inherit these owners; no privileged init service is needed.
COPY --from=builder /runtime/ /
USER 10001:10001
WORKDIR /var/lib/rust-example
# Exec-form entrypoint preserves signal delivery through Compose's init process.
ENTRYPOINT ["/usr/local/bin/rust-example"]
CMD ["driver"]
