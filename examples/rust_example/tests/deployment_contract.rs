//! Deployment parity and secret-safe interpolation tests for the single-binary package.
//! Ordinary tests are offline; the explicit ignored check requires Docker Compose only.

// Import the actual application schema, rather than duplicating its public option list.
use rust_example::config;

use std::collections::{BTreeMap, BTreeSet};

const ENV: &str = include_str!("../.env.example");
const COMPOSE: &str = include_str!("../compose.yaml");
const DEPLOYMENT: &[&str] = &[
    "COMPOSE_PROJECT_NAME",
    "DRIVER_IMAGE",
    "DRIVER_RESTART_POLICY",
    "DRIVER_CERTIFICATE_HOST_PATH",
    "DRIVER_LOCK_HOST_PATH",
    "DRIVER_STOP_GRACE_PERIOD",
    "DRIVER_HEALTHCHECK_INTERVAL",
    "DRIVER_HEALTHCHECK_TIMEOUT",
    "DRIVER_HEALTHCHECK_RETRIES",
    "DRIVER_HEALTHCHECK_START_PERIOD",
    "DRIVER_CPU_LIMIT",
    "DRIVER_MEMORY_LIMIT",
    "DRIVER_PIDS_LIMIT",
    "DRIVER_LOG_MAX_SIZE",
    "DRIVER_LOG_MAX_FILES",
];

fn inventory() -> BTreeMap<&'static str, &'static str> {
    let mut values = BTreeMap::new();
    for line in ENV
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
    {
        let (key, value) = line.split_once('=').expect("environment assignment");
        assert!(
            values.insert(key, value).is_none(),
            "duplicate public option: {key}"
        );
    }
    values
}

#[test]
fn complete_inventory_and_explicit_forwarding() {
    let values = inventory();
    let expected: BTreeSet<_> = config::OPTIONS
        .iter()
        .map(|(key, _)| *key)
        .chain(DEPLOYMENT.iter().copied())
        .collect();
    assert_eq!(
        values.keys().copied().collect::<BTreeSet<_>>(),
        expected,
        "application and deployment options must be documented exactly once"
    );
    for (key, default) in config::OPTIONS {
        assert!(
            COMPOSE.contains(&format!("      {key}: \"${{{key}:")),
            "application option must be explicitly forwarded: {key}"
        );
        if *key == "VIDEOIPATH_URL" {
            assert!(values[key].ends_with(".invalid/x-driver/v1"));
        } else {
            assert_eq!(
                values[key], *default,
                "documented application default: {key}"
            );
        }
        if !default.is_empty() {
            assert!(
                COMPOSE.contains(&format!("${{{key}:-{default}}}")),
                "Compose default differs from the application: {key}"
            );
        }
    }
    for key in DEPLOYMENT {
        assert!(
            COMPOSE.contains(&format!("${{{key}:")),
            "unused deployment option: {key}"
        );
    }
}

#[test]
fn driverhub_declares_every_application_input_once() {
    let declaration: serde_json::Value =
        serde_json::from_str(include_str!("../driver-config.json")).unwrap();
    let mut expected: BTreeMap<_, _> = config::OPTIONS.iter().copied().collect();
    let mut names = BTreeSet::new();
    for input in declaration["inputs"].as_array().unwrap() {
        assert!(names.insert(input["name"].as_str().unwrap()));
        let binding = input["env"].as_str().unwrap();
        let default = expected.remove(binding).expect("unique runtime binding");
        let required = matches!(
            binding,
            "VIDEOIPATH_URL" | "VIDEOIPATH_TOKEN" | "VIDEOIPATH_NODE_ID"
        );
        assert_eq!(input["required"], required, "{binding}");
        assert_eq!(
            input["secret"].as_bool().unwrap_or(false),
            binding == "VIDEOIPATH_TOKEN"
        );
        if let Ok(number) = default.parse::<u64>() {
            assert_eq!(input["type"], "integer");
            assert_eq!(input["default"], number, "{binding}");
        } else {
            assert_eq!(input["type"], "string");
            assert_eq!(
                input["default"].as_str().unwrap_or_default(),
                default,
                "{binding}"
            );
        }
    }
    assert!(
        expected.is_empty(),
        "missing DriverHub bindings: {expected:?}"
    );
}

#[test]
fn delivery_keeps_build_context_and_identity_boundaries() {
    // These are regression guards on security boundaries, not a substitute for image inspection.
    let dockerfile = include_str!("../Dockerfile");
    assert!(!COMPOSE.contains("    build:"));
    assert!(!COMPOSE.contains("ports:"));
    assert!(!COMPOSE.contains("depends_on:"));
    assert!(!COMPOSE.contains("simulator"));
    assert!(!COMPOSE.contains("sockets:"));
    assert!(dockerfile.contains("cargo build --locked --release --bin rust-example"));
    assert!(!dockerfile.contains("COPY . ."));
    assert_eq!(
        dockerfile
            .lines()
            .filter(|line| line.starts_with("FROM ") && line.contains("@sha256:"))
            .count(),
        2
    );
    assert!(
        include_str!("../.dockerignore")
            .lines()
            .any(|line| line == "**")
    );
    for key in [
        "COMPOSE_PROJECT_NAME",
        "DRIVER_IMAGE",
        "VIDEOIPATH_TOKEN",
        "VIDEOIPATH_NODE_ID",
    ] {
        assert!(
            inventory()[key].is_empty(),
            "identity/credential placeholder must be unusable: {key}"
        );
    }
}

#[test]
fn required_application_settings_fail_closed_without_secret_echo() {
    let valid: BTreeMap<String, String> = [
        ("VIDEOIPATH_URL", "wss://vip.example.test/x-driver/v1"),
        (
            "VIDEOIPATH_TOKEN",
            "test-only$DOLLAR # \"double\" 'single' spaced",
        ),
        ("VIDEOIPATH_NODE_ID", "afe3cf0d-d906-4a89-a495-5f6a989a16c1"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    assert!(config::Config::parse(&valid, true).is_ok());
    for key in ["VIDEOIPATH_URL", "VIDEOIPATH_TOKEN", "VIDEOIPATH_NODE_ID"] {
        for missing in [true, false] {
            let mut inputs = valid.clone();
            if missing {
                inputs.remove(key);
            } else {
                inputs.insert(key.into(), String::new());
            }
            let error = config::Config::parse(&inputs, true)
                .err()
                .expect("missing and empty required settings must fail")
                .to_string();
            assert!(
                error.contains(key),
                "configuration error must identify the setting"
            );
            assert!(
                !error.contains(&valid["VIDEOIPATH_TOKEN"]),
                "configuration error exposed credential"
            );
        }
    }
}

#[test]
#[ignore = "requires Docker Compose; run explicitly with --ignored"]
fn compose_preserves_literal_credentials_without_printing_them() {
    use std::{fs, io::Write, os::unix::fs::OpenOptionsExt, process::Command};
    // This is synthetic data. Resolved configuration stays in memory and is never logged or saved.
    let literal = "test-only$DOLLAR # \"double\" 'single' spaced";
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let scratch = if std::path::Path::new("/tmp/opencode").is_dir() {
        std::path::PathBuf::from("/tmp/opencode")
    } else {
        std::env::temp_dir()
    };
    let path = scratch.join(format!(
        "rust-example-compose-{}-{}.env",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    // RAII also removes the protected fixture if an assertion unwinds.
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(path.clone());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    for (key, value) in inventory() {
        let replacement = match key {
            "COMPOSE_PROJECT_NAME" => "rust-example-contract-check",
            "DRIVER_IMAGE" => "rust-example:contract-test",
            "VIDEOIPATH_URL" => "wss://vip.example.test/x-driver/v1",
            "VIDEOIPATH_NODE_ID" => "afe3cf0d-d906-4a89-a495-5f6a989a16c1",
            // Compose single quotes preserve $, #, spaces and double quotes literally.
            "VIDEOIPATH_TOKEN" => "'test-only$DOLLAR # \"double\" \\'single\\' spaced'",
            _ => value,
        };
        writeln!(file, "{key}={replacement}").unwrap();
    }
    drop(file);
    let invoke = |args: &[&str]| {
        let mut command = Command::new("docker");
        // A contaminated developer shell must not affect the fixture or inject a real secret.
        command
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
        command
            .current_dir(root)
            .args(["compose", "--env-file"])
            .arg(&path)
            .args(["-f", "compose.yaml"])
            .args(args)
            .output()
            .expect("Docker Compose executable required")
    };
    let quiet = invoke(&["config", "--quiet"]);
    assert!(
        quiet.status.success(),
        "Compose quiet validation failed (output suppressed)"
    );
    let built = invoke(&["-f", "compose.build.yaml", "config", "--quiet"]);
    assert!(
        built.status.success(),
        "Compose build-override validation failed (output suppressed)"
    );
    let output = invoke(&["config", "--format", "json"]);
    assert!(
        output.status.success(),
        "Compose interpolation failed (output suppressed)"
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)
        .expect("Compose must return valid JSON; response deliberately not printed");
    let services = &parsed["services"];
    // Compose escapes literal dollars when rendering a reusable config document.
    // This is serialization escaping, not a second interpolation of the credential.
    // Use boolean assertions: assert_eq! would print both token strings on failure.
    assert!(
        services["driver"]["environment"]["VIDEOIPATH_TOKEN"].as_str()
            == Some(literal.replace('$', "$$").as_str()),
        "literal credential interpolation changed"
    );
    assert!(services.as_object().is_some_and(|items| items.len() == 1));
    let driver = &services["driver"];
    assert!(driver["read_only"] == true);
    assert!(driver["init"] == true);
    assert!(driver["user"] == "10001:10001");
    assert!(driver["cap_drop"] == serde_json::json!(["ALL"]));
    assert!(driver["security_opt"] == serde_json::json!(["no-new-privileges:true"]));
    assert!(driver.get("ports").is_none());
    assert!(driver.get("build").is_none());
    assert!(driver.get("depends_on").is_none());
    assert!(driver["restart"] == "on-failure:5");
    assert!(driver["command"] == serde_json::json!(["driver"]));
    assert!(
        parsed["volumes"]
            .as_object()
            .is_some_and(|items| items.len() == 1)
    );
    let forwarded: BTreeSet<_> = driver["environment"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert!(forwarded == config::OPTIONS.iter().map(|(key, _)| *key).collect());
    for obsolete in [
        "DRIVER_SOCKET",
        "DRIVER_CONTROL_SOCKET",
        "DRIVER_OPERATION_TIMEOUT_MS",
    ] {
        assert!(!forwarded.contains(obsolete));
    }
    // A malformed operator file can make Compose echo source text in an error.
    // The wrapper must suppress those raw diagnostics, even on its failure path.
    fs::write(&path, "VIDEOIPATH_TOKEN='test-only-sensitive-unclosed\n").unwrap();
    let failure = Command::new("sh")
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .current_dir(root)
        .arg("scripts/check-compose.sh")
        .arg(&path)
        .output()
        .unwrap();
    assert!(!failure.status.success(), "malformed environment must fail");
    for bytes in [&failure.stdout, &failure.stderr] {
        assert!(
            !String::from_utf8_lossy(bytes).contains("test-only-sensitive"),
            "validation wrapper leaked environment source text"
        );
    }
}
