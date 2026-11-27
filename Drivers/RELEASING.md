# Publishing a Driver Hub-compatible driver

A release lets Driver Hub discover one driver's updates and lets an operator install the same image at a disconnected site. Publishing is optional. The usual [driver contribution requirements](../CONTRIBUTING.md#driver-expectations) still apply. A release is not Nevion certification. It requires no change to the VideoIPath External Driver API. What Driver Hub requires of the driver itself, such as its image, inputs, runtime behaviour and health endpoint, is in [Driver Hub compatibility](#driver-hub-compatibility), which ends with a checklist.

## What to publish

Choose a repository-unique `driverId`, such as `exampleco-meter`. Keep it across directory moves and display-name changes; it identifies the software, not a device's `node_id`. Version each driver independently using [Semantic Versioning](https://semver.org/). Tag the reviewed source commit `drivers/<driverId>/v<version>`, for example `drivers/exampleco-meter/v1.4.0`. Mark a prerelease such as `1.5.0-rc.1` as a GitHub prerelease.

Contributed drivers use `Drivers/<Vendor>/<Driver>`. The schema also accepts `examples/rust_example` so the [Rust teaching example](../examples/rust_example/README.md) can exercise the same packaging and offline import contract under the stable ID `rust-example`. A locally packaged example is a customer build, not an upstream release.

Attach these four files to the **same GitHub Release**:

| Asset | Contents |
| --- | --- |
| `driver-release.json` | Version, source, platforms, and identities of the other three files. See the [release record schema](schemas/driver-release-v1.schema.json). |
| `driver-config.json` | Byte-for-byte copy of the input declaration in the tagged driver directory. See the [input declaration schema](schemas/driver-config-v1.schema.json). Actual site settings never go here. |
| `<name>.oci.tar.gz` | A gzip-compressed OCI image layout: `oci-layout`, `index.json`, and all referenced blobs. `index.json` points to one released manifest or multi-platform index. No external layers, builds or registry access may be needed at installation. |
| `deployment.tar.gz` | At its root: `README.md`, `compose.yaml`, and `RELEASE-NOTES.md`. Include referenced runtime files and redistribution notices at their documented relative paths. Package only reviewed regular files and directories; exclude site data, credentials and links outside the package. |

Example `driver-release.json` (illustrative hashes):

```json
{
  "schemaVersion": 1,
  "driverId": "exampleco-meter",
  "displayName": "ExampleCo Meter",
  "version": "1.4.0",
  "source": {
    "commit": "0123456789abcdef0123456789abcdef01234567",
    "directory": "Drivers/ExampleCo/Meter"
  },
  "configuration": {
    "file": "driver-config.json",
    "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
  },
  "image": {
    "digest": "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "platforms": ["linux/amd64"],
    "archive": "exampleco-meter-1.4.0.oci.tar.gz",
    "archiveSha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "deploymentBundle": {
    "file": "deployment.tar.gz",
    "sha256": "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"
  }
}
```

File checksums are SHA-256 of the **downloaded bytes**, including compression. `image.digest` is the descriptor digest of the released manifest or index inside the layout, not the hash of the tarball, wrapper `index.json`, Docker image ID, or local tag. `platforms` lists runnable OCI `os/architecture[/variant]` combinations. Both JSON files use UTF-8 and must validate against their respective v1 schemas. Bundle the schemas with the consumer for offline validation.

## Declare what the image reads

Keep `driver-config.json` next to the driver's `README.md`. Stable input names let an operator reuse settings across versions; the binding tells Driver Hub where this particular image reads each value. Declare all configurable driver inputs needed for the documented deployment, including the VideoIPath connection and stable device identity where supplied to the image. Packaging controls such as `DRIVER_IMAGE`, host paths, and the Compose project name are deployment settings, not inputs to the driver.

```json
{
  "schemaVersion": 1,
  "inputs": [
    {"name": "videoipathUrl", "type": "string", "required": true, "minLength": 1, "env": "VIDEOIPATH_URL"},
    {"name": "nodeId", "type": "string", "required": true, "env": "VIDEOIPATH_NODE_ID"},
    {"name": "videoipathToken", "type": "string", "required": true, "secret": true, "file": "/run/secrets/videoipath-token"},
    {"name": "deviceHost", "type": "string", "required": true, "minLength": 1, "env": "DEVICE_HOST"},
    {"name": "devicePort", "type": "integer", "required": false, "default": 443, "minimum": 1, "maximum": 65535, "env": "DEVICE_PORT"}
  ]
}
```

- Use a unique `name` and exactly one unique binding per input: `env` is the container environment variable; `file` is the absolute **container** path where the image reads the value. The packaged Compose service must forward or mount the corresponding value. A file binding is not a customer host path.
- Values are JSON strings, integers or booleans; `null` is not an input value. Check string lengths in Unicode code points and integer bounds inclusively. Reject inverted bounds and defaults outside them. Apply a default only if the input is absent; reject an absent required input without a default. Leave an absent optional input unbound unless it has a default. Deliver strings literally as UTF-8, integers as decimal text and booleans as `true` or `false`; do not append a newline to file values.
- `secret: true` means the operator supplies a site-local credential reference. Resolve it only at deployment and bind its value as declared; prefer a read-only file when the image supports it. Never publish a secret or secret default, or report its value in validation messages.
- Driver Hub's exact rules, including the inputs it supplies itself and how secret values reach the container, are in [Declaration](#declaration-driver-configjson).
- On install, upgrade, migration **and rollback**, validate the intended settings against the **target version's** declaration. Report missing, unrecognized, wrongly typed or out-of-range settings by name and check required credential references are available before stopping the running instance. A newly required credential or a saved port of `70000` in the example must block cutover even offline. Passing this check does not establish device connectivity or VideoIPath approval.

## Make the release runnable offline

Use one released driver image per deployment. The packaged Compose service uses `image: "${DRIVER_IMAGE:?Set DRIVER_IMAGE}"` and no `build:`. Use `pull_policy: never` or the equivalent launch option. The process must start without downloading code, dependencies, or other images. Device, VideoIPath and other documented site-local connections are expected. The [Rust Compose example](../examples/rust_example/compose.yaml) illustrates local image selection; it is not a commissioned release.

The bundled README must tell an operator how to supply settings and credential files, prepare local paths and permissions, preserve the device's `node_id` and persistent state, and check both registration and device operation. Document tested device/firmware/VideoIPath combinations separately from expected compatibility. Put upgrade steps, required backups and downgrade limits in `RELEASE-NOTES.md` and the GitHub Release body. Verify redistribution rights for the **whole image**, including vendor SDKs; provide notices and corresponding source where required. Drivers with non-redistributable dependencies can still use the source contribution path.

## Publish a version

1. The contributor submits the driver, declaration, Compose configuration and documentation for review. A repository maintainer checks the driver ID and authorizes a publisher with tag/release access. Protect release tags and use immutable GitHub releases where available. Never move a published tag or replace its assets; any change to released bytes requires a new version.
2. Build from the exact reviewed commit. Test the **candidate image** using the packaged configuration on the declared platforms. Describe what was tested and distinguish simulated or untested combinations from verified ones. Neither a validation record nor live commissioning is required to publish a Driver Hub package. Export those same manifests and layers; do not rebuild between testing and publication.
3. Stage the release files. For example, in a POSIX shell at the repository root, with the reviewed tag available locally, export its tracked files into an empty staging directory. This example driver needs only three deployment files; add any other runtime files and notices explicitly.

   ```sh
   set -eu
   tag=drivers/exampleco-meter/v1.4.0
   dir=Drivers/ExampleCo/Meter
   commit=$(git rev-parse "$tag^{commit}")
   out=$(mktemp -d)
   git show "$commit:$dir/driver-config.json" > "$out/driver-config.json"
   git show "$commit:$dir/RELEASE-NOTES.md" > "$out/notes.md"
   printf '\n[Driver README at this revision](https://github.com/VideoIPath/VideoIPath_External_Drivers/blob/%s/%s/README.md)\n' \
     "$commit" "$dir" >> "$out/notes.md"
   git archive --format=tar.gz --output="$out/deployment.tar.gz" "$commit:$dir" \
     README.md compose.yaml RELEASE-NOTES.md
   ```

   For a tested OCI manifest/index already in a registry, export it by digest, for example with Skopeo (`TESTED_IMAGE` is `registry/repository@sha256:…`):

   ```sh
   skopeo copy --all --preserve-digests "docker://$TESTED_IMAGE" \
     "oci-archive:$out/image.tar"
   gzip -n "$out/image.tar"
   mv "$out/image.tar.gz" "$out/exampleco-meter-1.4.0.oci.tar.gz"
   (cd "$out" && sha256sum driver-config.json deployment.tar.gz *.oci.tar.gz)
   ```

   A local OCI layout from the tested build is equally valid. Fill the release record from the commit, exported descriptors and final file hashes. A registry is never required at the deployment site.
4. Validate both schemas, then verify their cross-file claims: tag ID and version, peeled commit, source directory and exact declaration bytes, all three file hashes, root image digest, and digest and size of every referenced blob. The runnable platforms in the archive must equal the declaration. Reject incomplete archives even if the outer checksum matches. Check the bundled Compose bindings against what the image actually reads. Exercise the offline install below with an empty image cache, including rejection of an altered asset and invalid settings before cutover.
5. Push the reviewed tag and upload all four verified files to a **draft** GitHub Release. For example, after the tag is on GitHub:

   ```sh
   gh release create "$tag" --verify-tag --draft --title "ExampleCo Meter 1.4.0" \
     --notes-file "$out/notes.md" "$out/driver-release.json" "$out/driver-config.json" \
     "$out/exampleco-meter-1.4.0.oci.tar.gz" "$out/deployment.tar.gz"
   ```

   Add `--prerelease` for a prerelease. Publish the complete draft once its assets have been verified.

## Discovery, import and recovery

Enumerate all pages of published GitHub Releases from the trusted repository. Match the tag convention, verify its record, and compare SemVer **only within the same `driverId`**. Ignore drafts, unrelated tags, repository-wide “Latest” and source-only directories. Exclude GitHub prereleases and SemVer prereleases from stable suggestions; equal SemVer precedence (including versions differing only in build metadata) is not an upgrade.

Download the four assets together on a trusted workstation. Verify the release against the authorized upstream publisher before transfer, or use the site's trusted signing/verification process. File hashes alone do not authenticate a publisher. At import, recheck the schemas, file hashes, declaration and OCI contents offline before cataloging the package as installable. Reject unknown schemas, missing or conflicting content, unsafe archive paths or links, and attempts to overwrite a known `(driverId, version)` with different bytes. An identical re-import is a no-op. Retain the original assets; select a matching runnable platform before deployment.

For a manual installation on a matching Linux host, the following shows the last step **after verification**. With Skopeo and Docker Compose v2 installed locally, run from the transferred release directory; the deployment directory must be new. Supply the site settings and credential mounts as described in the bundled README before starting Compose.

```sh
set -eu
gzip -dc exampleco-meter-1.4.0.oci.tar.gz > image.tar
export DRIVER_IMAGE=local/exampleco-meter:1.4.0
skopeo copy oci-archive:image.tar "docker-daemon:$DRIVER_IMAGE"
mkdir deployment
tar -xzf deployment.tar.gz -C deployment
# Supply site settings, then start under this device's persistent project name.
docker compose -p meter01 -f deployment/compose.yaml up -d --no-build --pull never
```

Driver Hub does not use the deployment bundle or Compose. It verifies the release, distributes the image to its Nomad clients itself and runs it as a Nomad job; see [Driver Hub compatibility](#driver-hub-compatibility). For a manual installation, bind `DRIVER_IMAGE` to the **verified local image**, not a mutable registry tag. On host replacement, import the retained release, restore site configuration, credential references and state, and transfer ownership of the same `node_id` before starting the replacement. For rollback, select a previously retained release, check that version's inputs and follow its state-restore instructions. Keeping an image does not undo a data migration.

Keep customer-built images in a separate local catalog namespace with their actual image digest, source revision and matching configuration declaration. A build from a release tag is still a customer build and must never replace or impersonate an upstream release. Scheduling, credential storage, site approval and host operations belong to the deployment environment.

## Driver Hub compatibility

Driver Hub is a separate application that lists this repository's driver releases and deploys one instance of a driver per device as a HashiCorp Nomad job. VideoIPath itself does not deploy external drivers. Supporting Driver Hub is **optional**: a driver that does not meet these requirements can still be contributed and released. This section describes what Driver Hub checks and expects, and ends with a [checklist](#driver-hub-checklist).

To see whether Driver Hub could build and deploy a driver directory, run this from the repository root (Python 3.11+ with `jsonschema`):

```sh
python3 tools/validate-driver.py directory Drivers/<Vendor>/<Driver>
```

The [Drivers workflow](../.github/workflows/drivers.yml) runs the same check on every pull request and reports the result as annotations and a job summary. It never fails the pull request. See [tools/README.md](../tools/README.md) for the other checks, including a check of a release's assets.

### Listing and versions

- Driver Hub lists the repository's GitHub Releases. A release counts only if it is published (not a draft), its tag matches `drivers/<driverId>/v<version>`, and it has an uploaded asset named exactly `driver-release.json`.
- `driverId` is lowercase letters and digits in hyphen-separated groups, such as `exampleco-meter`. `version` is [Semantic Versioning 2.0.0](https://semver.org/) without a leading `v` and without leading zeros.
- A release is a prerelease if it is marked as one on GitHub, or if its version (ignoring `+build` metadata) contains `-`. Driver Hub hides prereleases unless the operator asks for them.
- Versions starting `0.0.0-local.` are reserved for Driver Hub's own builds and are refused.

### Release record and assets

`driver-release.json` is described in [What to publish](#what-to-publish). Driver Hub requires:

- `schemaVersion` 1, and `driverId` and `version` equal to the tag's.
- A non-empty `displayName`.
- `source.commit` as 40 lowercase hex characters, and a non-empty `source.directory`.
- `origin` absent or `"maintainer"`.
- `configuration.file` set to `driver-config.json`, and `configuration.sha256` set to the SHA-256 of that asset's bytes.
- `image.digest` as `sha256:<64 hex>`, the digest of the root manifest or index descriptor. `image.archive` names an uploaded asset matching `[A-Za-z0-9_.-]+.tar.gz` (this repository uses `.oci.tar.gz`). `image.archiveSha256` is the SHA-256 of the archive's bytes. `image.platforms` is a non-empty list such as `["linux/amd64"]`.
- The assets named by `configuration.file` and `image.archive` uploaded on the same release.

Driver Hub ignores `deploymentBundle`; it is this repository's convention for manual Compose installations.

### Image

- The archive is a gzipped OCI image layout: `oci-layout` with `imageLayoutVersion` 1.0.0, `index.json`, and `blobs/sha256/…`. It is at most 2 GiB to download, 8 GiB expanded and 32,768 entries.
- The root is `image.digest`. It is either one image manifest, or an index with exactly one manifest per platform, each with a platform descriptor. Build without attestations (`docker buildx build --provenance=false --sbom=false`), so the index holds only runnable images.
- Every reachable blob must be present with the right size and SHA-256. Every image config must state `os` and `architecture` (and `variant`, if any). Every platform listed in the record must be in the image.
- Driver Hub runs `linux/amd64` and `linux/arm64`, and places a driver only on architectures its image contains. A multi-platform image runs on a mixed cluster.

### Declaration (`driver-config.json`)

The top level has `schemaVersion` 1, `inputs` (an array), and optionally `controlHealth`, `nodeIdInput` and `videoIPathInputs`. Driver Hub refuses anything else. Each input:

| Field | Rule |
| --- | --- |
| `name` | Required. Matches `^[A-Za-z][A-Za-z0-9_.-]*$` and is unique. |
| `type` | Required. `string`, `integer` or `boolean`. |
| `required` | Required. A boolean. |
| `env` or `file` | Exactly one. `env` is an environment variable name matching `^[A-Za-z_][A-Za-z0-9_]*$` that does not start with `NOMAD_`. `file` is an absolute container path made of `[A-Za-z0-9_.-]` segments, with no `.` or `..` segment; it is mounted read-only. No two inputs share a binding. |
| `secret` | Optional boolean. Strings only, and never with a `default`. |
| `default` | Optional. Must itself be valid for the input: right type, within the bounds. |
| `minimum`, `maximum` | Optional. Integers only, `minimum` ≤ `maximum`. |
| `minLength`, `maxLength` | Optional. Strings only, integers ≥ 0, `minLength` ≤ `maxLength`. Driver Hub enforces them on non-secret values and reports a value that is too long as `too_long`. On secrets they are accepted but not applied. |
| `description` | Optional. A non-empty string of at most 1,000 characters, shown to operators as the input's hint. |

Any other input property, such as `label` or `enum`, is refused.

**Inputs Driver Hub supplies.** Operators cannot override these:

- `VIDEOIPATH_NODE_ID` is always set to the device's stable, lowercase, non-nil UUID, whether or not the driver declares it.
- `VIDEOIPATH_URL` and `VIDEOIPATH_TOKEN` are filled from the site when the device takes its VideoIPath target from the site, which is the default. The URL is the complete `wss://<host>/x-driver/v1`; the token is reserved for the device from a pool. If the driver does not declare these inputs, Driver Hub adds them. For a manually connected device, the operator enters both.

The simplest driver reads exactly these three variables. If the image reads them under other names or from files, say so:

- `nodeIdInput` names the input that carries the node ID. It must be a non-secret `string` input without a `default`. Use it only if the node ID is not bound to `VIDEOIPATH_NODE_ID`.
- `videoIPathInputs` is `{"url": "<input name>", "token": "<input name>"}`; either key is optional. `url` must name a non-secret string input and `token` a secret string input. Neither may be the node-ID input, nor bound to `VIDEOIPATH_NODE_ID` or to the other role's variable. No other input may be bound to that role's variable (`VIDEOIPATH_URL` or `VIDEOIPATH_TOKEN`).

**Secrets.** For a `secret` input the operator never enters the value. They enter the name of a key (`^[A-Za-z_][A-Za-z0-9_]{0,127}$`) in the Nomad Variable `nomad/jobs/driverhub-<node UUID>`. Nomad renders the value into the declared environment variable or read-only file when the task starts. Read secrets from that variable or file at start-up, and never log them.

### Runtime

- **One instance per device.** Each device runs exactly one instance of the image: a Nomad job with count 1, pinned by digest. It is stopped when an operator asks.
- **Replacement.** If its machine loses contact for 30 seconds, Nomad stops that instance and starts a replacement on another machine with the same node ID. VideoIPath lets a newer session with the same node ID replace the older one. The driver must tolerate both being replaced and replacing; a lock file on one machine cannot prevent the other instance.
- **Stopping.** Driver Hub stops every driver task with SIGTERM and kills it 5 seconds later (`kill_signal = "SIGTERM"`, `kill_timeout = "5s"`). Handle SIGINT the same way. Close the VideoIPath session and release the device well within those 5 seconds. Nomad's Docker driver starts no init process by default, so the driver may run as PID 1 and must install its own signal handlers.
- **Resources.** Fixed at 500 MHz CPU and 256 MiB memory.
- **Fresh containers.** Every restart, update or move starts a fresh container. No volume is mounted except declared `file` inputs, so nothing written inside the container survives. Rebuild state from VideoIPath and the device.
- **Network.** Outbound connections only (VideoIPath and the device), apart from the `controlHealth` port. The image's default `ENTRYPOINT`/`CMD` must start the driver.
- **File system.** The container may run with a read-only root file system. Anything the driver writes must go to a path that works without a volume, such as a directory the image owns or a temporary file system. The image should not run as root.

### Monitoring (`controlHealth`)

Declare `"controlHealth": {"port": <1-65535>, "path": "<path>"}` to let Driver Hub report whether the driver is connected. The path matches `^/[A-Za-z0-9_/-]*$` and contains no `//`.

- Driver Hub registers a Nomad HTTP check named `driver-communication`. It sends `GET <path>` to the port every 10 seconds, with a 2-second timeout. Listen on that port on all interfaces inside the container, over plain HTTP; Driver Hub maps a host port to it.
- Answer 2xx only while the authenticated VideoIPath session is established **and** communication with the device works. Driver Hub then shows "driver reports connected". Otherwise answer non-2xx. A live process alone is not enough.
- A result older than 30 seconds, or no single current instance, counts as unknown. Without `controlHealth`, communication shows as *not observable*; the driver is still deployable.
- Driver Hub never reports VideoIPath approval as verified.
- The endpoint must not expose secrets, issue device commands or require authentication.
- Driver Hub always checks the declared port. If an input makes the port configurable, its `default` must be the declared port; fixing it with equal `minimum` and `maximum` prevents a mismatch.

### Updates and rollback

- Driver Hub suggests the newest stable version of the same `driverId`, compared numerically by SemVer. A stable release beats a prerelease of the same core version. Local builds are never suggested as outdated.
- An update keeps the node ID and the current values, and validates them against the new version's declaration. A version that adds a required input without a default is refused until the operator enters a value. When the operator edits values for the new version, inputs it no longer declares are dropped.
- Rollback restores the previous version with the values saved with it.

So keep input names and bindings stable across versions, add new inputs as optional with defaults where possible, and never reuse a version number or change a published release's assets.

### Building from source

Driver Hub can build a driver itself, without a release. It builds `https://github.com/VideoIPath/VideoIPath_External_Drivers.git#<commit>:<directory>` with Docker Buildx for `linux/amd64` or `linux/arm64`, with a 30-minute limit.

- `<directory>` matches `^(Drivers|examples)(/[A-Za-z0-9_-][A-Za-z0-9_.-]*)+$`. A `Dockerfile` must be at its root and must build with that directory as the whole build context, so it cannot copy files from outside it.
- `<directory>/driver-config.json` at that commit is used, unless the operator supplies one.
- The result becomes version `0.0.0-local.<uuid>` of the chosen `driverId`.

### Driver Hub checklist

A driver is fully supported by Driver Hub when all of these hold. `python3 tools/validate-driver.py directory Drivers/<Vendor>/<Driver>` checks the directory name, the `Dockerfile`'s presence and the declaration; the rest needs a review or a test run.

- [ ] The driver lives in `Drivers/<Vendor>/<Driver>`, using only letters, digits, `_`, `-` and `.`, with no name starting with `.`.
- [ ] A `Dockerfile` at the directory root builds with only that directory as context, within 30 minutes, for every platform you support (`linux/amd64`, `linux/arm64`).
- [ ] `driver-config.json` sits next to the `Dockerfile` and declares every input the driver reads, using only the properties in the [table above](#declaration-driver-configjson), with defaults within their bounds and unique names and bindings.
- [ ] The driver takes its node ID from `VIDEOIPATH_NODE_ID` and its VideoIPath endpoint and token from `VIDEOIPATH_URL` and `VIDEOIPATH_TOKEN`, or declares `nodeIdInput` and `videoIPathInputs`. It uses the URL exactly as given (`wss://<host>/x-driver/v1`).
- [ ] Secrets are declared `secret: true` without a default, read from their variable or file at start-up, and never logged.
- [ ] The image's default `ENTRYPOINT`/`CMD` starts the driver, as a non-root user.
- [ ] The driver works in a fresh container with no volume and a read-only root file system, within 500 MHz CPU and 256 MiB memory, and rebuilds its state from VideoIPath and the device.
- [ ] It makes outbound connections only, apart from the `controlHealth` port.
- [ ] It tolerates being replaced by, and replacing, another instance with the same node ID.
- [ ] SIGTERM and SIGINT close the VideoIPath session and release the device, and the process exits well within 5 seconds, also when it runs as PID 1.
- [ ] Recommended: `controlHealth` is declared. The endpoint answers plain HTTP `GET` on all interfaces within 2 seconds, and answers 2xx only while the VideoIPath session is authenticated and the device is reachable. It needs no authentication, exposes no secrets and issues no device commands.
- [ ] A new version keeps input names and bindings, and adds new inputs as optional with defaults where possible.
- [ ] For a release: the tag is `drivers/<driverId>/v<version>`, all four assets are on the same release, which is created as a draft first, and `python3 tools/validate-driver.py release <assets-directory> --tag <tag>` passes.
