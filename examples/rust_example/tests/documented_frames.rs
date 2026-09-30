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
        device::Client,
        domain::{Device, Location},
        simulator,
    };
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    use tokio_util::sync::CancellationToken;

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
    let scratch = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let socket = scratch.path().join("target.sock");
    let control = scratch.path().join("control.sock");
    let stop = CancellationToken::new();
    let (path, token) = (socket.clone(), stop.clone());
    let server = tokio::spawn(async move { simulator::serve(&path, &control, token).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        let device = loop {
            if let Ok(device) = Client::connect(&socket, Duration::from_secs(1)).await {
                break device;
            }
            tokio::task::yield_now().await;
        };
        let registry = Registry::new();
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
        // Read the independently owned target state, not just the response map.
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
        device.close().await;
    })
    .await
    .unwrap();
    stop.cancel();
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
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
