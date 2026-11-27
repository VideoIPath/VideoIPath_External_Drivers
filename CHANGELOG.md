# Changelog

## [Unreleased]

- Document what Driver Hub requires of a contributed driver, with a checklist, in Drivers/RELEASING.md. Align the declaration and release schemas with Driver Hub, and add an informational pull-request check of whether Driver Hub could build and deploy each driver.

- Simplify the Rust API example to one driver with in-memory demo state; move the independent simulator to optional test tooling.
- Refocus the Rust example and repository guides on building your own driver, with an executable parameter walkthrough and API-oriented source comments.
- Extend the Rust example with redundant SDP, selectable receiver resources, boundary tests, and a repeatable local soak workload.
- Build the Rust example with a separate device simulator, secure container deployment, commented protocol examples, and local validation tests.
- Clarify that the Rust example's commissioning status is separate from the maturity of the External Driver API.
- Add a Rust example overview inspired by the repository guide, with planned capabilities and explicit validation limits.
- Prepare the `rust_example` example directory and exclude its local implementation plan from Git.
