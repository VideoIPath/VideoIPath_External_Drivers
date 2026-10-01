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

#[test]
fn runtime_cli_exposes_only_the_driver_and_local_diagnostics() {
    let binary = env!("CARGO_BIN_EXE_rust-example");
    let help = std::process::Command::new(binary)
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for command in ["driver", "health", "status", "--version"] {
        assert!(text.contains(command));
    }
    for command in ["simulator", "simctl"] {
        assert!(!text.contains(command));
        let result = std::process::Command::new(binary)
            .env_clear()
            .arg(command)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("unknown command"));
    }
}
