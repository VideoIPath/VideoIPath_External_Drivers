#!/bin/sh
# Explicit local Docker test only. Creates its own project/volumes and removes them.
# No VideoIPath target is contacted: the driver attempts only its own loopback port 9.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
image=${1:?usage: sh scripts/runtime-smoke.sh local-image-reference}
scratch=$(mktemp -d "${TMPDIR:-/tmp}/rust-example-smoke.XXXXXX")
mkdir -m 700 "$scratch/locks" "$scratch/certificates"
# Restrict the ownership helper to this newly created test directory. It is not
# the runtime image, needs no network, and never touches a real deployment lock.
docker run --rm --network none --read-only --cap-drop ALL --cap-add CHOWN \
  --user 0 -v "$scratch/locks:/locks" --entrypoint /bin/chown \
  rust:1.98.1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f \
  10001:10001 /locks
umask 077
cat > "$scratch/.env" <<EOF
COMPOSE_PROJECT_NAME=rust-example-smoke-$$
DRIVER_IMAGE=$image
VIDEOIPATH_URL=wss://127.0.0.1:9/x-driver/v1
VIDEOIPATH_TOKEN=synthetic-smoke-only-not-a-real-token
VIDEOIPATH_NODE_ID=8fc48734-b6d4-4e40-b377-c460ca4dd420
DRIVER_LOCK_HOST_PATH=$scratch/locks
DRIVER_CERTIFICATE_HOST_PATH=$scratch/certificates
DRIVER_HEALTHCHECK_INTERVAL=1s
DRIVER_HEALTHCHECK_START_PERIOD=1s
DRIVER_LOG_LEVEL=warn
DRIVER_RESTART_POLICY=no
EOF
compose() {
  # Explicitly discard ambient credentials/Compose settings; --env-file alone
  # would NOT override an exported shell variable.
  env -i PATH="$PATH" HOME="$HOME" docker compose --env-file "$scratch/.env" -f compose.yaml "$@"
}
cleanup() {
  compose down --volumes --remove-orphans >/dev/null 2>&1 || true
  # Traverse the private UID 10001 directory only to restore this test directory's
  # ownership. DAC_OVERRIDE is confined to this disposable cleanup helper/mount.
  docker run --rm --network none --read-only --cap-drop ALL --cap-add CHOWN --cap-add DAC_OVERRIDE \
    --user 0 -v "$scratch/locks:/locks" --entrypoint /bin/chown \
    rust:1.98.1-alpine@sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f \
    -R "$(id -u):$(id -g)" /locks >/dev/null 2>&1 || true
  rm -rf "$scratch"
}
trap cleanup EXIT HUP INT TERM
compose config --quiet >/dev/null 2>&1
compose up -d --wait --wait-timeout 30
compose exec -T driver /usr/local/bin/rust-example health
compose exec -T simulator /usr/local/bin/rust-example health
compose exec -T simulator /usr/local/bin/rust-example simctl set null level 60
compose exec -T driver /usr/local/bin/rust-example status
# A second owner must fail before any target/VIP connection; never scale the project.
if compose run --rm --no-deps driver >"$scratch/duplicate.log" 2>&1; then
  echo 'singleton test unexpectedly succeeded' >&2
  exit 1
fi
if ! grep -q 'node ownership unavailable' "$scratch/duplicate.log"; then
  echo 'singleton test failed for an unexpected reason (details suppressed)' >&2
  exit 1
fi
# Recreate only after stopping the owner. The stable UUID stays in the same fixture.
compose stop driver
compose up -d --wait --wait-timeout 30 driver
compose exec -T simulator /usr/local/bin/rust-example simctl status
# The simulator's per-socket kernel locks permit safe stale-socket recovery after
# a crash. This kills only this disposable test project, never an operator service.
compose kill -s SIGKILL simulator
compose up -d --wait --wait-timeout 30 simulator
compose exec -T simulator /usr/local/bin/rust-example simctl status
compose stop
echo 'Runtime smoke passed: non-root/read-only services, local health, singleton refusal, restart, crash recovery and stop.'
