#!/bin/sh
# Explicit local Docker test only. Creates its own project/volumes and removes them.
# No VideoIPath target is contacted: the driver attempts only its own loopback port 9.
# Build the current checkout first; this script checks the supplied image, not cached source.
set -eu
cd "$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
image=${1:?usage: sh scripts/runtime-smoke.sh local-image-reference}
version=$(awk -F '"' '/^version = / { print $2; exit }' Cargo.toml)
reported=$(docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges "$image" --version)
if [ "$reported" != "rust-example $version" ]; then
  echo 'Image version differs from this checkout; rebuild the runtime image before this test.' >&2
  exit 1
fi
help=$(docker run --rm --network none --read-only --cap-drop ALL --security-opt no-new-privileges "$image" --help)
if printf '%s\n' "$help" | grep -Eq 'simulator|simctl'; then
  echo 'Image still exposes lab commands; rebuild the single-process runtime image.' >&2
  exit 1
fi
scratch_root=${TMPDIR:-/tmp}
if [ -d /tmp/opencode ]; then scratch_root=/tmp/opencode; fi
scratch=$(mktemp -d "$scratch_root/rust-example-smoke.XXXXXX")
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
[ "$(compose config --services)" = driver ]
compose run --rm --no-deps --pull never driver check-config
if compose run --rm --no-deps --pull never -e DRIVER_REQUEST_TIMEOUT_MS=9000 driver check-config; then
  echo 'invalid candidate settings unexpectedly passed preflight' >&2
  exit 1
fi
compose up -d --wait --wait-timeout 30
check_driver() {
  id=$(compose ps -q driver)
  [ -n "$id" ]
  [ "$(compose ps --services --status running)" = driver ]
  [ "$(docker inspect --format '{{.Config.User}} {{.HostConfig.ReadonlyRootfs}} {{len .HostConfig.PortBindings}}' "$id")" = '10001:10001 true 0' ]
  # Project configuration preserves the UUID across restart/recreate. Inspect only
  # this nonsecret equality, never dump the container environment (it has a token).
  [ "$(docker inspect --format '{{range .Config.Env}}{{if eq . "VIDEOIPATH_NODE_ID=8fc48734-b6d4-4e40-b377-c460ca4dd420"}}stable{{end}}{{end}}' "$id")" = stable ]
  compose exec -T driver /usr/local/bin/rust-example health
  status=$(compose exec -T driver /usr/local/bin/rust-example status)
  printf '%s\n' "$status" | grep -qx 'videoipath=false'
}
check_driver
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
before=$(compose ps -q driver)
compose stop driver
compose up -d --force-recreate --wait --wait-timeout 30 driver
[ "$(compose ps -q driver)" != "$before" ]
check_driver
compose restart driver
compose up -d --wait --wait-timeout 30 driver
check_driver
compose stop
[ -z "$(compose ps --services --status running)" ]
echo 'Runtime smoke passed: one non-root/read-only driver, liveness, singleton refusal, stable UUID, recreate, restart and stop. VideoIPath readiness remains false.'
