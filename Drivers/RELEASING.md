# Publishing a DriverHub-compatible driver

A release lets DriverHub discover one driver's updates and lets an operator install the same image at a disconnected site. Publishing is optional. The usual [driver contribution requirements](../CONTRIBUTING.md#driver-expectations) still apply. A release is not Nevion certification. It requires no change to the VideoIPath External Driver API.

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

Keep `driver-config.json` next to the driver's `README.md`. Stable input names let an operator reuse settings across versions; the binding tells DriverHub where this particular image reads each value. Declare all configurable driver inputs needed for the documented deployment, including the VideoIPath connection and stable device identity where supplied to the image. Packaging controls such as `DRIVER_IMAGE`, host paths, and the Compose project name are deployment settings, not inputs to the driver.

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
- On install, upgrade, migration **and rollback**, validate the intended settings against the **target version's** declaration. Report missing, unrecognized, wrongly typed or out-of-range settings by name and check required credential references are available before stopping the running instance. A newly required credential or a saved port of `70000` in the example must block cutover even offline. Passing this check does not establish device connectivity or VideoIPath approval.

## Make the release runnable offline

Use one released driver image per deployment. The packaged Compose service uses `image: "${DRIVER_IMAGE:?Set DRIVER_IMAGE}"` and no `build:`. Use `pull_policy: never` or the equivalent launch option. The process must start without downloading code, dependencies, or other images. Device, VideoIPath and other documented site-local connections are expected. The [Rust Compose example](../examples/rust_example/compose.yaml) illustrates local image selection; it is not a commissioned release.

The bundled README must tell an operator how to supply settings and credential files, prepare local paths and permissions, preserve the device's `node_id` and persistent state, and check both registration and device operation. Document tested device/firmware/VideoIPath combinations separately from expected compatibility. Put upgrade steps, required backups and downgrade limits in `RELEASE-NOTES.md` and the GitHub Release body. Verify redistribution rights for the **whole image**, including vendor SDKs; provide notices and corresponding source where required. Drivers with non-redistributable dependencies can still use the source contribution path.

## Publish a version

1. The contributor submits the driver, declaration, Compose configuration, documentation and commissioning evidence for review. A repository maintainer checks the driver ID and authorizes a publisher with tag/release access. Protect release tags and use immutable GitHub releases where available. Never move a published tag or replace its assets; any change to released bytes requires a new version.
2. Build from the exact reviewed commit. Test the **candidate image** using the packaged configuration on the declared platforms, including the documented device and VideoIPath checks. Export those same manifests and layers; do not rebuild between testing and publication.
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

DriverHub can perform the same import with its own runtime. Bind `DRIVER_IMAGE` to the **verified local image**, not a mutable registry tag. On host replacement, import the retained release, restore site configuration, credential references and state, and transfer ownership of the same `node_id` before starting the replacement. For rollback, select a previously retained release, check that version's inputs and follow its state-restore instructions. Keeping an image does not undo a data migration.

Keep customer-built images in a separate local catalog namespace with their actual image digest, source revision and matching configuration declaration. A build from a release tag is still a customer build and must never replace or impersonate an upstream release. Scheduling, credential storage, site approval and host operations belong to the deployment environment.
