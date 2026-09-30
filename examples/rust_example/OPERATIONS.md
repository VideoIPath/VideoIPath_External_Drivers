# Operating the Rust external-driver example

This example runs one driver and one independent simulated device. **This example has no live VideoIPath validation.** Local tests and container health establish local behavior; target acceptance requires the commissioning sequence below. The simulator models device control, not physical media transmission.

## Deployment contract

Use a Linux Docker Engine host outside the VideoIPath cluster and Docker Compose v2 with `up --wait` and long-form bind mounts. Compose configuration was checked with **2.40.3**. Run commands in `examples/rust_example/` unless stated otherwise. No prebuilt public image is provided.

| Resource | Contract |
| --- | --- |
| Driver | `rust-example driver`; outbound verified TLS WebSocket to the configured VideoIPath virtual IP |
| Simulator | `rust-example simulator`; separate process, no network, no VideoIPath credentials |
| Device API | Unix socket `/run/rust-example/device.sock`; simulator-only protocol, not SPEC-23 |
| Local controls | Separate `/run/rust-example/control.sock`; invoke `rust-example simctl` in the simulator container |
| Identity | One externally provisioned stable node UUID per simulated target; never scale either service |
| Socket volume | Project-private, directory owned by UID/GID `10001:10001`; both processes use that identity |
| Driver state | Project-private named volume at `/var/lib/rust-example`; local heartbeat/readiness diagnostics |
| Simulator health | Separate project-private volume at `/var/lib/rust-example-simulator` |
| Lock | Process-held node-ID lock in the **same host directory** for every project, default `/var/lock/videoipath-drivers` |
| Runtime restrictions | Non-root, read-only root filesystem, all capabilities dropped, no-new-privileges, no published ports |

The shared socket directory is the trust boundary between these two services. Docker administration is privileged access. The simulator's control state survives a **driver** restart, but a **simulator** restart resets the modeled device to defaults. Health volumes do not make device control state durable.

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

`.env.example` is the complete operator inventory: every public application setting is explicitly forwarded to the driver; deployment settings govern Compose. The simulator receives only socket paths and its dedicated state path. There is no detailed-payload logging option and no `*_FILE` secret-loading option.

Keep the default in-container directory paths for Compose. Named volumes inherit ownership from those image directories on first use. Changing `DRIVER_STATE_DIRECTORY` requires preparing the new volume's UID/GID `10001:10001` ownership before launch; changing an environment value does not initialize existing volume permissions. Socket paths may use distinct filenames **inside** `/run/rust-example`; paths outside that shared mount are for separately prepared native deployments. Changing the lock target inside the container never changes the requirement for one common host lock directory.

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
docker compose --env-file .env -f compose.yaml logs --tail 200 driver simulator
# Each check reads only its own local heartbeat, using that service's state directory.
docker compose --env-file .env -f compose.yaml exec -T driver rust-example health
docker compose --env-file .env -f compose.yaml exec -T simulator rust-example health
# Read sanitized dependency state separately; this is not a secret-bearing environment dump.
docker compose --env-file .env -f compose.yaml exec -T driver rust-example status
```

Use the simulator commands and their supported arguments in [the simulator guide](docs/SIMULATOR.md). Invoke controls through `docker compose exec -T simulator rust-example simctl …`; they are local device operations, not VideoIPath API calls. No public control listener is exposed.

| Signal | Meaning and action |
| --- | --- |
| Local health / `up --wait` | A recent local heartbeat; confirms liveness, not an authenticated VideoIPath session |
| Readiness file | Separate `device` and `videoipath` flags; both must be true for connected operation |
| Dependency outage | Inspect sanitized logs/readiness and restore the dependency; do not restart solely because it is unavailable |
| `unhealthy` | Inspect local progress; plain Compose does not automatically restart an unhealthy process |
| Permanent startup/auth/TLS failure | Correct the setting, trust chain or authorization; the default `on-failure:5` bounds container retries |
| Lock refusal | Find the existing owner; stop it intentionally. Never delete a lock file to bypass a live owner |

Logs rotate with the Docker `local` driver; CPU, memory, PID and log limits are set per service. Load-test changes to bounds and deadlines. Keep device-operation timeout < ordinary-request timeout <= IPT timeout and Compose stop grace greater than application shutdown timeout. Defaults are 3 s / 5 s / 20 s for operations and 5 s / 20 s for application shutdown / container grace.

### 4. Stop, upgrade and roll back

```sh
# Stop driver first to stop new operations before stopping the simulated device.
docker compose --env-file .env -f compose.yaml stop driver
docker compose --env-file .env -f compose.yaml stop simulator
# Remove containers/network while retaining named volumes; do not use --volumes for routine shutdown.
docker compose --env-file .env -f compose.yaml down
```

Before an upgrade, securely back up `.env` and record node ID, current image digest, public CA reference, scenario/version and any required acceptance evidence. The node ID lives in configuration, not a regenerated container identity. Simulator control state is ephemeral; capture the intended scenario through the documented commands, not by assuming a volume backup preserves it.

Pull/build the candidate without starting a second owner. Stop the existing driver, update `DRIVER_IMAGE`, then recreate the services with the commands above. Check liveness, readiness, Inventory continuity and a controlled write/readback. Roll back by stopping the candidate and restoring the previous image reference with the same node ID and configuration. Recreating the simulator resets its state; reapply the intended scenario. Never use rolling overlap or `--scale` for this node.

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
| Control and monitoring | Controlled reads/writes with independent simulator readback; partial failures; local changes appear asynchronously |
| Media configuration model | SDP/IPT/drift/repair and applicable scenarios match simulated state; no claim of media transport |
| Lifecycle | Dependency loss/recovery, singleton refusal, preserved Inventory identity and references after driver recreation |

Stop at the first failing layer and record sanitized evidence. This example's unperformed target acceptance does not establish anything about the maturity of the VideoIPath API. Replacing the simulator with a physical device also requires firmware/API, physical readback and media validation.
