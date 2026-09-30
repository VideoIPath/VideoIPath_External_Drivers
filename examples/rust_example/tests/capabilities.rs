//! Complete-frame tests exercise the protocol -> application -> device boundary.
//! The fake deliberately has independent mutable state: drift tests would fail
//! if checking merely echoed the last requested configuration.
use rust_example::{
    application::{Outcome, handle},
    domain::*,
    protocol::{self, Request},
    registry::{Registry, validate_structure},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

struct Fake {
    values: Mutex<BTreeMap<Location, Values>>,
    media: Mutex<BTreeMap<PortKey, MediaState>>,
    apply_order: Mutex<Vec<PortKey>>,
    alarm_list: Mutex<Vec<Alarm>>,
    reads: AtomicUsize,
    writes: AtomicUsize,
    fail_read: AtomicBool,
    reject_config: AtomicBool,
    ignore_config: AtomicBool,
    uncertain_write: AtomicBool,
}
impl Fake {
    fn new(registry: &Registry) -> Self {
        Self {
            values: Mutex::new(
                registry
                    .locations()
                    .into_iter()
                    .map(|l| {
                        let v = registry.defaults(&l);
                        (l, v)
                    })
                    .collect(),
            ),
            media: Mutex::new(BTreeMap::new()),
            apply_order: Mutex::new(Vec::new()),
            alarm_list: Mutex::new(vec![]),
            reads: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            fail_read: AtomicBool::new(false),
            reject_config: AtomicBool::new(false),
            ignore_config: AtomicBool::new(false),
            uncertain_write: AtomicBool::new(false),
        }
    }
}
impl Device for Fake {
    async fn read_values(&self, location: Location) -> Result<Values, Failure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if self.fail_read.load(Ordering::SeqCst) {
            return Err(Failure::Unavailable);
        }
        Ok(self
            .values
            .lock()
            .unwrap()
            .get(&location)
            .cloned()
            .unwrap_or_default())
    }
    async fn set_value(
        &self,
        location: Location,
        id: String,
        value: Value,
    ) -> Result<Value, Failure> {
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.values
            .lock()
            .unwrap()
            .entry(location)
            .or_default()
            .insert(id, value.clone());
        if self.uncertain_write.load(Ordering::SeqCst) {
            Err(Failure::Ambiguous)
        } else {
            Ok(value)
        }
    }
    async fn alarms(&self) -> Result<Vec<Alarm>, Failure> {
        Ok(self.alarm_list.lock().unwrap().clone())
    }
    async fn sender_sdp(&self, key: PortKey) -> Result<Option<String>, Failure> {
        // A valid small MP2T fixture, not a universal SDP generator. The concrete
        // simulator's generation/rejection behavior has its own transport tests.
        Ok(self.media.lock().unwrap().get(&key).map(|s|format!("v=0\r\no=- 0 0 IN IP4 192.0.2.10\r\ns=Simulator\r\nc=IN IP4 {}\r\nt=0 0\r\nm=video {} RTP/AVP 33\r\na=rtpmap:33 MP2T/90000\r\n",s.main.destination,s.main.udp_port)))
    }
    async fn configure(&self, key: PortKey, state: Option<MediaState>) -> Result<(), Failure> {
        self.apply_order.lock().unwrap().push(key.clone());
        if self.reject_config.load(Ordering::SeqCst) {
            return Err(Failure::Unavailable);
        }
        if !self.ignore_config.load(Ordering::SeqCst) {
            let mut media = self.media.lock().unwrap();
            if let Some(state) = state {
                media.insert(key, state);
            } else {
                media.remove(&key);
            }
        }
        Ok(())
    }
    async fn configuration(&self, key: PortKey) -> Result<Option<MediaState>, Failure> {
        Ok(self.media.lock().unwrap().get(&key).cloned())
    }
}
async fn frame(device: &Fake, registry: &Registry, kind: &str, data: Value) -> Outcome {
    let wire =
        json!({"message_id":37,"message_type":kind,"data":data,"future_field":{"ignored":true}})
            .to_string();
    let outcome = handle(device, registry, Request::parse(&wire).unwrap()).await;
    assert_eq!(outcome.response["message_id"], 37);
    outcome
}
fn sender() -> Value {
    json!({"id":"tx-1","transceiver_type":"sender","enabled_config":{"main_ip_destination":{"udp_port":5004,"address":"239.1.1.1","network_interface":{"module_id":"module-1","port_id":"net-main"}},"format_info":{},"streaming":{}}})
}
fn receiver() -> Value {
    json!({"id":"rx-1","transceiver_type":"receiver","enabled_config":{"main_ip_source":{"udp_port":5004,"source_address":"192.0.2.20","destination_address":"239.1.1.1","network_interface":{"module_id":"module-1","port_id":"net-main"}},"format_attributes":{}}})
}
fn audio_receiver() -> Value {
    let mut config = receiver();
    config["id"] = json!("rx-audio");
    config
}
async fn set_sender(device: &Fake, registry: &Registry, config: Value) -> Outcome {
    frame(
        device,
        registry,
        "set_ipt_configurations",
        json!({"module_id":"module-1","sender_configs":[config]}),
    )
    .await
}

#[test]
fn decode_unknown_fields_and_usable_correlations() {
    let r =
        Request::parse(r#"{"message_id":1,"message_type":"get_structure","data":null,"new":17}"#)
            .unwrap();
    assert_eq!(r.message_id, 1);
    for wire in [
        r#"{"message_id":7,"message_type":false,"data":null}"#,
        r#"{"message_id":7,"message_type":"get_structure"}"#,
    ] {
        assert_eq!(Request::parse(wire).unwrap_err().message_id, Some(7));
    }
    for wire in [
        "{",
        r#"{"message_id":"7","message_type":"get_structure","data":null}"#,
        r#"{"message_id":null,"message_type":"get_structure","data":null}"#,
    ] {
        assert!(Request::parse(wire).unwrap_err().message_id.is_none());
    }
}

#[tokio::test]
async fn catalog_all_scopes_complete_maps_and_defaults() {
    let registry = Registry::new();
    let device = Fake::new(&registry);
    registry.validate().unwrap();
    let structure = frame(&device, &registry, "get_structure", Value::Null)
        .await
        .response["data"]
        .clone();
    assert_eq!(
        structure["dynamic_values"]["level"]["type_info"]["range"]["step"],
        1.0
    );
    assert_eq!(
        structure["dynamic_values"]["audio_gain"]["type_info"]["range"]["unit"],
        "dB"
    );
    assert_eq!(
        structure["dynamic_values"]["unbounded"]["type_info"]["range"],
        Value::Null
    );
    assert_eq!(
        structure["dynamic_values"]["trigger"]["value_info"],
        json!({"type":"write_only"})
    );
    assert_eq!(
        structure["dynamic_values"]["temperature"]["value_info"],
        json!({"type":"read_only","gpi_capable":false})
    );
    assert!(structure["dynamic_values"].get("sim.user.1").is_none());
    for location in registry.locations() {
        assert!(registry.known_location(&location));
        for (id, value) in registry.defaults(&location) {
            registry
                .parameter(&location, &id)
                .unwrap()
                .validate(&value)
                .unwrap();
        }
        let values = frame(
            &device,
            &registry,
            "get_dynamic_values",
            json!({"location":location}),
        )
        .await;
        assert!(values.response["data"].get("enabled").is_some());
        assert!(values.response["data"].get("trigger").is_none());
        let roundtrip: Location = serde_json::from_value(json!(location)).unwrap();
        assert_eq!(roundtrip, location);
    }
}

#[test]
fn structure_duplicates_missing_refs_and_orders() {
    let structure = Registry::new().structure();
    for order in ["senders_first", "receivers_first", "all"] {
        let mut s = structure.clone();
        s["configuration_order"] = json!(order);
        validate_structure(&s).unwrap();
    }
    let mut s = structure.clone();
    s["modules"][1]["id"] = json!("module-1");
    assert!(validate_structure(&s).is_err());
    let mut s = structure.clone();
    s["modules"][0]["senders"][1]["id"] = json!("tx-1");
    assert!(validate_structure(&s).is_err());
    let mut s = structure.clone();
    s["modules"][0]["senders"][0]["topology_info"]["interface_options"][0]["port_id"] =
        json!("missing");
    assert!(validate_structure(&s).is_err());
    let mut s = structure.clone();
    s["modules"][0]["network_interfaces"][0]["interface_2"]["id"] = json!("net-main");
    assert!(validate_structure(&s).is_err());
    let mut s = structure.clone();
    s["resource_links"][0]["from"]["port_id"] = json!("missing");
    assert!(validate_structure(&s).is_err());
    let mut s = structure.clone();
    s["modules"][0]["senders"][0]["topology_info"] = Value::Null;
    assert!(validate_structure(&s).is_err());
}

#[tokio::test]
async fn reads_selected_empty_unknown_hidden_and_snapshot_failure() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let result = frame(
        &d,
        &r,
        "get_dynamic_values",
        json!({"ids":["level","unknown","sim.user.1"]}),
    )
    .await;
    assert_eq!(result.response["data"], json!({"level":50,"sim.user.1":0}));
    assert_eq!(d.reads.load(Ordering::SeqCst), 1);
    assert_eq!(
        frame(&d, &r, "get_dynamic_values", json!({"ids":[]}))
            .await
            .response["data"],
        json!({})
    );
    assert_eq!(
        frame(&d, &r, "get_dynamic_values", json!({"ids":["trigger"]}))
            .await
            .response["message_type"],
        "error"
    );
    d.fail_read.store(true, Ordering::SeqCst);
    assert_eq!(
        frame(&d, &r, "get_dynamic_values", json!({"ids":["temperature"]}))
            .await
            .response["message_type"],
        "error"
    );
}

#[tokio::test]
async fn partial_set_types_access_steps_changed_only_and_action() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let outcome=frame(&d,&r,"set_dynamic_values",json!({"values":{"level":60,"audio_gain":0.25,"enabled":1,"temperature":22,"unknown":1,"mode":"active","trigger":true}})).await;
    assert_eq!(outcome.response["data"].as_object().unwrap().len(), 4);
    assert_eq!(outcome.response["data"]["unknown"], "Unknown parameter");
    assert_eq!(
        outcome.changes,
        vec![Event::Values {
            location: Location::root(),
            values: BTreeMap::from([
                ("level".into(), json!(60)),
                ("mode".into(), json!("active"))
            ])
        }]
    );
    assert_eq!(d.writes.load(Ordering::SeqCst), 3);
    let outcome = frame(&d, &r, "set_dynamic_values", json!({"values":{"level":60}})).await;
    assert_eq!(outcome.response["data"], json!({}));
    assert!(outcome.changes.is_empty());
    for invalid in [json!(true), json!("60"), json!(101), json!(2.5)] {
        let result = frame(
            &d,
            &r,
            "set_dynamic_values",
            json!({"values":{"level":invalid}}),
        )
        .await;
        assert!(result.response["data"]["level"].is_string());
    }
}

#[tokio::test]
async fn scope_identity_and_uncertain_write_readback() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let location = Location::port("module-1", PortKind::Sender, "tx-1");
    let changed = frame(
        &d,
        &r,
        "set_dynamic_values",
        json!({"location":location,"values":{"enabled":false}}),
    )
    .await;
    assert_eq!(
        protocol::event_frame(&changed.changes[0])["data"]["location"],
        json!(location)
    );
    assert_eq!(
        d.read_values(Location::root()).await.unwrap()["enabled"],
        true
    );
    d.uncertain_write.store(true, Ordering::SeqCst);
    let outcome = frame(&d, &r, "set_dynamic_values", json!({"values":{"level":71}})).await;
    assert_eq!(
        outcome.response["data"]["level"],
        Failure::Ambiguous.to_string()
    );
    assert_eq!(
        protocol::event_frame(&outcome.changes[0])["data"]["values"]["level"],
        71
    );
    assert_eq!(d.writes.load(Ordering::SeqCst), 2); // No blind retry.
}

#[tokio::test]
async fn alarms_all_severities_defaults_authoritative_clear_and_events() {
    let r = Registry::new();
    let d = Fake::new(&r);
    for severity in ["notification", "minor", "warning", "major", "critical"] {
        let alarm: Alarm = serde_json::from_value(
            json!({"id":severity,"location":null,"details":"Modeled fault","severity":severity}),
        )
        .unwrap();
        assert!(alarm.service_affecting);
        assert_eq!(
            protocol::event_frame(&Event::Alarm {
                alarm: alarm.clone()
            })["message_type"],
            "async_alarm"
        );
        d.alarm_list.lock().unwrap().push(alarm);
    }
    assert_eq!(
        frame(&d, &r, "alarms", Value::Null).await.response["data"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    d.alarm_list.lock().unwrap().clear();
    assert_eq!(
        frame(&d, &r, "alarms", Value::Null).await.response["data"],
        json!([])
    );
    assert_eq!(
        protocol::event_frame(&Event::AlarmCleared {
            location: Location::root(),
            id: "fault".into()
        }),
        json!({"message_id":null,"message_type":"async_alarm_cleared","data":{"id":"fault","location":null}})
    );
    assert_eq!(
        protocol::event_frame(&Event::PollAlarms),
        json!({"message_id":null,"message_type":"async_poll_alarms","data":null})
    );
}

#[tokio::test]
async fn ipt_enable_disable_compare_drift_repair_and_failures() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let config = sender();
    assert_eq!(
        set_sender(&d, &r, config.clone()).await.response["data"],
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    let check = |repair| json!({"module_id":"module-1","config":config,"set_again":repair});
    assert_eq!(
        frame(&d, &r, "check_ipt_configuration", check(false))
            .await
            .response["data"],
        true
    );
    d.media
        .lock()
        .unwrap()
        .values_mut()
        .next()
        .unwrap()
        .main
        .udp_port = 6000;
    assert_eq!(
        frame(&d, &r, "check_ipt_configuration", check(false))
            .await
            .response["data"],
        false
    );
    assert_eq!(
        frame(&d, &r, "check_ipt_configuration", check(true))
            .await
            .response["data"],
        true
    );
    d.media.lock().unwrap().clear();
    d.ignore_config.store(true, Ordering::SeqCst);
    assert_eq!(
        frame(&d, &r, "check_ipt_configuration", check(true))
            .await
            .response["message_type"],
        "error"
    );
    d.ignore_config.store(false, Ordering::SeqCst);
    d.reject_config.store(true, Ordering::SeqCst);
    assert_eq!(
        frame(&d, &r, "check_ipt_configuration", check(true))
            .await
            .response["message_type"],
        "error"
    );
    d.reject_config.store(false, Ordering::SeqCst);
    let mut disabled = config;
    disabled["enabled_config"] = Value::Null;
    assert!(
        set_sender(&d, &r, disabled).await.response["data"]["sender_config_errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn ipt_partial_errors_duplicate_pairs_and_multi_vertex() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let mut a = sender();
    a["sub_index"] = json!(1);
    let mut b = sender();
    b["sub_index"] = json!(2);
    let mut invalid = sender();
    invalid["id"] = json!("missing");
    let result=frame(&d,&r,"set_ipt_configurations",json!({"module_id":"module-1","sender_configs":[a,a,b,invalid],"receiver_configs":[receiver()]})).await;
    assert_eq!(
        result.response["data"]["sender_config_errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(d.media.lock().unwrap().len(), 3);
    let mut disabled = sender();
    disabled["sub_index"] = json!(1);
    disabled["enabled_config"] = Value::Null;
    set_sender(&d, &r, disabled).await;
    assert_eq!(d.media.lock().unwrap().len(), 2);
    let mut invalid = sender();
    invalid["sub_index"] = json!(0);
    assert_eq!(
        set_sender(&d, &r, invalid).await.response["data"]["sender_config_errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn sender_sdp_null_string_unsupported_and_unknown() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let data = json!({"module_id":"module-1","port_id":"tx-1"});
    assert!(frame(&d, &r, "get_sender_sdp", data.clone()).await.response["data"].is_null());
    set_sender(&d, &r, sender()).await;
    assert!(
        frame(&d, &r, "get_sender_sdp", data).await.response["data"]
            .as_str()
            .unwrap()
            .starts_with("v=0")
    );
    for port in ["tx-audio", "missing"] {
        assert_eq!(
            frame(
                &d,
                &r,
                "get_sender_sdp",
                json!({"module_id":"module-1","port_id":port})
            )
            .await
            .response["message_type"],
            "error"
        );
    }
}

#[test]
fn fec_srt_streaming_and_format_validation_normalization() {
    let r = Registry::new();
    let mut c = sender();
    let key = protocol::config_key("module-1", &c, Direction::Sender).unwrap();
    c["enabled_config"]["main_ip_destination"]["address"] = json!("192.0.2.30");
    c["enabled_config"]["fec"] = json!({"length":20,"depth":5,"skew":false,"overhead":20.0});
    c["enabled_config"]["srt"] = json!({"mode":"caller","buffer_latency":120,"overhead_bandwidth":25,"encryption":{"mode":"AES256","key":"SECRET-NEVER-LOG"}});
    c["enabled_config"]["streaming"] = json!({"qos":"00100000","ts_packet_length":188,"ts_packets_per_frame":7,"ts_rate":0,"ip_rate":0,"ttl":64});
    c["enabled_config"]["format_info"] = json!({"audio_pairs":{"G1P1":{"codec":"AAC_LC","bitrate":128000}},"video_codec":{"codec":"H.264","entropy_encoding":"CABAC","gop_struct":"IPB","gop_size":12},"attributes":{"audio":{"bit_depth":"L24","n_channels":2,"sample_rate":48000},"video":{"resolution":"1080p","sampling":"4:2:2"},"ptp_domain":127,"rtp_payload_type":96}});
    let state = protocol::media_state(&r, &key, &c).unwrap().unwrap();
    assert_eq!(state.streaming.as_ref().unwrap().dscp_byte, 32);
    assert_eq!(state.fec.as_ref().unwrap().rows, Some(5));
    assert!(!format!("{state:?}").contains("SECRET-NEVER-LOG"));
    let mut extra = c.clone();
    extra["enabled_config"]["future_ignored"] = json!("metadata");
    assert_eq!(
        protocol::media_state(&r, &key, &extra).unwrap(),
        Some(state)
    );
    for (field, value) in [("overhead", json!(101)), ("length", json!(0))] {
        let mut invalid = c.clone();
        invalid["enabled_config"]["fec"][field] = value;
        assert!(protocol::media_state(&r, &key, &invalid).is_err());
    }
    let mut invalid = c.clone();
    invalid["enabled_config"]["streaming"]["qos"] = json!("00102");
    assert!(protocol::media_state(&r, &key, &invalid).is_err());
    let mut invalid = c.clone();
    invalid["enabled_config"]["format_info"]["attributes"]["rtp_payload_type"] = json!(128);
    assert!(protocol::media_state(&r, &key, &invalid).is_err());
    let mut invalid = c.clone();
    invalid["enabled_config"]["srt"]["mode"] = json!("rendezvous");
    assert!(protocol::media_state(&r, &key, &invalid).is_err());
    c["enabled_config"]["fec"]["depth"] = Value::Null;
    assert!(
        protocol::media_state(&r, &key, &c)
            .unwrap()
            .unwrap()
            .fec
            .unwrap()
            .rows
            .is_none()
    );
}

#[test]
fn redundancy_partner_refs_and_receiver_sdp_cross_checks() {
    let r = Registry::new();
    let mut c = sender();
    let key = protocol::config_key("module-1", &c, Direction::Sender).unwrap();
    c["enabled_config"]["spare_ip_destination"] = json!({"address":"239.2.1.1","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-spare"}});
    c["enabled_config"]["resource_partner"] = json!({"module_id":"module-1","port_id":"rx-1"});
    let state = protocol::media_state(&r, &key, &c).unwrap().unwrap();
    assert!(state.spare.is_some());
    assert!(state.partner.is_some());
    c["enabled_config"]["resource_partner"]["port_id"] = json!("tx-1");
    assert!(protocol::media_state(&r, &key, &c).is_err());
    let mut c = audio_receiver();
    let key = protocol::config_key("module-1", &c, Direction::Receiver).unwrap();
    c["enabled_config"]["format_attributes"] = json!({"audio":{"bit_depth":"L24","n_channels":2,"sample_rate":48000},"rtp_payload_type":96});
    let sdp = "v=0\no=- 0 0 IN IP4 192.0.2.20\ns=Audio\nc=IN IP4 239.1.1.1\nt=0 0\nm=audio 5004 RTP/AVP 96\na=rtpmap:96 L24/48000/2\na=source-filter: incl IN IP4 239.1.1.1 192.0.2.20\n";
    c["enabled_config"]["sdp"] = json!(sdp);
    assert!(
        protocol::media_state(&r, &key, &c)
            .unwrap()
            .unwrap()
            .sdp
            .unwrap()
            .contains("\r\n")
    );
    for invalid in [
        sdp.replace("5004", "5006"),
        sdp.replace("L24/48000", "L24/44100"),
        sdp.replace("c=IN IP4 239.1.1.1", "c=IN IP4 239.1.1.9"),
        format!("{sdp}m=video 5006 RTP/AVP 97\n"),
    ] {
        c["enabled_config"]["sdp"] = json!(invalid);
        assert!(protocol::media_state(&r, &key, &c).is_err());
    }
}

#[tokio::test]
async fn unsupported_and_malformed_requests_are_sanitized_correlated_errors() {
    let r = Registry::new();
    let d = Fake::new(&r);
    for (kind, data) in [
        ("made_up", json!({"token":"SECRET"})),
        (
            "get_dynamic_values",
            json!({"location":{"module_id":"missing"}}),
        ),
        ("set_dynamic_values", json!({"values":false})),
        (
            "set_ipt_configurations",
            json!({"module_id":"module-1","sender_configs":[{}]}),
        ),
    ] {
        let result = frame(&d, &r, kind, data).await;
        assert_eq!(result.response["message_type"], "error");
        assert!(result.response["data"].is_string());
        assert!(!result.response.to_string().contains("SECRET"));
        assert!(result.changes.is_empty());
    }
}

#[test]
fn optional_metadata_is_validated_as_a_complete_map() {
    // None of these derived forms proves that a particular UI decoder accepts
    // them. The important local invariant is that optional absence is distinct
    // from an incomplete populated range, which must fail startup validation.
    let base = Registry::new().structure();
    for kind in [
        json!({"type":"number"}),
        json!({"type":"number","range":null}),
        json!({"type":"number","range":{"from":0,"to":1}}),
        json!({"type":"number","range":{"from":0,"to":1,"step":null,"unit":null}}),
    ] {
        let mut structure = base.clone();
        structure["dynamic_values"]["unbounded"]["type_info"] = kind;
        validate_structure(&structure).unwrap();
    }
    for kind in [
        json!({"type":"string"}),
        json!({"type":"string","values":[1]}),
        json!({"type":"number","range":{"from":0}}),
        json!({"type":"number","range":{"from":2,"to":1}}),
        json!({"type":"number","range":{"from":0,"to":1,"step":0}}),
    ] {
        let mut structure = base.clone();
        structure["dynamic_values"]["unbounded"]["type_info"] = kind;
        assert!(validate_structure(&structure).is_err());
    }
    let mut malformed = base.clone();
    malformed["modules"] = json!({});
    assert!(validate_structure(&malformed).is_err());
    let mut overrides = base;
    overrides["modules"][0]["senders"][0]["topology_info"]["main_interface_ip_overrides"] =
        json!({"invented":true});
    assert_eq!(validate_structure(&overrides), Err(Failure::Unsupported));
}

#[test]
fn documented_format_enum_choices_are_modeled_not_just_deserialized() {
    let r = Registry::new();
    let c = sender();
    let key = protocol::config_key("module-1", &c, Direction::Sender).unwrap();
    let cases: &[(&str, &[&str])] = &[
        ("colorimetry", &["BT601", "BT709", "BT2020", "BT2100"]),
        ("bit_depth", &["8", "10", "12", "16", "16f"]),
        (
            "exact_framerate",
            &[
                "23.97", "24", "25", "29.97", "30", "47.95", "48", "50", "59.94", "60",
            ],
        ),
        ("packed_mode", &["2110GPM", "2110BPM"]),
        (
            "resolution",
            &[
                "480i", "576i", "720p", "1080p", "1080i", "1080p 2k", "2160p", "2160p 4k",
            ],
        ),
        ("sampling", &["4:2:2", "4:2:0", "4:4:4"]),
        ("tcs", &["SDR", "PQ", "HLG", "SLOG3"]),
        ("tp", &["2110TPNL", "2110TPN", "2110TPW"]),
    ];
    for (field, choices) in cases {
        for choice in *choices {
            let mut c = sender();
            c["enabled_config"]["format_info"] = json!({"attributes":{"video":{*field:*choice}}});
            let modeled = protocol::media_state(&r, &key, &c).unwrap().unwrap();
            assert_eq!(
                modeled.format.attributes[&format!("video.{field}")],
                *choice
            );
        }
        let mut invalid = sender();
        invalid["enabled_config"]["format_info"] =
            json!({"attributes":{"video":{*field:"NOT_A_DOCUMENTED_CASE"}}});
        assert!(protocol::media_state(&r, &key, &invalid).is_err());
    }
    for codec in [
        "MPEG-1 Layer 2",
        "AAC_LC",
        "AAC_HE",
        "SMPTE 302M",
        "Dolby E",
        "Dolby Digital",
    ] {
        let mut c = sender();
        c["enabled_config"]["format_info"] =
            json!({"audio_pairs":{"G1P1":{"codec":codec,"bitrate":128000}}});
        assert_eq!(
            protocol::media_state(&r, &key, &c)
                .unwrap()
                .unwrap()
                .format
                .audio_pairs["G1P1"]
                .0,
            codec
        );
    }
    for codec in [
        "H.264",
        "H.265",
        "MPEG-2",
        "AVC-I 50",
        "AVC-I 100",
        "J2K",
        "TICO",
        "JPEG XS",
    ] {
        for entropy in ["CAVLC", "CABAC"] {
            for gop in ["Automatic", "IP", "IPB", "IPBB", "IPBBB", "IPBBBB"] {
                let mut c = sender();
                c["enabled_config"]["format_info"] = json!({"video_codec":{"codec":codec,"entropy_encoding":entropy,"gop_struct":gop,"gop_size":12}});
                assert_eq!(
                    protocol::media_state(&r, &key, &c)
                        .unwrap()
                        .unwrap()
                        .format
                        .video_codec,
                    Some((codec.into(), entropy.into(), gop.into(), 12))
                );
            }
        }
    }
    for depth in ["L16", "L20", "L24", "L32", "AM824"] {
        let mut c = sender();
        c["enabled_config"]["format_info"] = json!({"attributes":{"audio":{"bit_depth":depth}}});
        assert_eq!(
            protocol::media_state(&r, &key, &c)
                .unwrap()
                .unwrap()
                .format
                .attributes["audio.bit_depth"],
            depth
        );
    }
}

#[test]
fn srt_modes_encryption_and_network_negative_cases() {
    let r = Registry::new();
    let c = sender();
    let key = protocol::config_key("module-1", &c, Direction::Sender).unwrap();
    for mode in ["caller", "listener"] {
        for algorithm in [None, Some("AES128"), Some("AES192"), Some("AES256")] {
            let mut c = sender();
            c["enabled_config"]["main_ip_destination"]["address"] = json!("192.0.2.33");
            c["enabled_config"]["srt"] = json!({"mode":mode,"buffer_latency":120,"overhead_bandwidth":25,"encryption":algorithm.map(|a|json!({"mode":a,"key":"SECRET"}))});
            let modeled = protocol::media_state(&r, &key, &c)
                .unwrap()
                .unwrap()
                .srt
                .unwrap();
            assert_eq!(modeled.caller, mode == "caller");
            assert_eq!(
                modeled.encryption.as_ref().map(|e| e.algorithm.as_str()),
                algorithm
            );
            assert!(!format!("{modeled:?}").contains("SECRET"));
        }
    }
    for (field, value) in [
        ("udp_port", json!(0)),
        ("udp_port", json!(65536)),
        ("vlan", json!(4095)),
        ("address", json!("not an IP")),
        ("address", json!("0.0.0.0")),
    ] {
        let mut c = sender();
        c["enabled_config"]["main_ip_destination"][field] = value;
        assert!(protocol::media_state(&r, &key, &c).is_err());
    }
    let mut c = sender();
    c["enabled_config"]["main_ip_destination"]["network_interface"]["port_id"] = json!("missing");
    assert!(protocol::media_state(&r, &key, &c).is_err());
}

#[tokio::test]
async fn empty_arrays_defaults_and_batch_boundaries_do_not_apply_invalid_requests() {
    let r = Registry::new();
    let d = Fake::new(&r);
    let empty = frame(
        &d,
        &r,
        "set_ipt_configurations",
        json!({"module_id":"module-1"}),
    )
    .await;
    assert_eq!(
        empty.response["data"],
        json!({"sender_config_errors":[],"receiver_config_errors":[]})
    );
    let configs = vec![sender(); protocol::MAX_ITEMS + 1];
    let overloaded = frame(
        &d,
        &r,
        "set_ipt_configurations",
        json!({"module_id":"module-1","sender_configs":configs}),
    )
    .await;
    assert_eq!(overloaded.response["message_type"], "error");
    assert!(d.media.lock().unwrap().is_empty());
    let malformed = frame(
        &d,
        &r,
        "set_ipt_configurations",
        json!({"module_id":"module-1","sender_configs":[sender(),{"id":3}]}),
    )
    .await;
    assert_eq!(malformed.response["message_type"], "error");
    assert!(d.media.lock().unwrap().is_empty());
}

#[tokio::test]
async fn full_width_json_integer_ids_keep_success_and_error_correlation() {
    let registry = Registry::new();
    let device = Fake::new(&registry);
    for id in [
        json!(i64::MIN),
        json!(i64::MAX),
        json!((i64::MAX as u64) + 1),
        json!(u64::MAX),
    ] {
        for kind in ["get_structure", "not_supported"] {
            let wire = json!({"message_id":id,"message_type":kind,"data":null}).to_string();
            let request = Request::parse(&wire).unwrap();
            let outcome = handle(&device, &registry, request).await;
            // Compare serialized JSON numbers, not a narrowing as_i64 projection.
            assert_eq!(outcome.response["message_id"], id);
            assert_eq!(
                outcome.response["message_type"],
                if kind == "get_structure" {
                    kind
                } else {
                    "error"
                }
            );
        }
    }
    for wire in [
        format!(
            r#"{{"message_id":{},"message_type":false,"data":null}}"#,
            u64::MAX
        ),
        format!(
            r#"{{"message_id":{},"message_type":"get_structure"}}"#,
            u64::MAX
        ),
    ] {
        let error = Request::parse(&wire).unwrap_err();
        assert_eq!(error.message_id, Some(i128::from(u64::MAX)));
        assert_eq!(
            protocol::error_frame(error.message_id.unwrap(), error.failure)["message_id"],
            json!(u64::MAX)
        );
    }
}

#[tokio::test]
async fn configuration_orders_are_observed_at_the_device_boundary() {
    for (order, expected) in [
        (
            "senders_first",
            vec![Direction::Sender, Direction::Sender, Direction::Receiver],
        ),
        (
            "receivers_first",
            vec![Direction::Receiver, Direction::Sender, Direction::Sender],
        ),
        // `all` carries both groups in one SPEC request. Individual adapter calls
        // remain sequential; this test does not assert an atomic hardware batch.
        (
            "all",
            vec![Direction::Sender, Direction::Sender, Direction::Receiver],
        ),
    ] {
        let registry = Registry::new().with_configuration_order(order).unwrap();
        let device = Fake::new(&registry);
        assert_eq!(registry.structure()["configuration_order"], order);
        let mut second_sender = sender();
        second_sender["sub_index"] = json!(2);
        let result=frame(&device,&registry,"set_ipt_configurations",json!({"module_id":"module-1","sender_configs":[sender(),second_sender],"receiver_configs":[receiver()]})).await;
        assert_eq!(
            result.response["data"],
            json!({"sender_config_errors":[],"receiver_config_errors":[]})
        );
        let observed = device.apply_order.lock().unwrap();
        assert_eq!(
            observed.iter().map(|key| key.direction).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            observed
                .iter()
                .filter(|key| key.direction == Direction::Sender)
                .map(|key| key.sub_index)
                .collect::<Vec<_>>(),
            vec![None, Some(2)]
        );
        assert_eq!(device.media.lock().unwrap().len(), 3);
    }
    assert!(
        Registry::new()
            .with_configuration_order("invented")
            .is_err()
    );
}

#[tokio::test]
async fn gpio_access_variants_and_every_scope_route_to_the_canonical_identity() {
    let registry = Registry::new();
    let device = Fake::new(&registry);
    let definitions = registry.definitions(&Location::root());
    for (id, expected) in [
        (
            "signal_present",
            json!({"type":"read_only","gpi_capable":true}),
        ),
        (
            "mute",
            json!({"type":"read_write","gpi_capable":false,"gpo_capable":true}),
        ),
        (
            "enabled",
            json!({"type":"read_write","gpi_capable":true,"gpo_capable":true}),
        ),
        ("trigger", json!({"type":"write_only"})),
    ] {
        assert_eq!(definitions[id]["value_info"], expected);
    }
    let changed = frame(
        &device,
        &registry,
        "set_dynamic_values",
        json!({"values":{"signal_present":false,"mute":true}}),
    )
    .await;
    assert_eq!(
        changed.response["data"],
        json!({"signal_present":"Read-only parameter"})
    );
    assert_eq!(
        changed.changes,
        vec![Event::Values {
            location: Location::root(),
            values: BTreeMap::from([("mute".into(), json!(true))])
        }]
    );
    assert_eq!(
        device.read_values(Location::root()).await.unwrap()["signal_present"],
        true
    );
    // Repeat the same local ID at root, module, both receiver types, sender,
    // generic and network-interface scopes. A flat ID cache would fail here.
    for location in registry.locations() {
        let before = device.values.lock().unwrap().clone();
        let changed = frame(
            &device,
            &registry,
            "set_dynamic_values",
            json!({"location":location,"values":{"enabled":false}}),
        )
        .await;
        assert_eq!(changed.response["data"], json!({}));
        assert_eq!(
            changed.changes,
            vec![Event::Values {
                location: location.clone(),
                values: BTreeMap::from([("enabled".into(), json!(false))])
            }]
        );
        for (other, values) in before {
            if other != location {
                assert_eq!(device.read_values(other).await.unwrap(), values);
            }
        }
        let observed = frame(
            &device,
            &registry,
            "get_dynamic_values",
            json!({"location":location,"ids":["enabled"]}),
        )
        .await;
        assert_eq!(observed.response["data"], json!({"enabled":false}));
        let unchanged = frame(
            &device,
            &registry,
            "set_dynamic_values",
            json!({"location":location,"values":{"enabled":false}}),
        )
        .await;
        assert!(unchanged.changes.is_empty());
    }
}

#[test]
fn advertised_media_and_non_null_format_resource_references_stay_consistent() {
    let registry = Registry::new();
    let structure = registry.structure();
    assert_eq!(structure["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(structure["modules"][1]["label"], "Auxiliary Console 42");
    for (id, direction, format) in [
        ("rx-1", Direction::Receiver, "Video"),
        ("rx-audio", Direction::Receiver, "Audio"),
    ] {
        let key = PortKey {
            module_id: "module-1".into(),
            port_id: id.into(),
            sub_index: None,
            direction,
        };
        let port = registry.port(&key).unwrap();
        assert_eq!(port["topology_info"]["format_type"], format);
        assert_eq!(port["topology_info"]["sdp_support"], true);
    }
    let link = &structure["resource_links"][0];
    for (side, direction) in [("from", Direction::Receiver), ("to", Direction::Sender)] {
        let key = PortKey {
            module_id: link[side]["module_id"].as_str().unwrap().into(),
            port_id: link[side]["port_id"].as_str().unwrap().into(),
            sub_index: None,
            direction,
        };
        assert_eq!(
            registry.port(&key).unwrap()["topology_info"]["format_id"],
            "raw-video"
        );
    }
    registry.validate().unwrap();
    let mut bad = structure;
    bad["modules"][0]["senders"][0]["topology_info"]["format_id"] = json!({"invalid":true});
    assert!(validate_structure(&bad).is_err());
}

fn audio_sdp(mapping: &str) -> String {
    format!(
        "v=0\no=- 0 0 IN IP4 192.0.2.20\ns=Audio\nc=IN IP4 239.1.1.1\nt=0 0\nm=audio 5004 RTP/AVP 96\na=rtpmap:96 {mapping}\n"
    )
}
fn video_sdp() -> String {
    "v=0\no=- 0 0 IN IP4 192.0.2.20\ns=Video\nc=IN IP4 239.1.1.1\nt=0 0\nm=video 5004 RTP/AVP 33\na=rtpmap:33 MP2T/90000\n".into()
}

#[tokio::test]
async fn receiver_sdp_validation_does_not_depend_on_optional_attributes() {
    let registry = Registry::new();
    let device = Fake::new(&registry);
    for attrs in [
        None,
        Some(json!({})),
        Some(
            json!({"audio":{"bit_depth":"L24","sample_rate":48000,"n_channels":2},"rtp_payload_type":96}),
        ),
    ] {
        let mut config = audio_receiver();
        if let Some(attrs) = attrs {
            config["enabled_config"]["format_attributes"] = attrs;
        } else {
            config["enabled_config"]
                .as_object_mut()
                .unwrap()
                .remove("format_attributes");
        }
        config["enabled_config"]["sdp"] = json!(audio_sdp("L24/48000/2"));
        let result = frame(
            &device,
            &registry,
            "set_ipt_configurations",
            json!({"module_id":"module-1","receiver_configs":[config]}),
        )
        .await;
        assert_eq!(result.response["data"]["receiver_config_errors"], json!([]));
        for mapping in [
            "NOT_A_CODEC/banana/0",
            "NOT_A_CODEC/48000/2",
            "L24/banana/2",
            "L24/0/2",
            "L24/48000/0",
            "L24/48000/no",
            "L24/48000/65",
            "L24/48000/2/extra",
            "L24/4294967296/2",
            "L24/+48000/2",
            "L24/12345/2",
        ] {
            let mut invalid = config.clone();
            invalid["enabled_config"]["sdp"] = json!(audio_sdp(mapping));
            let before = device.apply_order.lock().unwrap().len();
            let result = frame(
                &device,
                &registry,
                "set_ipt_configurations",
                json!({"module_id":"module-1","receiver_configs":[invalid]}),
            )
            .await;
            assert_eq!(
                result.response["data"]["receiver_config_errors"]
                    .as_array()
                    .unwrap()
                    .len(),
                1,
                "accepted {mapping}"
            );
            assert_eq!(
                device.apply_order.lock().unwrap().len(),
                before,
                "invalid SDP reached device"
            );
        }
    }
    for (mut config, sdp) in [
        (receiver(), audio_sdp("L24/48000/2")),
        (audio_receiver(), video_sdp()),
    ] {
        config["enabled_config"]["sdp"] = json!(sdp);
        let key = protocol::config_key("module-1", &config, Direction::Receiver).unwrap();
        assert_eq!(
            protocol::media_state(&registry, &key, &config),
            Err(Failure::Mismatch)
        );
    }
}

#[test]
fn sdp_supported_forms_and_explicitly_unsupported_semantics() {
    let registry = Registry::new();
    for mapping in ["L16/44100", "L16/48000/2", "L24/96000/8"] {
        let mut config = audio_receiver();
        config["enabled_config"]["sdp"] = json!(audio_sdp(mapping));
        let key = protocol::config_key("module-1", &config, Direction::Receiver).unwrap();
        assert!(
            protocol::media_state(&registry, &key, &config)
                .unwrap()
                .unwrap()
                .sdp
                .is_some()
        );
    }
    let mut config = receiver();
    let key = protocol::config_key("module-1", &config, Direction::Receiver).unwrap();
    for sdp in [
        video_sdp(),
        video_sdp().replace("a=rtpmap:33 MP2T/90000\n", ""),
    ] {
        config["enabled_config"]["sdp"] = json!(sdp);
        protocol::media_state(&registry, &key, &config).unwrap();
    }
    for sdp in [
        video_sdp().replace("MP2T/90000", "NOT_A_CODEC/banana/0"),
        video_sdp().replace("MP2T/90000", "MP2T/48000"),
        video_sdp().replace("MP2T/90000", "MP2T/90000/2"),
        video_sdp().replace("MP2T/90000", "raw/90000"),
        format!("{}a=fmtp:33 sampling=YCbCr-4:2:2\n", video_sdp()),
        format!("{}a=crypto:secret\n", video_sdp()),
        format!("{}a=inactive\n", video_sdp()),
        format!("{}a=future-transport:unknown\n", video_sdp()),
        video_sdp().replace("239.1.1.1", "239.1.1.1/banana"),
        format!("{}a=group:DUP A B\n", video_sdp()),
    ] {
        config["enabled_config"]["sdp"] = json!(sdp);
        assert!(protocol::media_state(&registry, &key, &config).is_err());
    }
    config["enabled_config"]["sdp"] = json!(video_sdp());
    for attrs in [
        json!({"video":{"sampling":"4:2:2"}}),
        json!({"audio":{"bit_depth":"L24"}}),
    ] {
        config["enabled_config"]["format_attributes"] = attrs;
        assert!(protocol::media_state(&registry, &key, &config).is_err());
    }
    let mut config = audio_receiver();
    let key = protocol::config_key("module-1", &config, Direction::Receiver).unwrap();
    config["enabled_config"]["sdp"] = json!(audio_sdp("L24/48000/2"));
    for attrs in [
        json!({"audio":{"bit_depth":"L16"}}),
        json!({"audio":{"sample_rate":44100}}),
        json!({"audio":{"n_channels":1}}),
        json!({"rtp_payload_type":97}),
        json!({"video":{"sampling":"4:2:2"}}),
    ] {
        config["enabled_config"]["format_attributes"] = attrs;
        assert!(protocol::media_state(&registry, &key, &config).is_err());
    }
    config["enabled_config"]["format_attributes"] = json!({});
    config["enabled_config"]["spare_ip_source"] =
        config["enabled_config"]["main_ip_source"].clone();
    assert_eq!(
        protocol::media_state(&registry, &key, &config),
        Err(Failure::Unsupported)
    );
}

#[test]
fn unknown_json_object_fields_are_ignored_without_weakening_known_field_validation() {
    let registry = Registry::new();
    let config = sender();
    let key = protocol::config_key("module-1", &config, Direction::Sender).unwrap();
    let expected = protocol::media_state(&registry, &key, &config).unwrap();
    let mut extended = config;
    extended["future"] = json!({"anything":[1,2,3]});
    extended["enabled_config"]["future"] = json!(false);
    extended["enabled_config"]["main_ip_destination"]["future"] = json!("metadata");
    extended["enabled_config"]["main_ip_destination"]["network_interface"]["future"] = json!(null);
    extended["enabled_config"]["format_info"]["future"] = json!(17);
    extended["enabled_config"]["format_info"]["attributes"] = json!({"future":true});
    extended["enabled_config"]["streaming"]["future"] = json!(true);
    assert_eq!(
        protocol::media_state(&registry, &key, &extended).unwrap(),
        expected
    );
    extended["enabled_config"]["main_ip_destination"]["udp_port"] = json!("5004");
    assert!(protocol::media_state(&registry, &key, &extended).is_err());
    let data:protocol::GetValues=protocol::decode(&json!({"location":{"module_id":"module-1","port_location":{"type":"receiver","id":"rx-audio","future":true},"future":{}},"ids":["enabled"],"future":[] })).unwrap();
    assert_eq!(
        data.location,
        Location::port("module-1", PortKind::Receiver, "rx-audio")
    );
    assert!(protocol::decode::<protocol::GetValues>(&json!({"location":{"module_id":"module-1","port_location":{"type":"invented","id":"rx-audio"}}})).is_err());
}
