//! Real Unix streams exercise ownership and cancellation. These are local target
//! transport tests, not evidence of VideoIPath or physical-media compatibility.
use rust_example::{device::Client, domain::*, simulator};
use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::oneshot,
    task::JoinHandle,
};
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
        let directory = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .unwrap();
        let socket = directory.path().join("target.sock");
        let control = directory.path().join("control.sock");
        let stop = CancellationToken::new();
        let task = tokio::spawn({
            let socket = socket.clone();
            let control = control.clone();
            let stop = stop.clone();
            async move { simulator::serve(&socket, &control, stop).await }
        });
        // Yield until bind completes; a bounded readiness condition replaces sleeps.
        tokio::time::timeout(Duration::from_secs(2), async {
            while !control.exists() {
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
        Client::connect(&self.socket, Duration::from_secs(2))
            .await
            .unwrap()
    }
    async fn ctl(&self, command: &str, args: Vec<String>) -> Value {
        simulator::control(&self.control, command, &args)
            .await
            .unwrap()
    }
    async fn finish(mut self) {
        self.stop.cancel();
        self.task.take().unwrap().await.unwrap().unwrap();
        assert!(!self.socket.exists());
        assert!(!self.control.exists());
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

fn key() -> PortKey {
    PortKey {
        module_id: "module-1".into(),
        port_id: "tx-1".into(),
        sub_index: None,
        direction: Direction::Sender,
    }
}
fn media() -> MediaState {
    MediaState {
        main: Stream {
            destination: "239.1.1.1".parse().unwrap(),
            source: None,
            udp_port: 5004,
            vlan: None,
            interface: Interface {
                module: "module-1".into(),
                port: "net-main".into(),
            },
        },
        spare: None,
        format: Format {
            attributes: [("rtp_payload_type".into(), "111".into())].into(),
            video_codec: Some(("H.264".into(), "CABAC".into(), "Automatic".into(), 1)),
            ..Format::default()
        },
        fec: None,
        fec_enabled: false,
        srt: None,
        streaming: None,
        partner: None,
        sdp: None,
    }
}

#[tokio::test]
async fn state_survives_driver_restart_and_external_events_are_changed_only() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let mut events = client.subscribe();
    assert_eq!(
        client
            .set_value(Location::root(), "level".into(), json!(61))
            .await
            .unwrap(),
        json!(61)
    );
    assert!(matches!(
        events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert_eq!(
        client.read_values(Location::root()).await.unwrap()["level"],
        json!(61)
    );
    client.close().await;
    let client = rig.client().await;
    let mut events = client.subscribe();
    assert_eq!(
        client.read_values(Location::root()).await.unwrap()["level"],
        json!(61)
    );
    assert_eq!(
        rig.ctl("set", vec!["null".into(), "level".into(), "62".into()])
            .await,
        json!({"changed":true})
    );
    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        Event::Values {
            location: Location::root(),
            values: [("level".into(), json!(62))].into()
        }
    );
    assert_eq!(
        rig.ctl("set", vec!["null".into(), "level".into(), "62".into()])
            .await,
        json!({"changed":false})
    );
    // A following correlated snapshot is a stream-order barrier, not a sleep.
    client.read_values(Location::root()).await.unwrap();
    assert!(events.try_recv().is_err());
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn scopes_actions_rejections_and_independent_configuration_drift() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let module = Location::module("module-1");
    client
        .set_value(module.clone(), "enabled".into(), json!(false))
        .await
        .unwrap();
    assert_eq!(
        client.read_values(Location::root()).await.unwrap()["enabled"],
        json!(true)
    );
    assert_eq!(
        client.read_values(module).await.unwrap()["enabled"],
        json!(false)
    );
    assert_eq!(
        client
            .set_value(Location::root(), "temperature".into(), json!(25))
            .await,
        Err(Failure::ReadOnly)
    );
    assert_eq!(
        client
            .set_value(Location::root(), "level".into(), json!(true))
            .await,
        Err(Failure::Type)
    );
    client
        .set_value(Location::root(), "trigger".into(), json!(true))
        .await
        .unwrap();
    assert!(
        !client
            .read_values(Location::root())
            .await
            .unwrap()
            .contains_key("trigger")
    );
    assert_eq!(rig.ctl("status", vec![]).await["action_count"], 1);
    let state = media();
    client.configure(key(), Some(state.clone())).await.unwrap();
    assert_eq!(client.configuration(key()).await.unwrap(), Some(state));
    assert!(
        client
            .sender_sdp(key())
            .await
            .unwrap()
            .unwrap()
            .contains("239.1.1.1")
    );
    rig.ctl("drift", vec![serde_json::to_string(&key()).unwrap()])
        .await;
    assert_eq!(client.configuration(key()).await.unwrap(), None);
    assert_eq!(client.sender_sdp(key()).await.unwrap(), None);
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn alarm_snapshot_and_event_transitions_agree() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let mut events = client.subscribe();
    let alarm = Alarm {
        id: "link".into(),
        location: Location::root(),
        details: "Modeled loss".into(),
        severity: Severity::Major,
        service_affecting: true,
    };
    rig.ctl("raise", vec![serde_json::to_string(&alarm).unwrap()])
        .await;
    assert_eq!(
        events.recv().await.unwrap(),
        Event::Alarm {
            alarm: alarm.clone()
        }
    );
    assert_eq!(client.alarms().await.unwrap(), vec![alarm]);
    rig.ctl("clear", vec!["null".into(), "link".into()]).await;
    assert_eq!(
        events.recv().await.unwrap(),
        Event::AlarmCleared {
            location: Location::root(),
            id: "link".into()
        }
    );
    assert!(client.alarms().await.unwrap().is_empty());
    rig.ctl("poll-alarms", vec![]).await;
    assert_eq!(events.recv().await.unwrap(), Event::PollAlarms);
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn fault_rejection_is_definitive_and_disconnect_retires_clones() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let clone = client.clone();
    rig.ctl("fail-next", vec![]).await;
    assert_eq!(
        client
            .set_value(Location::root(), "trigger".into(), json!(true))
            .await,
        Err(Failure::Unavailable)
    );
    assert!(client.is_healthy());
    assert_eq!(rig.ctl("status", vec![]).await["action_count"], 0);
    rig.ctl("disconnect", vec![]).await;
    assert!(client.read_values(Location::root()).await.is_err());
    assert!(!clone.is_healthy());
    client.close().await;
    let fresh = rig.client().await;
    assert!(fresh.is_healthy());
    fresh.close().await;
    rig.finish().await;
}

/// Fake target helpers speak only the local protocol. An immediate reply checks
/// waiter-before-write registration; unsolicited events share that same reader.
async fn request(reader: &mut BufReader<UnixStream>) -> Value {
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn answer(reader: &mut BufReader<UnixStream>, id: &Value, value: Value) {
    let bytes = format!(
        "{}\n",
        json!({"kind":"reply","id":id,"ok":true,"value":value})
    );
    reader.get_mut().write_all(bytes.as_bytes()).await.unwrap();
}

#[tokio::test]
async fn immediate_replies_and_interleaved_events_have_one_reader() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        let read = request(&mut reader).await;
        reader
            .get_mut()
            .write_all(b"{\"kind\":\"event\",\"value\":{\"event\":\"poll_alarms\"}}\n")
            .await
            .unwrap();
        answer(&mut reader, &read["id"], json!({"level":50})).await;
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_secs(2))
        .await
        .unwrap();
    let mut events = client.subscribe();
    assert_eq!(
        client.read_values(Location::root()).await.unwrap()["level"],
        json!(50)
    );
    assert_eq!(events.recv().await.unwrap(), Event::PollAlarms);
    client.close().await;
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_after_transmission_closes_stream_without_replaying_action() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (sent, received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        let action = request(&mut reader).await;
        assert_eq!(action["command"], "set_value");
        sent.send(()).unwrap();
        // Applied-but-unacknowledged is the ambiguous outcome: client must close,
        // not send this action a second time on the same or a replacement stream.
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_secs(2))
        .await
        .unwrap();
    let operation = tokio::spawn({
        let client = client.clone();
        async move {
            client
                .set_value(Location::root(), "trigger".into(), json!(true))
                .await
        }
    });
    received.await.unwrap();
    operation.abort();
    let _ = operation.await;
    assert!(!client.is_healthy());
    assert!(client.alarms().await.is_err());
    client.close().await;
    server.await.unwrap();
}

#[tokio::test]
async fn timeout_fails_every_waiter_and_late_reply_cannot_reuse_stream() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        request(&mut reader).await;
        request(&mut reader).await;
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_millis(250))
        .await
        .unwrap();
    let (a, b) = tokio::join!(client.read_values(Location::root()), client.alarms());
    assert!(a.is_err() && b.is_err());
    assert!(!client.is_healthy());
    client.close().await;
    server.await.unwrap();
}

#[tokio::test]
async fn sockets_are_private_and_existing_paths_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let rig = Rig::start().await;
    assert_eq!(
        std::fs::metadata(&rig.socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(&rig.control)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert!(
        simulator::serve(&rig.socket, &rig.control, CancellationToken::new())
            .await
            .is_err()
    );
    let client = rig.client().await;
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn stale_socket_recovery_never_replaces_live_or_regular_paths() {
    use std::os::unix::fs::MetadataExt;
    let directory = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let socket = directory.path().join("target.sock");
    let control = directory.path().join("control.sock");
    // A listener that predates owner locks must still be protected from takeover.
    let live = UnixListener::bind(&socket).unwrap();
    let inode = std::fs::symlink_metadata(&socket).unwrap().ino();
    assert!(
        simulator::serve(&socket, &control, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(std::fs::symlink_metadata(&socket).unwrap().ino(), inode);
    drop(live); // Leaves the socket inode, as an ungraceful process exit would.
    let stopped = CancellationToken::new();
    let (s, c, t) = (socket.clone(), control.clone(), stopped.clone());
    let server = tokio::spawn(async move { simulator::serve(&s, &c, t).await });
    let client = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(client) = Client::connect(&socket, Duration::from_secs(1)).await {
                break client;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    client.close().await;
    stopped.cancel();
    server.await.unwrap().unwrap();
    std::fs::write(&socket, b"operator-owned file").unwrap();
    assert!(
        simulator::serve(&socket, &control, CancellationToken::new())
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(&socket).unwrap(), b"operator-owned file");
    std::fs::remove_file(&socket).unwrap();
    std::os::unix::fs::symlink(&control, &socket).unwrap();
    assert!(
        simulator::serve(&socket, &control, CancellationToken::new())
            .await
            .is_err()
    );
    assert!(
        std::fs::symlink_metadata(&socket)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[tokio::test]
async fn an_external_event_does_not_discard_a_partially_read_request() {
    let rig = Rig::start().await;
    let mut reader = BufReader::new(UnixStream::connect(&rig.socket).await.unwrap());
    reader
        .get_mut()
        .write_all(b"{\"version\":1,\"id\":1,\"command\":\"health\",\"args\":null}\n")
        .await
        .unwrap();
    assert_eq!(request(&mut reader).await["id"], 1);
    reader
        .get_mut()
        .write_all(b"{\"version\":1,\"id\":2,")
        .await
        .unwrap();
    rig.ctl("poll-alarms", vec![]).await;
    assert_eq!(request(&mut reader).await["kind"], "event");
    reader
        .get_mut()
        .write_all(b"\"command\":\"alarms\",\"args\":null}\n")
        .await
        .unwrap();
    let reply = request(&mut reader).await;
    assert_eq!(reply["id"], 2);
    assert_eq!(reply["value"], json!([]));
    drop(reader);
    rig.finish().await;
}

#[tokio::test]
async fn concurrency_admission_is_bounded_and_close_fails_all_waiters() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let (sent, received) = oneshot::channel();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        for _ in 0..32 {
            request(&mut reader).await;
        }
        sent.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_secs(5))
        .await
        .unwrap();
    let mut operations = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let client = client.clone();
        operations.spawn(async move { client.alarms().await });
    }
    received.await.unwrap();
    assert_eq!(client.alarms().await, Err(Failure::Overloaded));
    client.close().await;
    while let Some(result) = operations.join_next().await {
        assert!(result.unwrap().is_err());
    }
    server.await.unwrap();
}

#[tokio::test]
async fn oversized_unterminated_target_frame_retires_the_client() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        request(&mut reader).await;
        // No LF: the client must reject without waiting for a terminator or
        // allocating an attacker-controlled amount of memory.
        let _ = reader
            .get_mut()
            .write_all(&vec![b' '; 256 * 1024 + 1])
            .await;
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_secs(2))
        .await
        .unwrap();
    assert!(client.alarms().await.is_err());
    assert!(!client.is_healthy());
    client.close().await;
    server.await.unwrap();
}

async fn await_pending_delays(rig: &Rig, expected: u64) {
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if rig.ctl("status", vec![]).await["pending_delays"] == expected {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pre-apply lifecycle did not reach the expected state");
}

#[tokio::test]
async fn retired_preapply_delay_cannot_overwrite_a_new_generation_or_execute_an_action() {
    let rig = Rig::start().await;
    for (id, old_value, new_value) in [
        ("level", json!(11), json!(22)),
        ("trigger", json!(true), json!(false)),
    ] {
        let old = rig.client().await;
        rig.ctl("delay-next", vec!["10000".into()]).await;
        let pending = tokio::spawn({
            let client = old.clone();
            async move {
                client
                    .set_value(Location::root(), id.into(), old_value)
                    .await
            }
        });
        // This diagnostic is an explicit server-side pre-apply barrier, not a
        // scheduling sleep. The old operation has consumed its injected delay.
        await_pending_delays(&rig, 1).await;
        old.close().await;
        assert!(pending.await.unwrap().is_err());
        let new = rig.client().await;
        new.set_value(Location::root(), id.into(), new_value)
            .await
            .unwrap();
        // A 10-second delayed operation must be destroyed on EOF, not merely
        // finish eventually. With zero pending guards no old timer owns an apply.
        await_pending_delays(&rig, 0).await;
        assert_eq!(
            new.read_values(Location::root()).await.unwrap()["level"],
            json!(22)
        );
        if id == "trigger" {
            assert_eq!(rig.ctl("status", vec![]).await["action_count"], 1);
        }
        new.close().await;
    }
    rig.finish().await;
}

#[tokio::test]
async fn alarm_admission_counts_escaped_snapshot_bytes_and_preserves_polling() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let alarm = |id: String, details: String| Alarm {
        id,
        details,
        location: Location::root(),
        severity: Severity::Major,
        service_affecting: true,
    };
    // The count limit is usable with small records, including a complete poll.
    for n in 0..128 {
        rig.ctl(
            "raise",
            vec![serde_json::to_string(&alarm(format!("small-{n}"), "ok".into())).unwrap()],
        )
        .await;
    }
    assert_eq!(client.alarms().await.unwrap().len(), 128);
    for n in 0..128 {
        rig.ctl("clear", vec!["null".into(), format!("small-{n}")])
            .await;
    }

    // Each control-character byte expands to six JSON bytes. Per-field/count
    // limits alone would admit an unpollable snapshot far above 256 KiB.
    rig.ctl(
        "raise",
        vec![serde_json::to_string(&alarm("small".into(), "ok".into())).unwrap()],
    )
    .await;
    let escaped = "\0".repeat(2048);
    let mut admitted = 0;
    for n in 0..128 {
        let result = simulator::control(
            &rig.control,
            "raise",
            &[serde_json::to_string(&alarm(format!("large-{n}"), escaped.clone())).unwrap()],
        )
        .await;
        if result.is_err() {
            break;
        }
        admitted += 1;
    }
    assert!(admitted > 0 && admitted < 128);
    let snapshot = client.alarms().await.unwrap();
    assert_eq!(snapshot.len(), admitted + 1);
    assert!(
        snapshot
            .iter()
            .filter(|alarm| alarm.id != "small")
            .all(|alarm| alarm.details == escaped)
    );
    assert!(client.is_healthy());
    // A rejected admission changes neither revision nor the authoritative list.
    let before = rig.ctl("status", vec![]).await;
    assert!(
        simulator::control(
            &rig.control,
            "raise",
            &[serde_json::to_string(&alarm("overflow".into(), escaped.clone())).unwrap()]
        )
        .await
        .is_err()
    );
    // Oversized refresh must preserve the old small record as well.
    assert!(
        simulator::control(
            &rig.control,
            "raise",
            &[serde_json::to_string(&alarm("small".into(), escaped)).unwrap()]
        )
        .await
        .is_err()
    );
    assert_eq!(
        rig.ctl("status", vec![]).await["revision"],
        before["revision"]
    );
    assert_eq!(client.alarms().await.unwrap(), snapshot);
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn sender_sdp_describes_only_supported_codec_payload_and_transport() {
    let rig = Rig::start().await;
    let client = rig.client().await;
    let supported = media();
    client
        .configure(key(), Some(supported.clone()))
        .await
        .unwrap();
    let sdp = client.sender_sdp(key()).await.unwrap().unwrap();
    assert!(sdp.contains("m=video 5004 RTP/AVP 111\r\n"));
    assert!(sdp.contains("a=rtpmap:111 H264/90000\r\n"));
    assert!(sdp.contains("c=IN IP4 239.1.1.1/64\r\n"));
    assert!(!sdp.contains("raw/90000"));
    // The independently generated sender SDP must also configure the example's
    // matching receiver, not merely look plausible in an isolated string test.
    let receiver = PortKey {
        module_id: "module-1".into(),
        port_id: "rx-1".into(),
        sub_index: None,
        direction: Direction::Receiver,
    };
    let wire = json!({"enabled_config":{
        "main_ip_source":{"source_address":"192.0.2.10","destination_address":"239.1.1.1","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},
        "format_attributes":{"rtp_payload_type":111},"sdp":sdp
    }});
    let observed = rust_example::protocol::media_state(
        &rust_example::registry::Registry::new(),
        &receiver,
        &wire,
    )
    .unwrap();
    client
        .configure(receiver.clone(), observed.clone())
        .await
        .unwrap();
    assert_eq!(client.configuration(receiver).await.unwrap(), observed);
    let mut h265 = supported.clone();
    h265.format.video_codec.as_mut().unwrap().0 = "H.265".into();
    h265.format
        .attributes
        .insert("rtp_payload_type".into(), "112".into());
    client.configure(key(), Some(h265)).await.unwrap();
    assert!(
        client
            .sender_sdp(key())
            .await
            .unwrap()
            .unwrap()
            .contains("a=rtpmap:112 H265/90000")
    );
    let mut srt = supported.clone();
    srt.srt = Some(Srt {
        caller: true,
        latency_ms: 120,
        overhead_percent: 25,
        encryption: None,
    });
    let mut spare = supported.clone();
    spare.spare = Some(spare.main.clone());
    let mut unknown = supported.clone();
    unknown.format.video_codec = None;
    let mut raw = supported.clone();
    raw.format.video_codec.as_mut().unwrap().0 = "JPEG XS".into();
    let mut no_payload = supported.clone();
    no_payload.format.attributes.clear();
    let mut unsupported_attributes = supported;
    unsupported_attributes
        .format
        .attributes
        .insert("ptp_domain".into(), "127".into());
    for state in [srt, spare, unknown, raw, no_payload, unsupported_attributes] {
        client.configure(key(), Some(state)).await.unwrap();
        assert_eq!(client.sender_sdp(key()).await, Err(Failure::Unsupported));
        assert!(client.is_healthy());
    }
    client.configure(key(), None).await.unwrap();
    assert_eq!(client.sender_sdp(key()).await.unwrap(), None);
    client.close().await;
    rig.finish().await;
}

#[tokio::test]
async fn delayed_relay_checks_per_parameter_write_revision_at_actual_dispatch() {
    let directory = tempfile::tempdir().unwrap();
    let socket = directory.path().join("fake.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    async fn frame(reader: &mut BufReader<UnixStream>, value: Value) {
        reader
            .get_mut()
            .write_all(format!("{value}\n").as_bytes())
            .await
            .unwrap();
    }
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut reader = BufReader::new(stream);
        let health = request(&mut reader).await;
        answer(&mut reader, &health["id"], json!({"version":1})).await;
        let read = request(&mut reader).await;
        frame(&mut reader, json!({"kind":"event","revision":1,"value":{"event":"values","location":null,"values":{"level":33}}})).await;
        frame(
            &mut reader,
            json!({"kind":"reply","id":read["id"],"ok":true,"revision":1,"value":{"level":33}}),
        )
        .await;
        let write = request(&mut reader).await;
        frame(
            &mut reader,
            json!({"kind":"reply","id":write["id"],"ok":true,"revision":2,"value":44}),
        )
        .await;
        // Both a previously relayed event and an event delivered after the write
        // response must be recognized as stale at dispatch, without dropping it.
        frame(&mut reader, json!({"kind":"event","revision":1,"value":{"event":"values","location":null,"values":{"level":33}}})).await;
        frame(&mut reader, json!({"kind":"event","revision":1,"value":{"event":"values","location":null,"values":{"enabled":false}}})).await;
        frame(
            &mut reader,
            json!({"kind":"event","revision":0,"value":{"event":"poll_alarms"}}),
        )
        .await;
        let read = request(&mut reader).await;
        frame(
            &mut reader,
            json!({"kind":"reply","id":read["id"],"ok":true,"revision":100,"value":{"level":44}}),
        )
        .await;
        frame(&mut reader, json!({"kind":"event","revision":3,"value":{"event":"values","location":null,"values":{"level":45}}})).await;
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).await.unwrap(), 0);
    });
    let client = Client::connect(&socket, Duration::from_secs(2))
        .await
        .unwrap();
    let mut observations = client.subscribe_observed();
    client.read_values(Location::root()).await.unwrap();
    let relayed = observations.recv().await.unwrap();
    assert_eq!(relayed.revision, 1);
    assert!(client.event_is_current(&relayed));
    client
        .set_value(Location::root(), "level".into(), json!(44))
        .await
        .unwrap();
    assert!(!client.event_is_current(&relayed)); // Already queued before reply.
    let late = observations.recv().await.unwrap();
    assert!(!client.event_is_current(&late)); // Arrives after reply, not dropped.
    let unrelated = observations.recv().await.unwrap();
    assert!(client.event_is_current(&unrelated)); // No global revision floor.
    let mut other_scope = relayed.clone();
    if let Event::Values { location, .. } = &mut other_scope.event {
        *location = Location::module("module-1");
    }
    assert!(client.event_is_current(&other_scope));
    let poll = observations.recv().await.unwrap();
    assert!(client.event_is_current(&poll));
    for event in [
        Event::Alarm {
            alarm: Alarm {
                id: "alarm".into(),
                location: Location::root(),
                details: "local".into(),
                severity: Severity::Major,
                service_affecting: true,
            },
        },
        Event::AlarmCleared {
            location: Location::root(),
            id: "alarm".into(),
        },
    ] {
        assert!(
            client.event_is_current(&rust_example::device::ObservedEvent { revision: 0, event })
        );
    }
    client.read_values(Location::root()).await.unwrap();
    let after_read = observations.recv().await.unwrap();
    assert_eq!(after_read.revision, 3);
    assert!(client.event_is_current(&after_read)); // Read rev100 did not erase history.
    client.close().await;
    server.await.unwrap();
}
