//! The value rules in driver-config.json (`url`, `enum`, bounds) must agree with the real
//! configuration parser. `tools/tests` checks each case's `declared` verdict against the
//! declaration; this test checks its `parser` verdict against `Config::parse`, so the two
//! are compared through one shared file rather than a second copy of the rules.
use rust_example::config::Config;

use serde_json::Value;
use std::collections::BTreeMap;

fn declaration() -> Value {
    serde_json::from_str(include_str!("../driver-config.json")).unwrap()
}

/// Every input valid, so a case changes exactly one.
fn valid_environment() -> BTreeMap<String, String> {
    [
        ("VIDEOIPATH_URL", "wss://vip.example.test/x-driver/v1"),
        ("VIDEOIPATH_TOKEN", "local-test-only credential"),
        ("VIDEOIPATH_NODE_ID", "afe3cf0d-d906-4a89-a495-5f6a989a16c1"),
    ]
    .map(|(k, v)| (k.into(), v.into()))
    .into()
}

fn binding(declaration: &Value, input: &str) -> String {
    declaration["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == input)
        .unwrap_or_else(|| panic!("undeclared input {input}"))["env"]
        .as_str()
        .expect("every input is bound to an environment variable")
        .into()
}

fn parses(variable: String, value: String) -> bool {
    let mut environment = valid_environment();
    environment.insert(variable, value);
    Config::parse(&environment, true).is_ok()
}

#[test]
fn baseline_is_valid() {
    assert!(Config::parse(&valid_environment(), true).is_ok());
}

#[test]
fn parser_verdicts_match_the_shared_cases() {
    let declaration = declaration();
    let suite: Value = serde_json::from_str(include_str!("declared-values.json")).unwrap();
    let mut wrong = Vec::new();
    for case in suite["cases"].as_array().unwrap() {
        let input = case["input"].as_str().unwrap();
        let value = match &case["value"] {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        };
        let expected = case["parser"].as_bool().unwrap();
        if parses(binding(&declaration, input), value.clone()) != expected {
            wrong.push(format!(
                "{input}={value:?}: parser should {}",
                if expected { "accept" } else { "reject" }
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_declared_choice_is_accepted() {
    let declaration = declaration();
    for item in declaration["inputs"].as_array().unwrap() {
        let Some(choices) = item["enum"].as_array() else {
            continue;
        };
        let variable = item["env"].as_str().unwrap();
        for choice in choices {
            let choice = choice.as_str().unwrap();
            assert!(
                parses(variable.into(), choice.into()),
                "{variable}={choice}"
            );
            assert!(
                !parses(variable.into(), choice.to_uppercase()),
                "{variable} is case-sensitive"
            );
        }
    }
}
