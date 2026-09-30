# Both manifest-list digests were resolved from the official Docker registry on 2026-09-30.
# A version plus digest prevents a rebuild from silently selecting a different base.
FROM rust:1.98.1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f AS builder
WORKDIR /build
# Explicit inputs and the context allowlist keep local credentials and the private plan out.
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
# The native musl toolchain produces a static binary. The committed lockfile is
# mandatory; a build must never silently resolve new dependencies.
RUN cargo build --locked --release --bin rust-example
# Prepare owned writable mount points in the builder; the runtime has no shell.
RUN install -d -o 10001 -g 10001 -m 0700 /runtime/run/rust-example \
       /runtime/var/lib/rust-example /runtime/var/lib/rust-example-simulator \
       /runtime/var/lock/videoipath-drivers \
    && install -d -m 0755 /runtime/etc/rust-example/certificates

FROM gcr.io/distroless/static-debian13:nonroot@sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3 AS runtime
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
