//! Local control-plane evidence through the real Unix simulator adapter. DUP
//! grouping does not establish packet duplication, ST 2022-7 or live acceptance.
#![cfg(feature = "device-lab")]

use rust_example::{
    application::handle,
    domain::*,
    lab::{client::Client, simulator},
    protocol::{self, Request},
    registry::Registry,
};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

struct Rig {
    _directory: tempfile::TempDir,
    socket: PathBuf,
    control: PathBuf,
    stop: CancellationToken,
    task: Option<JoinHandle<Result<(), simulator::Error>>>,
}
impl Rig {
    async fn start() -> Self {
        let scratch = if std::path::Path::new("/tmp/opencode").is_dir() {
            "/tmp/opencode"
        } else {
            "/tmp"
        };
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(scratch)
            .unwrap();
        let socket = directory.path().join("d.sock");
        let control = directory.path().join("c.sock");
        let stop = CancellationToken::new();
        let task = tokio::spawn({
            let (socket, control, stop) = (socket.clone(), control.clone(), stop.clone());
            async move { simulator::serve(&socket, &control, stop).await }
        });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !control.exists() {
                assert!(!task.is_finished());
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        Self {
            _directory: directory,
            socket,
            control,
            stop,
            task: Some(task),
        }
    }
    async fn client(&self) -> Client {
        Client::connect(&self.socket, Duration::from_secs(3))
            .await
            .unwrap()
    }
    async fn finish(mut self) {
        self.stop.cancel();
        self.task.take().unwrap().await.unwrap().unwrap();
    }
}
impl Drop for Rig {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn sender(codec: &str, main: &str, spare: &str) -> Value {
    json!({"id":"tx-1","transceiver_type":"sender","enabled_config":{
        "main_ip_destination":{"address":main,"udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "spare_ip_destination":{"address":spare,"udp_port":5006,"network_interface":{"module_id":"module-1","port_id":"net-spare"}},
        "format_info":{"video_codec":{"codec":codec,"entropy_encoding":"CABAC","gop_struct":"Automatic","gop_size":1},"attributes":{"rtp_payload_type":111}}
    }})
}
fn receiver(main: &str, spare: &str, sdp: &str) -> Value {
    let ipv4 = main.parse::<std::net::IpAddr>().unwrap().is_ipv4();
    json!({"id":"rx-1","transceiver_type":"receiver","enabled_config":{
        "main_ip_source":{"destination_address":main,"source_address":if ipv4 {"192.0.2.10"} else {"2001:db8::10"},"udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "spare_ip_source":{"destination_address":spare,"source_address":if ipv4 {"198.51.100.10"} else {"2001:db8:1::10"},"udp_port":5006,"network_interface":{"module_id":"module-1","port_id":"net-spare"}},
        "format_attributes":{"rtp_payload_type":111},"sdp":sdp
    }})
}
fn key(config: &Value, direction: Direction) -> PortKey {
    protocol::config_key("module-1", config, direction).unwrap()
}
fn decode_receiver(config: &Value) -> Result<Option<MediaState>, Failure> {
    protocol::media_state(&Registry::new(), &key(config, Direction::Receiver), config)
}
async fn frame(client: &Client, kind: &str, data: Value) -> Value {
    let wire = json!({"message_id":42,"message_type":kind,"data":data}).to_string();
    let outcome = handle(client, &Registry::new(), Request::parse(&wire).unwrap()).await;
    assert_eq!(outcome.response["message_id"], 42);
    outcome.response
}
async fn apply(client: &Client, config: &Value, sender: bool) -> Value {
    frame(
        client,
        "set_ipt_configurations",
        json!({"module_id":"module-1",
        if sender {"sender_configs"} else {"receiver_configs"}:[config]}),
    )
    .await
}
async fn check(client: &Client, config: &Value, repair: bool) -> Value {
    frame(
        client,
        "check_ipt_configuration",
        json!({"module_id":"module-1","config":config,"set_again":repair}),
    )
    .await
}

/// Change only section occurrence order; group IDs retain their identity and
/// main/spare meaning. This must compare equal on a later configuration check.
fn reversed_sections(sdp: &str) -> String {
    let chunks: Vec<_> = sdp.split("m=video ").collect();
    assert_eq!(chunks.len(), 3);
    format!("{}m=video {}m=video {}", chunks[0], chunks[2], chunks[1])
}
fn fixture() -> String {
    "v=0\r\no=- 1 1 IN IP4 192.0.2.10\r\ns=Redundant video\r\nt=0 0\r\na=group:DUP A B\r\n\
     m=video 5004 RTP/AVP 111\r\nc=IN IP4 239.1.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:A\r\na=source-filter:incl IN IP4 239.1.1.1 192.0.2.10\r\n\
     m=video 5006 RTP/AVP 111\r\nc=IN IP4 239.2.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:B\r\na=source-filter:incl IN IP4 239.2.1.1 198.51.100.10\r\n".into()
}

#[test]
fn group_ids_and_per_leg_source_filters_survive_section_reordering() {
    let original = fixture();
    let config = receiver("239.1.1.1", "239.2.1.1", &original);
    let expected = decode_receiver(&config).unwrap().unwrap();
    let reordered = reversed_sections(&original)
        .replace("source-filter:incl IN IP4", "source-filter: incl  IN   IP4");
    let actual = decode_receiver(&receiver("239.1.1.1", "239.2.1.1", &reordered))
        .unwrap()
        .unwrap();
    assert_eq!(expected, actual);
    let sdp = actual.sdp.as_ref().unwrap();
    assert!(sdp.find("a=mid:A").unwrap() < sdp.find("a=mid:B").unwrap());
    assert_eq!(
        decode_receiver(&receiver("239.1.1.1", "239.2.1.1", sdp)).unwrap(),
        Some(actual)
    );
}

#[test]
fn invalid_group_membership_scopes_and_conflicting_sections_are_rejected() {
    let sdp = fixture();
    for bad in [
        sdp.replace("a=group:DUP A B\r\n", ""),
        sdp.replace("DUP A B", "DUP A X"),
        sdp.replace("DUP A B", "DUP B A"),
        sdp.replace("DUP A B", "DUP A A"),
        sdp.replace("DUP A B", "DUP A B C"),
        sdp.replace("DUP A B", "FID A B"),
        sdp.replace("a=mid:A\r\n", ""),
        sdp.replace("a=mid:B", "a=mid:A"),
        sdp.replace("a=mid:B", "a=mid:"),
        sdp.replace("a=mid:B", "a=mid:B invalid"),
        sdp.replace("a=mid:B", "a=mid:雪"),
        sdp.replace("a=mid:B", "a=mid:B\r\na=mid:C"),
        sdp.replace("a=group:DUP A B", "a=group:DUP A B\r\na=group:DUP A B"),
        sdp.replace("t=0 0\r\n", "t=0 0\r\na=mid:session\r\n"),
        format!("{sdp}a=group:DUP A B\r\n"),
        format!("{sdp}m=video 6000 RTP/AVP 111\r\na=mid:C\r\n"),
        sdp.replace("a=mid:B", "a=mid:B\r\nc=IN IP4 239.9.9.9/64"),
        sdp.replace("a=mid:B", "a=mid:B\r\na=rtpmap:111 H265/90000"),
        sdp.replace(
            "a=mid:B",
            "a=mid:B\r\na=source-filter:incl IN IP4 239.2.1.1 192.0.2.99",
        ),
        sdp.replace("t=0 0", "t=0 0\r\na=rtpmap:111 H264/90000"),
    ] {
        assert!(decode_receiver(&receiver("239.1.1.1", "239.2.1.1", &bad)).is_err());
    }
    let state = decode_receiver(&receiver("239.1.1.1", "239.2.1.1", &sdp))
        .unwrap()
        .unwrap();
    assert!(protocol::validate_sdp(&sdp, &state.main, &state.format, Some("Video")).is_err());
}

#[test]
fn duplicate_semantics_and_transport_fields_are_checked_without_optional_format_hints() {
    let sdp = fixture();
    let offset = sdp.find("m=video 5006").unwrap();
    // Mutate only the spare: independently valid mappings can still contradict
    // duplication. Different PTs are rejected as local policy, not an RFC ban.
    for (from, to) in [
        ("H264/90000", "H265/90000"),
        ("H264/90000", "MP2T/90000"),
        ("H264/90000", "raw/90000"),
        ("H264/90000", "H264/48000"),
        ("H264/90000", "H264/90000/2"),
        ("111", "112"),
        ("5006", "6006"),
        ("239.2.1.1", "239.2.1.9"),
        ("198.51.100.10", "198.51.100.99"),
        ("IP4", "IP6"),
        ("RTP/AVP", "UDP"),
        ("source-filter:incl", "source-filter:excl"),
    ] {
        let bad = format!("{}{}", &sdp[..offset], sdp[offset..].replace(from, to));
        let mut config = receiver("239.1.1.1", "239.2.1.1", &bad);
        config["enabled_config"]["format_attributes"] = json!({});
        assert!(decode_receiver(&config).is_err(), "accepted {from} -> {to}");
    }
    for suffix in [
        "a=fmtp:111 sampling=YCbCr-4:2:2\r\n",
        "a=inactive\r\n",
        "a=crypto:unsupported\r\n",
        "a=ssrc-group:DUP 1 2\r\n",
    ] {
        assert!(
            decode_receiver(&receiver(
                "239.1.1.1",
                "239.2.1.1",
                &format!("{sdp}{suffix}")
            ))
            .is_err()
        );
    }
    let mut config = receiver("239.1.1.1", "239.2.1.1", &sdp);
    config["enabled_config"]["fec_enabled"] = json!(true);
    assert_eq!(decode_receiver(&config), Err(Failure::Unsupported));
    let unicast = sdp
        .replace("239.1.1.1/64", "192.0.2.30")
        .replace("239.1.1.1", "192.0.2.30")
        .replace("239.2.1.1/64", "198.51.100.30")
        .replace("239.2.1.1", "198.51.100.30");
    let mut config = receiver("192.0.2.30", "198.51.100.30", &unicast);
    decode_receiver(&config).unwrap();
    config["enabled_config"]["srt"] =
        json!({"mode":"caller","buffer_latency":120,"overhead_bandwidth":25});
    assert_eq!(decode_receiver(&config), Err(Failure::Unsupported));
}

#[test]
fn parsing_is_bounded_and_single_stream_api_remains_available() {
    let sdp = fixture();
    let mut config = receiver("239.1.1.1", "239.2.1.1", &sdp);
    for bad in [
        "x".repeat(65537),
        sdp.replace("s=Redundant video", &format!("s={}", "x".repeat(2049))),
        format!("{sdp}{}", "a=mid:overflow\n".repeat(257)),
        format!("{sdp}\0"),
    ] {
        config["enabled_config"]["sdp"] = json!(bad);
        assert!(decode_receiver(&config).is_err());
    }
    let single = sdp
        .split("m=video 5006")
        .next()
        .unwrap()
        .replace("a=group:DUP A B\r\n", "");
    config["enabled_config"]["sdp"] = json!(single);
    config["enabled_config"]["spare_ip_source"] = Value::Null;
    let state = decode_receiver(&config).unwrap().unwrap();
    assert_eq!(
        protocol::validate_sdp(&single, &state.main, &state.format, Some("Video")).unwrap(),
        state.sdp.unwrap()
    );
    for bad in [
        single.replace("a=mid:A", "a=mid:A\r\na=mid:B"),
        single.replace("a=mid:A", "a=mid:"),
    ] {
        assert!(protocol::validate_sdp(&bad, &state.main, &state.format, Some("Video")).is_err());
    }
}

#[tokio::test]
async fn h264_h265_dual_stream_roundtrip_checks_normalization_drift_and_failed_repair() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    for (codec, main, spare) in [
        ("H.264", "239.1.1.1", "239.2.1.1"),
        ("H.265", "192.0.2.30", "198.51.100.30"),
        ("H.265", "ff3e::1234", "ff3e::5678"),
        ("H.264", "2001:db8::30", "2001:db8:1::30"),
    ] {
        let tx = sender(codec, main, spare);
        assert_eq!(
            apply(&client, &tx, true).await["data"]["sender_config_errors"],
            json!([])
        );
        let sdp_response = frame(
            &client,
            "get_sender_sdp",
            json!({"module_id":"module-1","port_id":"tx-1"}),
        )
        .await;
        let sdp = sdp_response["data"].as_str().unwrap();
        assert!(sdp.contains("a=group:DUP main spare\r\n"));
        assert_eq!(sdp.matches("a=mid:").count(), 2);
        let rx = receiver(main, spare, sdp);
        assert_eq!(
            apply(&client, &rx, false).await["data"]["receiver_config_errors"],
            json!([])
        );
        let desired = decode_receiver(&rx).unwrap();
        let rx_key = key(&rx, Direction::Receiver);
        assert_eq!(client.configuration(rx_key.clone()).await.unwrap(), desired);
        let mut reordered = rx.clone();
        reordered["enabled_config"]["sdp"] = json!(reversed_sections(sdp).replace("\r\n", "\n"));
        assert_eq!(check(&client, &reordered, false).await["data"], true);

        let mut drift = desired.clone().unwrap();
        drift.spare.as_mut().unwrap().udp_port = 6006;
        simulator::control(
            &rig.control,
            "drift",
            &[json!(rx_key).to_string(), json!(drift).to_string()],
        )
        .await
        .unwrap();
        assert_eq!(check(&client, &rx, false).await["data"], false);
        assert_eq!(check(&client, &rx, true).await["data"], true);
        assert_eq!(client.configuration(rx_key.clone()).await.unwrap(), desired);

        // Invalid spare SDP must fail before any target write and preserve both
        // previously applied legs; it cannot become a false successful repair.
        let mut bad = rx.clone();
        bad["enabled_config"]["sdp"] = json!(sdp.replace("m=video 5006", "m=video 6006"));
        assert_eq!(
            apply(&client, &bad, false).await["data"]["receiver_config_errors"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(check(&client, &bad, true).await["message_type"], "error");
        assert_eq!(client.configuration(rx_key).await.unwrap(), desired);
    }
    let mut disabled = sender("H.264", "239.1.1.1", "239.2.1.1");
    disabled["enabled_config"] = Value::Null;
    assert_eq!(
        apply(&client, &disabled, true).await["data"]["sender_config_errors"],
        json!([])
    );
    assert_eq!(
        client
            .sender_sdp(key(&disabled, Direction::Sender))
            .await
            .unwrap(),
        None
    );
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn concurrent_pair_updates_and_sdp_reads_never_tear_main_from_spare() {
    let rig = Rig::start().await;
    let first = rig.client().await;
    let second = rig.client().await;
    let observer = rig.client().await;
    let tx = sender("H.264", "239.1.1.1", "239.2.1.1");
    let tx_key = key(&tx, Direction::Sender);
    let a = protocol::media_state(&Registry::new(), &tx_key, &tx)
        .unwrap()
        .unwrap();
    let mut b = a.clone();
    b.main.udp_port = 7004;
    b.spare.as_mut().unwrap().udp_port = 7006;
    first
        .configure(tx_key.clone(), Some(a.clone()))
        .await
        .unwrap();
    let sdp_a = first.sender_sdp(tx_key.clone()).await.unwrap();
    first
        .configure(tx_key.clone(), Some(b.clone()))
        .await
        .unwrap();
    let sdp_b = first.sender_sdp(tx_key.clone()).await.unwrap();
    // Independent clients can race, but one short target lock owns each complete
    // pair and generation. Readback may see either writer, never a hybrid pair.
    tokio::join!(
        async {
            for _ in 0..16 {
                first
                    .configure(tx_key.clone(), Some(a.clone()))
                    .await
                    .unwrap();
            }
        },
        async {
            for _ in 0..16 {
                second
                    .configure(tx_key.clone(), Some(b.clone()))
                    .await
                    .unwrap();
            }
        },
        async {
            for _ in 0..32 {
                let observed = observer
                    .configuration(tx_key.clone())
                    .await
                    .unwrap()
                    .unwrap();
                assert!(observed == a || observed == b);
                let sdp = observer.sender_sdp(tx_key.clone()).await.unwrap();
                assert!(sdp == sdp_a || sdp == sdp_b);
            }
        }
    );
    first.close().await;
    second.close().await;
    observer.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn sender_rejects_fec_srt_unknown_formats_and_identical_paths_instead_of_fabricating_sdp() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let tx = sender("H.264", "192.0.2.30", "198.51.100.30");
    let tx_key = key(&tx, Direction::Sender);
    let state = protocol::media_state(&Registry::new(), &tx_key, &tx)
        .unwrap()
        .unwrap();
    let mut fec = state.clone();
    fec.fec = Some(Fec {
        columns: 20,
        rows: Some(5),
        skew: false,
        overhead_percent: 20.0,
    });
    let mut fec_enabled = state.clone();
    fec_enabled.fec_enabled = true;
    let mut srt = state.clone();
    srt.srt = Some(Srt {
        caller: true,
        latency_ms: 120,
        overhead_percent: 25,
        encryption: None,
    });
    let mut raw = state.clone();
    raw.format.video_codec.as_mut().unwrap().0 = "raw".into();
    let mut same = state.clone();
    same.spare = Some(same.main.clone());
    let mut unspecified = state;
    unspecified.format.attributes.clear();
    for unsupported in [fec, fec_enabled, srt, raw, same, unspecified] {
        client
            .configure(tx_key.clone(), Some(unsupported))
            .await
            .unwrap();
        assert_eq!(
            client.sender_sdp(tx_key.clone()).await,
            Err(Failure::Unsupported)
        );
    }
    client.close().await;
    rig.finish().await;
}
