# Operating the Rust external-driver example

This example runs **one driver with in-memory demonstration state**. No simulator
service or device socket is needed. The model shows API behavior, not physical
media transmission. This example has no live VideoIPath commissioning evidence;
container health alone does not establish target acceptance.

## Deployment contract

Use a Linux Docker Engine host outside the VideoIPath cluster and Docker Compose v2 with `up --wait` and long-form bind mounts. Compose configuration was checked with **2.40.3**. Run commands in `examples/rust_example/` unless stated otherwise. No prebuilt public image is provided.

| Resource | Contract |
| --- | --- |
| Driver | `rust-example driver`; outbound verified TLS WebSocket to the configured VideoIPath virtual IP |
| Demonstration state | In-memory values, alarms and configurations; no physical device connection |
| Identity | One externally provisioned stable node UUID per example instance; never scale the service |
| Driver state | Project-private named volume at `/var/lib/rust-example`; local heartbeat/readiness diagnostics |
| Lock | Process-held node-ID lock in the **same host directory** for every project, default `/var/lock/videoipath-drivers` |
| Runtime restrictions | Non-root, read-only root filesystem, all capabilities dropped, no-new-privileges, no published ports |

State survives a WebSocket reconnect but resets when the driver process restarts.
Health volumes do not make control state durable. While connected, a ten-second
teaching clock alternates `signal_present` and raises/clears a clearly labeled demo
warning. It does not represent a physical fault or background sensor polling.

### 1. Prepare configuration and directories

```sh
# Create the plaintext secret file privately; edit it with a local editor, never paste tokens into commands.
umask 077
cp -n .env.example .env
chmod 600 .env
# Prepare the single shared host lock directory; all driver projects must use this exact host path.
sudo install -d -o 10001 -g 10001 -m 0700 /var/lock/videoipath-drivers
# Only public CA certificates belong here; bind creation is disabled to catch misspelled paths.
mkdir -p deployment/certificates
```

Set the empty `COMPOSE_PROJECT_NAME`, `DRIVER_IMAGE`, `VIDEOIPATH_TOKEN`, and `VIDEOIPATH_NODE_ID`, and replace the deliberately invalid URL. Generate a UUID once using your platform's UUID generator and preserve it in the protected configuration backup. Example UUIDs and placeholder credentials are not deployment identities.

The URL is the complete lower-case endpoint, `wss://<videoipath-virtual-ip>/x-driver/v1`, **without a query**. The application appends `node_id` and sends the token in the bearer header. URL credentials, fragments, plaintext WebSockets and disabled TLS verification are unsupported.

`.env.example` is the complete operator inventory: application settings are explicitly
forwarded; deployment settings govern Compose. There are no target socket settings,
detailed-payload logging option or `*_FILE` secret-loading option.

Keep the default in-container paths for Compose. Named volumes inherit ownership
from image directories on first use. Changing `DRIVER_STATE_DIRECTORY` requires
preparing UID/GID `10001:10001` ownership; an environment change does not repair
existing permissions. All projects must use one common underlying host lock directory.

### Secrets and interpolation

Compose `.env` is an interpolation file, not a shell script: do not `source` it. Exported shell variables override `.env`, including with `--env-file`; clear conflicting `VIDEOIPATH_*`, `DRIVER_*`, and `COMPOSE_*` variables in deployment automation. Do not print the environment to check it.

Single quotes preserve literal `$`, spaces, double quotes and `#`. Compose supports an escaped single quote inside a single-quoted value. This **synthetic, non-deployable** quoting example is exercised by the interpolation test:

```dotenv
# Example syntax only; never use this string as a deployment credential.
VIDEOIPATH_TOKEN='test-only$DOLLAR # "double" \'single\' spaced'
```

Unquoted and double-quoted values undergo interpolation. Do not add a second `$` escape inside a single-quoted `.env` value. Protect `.env` with mode `0600`, omit it from support bundles, and never run plain `docker compose config` or save resolved configuration containing real secrets. Docker inspect privileges can expose environment credentials. A secret manager may supply the direct environment variable instead.

### 2. Select an image and validate

For an operator-supplied registry image, use an immutable tag, preferably `registry/repository@sha256:<digest>`, in `.env`. Record its digest and the previous image for rollback. Project and image placeholders are intentionally empty, so the supplied `.env.example` fails validation unchanged.

```sh
# Quiet validation checks interpolation/schema without rendering bearer credentials.
sh scripts/check-compose.sh
# Image-only deployment pulls exactly the configured reference; it never builds source implicitly.
docker compose --env-file .env -f compose.yaml pull
```

For a deliberate local build, set a unique local immutable test tag in `DRIVER_IMAGE`, then use the override:

```sh
# Cargo.lock and the digest-pinned Dockerfile define this candidate's inputs.
docker compose --env-file .env -f compose.yaml -f compose.build.yaml build
# The override refuses implicit pulls; start the image just built with the same files.
docker compose --env-file .env -f compose.yaml -f compose.build.yaml up -d --wait
```

The build context is a deny-by-default allowlist: Cargo manifests/lock and Rust source only. The Dockerfile copies explicit inputs, never the workspace. The private plan, populated `.env`, CA material, tests, caches and `target/` do not enter the context. Runtime contains the compiled binary and public trust roots, not the builder's compiler or Cargo cache. Before release, inspect the actual image/layers and scan the **same candidate image** for fixable high/critical vulnerabilities. A successful build is not a vulnerability scan or target-compatibility result.

| Pinned input | Source and verification |
| --- | --- |
| Rust `1.98.1-alpine` (Alpine 3.24.2, native musl) | Official `library/rust` registry; digest `sha256:7cc1c22d77d9432f7fe012a70e6d3e555af54c2a6832700ed7d553f1769ae89f` |
| Distroless `static-debian13:nonroot` | [Maintained distroless images](https://github.com/GoogleContainerTools/distroless); digest `sha256:e2e927ec666bae08560abb3c55d0659eceabb657f56b6782ab500a9fc7f555e3` |
| Public trust roots | Included in the pinned runtime; no mutable runtime `apt install` step |

References were checked on 2026-09-30. A multi-architecture base manifest is not evidence that this application was tested on every architecture. Review base-image/dependency updates together, rerun checks, rebuild, scan and record the replacement digest.

### 3. Start and inspect

```sh
# For a pulled or already-built image, start the image-only deployment.
docker compose --env-file .env -f compose.yaml up -d --wait
# Show service status without rendering configuration or environment secrets.
docker compose --env-file .env -f compose.yaml ps
# Normal logs contain sanitized operational events; review before sharing externally.
docker compose --env-file .env -f compose.yaml logs --tail 200 driver
# This check reads only the local heartbeat.
docker compose --env-file .env -f compose.yaml exec -T driver rust-example health
# Read sanitized dependency state separately; this is not a secret-bearing environment dump.
docker compose --env-file .env -f compose.yaml exec -T driver rust-example status
```

For offline API lessons, run `cargo run --locked --example api_walkthrough` natively.
The optional [device lab](tools/device_lab/) is separate native test equipment;
its old `simulator`/`simctl` commands do not exist in the default driver image.

| Signal | Meaning and action |
| --- | --- |
| Local health / `up --wait` | A recent local heartbeat; confirms liveness, not an authenticated VideoIPath session |
| Readiness file | `device` means in-memory model available, **not physical equipment**; `videoipath` means backend session connected |
| Dependency outage | Inspect sanitized logs/readiness and restore the dependency; do not restart solely because it is unavailable |
| `unhealthy` | Inspect local progress; plain Compose does not automatically restart an unhealthy process |
| Permanent startup/auth/TLS failure | Correct the setting, trust chain or authorization; the default `on-failure:5` bounds container retries |
| Lock refusal | Find the existing owner; stop it intentionally. Never delete a lock file to bypass a live owner |

Logs rotate with the Docker `local` driver; CPU, memory, PID and log limits are set.
Keep ordinary-request timeout <= IPT timeout and stop grace above shutdown timeout.
Defaults are 5/20 seconds for requests and 5/20 seconds for shutdown/container grace.
When adding a vendor adapter, give its operations deadlines below request deadlines.

### 4. Stop, upgrade and roll back

```sh
# Stop the sole owner before upgrade or rollback.
docker compose --env-file .env -f compose.yaml stop driver
# Remove containers/network while retaining named volumes; do not use --volumes for routine shutdown.
docker compose --env-file .env -f compose.yaml down
```

Before an upgrade, securely back up `.env` and record node ID, image digest, public
CA reference and acceptance evidence. Node identity lives in configuration, not a
regenerated container identity. Demo control state is ephemeral; record and reapply
the intended configuration rather than assuming a diagnostic volume preserves it.

Pull/build without starting a second owner. Stop the old driver, update
`DRIVER_IMAGE`, then recreate it. Check liveness, readiness, Inventory continuity
and controlled read/write behavior. Roll back by stopping the candidate and restoring
the recorded image **and its matching Compose/configuration files**, with the same
node ID. Version 0.2 used two services; stop that entire old project before moving
to 0.3, and retain its files for rollback. Reapply demo state after restart.
Never use rolling overlap or `--scale` for this node.

The OS lock blocks same-host duplicates only when every project binds the same underlying host directory. Different hosts need explicit singleton ownership; this example provides no cross-host lease or leader election. Changing a node ID creates a replacement identity and requires deliberate Inventory/reference migration.

### Token and CA rotation

Provision a separately revocable deployment token where available. Update it privately in `.env`, stop the old driver, recreate the driver, verify readiness and target acceptance, then retire the old token under your operational procedure. A token shared by other drivers has a wider revocation scope.

For a private CA, put the **public** PEM bundle in `deployment/certificates/`, readable by UID 10001, and set `VIDEOIPATH_CA_BUNDLE` to `/etc/rust-example/certificates/<file>.pem`. Never mount a private CA key. Coordinate overlapping trust for certificate rotation, update the bundle, recreate the driver, and check hostname/chain verification and connection recovery. Blank uses the runtime's normal trust roots.

## Optional host boot launcher

`on-failure:5` does not restore stopped containers after host/daemon restart. `deployment/rust-example@.service` is an optional Linux systemd **sample**, supplied but not installed or enabled. It runs image-only `compose up -d --wait` after Docker and the network are available and keeps credentials in `.env`. It is a boot launcher, not continuous liveness monitoring. After a standalone Docker daemon restart, explicitly restart this launcher and verify services.

To adopt it, first place the reviewed deployment under `/opt/rust-example/<instance>/` with protected `.env`, Compose files and certificate directory; verify `/usr/bin/docker`, host lock permissions and configured image availability. The following is a manual operator installation procedure, not part of building this example:

```sh
# Install the reviewed template only when host-service installation has been approved.
sudo install -m 0644 deployment/rust-example@.service /etc/systemd/system/rust-example@.service
sudo systemctl daemon-reload
# Replace demo with your deployment directory name; enabling deliberately starts that deployment.
sudo systemctl enable --now rust-example@demo.service
# Check the launcher result; also inspect Compose ps/readiness because oneshot active is not service health.
sudo systemctl status rust-example@demo.service
# After an authorized Docker daemon restart, rerun the launcher to restore and check containers.
sudo systemctl restart rust-example@demo.service
```

Verify an actual authorized host reboot and daemon restart before relying on recovery; neither is claimed tested by this handoff. Monitor process and dependency health separately. To remove the sample:

```sh
# Disable and stop this instance; remove the shared template only after every instance is disabled.
sudo systemctl disable --now rust-example@demo.service
sudo rm /etc/systemd/system/rust-example@.service
sudo systemctl daemon-reload
```

## Local deployment checks

```sh
# Check inventory/default parity and image/deployment boundaries without a backend.
cargo test --locked --test deployment_contract
# This opt-in test uses a mode-0600 synthetic .env, tests literal special characters,
# captures only synthetic resolved configuration in memory, and deletes the fixture afterward.
cargo test --locked --test deployment_contract -- --ignored
# Validate your protected real configuration quietly; no resolved config is captured.
sh scripts/check-compose.sh
```

Consult [VALIDATION.md](VALIDATION.md) for checks actually completed and remaining gates. Source/Compose validation, image build, vulnerability scan, hardened runtime testing and live target commissioning are distinct results.

## Target commissioning — deferred

Record the exact VideoIPath release/backend SHA and immutable driver image digest. Validate, in order:

| Stage | Acceptance evidence |
| --- | --- |
| Session and structure | Authenticated TLS session; backend accepts the advertised structure without decoder errors |
| Discovery and Inventory | One candidate for the stable UUID; operator add/sync as appropriate; Device UP and expected topology |
| Parameter catalog | Complete metadata and scoped addresses; expected Panel presentation |
| Control and monitoring | Controlled reads/writes against demo state; partial failures; teaching changes appear asynchronously |
| Media configuration model | SDP/IPT/drift/repair and applicable scenarios match simulated state; no claim of media transport |
| Lifecycle | Dependency loss/recovery, singleton refusal, preserved Inventory identity and references after driver recreation |

Stop at the first failing layer and record sanitized evidence. This example's
unperformed acceptance says nothing about API maturity. Replacing `DemoDevice`
with a physical adapter additionally requires firmware/API, physical readback,
device-loss behavior and media validation.
