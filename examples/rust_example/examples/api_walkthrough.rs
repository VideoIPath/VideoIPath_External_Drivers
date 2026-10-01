//! First run: cargo run --locked --example api_walkthrough
//!
//! Follow complete SPEC-23 frames through the same application and DemoDevice
//! used by the driver. No environment file, credentials, sockets or lab feature.
//! This demonstrates an in-memory model, not physical readback or media traffic.
use rust_example::{
    application,
    demo::DemoDevice,
    domain::{Device, Direction, Event},
    protocol::{self, Request},
    registry::Registry,
};
use serde_json::{Value, json};
use std::sync::Arc;

/// Requests stay visible at each call site. Only get_structure is summarized;
/// every other response and event is a complete protocol frame.
async fn exchange(device: &DemoDevice, registry: &Registry, label: &str, frame: Value) -> Value {
    println!("\n--- {label} ---\nREQUEST {frame}");
    let request = Request::parse(&frame.to_string()).expect("valid teaching frame");
    let outcome = application::handle(device, registry, request).await;
    assert_eq!(outcome.response["message_id"], frame["message_id"]);
    if frame["message_type"] == "get_structure" {
        assert_eq!(outcome.response["message_type"], "get_structure");
        let structure = &outcome.response["data"];
        let modules: Vec<Value> = structure["modules"].as_array().unwrap().iter().map(|module| {
            let ids = |list: &str| -> Vec<Value> {
                module[list].as_array().unwrap().iter().map(|port| port["id"].clone()).collect()
            };
            json!({"id":module["id"],"senders":ids("senders"),"receivers":ids("receivers"),
                "generic_ports":ids("generic_ports"),"network_interfaces":module["network_interfaces"]})
        }).collect();
        println!(
            "RESPONSE SUMMARY (not a wire frame; full structure intentionally omitted) {}",
            json!({
                "message_id":outcome.response["message_id"],"label":structure["label"],
                "configuration_order":structure["configuration_order"],
                "root_parameter_ids":structure["dynamic_values"].as_object().unwrap().keys().collect::<Vec<_>>(),
                "audio_gain_definition":structure["dynamic_values"]["audio_gain"],
                "modules":modules,"resource_links":structure["resource_links"]
            })
        );
    } else {
        println!("RESPONSE {}", outcome.response);
    }
    // The network session awaits response delivery before these causal events.
    for event in &outcome.changes {
        println!("EVENT {}", protocol::event_frame(event));
    }
    outcome.response["data"].clone()
}

fn assert_applied(data: Value) {
    assert_eq!(
        data,
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Arc::new(Registry::new());
    registry.validate()?;
    let device = DemoDevice::new(registry.clone());
    println!("External Driver API: offline in-memory walkthrough");
    println!(
        "REQUEST / RESPONSE / EVENT are complete JSON frames, except the marked structure summary."
    );

    // These two messages belong to the session, not application::handle. Show
    // the exact echo rule without pretending that a device operation handles it.
    let keepalive = r#"{"message_id":1,"message_type":"keepalive","data":null}"#;
    println!(
        "\n--- Keepalive: session-level exact echo ---\nREQUEST {keepalive}\nRESPONSE {keepalive}"
    );

    exchange(
        &device,
        &registry,
        "Discovery advertises definitions, not current values",
        json!({
            "message_id":2,"message_type":"get_structure","data":null
        }),
    )
    .await;
    exchange(&device, &registry, "Read numeric, boolean, choice and read-only sensor values", json!({
        "message_id":3,"message_type":"get_dynamic_values",
        "data":{"location":null,"ids":["audio_gain","level","enabled","mode","signal_present","temperature"]}
    })).await;

    // Numeric half-steps, booleans and enumerated strings use registry validation.
    // Failures are a direct ID map; valid siblings still apply and emit one delta.
    let values = json!({"audio_gain":2.5,"enabled":false,"mode":"active","level":101,
        "temperature":30,"unknown":1});
    exchange(&device, &registry, "Partial write: valid siblings change; invalid IDs fail", json!({
        "message_id":4,"message_type":"set_dynamic_values","data":{"location":null,"values":values}
    })).await;
    let readback = exchange(
        &device,
        &registry,
        "Readback comes from the DemoDevice model",
        json!({
            "message_id":5,"message_type":"get_dynamic_values",
            "data":{"location":null,"ids":["audio_gain","enabled","mode","level","temperature"]}
        }),
    )
    .await;
    assert_eq!(readback["audio_gain"], 2.5);
    assert_eq!(readback["level"], 50);
    exchange(
        &device,
        &registry,
        "Repeat the same values: no changed-value EVENT",
        json!({
            "message_id":6,"message_type":"set_dynamic_values",
            "data":{"location":null,"values":{"audio_gain":2.5,"enabled":false,"mode":"active"}}
        }),
    )
    .await;

    // A repeated local ID is a different parameter at each structured location.
    for (index, location) in [
        json!({"module_id":"module-1","port_location":null}),
        json!({"module_id":"module-1","port_location":{"type":"sender","id":"tx-1"}}),
        json!({"module_id":"module-1","port_location":{"type":"receiver","id":"rx-1"}}),
        json!({"module_id":"module-1","port_location":{"type":"generic","id":"fan-1"}}),
        json!({"module_id":"module-1","port_location":{"type":"network_interface","id":"net-main"}}),
    ].into_iter().enumerate() {
        exchange(&device, &registry, "Location-scoped read: enabled remains true here", json!({
            "message_id":7 + index,"message_type":"get_dynamic_values",
            "data":{"location":location,"ids":["enabled"]}
        })).await;
    }
    exchange(&device, &registry, "Scoped write: sender enabled changes independently", json!({
        "message_id":12,"message_type":"set_dynamic_values",
        "data":{"location":{"module_id":"module-1","port_location":{"type":"sender","id":"tx-1"}},
            "values":{"enabled":false}}
    })).await;
    for message_id in [13, 14] {
        exchange(&device, &registry, "Write-only action: each request executes, no value EVENT", json!({
            "message_id":message_id,"message_type":"set_dynamic_values","data":{"location":null,"values":{"trigger":true}}
        })).await;
    }
    println!(
        "MODEL OBSERVATION (not a protocol frame): action count = {}",
        device.action_count()?
    );
    exchange(&device, &registry, "An action has no fabricated readable state", json!({
        "message_id":15,"message_type":"get_dynamic_values","data":{"location":null,"ids":["trigger"]}
    })).await;

    exchange(
        &device,
        &registry,
        "Initial complete alarm snapshot",
        json!({
            "message_id":16,"message_type":"alarms","data":null
        }),
    )
    .await;
    // The driver calls tick every ten seconds. Here deterministic steps make the
    // complete raise/poll/clear/poll lifecycle visible without waiting or tasks.
    for message_id in [17, 18] {
        println!("\n--- Local teaching-clock tick: signal changes independently ---");
        for event in device.tick()? {
            println!("EVENT {}", protocol::event_frame(&event));
        }
        exchange(
            &device,
            &registry,
            "Authoritative full snapshot after tick",
            json!({
                "message_id":message_id,"message_type":"alarms","data":null
            }),
        )
        .await;
    }
    println!("\n--- Optional request for a near-term full alarm poll ---");
    println!("EVENT {}", protocol::event_frame(&Event::PollAlarms));

    exchange(&device, &registry, "Supported SDP is null while the sender is disabled", json!({
        "message_id":19,"message_type":"get_sender_sdp","data":{"module_id":"module-1","port_id":"tx-1"}
    })).await;
    // Ordinary ports use sub_index:null. Interface references must resolve to
    // advertised interfaces. This SDP subset requires explicit H.264/H.265 + PT.
    let mut sender = json!({"id":"tx-1","sub_index":null,"transceiver_type":"sender",
    "enabled_config":{
        "main_ip_destination":{"address":"239.1.1.1","udp_port":5004,"vlan":null,
            "network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "spare_ip_destination":null,
        "format_info":{"video_codec":{"codec":"H.264","entropy_encoding":"CABAC","gop_struct":"IP","gop_size":30},
            "attributes":{"rtp_payload_type":96}},
        "streaming":{"qos":"00100000","ts_packet_length":188,"ts_packets_per_frame":7,"ts_rate":0,"ip_rate":0,"ttl":64},
        "fec":null,"srt":null,"resource_partner":null
    }});
    assert_applied(
        exchange(
            &device,
            &registry,
            "Enable one H.264 sender stream",
            json!({
                "message_id":20,"message_type":"set_ipt_configurations",
                "data":{"module_id":"module-1","sender_configs":[sender],"receiver_configs":[]}
            }),
        )
        .await,
    );
    exchange(&device, &registry, "Read the configured single-stream sender SDP", json!({
        "message_id":21,"message_type":"get_sender_sdp","data":{"module_id":"module-1","port_id":"tx-1"}
    })).await;
    sender["enabled_config"]["format_info"]["video_codec"]["codec"] = json!("H.265");
    sender["enabled_config"]["spare_ip_destination"] = json!({"address":"239.2.1.1","udp_port":5006,"vlan":null,
        "network_interface":{"module_id":"module-1","port_id":"net-spare"}});
    assert_applied(
        exchange(
            &device,
            &registry,
            "Switch to paired H.265 streams",
            json!({
                "message_id":22,"message_type":"set_ipt_configurations",
                "data":{"module_id":"module-1","sender_configs":[sender],"receiver_configs":[]}
            }),
        )
        .await,
    );
    let sdp = exchange(&device, &registry, "Paired SDP describes both legs using DUP mids", json!({
        "message_id":23,"message_type":"get_sender_sdp","data":{"module_id":"module-1","port_id":"tx-1"}
    })).await;
    assert!(sdp.as_str().unwrap().contains("a=group:DUP main spare"));
    let mut receiver = json!({"id":"rx-1","sub_index":null,"transceiver_type":"receiver",
    "enabled_config":{
        "main_ip_source":{"source_address":"192.0.2.10","destination_address":"239.1.1.1","udp_port":5004,
            "network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "spare_ip_source":{"source_address":"198.51.100.10","destination_address":"239.2.1.1","udp_port":5006,
            "network_interface":{"module_id":"module-1","port_id":"net-spare"}},
        "format_attributes":{"rtp_payload_type":96},"fec_enabled":false,"srt":null,"sdp":sdp,
        "source_meta":{"label":"Demo sender","pid":"demo-device"}
    }});
    assert_applied(
        exchange(
            &device,
            &registry,
            "Configure a receiver with matching transport and sender SDP",
            json!({
                "message_id":24,"message_type":"set_ipt_configurations",
                "data":{"module_id":"module-1","sender_configs":[],"receiver_configs":[receiver]}
            }),
        )
        .await,
    );
    assert_eq!(
        exchange(
            &device,
            &registry,
            "Compare receiver configuration with model state",
            json!({
                "message_id":25,"message_type":"check_ipt_configuration",
                "data":{"module_id":"module-1","config":receiver,"set_again":false}
            })
        )
        .await,
        true
    );

    // Mutate the device model outside the request path. A real adapter would
    // observe a front-panel/vendor change here, rather than echoing its last set.
    let key = protocol::config_key("module-1", &sender, Direction::Sender)?;
    let mut local = device.configuration(key.clone()).await?.unwrap();
    local.main.udp_port = 6000;
    device.configure(key, Some(local)).await?;
    println!(
        "\nMODEL CHANGE (not a protocol frame): sender main UDP port changed locally to 6000."
    );
    for (message_id, set_again) in [(26, false), (27, true)] {
        assert_eq!(
            exchange(
                &device,
                &registry,
                "Detect drift; set_again permits reapply and readback",
                json!({
                    "message_id":message_id,"message_type":"check_ipt_configuration",
                    "data":{"module_id":"module-1","config":sender,"set_again":set_again}
                })
            )
            .await,
            set_again
        );
    }
    sender["enabled_config"] = Value::Null;
    receiver["enabled_config"] = Value::Null;
    assert_applied(exchange(&device, &registry, "Null enabled_config disables both ports", json!({
        "message_id":28,"message_type":"set_ipt_configurations",
        "data":{"module_id":"module-1","sender_configs":[sender],"receiver_configs":[receiver]}
    })).await);
    assert_eq!(
        exchange(
            &device,
            &registry,
            "Verify disabled sender state",
            json!({
                "message_id":29,"message_type":"check_ipt_configuration",
                "data":{"module_id":"module-1","config":sender,"set_again":false}
            })
        )
        .await,
        true
    );
    assert!(exchange(&device, &registry, "Disabled sender SDP returns to null", json!({
        "message_id":30,"message_type":"get_sender_sdp","data":{"module_id":"module-1","port_id":"tx-1"}
    })).await.is_null());
    exchange(
        &device,
        &registry,
        "Unsupported requests get a correlated error",
        json!({
            "message_id":31,"message_type":"unknown_request","data":null
        }),
    )
    .await;
    println!("\n--- Incoming error: session consumes it without replying (no error loop) ---");
    println!(
        "REQUEST {}",
        json!({"message_id":32,"message_type":"error","data":"Example peer diagnostic"})
    );
    println!("NO RESPONSE (session rule; not dispatched to the device)");
    println!(
        "\nDone. Next: registry.rs defines capabilities; application.rs maps requests; domain::Device is the vendor-operation seam."
    );
    Ok(())
}
