# Run the Rust example with DriverHub

`rust-example` teaches the external-driver API using an in-memory device. There is no physical-device address, device firmware, or vendor credential to configure. No VideoIPath release or physical device is claimed as commissioned. The supported package platform is Linux x86-64 (`linux/amd64`); use Docker Engine and Compose v2 with `--wait` and `--pull never`.

## Import and configure

1. Transfer the release record, `driver-config.json`, the named `.oci.tar.gz` and `deployment.tar.gz`. DriverHub must verify their checksums, image digest and platform before installation. A local package is a customer build. Retain the verified files for recovery.
2. Import the verified OCI image into the local runtime. Without DriverHub, on a matching host with Skopeo, decompress the verified image archive to `image.tar` and run `skopeo copy oci-archive:image.tar docker-daemon:local/rust-example:<unique-tag>` using a unique local tag for this digest. Extract the verified deployment bundle into a new directory and run the remaining commands there.
3. Set `DRIVER_IMAGE` to the verified local image reference and `COMPOSE_PROJECT_NAME` to the persistent name for this instance. DriverHub's site configuration or a manually prepared `.env` supplies these deployment settings. The bundled `.env.example` documents defaults and host controls.
4. Supply the full `wss://<host>/x-driver/v1` endpoint, a stable non-placeholder node UUID, and a local credential reference for `videoipathToken`. The process reads the resolved secret in `VIDEOIPATH_TOKEN`; it does not read a token file. Keep actual values out of release files and logs. For manual Compose, copy `.env.example` to `.env` with mode `0600`; edit privately and single-quote literal values containing `$` or `#`. Exported environment variables take precedence over `.env`.
5. Prepare the shared host lock directory (default `/var/lock/videoipath-drivers`, UID:GID `10001:10001`, mode `0700`). Every instance on that host must bind the same underlying lock directory. Create `deployment/certificates/` even when using system trust. For a private CA, place only the public certificate there and set `VIDEOIPATH_CA_BUNDLE` to its readable container path under `/etc/rust-example/certificates/`. The process needs outbound access to the configured VideoIPath endpoint; it publishes no host port.

## Validate before stopping an existing instance

Validate the intended settings against this version's `driver-config.json`. Then run the candidate image's own parser using the **target** Compose settings. This also checks UUID/URL rules, readable CA path, and related timeouts; it does not acquire the node lock, open a connection or print setting values:

```sh
docker compose -f compose.yaml config --quiet
docker compose -f compose.yaml run --rm --no-deps --pull never driver check-config
```

Stop the previous owner before starting the replacement, including when moving between hosts. Never run two instances using the same node UUID. Retain the same project name, UUID, credential reference and trust configuration.

```sh
docker compose -f compose.yaml up -d --wait --no-build --pull never
docker compose -f compose.yaml exec -T driver rust-example health
docker compose -f compose.yaml exec -T driver rust-example status
```

`health` checks the local heartbeat. `videoipath=true` in `status` indicates a backend connection; `device=true` refers to the in-memory model. Confirm the expected identity and approval in VideoIPath Inventory and read `audio_gain` to check the example's advertised control.

## Recovery and rollback

Back up the protected site settings, UUID, CA references and the previous four package files before upgrading. The named volume holds diagnostics; it does not persist the demo's controls or alarms. Record desired demo values and reapply after restart. To roll back, stop the current instance, restore the previous deployment files and image, validate the previous configuration, and start it as the sole owner. For a host move, fence the old instance first; there is no cross-host lock. See `RELEASE-NOTES.md` for version-specific migration limits.
