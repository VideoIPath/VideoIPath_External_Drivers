# Contributing external drivers

This repository is for external drivers that contributors use with VideoIPath. Contributions are made through pull requests. Please also read the [Code of Conduct](CODE_OF_CONDUCT.md).

## Driver expectations

- Contribute a driver that you have tested with a real device and VideoIPath. Verify the capabilities you claim to support; a driver does not have to implement every External Driver API capability.
- Follow the driver-development guidance in the [main README](README.md). Python and Rust examples are planned as references; drivers may be written in any language.
- Put each driver in its own `Drivers/<Vendor>/<Driver>/` directory. Include the source code, dependency information, build and installation instructions, deployment configuration, and driver README in that directory. Someone should be able to build and run the driver using the contents of that directory and its documented dependencies.
- Document the device model, firmware, and VideoIPath version used for testing. Distinguish tested combinations from other devices or versions the driver is expected to support, and describe any known limitations.
- Include a Docker Compose example showing the configuration needed to run the driver. Document the steps to build the image locally and ensure the example uses that image. A published or prebuilt image is not required; a Dockerfile is optional if the build instructions provide another way to build the image.
- Do not commit credentials, tokens, private keys, certificates, or other secrets. Use placeholders and document how users supply their own values at deployment time.

## Driver README

Each driver directory must contain a `README.md`. You can copy [the driver README template](Drivers/README.template.md) to get started. Cover at least:

- What the driver does, the device models and firmware it supports, and a high-level explanation of how it works.
- The capabilities it implements and the device and VideoIPath versions on which the driver was tested, distinct from any other expected-to-work combinations.
- Dependencies, build and installation steps, configuration options, and how to run it using the included Compose example.
- How to check that it is working, and any known limitations or issues.

## Submit a pull request

Add your driver under `Drivers/<Vendor>/<Driver>/` and submit a pull request. In the pull request description, identify the driver and device, summarize what you tested (including device firmware and VideoIPath version), and call out any limitations. Keep the documentation and Compose example in the same pull request so reviewers can understand how to build and use the driver.

Contributed driver code must be distributable under this repository's [GNU GPL version 3 license](LICENSE). Only submit code you have the right to contribute on those terms. Identify any third-party code included in the driver and any external dependencies, along with their licenses; ensure you have the rights needed to distribute the included code under the repository's license.

## Maintenance and support

Nevion does not maintain or support contributed drivers. Anyone may propose extensions, troubleshooting improvements, and fixes through pull requests. Document known limitations so others can assess whether a driver meets their needs.
