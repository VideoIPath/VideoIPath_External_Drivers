//! Keep the teaching frames executable: documentation examples are local inputs,
//! not an official schema or evidence of backend acceptance.
use rust_example::{domain::Direction, protocol, registry::Registry};
use serde_json::Value;

#[test]
fn redundant_sdp_documented_configurations_decode_without_guessing_defaults() {
    let document = include_str!("../docs/REDUNDANT_SDP.md");
    let registry = Registry::new();
    let mut configured = 0;
    let mut disabled = 0;
    for block in document.split("```json\n").skip(1) {
        let text = block.split("```").next().unwrap();
        let frame: Value = serde_json::from_str(text).unwrap();
        assert!(frame["message_id"].is_u64());
        if frame["message_type"] != "set_ipt_configurations" {
            continue;
        }
        for (field, direction) in [
            ("sender_configs", Direction::Sender),
            ("receiver_configs", Direction::Receiver),
        ] {
            for config in frame["data"][field].as_array().into_iter().flatten() {
                let key = protocol::config_key("module-1", config, direction).unwrap();
                if protocol::media_state(&registry, &key, config)
                    .unwrap()
                    .is_some()
                {
                    configured += 1;
                } else {
                    disabled += 1;
                }
            }
        }
    }
    // Guard against accidentally deleting/mislabeling all the executable blocks.
    assert_eq!((configured, disabled), (2, 2));
}
