# Changelog

## [Unreleased]

- Prepare 0.4.0: `driver-config.json` uses schemaVersion 2 to declare what the configuration parser already enforces. `videoipathUrl` accepts only `wss://<host>/x-driver/v1` without credentials, query or fragment; `logLevel` and `configurationOrder` accept only their listed words. Runtime behavior is unchanged. `tests/declared-values.json` checks the declaration against the parser and lists the rules the declaration cannot express.

### Migrating from 0.3 to 0.4

Nothing changes for manual Compose installations. A Driver Hub release that does not read schemaVersion 2 refuses the 0.4 package and source builds of this directory; keep using 0.3.0 with it. Input names, bindings and defaults are unchanged, so values saved for 0.3.0 carry over, but a value 0.3.0 accepted at deployment and then refused at start-up (such as a `ws://` URL) is now refused before deployment.

- Add DriverHub v1 packaging and offline configuration preflight for the Rust teaching example. The stable ID is `rust-example`; packaging does not change the node UUID, demo state, or commissioning status.
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
