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

## Verification

Describe how to confirm that the driver is connected and its documented capabilities work. Include any useful log messages or checks.

## Limitations and known issues

Describe unsupported features, device or firmware restrictions, and other known issues. State any important differences between the driver and the repository's examples or development guide.
