# <Driver name>

## Overview

Describe what this driver does and the device it connects to VideoIPath. Explain at a high level how the driver communicates with the device and which VideoIPath capabilities it exposes.

## Supported devices and tested setup

| Item | Details |
| --- | --- |
| Tested device model(s) | <models used for testing> |
| Tested device firmware | <firmware version(s) used for testing> |
| Tested VideoIPath version | <version(s) used for testing> |
| Driver version or revision | <version or commit, if applicable> |

List the capabilities you verified on a real device, such as discovery, parameters, alarms, or endpoint configuration. If you expect the driver to work with other device models, firmware, or VideoIPath versions, distinguish those from the tested combinations.

## Prerequisites and dependencies

List required software, services, device settings, network access, and any third-party code included with the driver or external dependencies, with their licenses. State any minimum versions that matter.

## Build and installation

Explain how to build the driver and its container image locally from the files in this directory. State the resulting image name and tag so they match the Compose example. Include installation steps and any preparation needed on the device or VideoIPath. A prebuilt image is not required.

## Configuration and running with Docker Compose

Point to the Compose example in this directory. Document each relevant option, including connection addresses, authentication values, ports, volumes, and other device-specific settings. Explain how to provide secrets at deployment time without committing them to the repository. Show the commands needed to start the driver.

## Running a packaged release (omit for source-only drivers)

- Give the stable driver ID, release tag, supported host platforms, required runtime versions and the offline start command. Refer to the tested setup above.
- Link to `driver-config.json`. Explain where the operator obtains each required input, including credential references, the VideoIPath address, and the stable `node_id` for this device. List any site-supplied files, mounts, permissions, ports and local services that must exist before startup. Use placeholders only.
- Explain which settings and state must survive an upgrade or host replacement, how to back them up and restore them, and when a state or configuration change prevents downgrade. Identify version-specific steps in the bundled release notes.
- Explain how to check process health, VideoIPath registration/approval and one documented device capability after deployment.

## Driver Hub (omit if not supported)

State whether the driver meets the [Driver Hub checklist](https://github.com/VideoIPath/VideoIPath_External_Drivers/blob/main/Drivers/RELEASING.md#driver-hub-checklist), and list anything it does not meet. `python3 tools/validate-driver.py directory Drivers/<Vendor>/<Driver>` checks the parts that can be checked statically.

- **Supported platforms:** `linux/amd64`, `linux/arm64`, or both. Driver Hub places the driver only on architectures its image contains.
- **Inputs:** one row per input in `driver-config.json`. Under "Supplied by", say whether Driver Hub fills the input itself (`VIDEOIPATH_NODE_ID` always; `VIDEOIPATH_URL` and `VIDEOIPATH_TOKEN` for a site-connected device) or the operator enters it. For a secret, the operator enters the name of a key in the device's Nomad Variable, never the value. Under "Accepted values", give what the driver's parser accepts: the URL schemes and path, the enumerated choices, or the bounds. Declare the same rules in `driver-config.json` as `url` or `enum` ([`schemaVersion` 2](https://github.com/VideoIPath/VideoIPath_External_Drivers/blob/main/Drivers/RELEASING.md#declaration-schemaversion-2)). Declare only schemes the driver genuinely supports and you have tested.

  | Name | Type | Required | Binding | Secret | Default | Accepted values | Description | Supplied by |
  | --- | --- | --- | --- | --- | --- | --- | --- | --- |
  | `<name>` | `string` | yes | `env: VIDEOIPATH_URL` | no | | `wss` only, path `/x-driver/v1`, no credentials, query or fragment | <hint shown to operators> | Driver Hub (site) or operator |
  | `<name>` | `string` | yes | `env: VIDEOIPATH_NODE_ID` | no | | | <hint> | Driver Hub |
  | `<name>` | `string` | yes | `file: /run/secrets/<name>` | yes | | | <hint> | Operator (Nomad Variable key) |
  | `<name>` | `string` | no | `env: <NAME>` | no | `info` | one of `trace`, `debug`, `info`, `warn`, `error` | <hint> | Operator |

- **Site prerequisites:** what must be true at the site for valid values to work, and which the declaration cannot check: DNS names that must resolve, the CA that must be trusted (and how to supply it), listeners that must be reachable from the Nomad clients, firewall openings. Keep these apart from the accepted values above: a value can be valid and still fail at a site that lacks a prerequisite.

- **Health endpoint:** the declared `controlHealth` port and path, and exactly when it answers 2xx (only while the VideoIPath session is authenticated and the device is reachable). If there is none, say so: Driver Hub then shows communication as not observable.
- **Runtime:** anything the driver writes and where (no volume survives a restart), how it rebuilds state from VideoIPath and the device, and how long its shutdown takes after SIGTERM (Driver Hub kills it after 5 seconds).

## Verification

Describe how to confirm that the driver is connected and its documented capabilities work. Include any useful log messages or checks.

## Limitations and known issues

Describe unsupported features, device or firmware restrictions, and other known issues. State any important differences between the driver and the repository's examples or development guide.
