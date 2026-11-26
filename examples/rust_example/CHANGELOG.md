# Changelog

## [Unreleased]

- Make 0.3.0 a single-process API example with DemoDevice, one-service Compose, and an opt-in device lab instead of an embedded simulator platform.
- Add a credential-free API walkthrough and deterministic in-memory demo tests for values, alarms, SDP, and configuration drift.
- Lead with API lessons and a tested gain-control walkthrough; explain how to replace simulated behavior with your device's capabilities rather than copy the sample architecture.
- Extend the 0.2.0 example with redundant SDP, selectable receiver resource partners, boundary tests, and opt-in local soak validation.

### Migrating from 0.2 to 0.3

Stop the entire 0.2 Compose project (both services) before starting the 0.3 driver.
Keep the 0.2 image and its matching Compose and configuration files for rollback,
preserve the node UUID, and reapply demo state after restart. See
[stop, upgrade and roll back](OPERATIONS.md#4-stop-upgrade-and-roll-back).

## [0.1.0] — 2026-09-30

- Add the Rust 0.1.0 reference driver, independent simulator, documented protocol scenarios, secure Compose delivery, and local test suite; live VideoIPath commissioning remains separate.
