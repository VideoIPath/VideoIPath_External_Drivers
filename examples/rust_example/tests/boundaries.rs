//! Simulator limits are deliberate local policies, not additional SPEC wire rules.
//! Exercise both accepted endpoints and their immediate neighbours, through the
//! same codec used by handlers; never confuse storing these values with media I/O.
use rust_example::{
    domain::{Direction, MediaState},
    protocol,
    registry::Registry,
};
use serde_json::{Value, json};

fn sender() -> Value {
    json!({"id":"tx-1","transceiver_type":"sender","enabled_config":{
        "main_ip_destination":{"address":"239.1.1.1","udp_port":5004,
            "network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "format_info":{}
    }})
}

fn state(value: &Value) -> Result<Option<MediaState>, rust_example::domain::Failure> {
    let key = protocol::config_key("module-1", value, Direction::Sender)?;
    protocol::media_state(&Registry::new(), &key, value)
}

fn accepted(value: &Value) -> MediaState {
    state(value).unwrap().unwrap()
}

#[test]
fn all_audio_pair_slots_handle_absent_null_and_bitrate_bounds() {
    let names = [
        "G1P1", "G1P2", "G2P1", "G2P2", "G3P1", "G3P2", "G4P1", "G4P2",
    ];
    for bitrate in [0, u32::MAX] {
        let mut input = sender();
        let mut pairs = serde_json::Map::new();
        for name in names {
            pairs.insert(name.into(), json!({"codec":"AAC_LC","bitrate":bitrate}));
        }
        input["enabled_config"]["format_info"]["audio_pairs"] = Value::Object(pairs);
        let observed = accepted(&input);
        assert_eq!(observed.format.audio_pairs.len(), 8);
        for name in names {
            assert_eq!(
                observed.format.audio_pairs[name],
                ("AAC_LC".into(), bitrate)
            );
        }
        // Null disables just that modeled pair; its siblings must survive intact.
        for name in names {
            input["enabled_config"]["format_info"]["audio_pairs"][name] = Value::Null;
            assert!(!accepted(&input).format.audio_pairs.contains_key(name));
        }
        assert!(accepted(&input).format.audio_pairs.is_empty());
    }
    for invalid in [
        json!(-1),
        json!(u64::from(u32::MAX) + 1),
        json!(true),
        json!(1.5),
        Value::Null,
    ] {
        let mut input = sender();
        input["enabled_config"]["format_info"]["audio_pairs"] =
            json!({"G4P2":{"codec":"AAC_LC","bitrate":invalid}});
        assert!(state(&input).is_err());
    }
}

#[test]
fn format_integer_boundaries_preserve_exact_values_and_optional_nulls() {
    for (field, minimum, maximum) in [
        ("frame_size", 0, u32::MAX),
        ("n_channels", 1, u32::MAX),
        ("sample_rate", 1, u32::MAX),
        ("link_offset", 0, u32::MAX),
    ] {
        for value in [minimum, maximum] {
            let mut input = sender();
            input["enabled_config"]["format_info"] = json!({"attributes":{"audio":{field:value}}});
            assert_eq!(
                accepted(&input).format.attributes[&format!("audio.{field}")],
                value.to_string()
            );
        }
        for value in [
            json!(-1),
            json!(u64::from(maximum) + 1),
            json!(true),
            json!(0.5),
        ] {
            let mut input = sender();
            input["enabled_config"]["format_info"] = json!({"attributes":{"audio":{field:value}}});
            assert!(state(&input).is_err());
        }
        let mut input = sender();
        input["enabled_config"]["format_info"] = json!({"attributes":{"audio":{field:null}}});
        assert!(accepted(&input).format.attributes.is_empty());
        if minimum == 1 {
            input["enabled_config"]["format_info"]["attributes"]["audio"][field] = json!(0);
            assert!(state(&input).is_err());
        }
    }
    for (field, max) in [("ptp_domain", 255), ("rtp_payload_type", 127)] {
        for value in [0, max] {
            let mut input = sender();
            input["enabled_config"]["format_info"] = json!({"attributes":{field:value}});
            assert_eq!(accepted(&input).format.attributes[field], value.to_string());
        }
        let mut input = sender();
        input["enabled_config"]["format_info"] = json!({"attributes":{field:max+1}});
        assert!(state(&input).is_err());
    }
    for size in [
        json!(0),
        json!(u32::MAX),
        json!(u64::from(u32::MAX) + 1),
        json!(-1),
        json!(false),
    ] {
        let mut input = sender();
        input["enabled_config"]["format_info"] = json!({"video_codec":{"codec":"H.264","entropy_encoding":"CABAC","gop_struct":"Automatic","gop_size":size}});
        let valid = size.as_u64().is_some_and(|n| n <= u64::from(u32::MAX));
        assert_eq!(state(&input).is_ok(), valid);
        if valid {
            assert_eq!(
                accepted(&input).format.video_codec.unwrap().3,
                u32::try_from(size.as_u64().unwrap()).unwrap()
            );
        }
    }
}

#[test]
fn streaming_and_fec_full_boundary_matrix() {
    let streaming = json!({"qos":"00000000","ts_packet_length":188,"ts_packets_per_frame":1,"ts_rate":0,"ip_rate":0,"ttl":1});
    for (field, values) in [
        ("qos", vec![json!("00000000"), json!("11111111")]),
        ("ts_packet_length", vec![json!(188), json!(204)]),
        ("ts_packets_per_frame", vec![json!(1), json!(u16::MAX)]),
        ("ts_rate", vec![json!(0), json!(u64::MAX)]),
        ("ip_rate", vec![json!(0), json!(u64::MAX)]),
        ("ttl", vec![json!(1), json!(255)]),
    ] {
        for value in values {
            let mut input = sender();
            input["enabled_config"]["streaming"] = streaming.clone();
            input["enabled_config"]["streaming"][field] = value;
            assert!(accepted(&input).streaming.is_some());
        }
        for value in [Value::Null, json!(true), json!(-1), json!(0.5)] {
            let mut input = sender();
            input["enabled_config"]["streaming"] = streaming.clone();
            input["enabled_config"]["streaming"][field] = value;
            assert!(state(&input).is_err());
        }
    }
    for (field, value) in [
        ("qos", json!("0000000")),
        ("qos", json!("0000000x")),
        ("ttl", json!(0)),
        ("ttl", json!(256)),
        ("ts_packets_per_frame", json!(0)),
        ("ts_packets_per_frame", json!(65536)),
        ("ts_packet_length", json!(189)),
    ] {
        let mut input = sender();
        input["enabled_config"]["streaming"] = streaming.clone();
        input["enabled_config"]["streaming"][field] = value;
        assert!(state(&input).is_err());
    }
    for length in [1, u32::MAX] {
        for depth in [None, Some(1), Some(u32::MAX)] {
            for overhead in [0.0, 100.0] {
                for skew in [false, true] {
                    let mut input = sender();
                    input["enabled_config"]["fec"] =
                        json!({"length":length,"depth":depth,"overhead":overhead,"skew":skew});
                    let fec = accepted(&input).fec.unwrap();
                    assert_eq!(
                        (fec.columns, fec.rows, fec.skew, fec.overhead_percent),
                        (length, depth, skew, overhead)
                    );
                }
            }
        }
    }
    for (field, value) in [
        ("length", json!(0)),
        ("length", json!(u64::from(u32::MAX) + 1)),
        ("depth", json!(0)),
        ("depth", json!(u64::from(u32::MAX) + 1)),
        ("overhead", json!(-0.01)),
        ("overhead", json!(100.01)),
        ("skew", json!(1)),
    ] {
        let mut input = sender();
        input["enabled_config"]["fec"] = json!({"length":1,"depth":null,"overhead":0,"skew":false});
        input["enabled_config"]["fec"][field] = value;
        assert!(state(&input).is_err());
    }
}

#[test]
fn srt_transport_encryption_and_numeric_boundaries_are_local_model_rules() {
    for direction in [Direction::Sender, Direction::Receiver] {
        for mode in ["caller", "listener"] {
            let mut input = sender();
            if direction == Direction::Receiver {
                input["id"] = json!("rx-1");
                input["transceiver_type"] = json!("receiver");
                input["enabled_config"] = json!({"main_ip_source":{"source_address":"192.0.2.20","destination_address":"192.0.2.10","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}}});
            } else {
                input["enabled_config"]["main_ip_destination"]["address"] = json!("192.0.2.20");
            }
            let key = protocol::config_key("module-1", &input, direction).unwrap();
            let registry = Registry::new();
            for (latency, overhead) in [(0, 0), (u32::MAX, 100)] {
                input["enabled_config"]["srt"] =
                    json!({"mode":mode,"buffer_latency":latency,"overhead_bandwidth":overhead});
                assert!(protocol::media_state(&registry, &key, &input).is_ok());
            }
            for size in [0, 1, 1024, 1025] {
                // Generated synthetic material only; never echo it in a failed assertion.
                input["enabled_config"]["srt"]["encryption"] =
                    json!({"mode":"AES256","key":"x".repeat(size)});
                assert_eq!(
                    protocol::media_state(&registry, &key, &input).is_ok(),
                    (1..=1024).contains(&size)
                );
            }
            input["enabled_config"]["srt"]["encryption"] = Value::Null;
            for (field, value) in [
                ("buffer_latency", json!(u64::from(u32::MAX) + 1)),
                ("overhead_bandwidth", json!(101)),
                ("overhead_bandwidth", json!(true)),
            ] {
                let mut bad = input.clone();
                bad["enabled_config"]["srt"][field] = value;
                assert!(protocol::media_state(&registry, &key, &bad).is_err());
            }
            let field = if direction == Direction::Sender {
                "main_ip_destination"
            } else {
                "main_ip_source"
            };
            let address = if direction == Direction::Sender {
                "address"
            } else {
                "destination_address"
            };
            input["enabled_config"][field][address] = json!("239.1.1.1");
            assert!(protocol::media_state(&registry, &key, &input).is_err());
        }
    }
}

#[test]
fn index_vlan_port_and_nested_default_boundaries() {
    for index in [
        json!(null),
        json!(1),
        json!(64),
        json!(0),
        json!(65),
        json!(true),
    ] {
        let mut input = sender();
        input["sub_index"] = index.clone();
        assert_eq!(
            state(&input).is_ok(),
            index.is_null() || matches!(index.as_u64(), Some(1 | 64))
        );
    }
    for (field, values) in [
        ("udp_port", vec![json!(1), json!(65535)]),
        ("vlan", vec![json!(null), json!(1), json!(4094)]),
    ] {
        for value in values {
            let mut input = sender();
            input["enabled_config"]["main_ip_destination"][field] = value;
            assert!(state(&input).is_ok());
        }
    }
    let baseline = accepted(&sender());
    for field in [
        "format_info",
        "fec",
        "streaming",
        "resource_partner",
        "srt",
        "spare_ip_destination",
    ] {
        let mut input = sender();
        input["enabled_config"][field] = Value::Null;
        assert_eq!(accepted(&input), baseline);
        // Empty objects are not universally equivalent to null/default. Only
        // documented optional-map defaults are accepted; required fields remain strict.
        input["enabled_config"][field] = json!({});
        assert_eq!(
            state(&input).is_ok(),
            matches!(field, "format_info" | "streaming")
        );
    }
    let mut disabled = sender();
    disabled["enabled_config"] = Value::Null;
    assert_eq!(state(&disabled).unwrap(), None);
    disabled.as_object_mut().unwrap().remove("enabled_config");
    assert!(state(&disabled).is_err());
}
