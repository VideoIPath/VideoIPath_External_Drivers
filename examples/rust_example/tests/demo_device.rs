//! Default-feature lessons exercise the real in-memory Device and SPEC handlers.
//! These checks prove model behavior, not physical media or backend acceptance.
use rust_example::{
    application::{self, Outcome},
    demo::DemoDevice,
    domain::{Device, Direction, Event, Location, PortKind},
    protocol::{self, Request},
    registry::Registry,
};
use serde_json::{Value, json};
use std::sync::Arc;

fn setup() -> (Arc<Registry>, DemoDevice) {
    let registry = Arc::new(Registry::new());
    registry.validate().unwrap();
    let device = DemoDevice::new(registry.clone());
    (registry, device)
}

async fn request(device: &DemoDevice, registry: &Registry, kind: &str, data: Value) -> Outcome {
    let frame = json!({"message_id":17,"message_type":kind,"data":data});
    let request = Request::parse(&frame.to_string()).unwrap();
    let outcome = application::handle(device, registry, request).await;
    assert_eq!(outcome.response["message_id"], 17);
    assert!(
        outcome.response["message_type"] == kind || outcome.response["message_type"] == "error"
    );
    outcome
}

fn sender(codec: &str, paired: bool) -> Value {
    json!({
        "id":"tx-1","sub_index":null,"transceiver_type":"sender",
        "enabled_config":{
            "main_ip_destination":{"address":"239.1.1.1","udp_port":5004,"vlan":null,
                "network_interface":{"module_id":"module-1","port_id":"net-main"}},
            "spare_ip_destination":if paired { json!({"address":"239.2.1.1","udp_port":5006,
                "network_interface":{"module_id":"module-1","port_id":"net-spare"}}) } else { Value::Null },
            "format_info":{"video_codec":{"codec":codec,"entropy_encoding":"CABAC",
                "gop_struct":"IP","gop_size":30},"attributes":{"rtp_payload_type":96}},
            "streaming":{"qos":"00100000","ts_packet_length":188,"ts_packets_per_frame":7,
                "ts_rate":0,"ip_rate":0,"ttl":32},
            "fec":null,"srt":null,"resource_partner":null
        }
    })
}

fn receiver(sdp: &str, paired: bool) -> Value {
    json!({
        "id":"rx-1","sub_index":null,"transceiver_type":"receiver",
        "enabled_config":{
            "main_ip_source":{"source_address":"192.0.2.10","destination_address":"239.1.1.1",
                "udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},
            "spare_ip_source":if paired { json!({"source_address":"198.51.100.10",
                "destination_address":"239.2.1.1","udp_port":5006,
                "network_interface":{"module_id":"module-1","port_id":"net-spare"}}) } else { Value::Null },
            "format_attributes":{"rtp_payload_type":96},"fec_enabled":false,"srt":null,"sdp":sdp,
            "source_meta":{"label":"Demo sender","pid":"demo-device"}
        }
    })
}

async fn apply(
    device: &DemoDevice,
    registry: &Registry,
    senders: Vec<Value>,
    receivers: Vec<Value>,
) -> Value {
    let outcome = request(
        device,
        registry,
        "set_ipt_configurations",
        json!({
            "module_id":"module-1","sender_configs":senders,"receiver_configs":receivers
        }),
    )
    .await;
    assert_eq!(outcome.response["message_type"], "set_ipt_configurations");
    assert!(outcome.changes.is_empty());
    outcome.response["data"].clone()
}

fn assert_applied(result: Value) {
    assert_eq!(
        result,
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
}

async fn check(device: &DemoDevice, registry: &Registry, config: &Value, set_again: bool) -> Value {
    let result = request(
        device,
        registry,
        "check_ipt_configuration",
        json!({
            "module_id":"module-1","config":config,"set_again":set_again
        }),
    )
    .await;
    assert_eq!(result.response["message_type"], "check_ipt_configuration");
    result.response["data"].clone()
}

#[tokio::test]
async fn writes_read_back_changes_only_and_preserve_partial_failures() {
    let (registry, device) = setup();
    let before = request(
        &device,
        &registry,
        "get_dynamic_values",
        json!({"location":null,"ids":["audio_gain","level","temperature","missing"]}),
    )
    .await;
    assert_eq!(
        before.response["data"],
        json!({"audio_gain":0.0,"level":50,"temperature":21.0})
    );

    let data = json!({"location":null,"values":{
        "audio_gain":2.5,"mode":"active","mute":true,"level":true,
        "temperature":22.0,"missing":1
    }});
    let result = request(&device, &registry, "set_dynamic_values", data.clone()).await;
    assert_eq!(
        result.response["data"],
        json!({
            "level":"Invalid value type","temperature":"Read-only parameter","missing":"Unknown parameter"
        })
    );
    assert_eq!(result.changes.len(), 1);
    assert_eq!(
        protocol::event_frame(&result.changes[0]),
        json!({
            "message_id":null,"message_type":"async_values_changed",
            "data":{"location":null,"values":{"audio_gain":2.5,"mode":"active","mute":true}}
        })
    );
    let observed = device.read_values(Location::root()).await.unwrap();
    assert_eq!(observed["audio_gain"], 2.5);
    assert_eq!(observed["level"], 50);
    assert_eq!(observed["temperature"], 21.0);
    assert!(!observed.contains_key("missing"));
    let repeated = request(&device, &registry, "set_dynamic_values", data).await;
    assert_eq!(repeated.response["data"], result.response["data"]);
    assert!(repeated.changes.is_empty());
    let read = request(
        &device,
        &registry,
        "get_dynamic_values",
        json!({"ids":["audio_gain","mode","mute"]}),
    )
    .await;
    assert_eq!(
        read.response["data"],
        json!({"audio_gain":2.5,"mode":"active","mute":true})
    );

    for invalid in [json!(2.25), json!(20.5), json!("2.5")] {
        let result = request(
            &device,
            &registry,
            "set_dynamic_values",
            json!({"values":{"audio_gain":invalid}}),
        )
        .await;
        assert!(result.response["data"]["audio_gain"].is_string());
        assert!(result.changes.is_empty());
        assert_eq!(
            device.read_values(Location::root()).await.unwrap()["audio_gain"],
            2.5
        );
    }
}

#[tokio::test]
async fn locations_isolate_identical_ids_and_unknown_locations_never_fall_back() {
    let (registry, device) = setup();
    let locations = [
        Location::root(),
        Location::module("module-1"),
        Location::port("module-1", PortKind::Sender, "tx-1"),
        Location::port("module-1", PortKind::Receiver, "rx-1"),
        Location::port("module-1", PortKind::Generic, "fan-1"),
        Location::port("module-1", PortKind::NetworkInterface, "net-main"),
    ];
    for (index, location) in locations.iter().enumerate() {
        let result = request(
            &device,
            &registry,
            "set_dynamic_values",
            json!({"location":location,"values":{"enabled":false}}),
        )
        .await;
        assert_eq!(result.response["data"], json!({}));
        assert_eq!(result.changes.len(), 1);
        assert_eq!(
            protocol::event_frame(&result.changes[0])["data"]["location"],
            json!(location)
        );
        for (other_index, other) in locations.iter().enumerate() {
            let read = request(
                &device,
                &registry,
                "get_dynamic_values",
                json!({"location":other,"ids":["enabled"]}),
            )
            .await;
            assert_eq!(
                read.response["data"],
                json!({"enabled":other_index > index})
            );
        }
    }
    let wrong = request(
        &device,
        &registry,
        "set_dynamic_values",
        json!({
            "location":{"module_id":"missing","port_location":null},"values":{"audio_gain":7.5}
        }),
    )
    .await;
    assert_eq!(wrong.response["message_type"], "error");
    assert!(wrong.changes.is_empty());
    assert_eq!(
        device.read_values(Location::root()).await.unwrap()["audio_gain"],
        0.0
    );
}

#[tokio::test]
async fn actions_count_each_invocation_without_readable_trigger_state() {
    let (registry, device) = setup();
    for count in 1..=2 {
        let result = request(
            &device,
            &registry,
            "set_dynamic_values",
            json!({"values":{"trigger":true}}),
        )
        .await;
        assert_eq!(result.response["data"], json!({}));
        assert!(result.changes.is_empty());
        assert_eq!(device.action_count().unwrap(), count);
    }
    assert!(
        !device
            .read_values(Location::root())
            .await
            .unwrap()
            .contains_key("trigger")
    );
    let all = request(&device, &registry, "get_dynamic_values", json!({})).await;
    assert!(all.response["data"].get("trigger").is_none());
    let explicit = request(
        &device,
        &registry,
        "get_dynamic_values",
        json!({"ids":["trigger"]}),
    )
    .await;
    assert_eq!(explicit.response["message_type"], "error");
    assert_eq!(explicit.response["data"], "Write-only parameter");
    let invalid = request(
        &device,
        &registry,
        "set_dynamic_values",
        json!({"values":{"trigger":1}}),
    )
    .await;
    assert_eq!(invalid.response["data"]["trigger"], "Invalid value type");
    assert_eq!(device.action_count().unwrap(), 2);
}

#[tokio::test]
async fn deterministic_ticks_raise_clear_and_refresh_full_alarm_snapshots() {
    let (registry, device) = setup();
    let initial = request(&device, &registry, "alarms", Value::Null).await;
    assert_eq!(initial.response["data"], json!([]));
    for present in [false, true, false, true] {
        let events = device.tick().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(
            protocol::event_frame(&events[0]),
            json!({
                "message_id":null,"message_type":"async_values_changed",
                "data":{"location":null,"values":{"signal_present":present}}
            })
        );
        let signal = request(
            &device,
            &registry,
            "get_dynamic_values",
            json!({"ids":["signal_present"]}),
        )
        .await;
        assert_eq!(signal.response["data"], json!({"signal_present":present}));
        let poll = request(&device, &registry, "alarms", Value::Null).await;
        let event = protocol::event_frame(&events[1]);
        assert!(event["message_id"].is_null());
        if present {
            assert_eq!(
                event,
                json!({"message_id":null,"message_type":"async_alarm_cleared",
                "data":{"location":null,"id":"Demo signal absent"}})
            );
            assert_eq!(poll.response["data"], json!([]));
        } else {
            assert_eq!(event["message_type"], "async_alarm");
            assert_eq!(
                event["data"],
                json!({"id":"Demo signal absent","location":null,
                "details":"Teaching clock input; not a physical fault","severity":"warning","service_affecting":false})
            );
            assert_eq!(poll.response["data"], json!([event["data"]]));
        }
        assert_eq!(
            request(&device, &registry, "alarms", Value::Null)
                .await
                .response,
            poll.response
        );
    }
    assert_eq!(
        protocol::event_frame(&Event::PollAlarms),
        json!({"message_id":null,"message_type":"async_poll_alarms","data":null})
    );
}

#[tokio::test]
async fn h264_h265_single_and_paired_sdp_configure_check_and_disable_both_directions() {
    let (registry, device) = setup();
    let sdp_query = json!({"module_id":"module-1","port_id":"tx-1"});
    assert!(
        request(&device, &registry, "get_sender_sdp", sdp_query.clone())
            .await
            .response["data"]
            .is_null()
    );
    for (codec, encoding) in [("H.264", "H264"), ("H.265", "H265")] {
        for paired in [false, true] {
            let tx = sender(codec, paired);
            assert_applied(apply(&device, &registry, vec![tx.clone()], vec![]).await);
            let result = request(&device, &registry, "get_sender_sdp", sdp_query.clone()).await;
            assert_eq!(result.response["message_type"], "get_sender_sdp");
            let sdp = result.response["data"].as_str().unwrap();
            assert!(sdp.contains(&format!("a=rtpmap:96 {encoding}/90000\r\n")));
            assert!(sdp.contains("m=video 5004 RTP/AVP 96\r\nc=IN IP4 239.1.1.1/32"));
            assert_eq!(sdp.contains("a=group:DUP main spare"), paired);
            assert_eq!(
                sdp.lines().filter(|line| line.starts_with("m=")).count(),
                if paired { 2 } else { 1 }
            );
            if paired {
                assert!(sdp.contains("m=video 5006 RTP/AVP 96\r\nc=IN IP4 239.2.1.1/32"));
            }
            let rx = receiver(sdp, paired);
            assert_applied(apply(&device, &registry, vec![], vec![rx.clone()]).await);
            for (config, direction) in [(&tx, Direction::Sender), (&rx, Direction::Receiver)] {
                let key = protocol::config_key("module-1", config, direction).unwrap();
                let expected = protocol::media_state(&registry, &key, config).unwrap();
                assert_eq!(device.configuration(key.clone()).await.unwrap(), expected);
                assert_eq!(check(&device, &registry, config, false).await, true);
                let mut disabled = config.clone();
                disabled["enabled_config"] = Value::Null;
                assert_eq!(check(&device, &registry, &disabled, false).await, false);
                let (senders, receivers) = if direction == Direction::Sender {
                    (vec![disabled.clone()], vec![])
                } else {
                    (vec![], vec![disabled.clone()])
                };
                assert_applied(apply(&device, &registry, senders, receivers).await);
                assert_eq!(device.configuration(key).await.unwrap(), None);
                assert_eq!(check(&device, &registry, &disabled, false).await, true);
            }
            assert!(
                request(&device, &registry, "get_sender_sdp", sdp_query.clone())
                    .await
                    .response["data"]
                    .is_null()
            );
        }
    }
    let unsupported = request(
        &device,
        &registry,
        "get_sender_sdp",
        json!({"module_id":"module-1","port_id":"tx-audio"}),
    )
    .await;
    assert_eq!(unsupported.response["message_type"], "error");
}

#[tokio::test]
async fn checks_detect_out_of_band_model_drift_and_reapply_requested_state() {
    let (registry, device) = setup();
    let tx = sender("H.264", true);
    assert_applied(apply(&device, &registry, vec![tx.clone()], vec![]).await);
    let sdp = device
        .sender_sdp(protocol::config_key("module-1", &tx, Direction::Sender).unwrap())
        .await
        .unwrap()
        .unwrap();
    let rx = receiver(&sdp, true);
    assert_applied(apply(&device, &registry, vec![], vec![rx.clone()]).await);
    for (config, direction) in [(tx, Direction::Sender), (rx, Direction::Receiver)] {
        let key = protocol::config_key("module-1", &config, direction).unwrap();
        let expected = protocol::media_state(&registry, &key, &config).unwrap();
        let mut drifted = expected.clone().unwrap();
        drifted.main.udp_port = 6000;
        // A local device change bypasses application::handle, so a cached request
        // masquerading as readback would make the following comparison fail.
        device
            .configure(key.clone(), Some(drifted.clone()))
            .await
            .unwrap();
        assert_eq!(check(&device, &registry, &config, false).await, false);
        assert_eq!(
            device.configuration(key.clone()).await.unwrap(),
            Some(drifted)
        );
        assert_eq!(check(&device, &registry, &config, true).await, true);
        assert_eq!(device.configuration(key.clone()).await.unwrap(), expected);
        device.configure(key.clone(), None).await.unwrap();
        assert_eq!(check(&device, &registry, &config, false).await, false);
        assert_eq!(check(&device, &registry, &config, true).await, true);
        assert_eq!(device.configuration(key).await.unwrap(), expected);
    }
}

#[tokio::test]
async fn partial_ipt_errors_preserve_valid_instances_and_registry_capacity_bounds() {
    let (registry, device) = setup();
    let mut valid = sender("H.264", false);
    valid["sub_index"] = json!(64); // Registry's maximum, not a SPEC-23 limit.
    let mut invalid = valid.clone();
    invalid["sub_index"] = json!(65);
    let errors = apply(&device, &registry, vec![valid.clone(), invalid], vec![]).await;
    assert_eq!(
        errors,
        json!({"sender_config_errors":[{"id":"tx-1","sub_index":65,"error":"Out of range"}],"receiver_config_errors":[]})
    );
    let key = protocol::config_key("module-1", &valid, Direction::Sender).unwrap();
    assert_eq!(
        device.configuration(key.clone()).await.unwrap(),
        protocol::media_state(&registry, &key, &valid).unwrap()
    );
    let mut ordinary = key;
    ordinary.sub_index = None;
    assert_eq!(device.configuration(ordinary).await.unwrap(), None);
}

#[tokio::test]
async fn resource_partner_choices_are_applied_and_unadvertised_links_rejected() {
    let (registry, device) = setup();
    let mut tx = sender("H.264", false);
    for partner in ["rx-1", "rx-alt"] {
        tx["enabled_config"]["resource_partner"] =
            json!({"module_id":"module-1","port_id":partner,"sub_index":null});
        assert_applied(apply(&device, &registry, vec![tx.clone()], vec![]).await);
        let key = protocol::config_key("module-1", &tx, Direction::Sender).unwrap();
        assert_eq!(
            device
                .configuration(key)
                .await
                .unwrap()
                .unwrap()
                .partner
                .unwrap()
                .port_id,
            partner
        );
        assert_eq!(check(&device, &registry, &tx, false).await, true);
    }
    tx["enabled_config"]["resource_partner"]["port_id"] = json!("rx-audio");
    let errors = apply(&device, &registry, vec![tx.clone()], vec![]).await;
    assert_eq!(
        errors["sender_config_errors"],
        json!([{"id":"tx-1","sub_index":null,"error":"Unsupported capability"}])
    );
    let key = protocol::config_key("module-1", &tx, Direction::Sender).unwrap();
    assert_eq!(
        device
            .configuration(key)
            .await
            .unwrap()
            .unwrap()
            .partner
            .unwrap()
            .port_id,
        "rx-alt"
    );
}
