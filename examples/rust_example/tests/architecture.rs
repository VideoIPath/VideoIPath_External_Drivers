//! Small source-level boundary guards supplement, not replace, dependency review.

#[test]
fn application_and_domain_do_not_construct_transports_or_tasks() {
    for source in [
        include_str!("../src/application.rs"),
        include_str!("../src/domain.rs"),
    ] {
        for forbidden in [
            "tokio_tungstenite",
            "tokio::net",
            "tokio::spawn",
            "device::Client",
            "simulator::",
            "WebSocketStream",
        ] {
            assert!(
                !source.contains(forbidden),
                "capability boundary imported transport/task ownership: {forbidden}"
            );
        }
    }
}

#[test]
fn canonical_package_version_is_reported_by_structure_and_executable() {
    assert_eq!(
        rust_example::registry::Registry::new().structure()["version"],
        env!("CARGO_PKG_VERSION")
    );
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_rust-example"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(result.status.success());
    assert_eq!(
        String::from_utf8(result.stdout).unwrap().trim(),
        format!("rust-example {}", env!("CARGO_PKG_VERSION"))
    );
}
