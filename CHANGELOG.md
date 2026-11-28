# Changelog

## [Unreleased]

- Add `driver-config.json` schemaVersion 2, which adds per-input `url` and `enum` value constraints, with declaration and value fixtures shared with Driver Hub. The validator picks the schema by version and refuses unknown versions; version 1 is unchanged. Driver Hub reads version 2; older Driver Hub releases refuse it, which the pull-request check notes. The Rust example declares version 2 from 0.4.0.
- Validate the Rust example's declaration before packaging it, and verify the finished assets before writing them. CI checks the packaged release with `tools/validate-driver.py release`.
- Document what Driver Hub requires of a contributed driver, with a checklist, in Drivers/RELEASING.md. Align the declaration and release schemas with Driver Hub, and add an informational pull-request check of whether Driver Hub could build and deploy each driver.

- Simplify the Rust API example to one driver with in-memory demo state; move the independent simulator to optional test tooling.
- Refocus the Rust example and repository guides on building your own driver, with an executable parameter walkthrough and API-oriented source comments.
- Extend the Rust example with redundant SDP, selectable receiver resources, boundary tests, and a repeatable local soak workload.
- Build the Rust example with a separate device simulator, secure container deployment, commented protocol examples, and local validation tests.
- Clarify that the Rust example's commissioning status is separate from the maturity of the External Driver API.
- Add a Rust example overview inspired by the repository guide, with planned capabilities and explicit validation limits.
- Prepare the `rust_example` example directory and exclude its local implementation plan from Git.
