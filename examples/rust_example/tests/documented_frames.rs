//! Keep the teaching frames executable: documentation examples are local inputs,
//! not an official schema or evidence of backend acceptance.
use rust_example::{domain::Direction, protocol, registry::Registry};
use serde_json::Value;

/// Follow the README's first lesson through the same handler and target adapter
/// used by the driver. A fake backend is not needed to learn the value mapping;
/// session tests separately prove actual response-before-event socket delivery.
#[tokio::test]
async fn readme_gain_frames_follow_observed_device_state() {
    use rust_example::{
        application,
        demo::DemoDevice,
        domain::{Device, Location},
    };
    use std::sync::Arc;

    let frames: Vec<Value> = include_str!("../README.md")
        .split("```json\n")
        .skip(1)
        .map(|block| serde_json::from_str(block.split("```").next().unwrap()).unwrap())
        .collect();
    assert_eq!(
        frames.len(),
        5,
        "keep the first lesson's five frames executable"
    );
    let registry = Arc::new(Registry::new());
    let device = DemoDevice::new(registry.clone());
    let definition = &registry.structure()["dynamic_values"]["audio_gain"];
    assert_eq!(definition["value_info"]["type"], "read_write");
    assert_eq!(definition["type_info"]["range"]["step"], 0.5);
    let read = application::handle(
        &device,
        &registry,
        serde_json::from_value(frames[0].clone()).unwrap(),
    )
    .await;
    assert_eq!(read.response, frames[1]);
    assert!(read.changes.is_empty());

    let set = application::handle(
        &device,
        &registry,
        serde_json::from_value(frames[2].clone()).unwrap(),
    )
    .await;
    assert_eq!(set.response, frames[3]);
    assert_eq!(set.changes.len(), 1);
    assert_eq!(protocol::event_frame(&set.changes[0]), frames[4]);
    // Read the model's state, not just the response map; this is not hardware evidence.
    let observed = device.read_values(Location::root()).await.unwrap();
    assert_eq!(observed["audio_gain"], 2.5);

    let same = application::handle(
        &device,
        &registry,
        serde_json::from_value(frames[2].clone()).unwrap(),
    )
    .await;
    assert_eq!(same.response, frames[3]);
    assert!(
        same.changes.is_empty(),
        "unchanged state must not cause a second notification"
    );
}

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
