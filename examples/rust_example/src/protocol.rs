//! Derived SPEC-23 codec, based on the later array-based IPT contract. This is
//! not an official JSON Schema or evidence of acceptance by a VideoIPath release.
//! Unknown object fields are ignored; known fields still receive strict checks.
use crate::{domain::*, registry::Registry};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, net::IpAddr};

pub const ASYNC_VALUES_CHANGED: &str = "async_values_changed";
pub const MAX_ITEMS: usize = 256;

#[derive(Clone, Deserialize)]
pub struct Request {
    // i128 holds both signed and unsigned JSON integers accepted by serde_json.
    // We never narrow a usable session ID and lose its response correlation.
    pub message_id: i128,
    pub message_type: String,
    pub data: Value,
}
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Even unknown message-type text is peer-controlled. Diagnostics need
        // only the correlation identity, never a potentially secret payload.
        f.debug_struct("Request")
            .field("message_id", &self.message_id)
            .finish_non_exhaustive()
    }
}

/// Without a usable integer correlation ID, the session must close rather than
/// fabricate an ID. Keepalive exact echo uses the original text at session level.
#[derive(Debug, Clone, Copy)]
pub struct DecodeError {
    pub message_id: Option<i128>,
    pub failure: Failure,
}
impl Request {
    pub fn parse(text: &str) -> Result<Self, DecodeError> {
        let value: Value = serde_json::from_str(text).map_err(|_| DecodeError {
            message_id: None,
            failure: Failure::Invalid,
        })?;
        let message_id = value.get("message_id").and_then(|v| {
            v.as_i64()
                .map(i128::from)
                .or_else(|| v.as_u64().map(i128::from))
        });
        if !value.as_object().is_some_and(|o| o.contains_key("data")) {
            return Err(DecodeError {
                message_id,
                failure: Failure::Invalid,
            });
        }
        serde_json::from_value(value).map_err(|_| DecodeError {
            message_id,
            failure: Failure::Invalid,
        })
    }
}
pub fn error_frame(message_id: i128, failure: Failure) -> Value {
    json!({"message_id":message_id,"message_type":"error","data":failure.to_string()})
}
pub fn response(request: &Request, data: Value) -> Value {
    json!({"message_id":request.message_id,"message_type":request.message_type,"data":data})
}
pub fn event_frame(event: &Event) -> Value {
    let (kind, data) = match event {
        Event::Values { location, values } => (
            ASYNC_VALUES_CHANGED,
            json!({"location":location,"values":values}),
        ),
        Event::Alarm { alarm } => ("async_alarm", json!(alarm)),
        Event::AlarmCleared { location, id } => {
            ("async_alarm_cleared", json!({"location":location,"id":id}))
        }
        Event::PollAlarms => ("async_poll_alarms", Value::Null),
    };
    json!({"message_id":null,"message_type":kind,"data":data})
}

#[derive(Deserialize)]
pub struct GetValues {
    #[serde(default)]
    pub location: Location,
    pub ids: Option<Vec<String>>,
}
#[derive(Deserialize)]
pub struct SetValues {
    #[serde(default)]
    pub location: Location,
    #[serde(default)]
    pub values: Values,
}
#[derive(Deserialize)]
pub struct SdpRequest {
    pub module_id: String,
    pub port_id: String,
}
#[derive(Deserialize)]
pub struct IptBatch {
    pub module_id: String,
    #[serde(default)]
    pub sender_configs: Vec<Value>,
    #[serde(default)]
    pub receiver_configs: Vec<Value>,
}
#[derive(Deserialize)]
pub struct IptCheck {
    pub module_id: String,
    pub config: Value,
    pub set_again: bool,
}

pub fn decode<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, Failure> {
    serde_json::from_value(value.clone()).map_err(|_| Failure::Invalid)
}
fn object(value: &Value) -> Result<&serde_json::Map<String, Value>, Failure> {
    value.as_object().ok_or(Failure::Type)
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Failure> {
    value.get(key).and_then(Value::as_str).ok_or(Failure::Type)
}
fn uint(value: &Value, key: &str, max: u64) -> Result<u64, Failure> {
    let n = value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(Failure::Type)?;
    if n > max { Err(Failure::Range) } else { Ok(n) }
}
fn boolean(value: &Value, key: &str) -> Result<bool, Failure> {
    value.get(key).and_then(Value::as_bool).ok_or(Failure::Type)
}
fn optional<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|v| !v.is_null())
}
fn choice<'a>(value: &'a Value, key: &str, allowed: &[&str]) -> Result<&'a str, Failure> {
    let s = text(value, key)?;
    if allowed.contains(&s) {
        Ok(s)
    } else {
        Err(Failure::Unsupported)
    }
}

pub fn config_key(module: &str, config: &Value, direction: Direction) -> Result<PortKey, Failure> {
    let actual: Direction = decode(config.get("transceiver_type").ok_or(Failure::Invalid)?)?;
    if actual != direction {
        return Err(Failure::Invalid);
    }
    let sub_index = optional(config, "sub_index")
        .map(|v| {
            v.as_u64()
                .and_then(|n| u32::try_from(n).ok())
                .ok_or(Failure::Type)
        })
        .transpose()?;
    Ok(PortKey {
        module_id: module.into(),
        port_id: text(config, "id")?.into(),
        sub_index,
        direction,
    })
}

/// Translate a SPEC configuration into supported device semantics. Unknown
/// optional metadata disappears here; comparison never uses the original DTO.
pub fn media_state(
    registry: &Registry,
    key: &PortKey,
    config: &Value,
) -> Result<Option<MediaState>, Failure> {
    let port = registry.port(key)?;
    if port["topology_info"].is_null() {
        return Err(Failure::Unsupported);
    }
    let enabled = config.get("enabled_config").ok_or(Failure::Invalid)?;
    if enabled.is_null() {
        return Ok(None);
    }
    object(enabled)?;
    let sender = key.direction == Direction::Sender;
    let (main_field, spare_field) = if sender {
        ("main_ip_destination", "spare_ip_destination")
    } else {
        ("main_ip_source", "spare_ip_source")
    };
    let main = stream(
        registry,
        enabled.get(main_field).ok_or(Failure::Invalid)?,
        sender,
    )?;
    let spare = optional(enabled, spare_field)
        .map(|v| stream(registry, v, sender))
        .transpose()?;
    if spare.is_some() && port["topology_info"]["sips"] != true {
        return Err(Failure::Unsupported);
    }
    for stream in std::iter::once(&main).chain(spare.iter()) {
        let allowed = port["topology_info"]["interface_options"]
            .as_array()
            .is_some_and(|options| {
                options.iter().any(|i| {
                    i["module_id"] == stream.interface.module
                        && i["port_id"] == stream.interface.port
                })
            });
        if !allowed {
            return Err(Failure::Unsupported);
        }
    }
    let format = if sender {
        format(optional(enabled, "format_info").unwrap_or(&json!({})))?
    } else {
        Format {
            attributes: attributes(optional(enabled, "format_attributes").unwrap_or(&json!({})))?,
            ..Format::default()
        }
    };
    let fec = if sender {
        optional(enabled, "fec").map(fec).transpose()?
    } else {
        None
    };
    let fec_enabled = if sender {
        fec.is_some()
    } else {
        enabled
            .get("fec_enabled")
            .map(|v| v.as_bool().ok_or(Failure::Type))
            .transpose()?
            .unwrap_or(false)
    };
    let srt = optional(enabled, "srt").map(srt).transpose()?;
    if srt.is_some()
        && (main.destination.is_multicast()
            || spare.as_ref().is_some_and(|s| s.destination.is_multicast()))
    {
        return Err(Failure::Invalid);
    }
    let streaming = if sender {
        optional(enabled, "streaming")
            .map(streaming)
            .transpose()?
            .flatten()
    } else {
        None
    };
    let partner = if sender {
        optional(enabled, "resource_partner")
            .map(|p| {
                let partner = PortKey {
                    module_id: text(p, "module_id")?.into(),
                    port_id: text(p, "port_id")?.into(),
                    sub_index: optional(p, "sub_index")
                        .map(|n| {
                            n.as_u64()
                                .and_then(|v| u32::try_from(v).ok())
                                .ok_or(Failure::Type)
                        })
                        .transpose()?,
                    direction: Direction::Receiver,
                };
                registry.port(&partner)?;
                let allowed = registry.structure()["resource_links"]
                    .as_array()
                    .is_some_and(|links| {
                        links.iter().any(|l| {
                            l["type"] == "rx-tx"
                                && l["from"]["module_id"] == partner.module_id
                                && l["from"]["port_id"] == partner.port_id
                                && l["to"]["module_id"] == key.module_id
                                && l["to"]["port_id"] == key.port_id
                        })
                    });
                if !allowed {
                    return Err(Failure::Unsupported);
                }
                Ok(partner)
            })
            .transpose()?
    } else {
        None
    };
    let sdp = if sender {
        None
    } else {
        optional(enabled, "sdp")
            .map(|s| {
                if port["topology_info"]["sdp_support"] != true {
                    return Err(Failure::Unsupported);
                }
                // These RTP descriptions do not model SRT or FEC repair streams.
                if srt.is_some() || fec_enabled {
                    return Err(Failure::Unsupported);
                }
                validate_sdp_with_spare(
                    s.as_str().ok_or(Failure::Type)?,
                    &main,
                    spare.as_ref(),
                    &format,
                    port["topology_info"]["format_type"].as_str(),
                )
            })
            .transpose()?
    };
    Ok(Some(MediaState {
        main,
        spare,
        format,
        fec,
        fec_enabled,
        srt,
        streaming,
        partner,
        sdp,
    }))
}

fn stream(registry: &Registry, value: &Value, sender: bool) -> Result<Stream, Failure> {
    object(value)?;
    let destination = text(
        value,
        if sender {
            "address"
        } else {
            "destination_address"
        },
    )?
    .parse::<IpAddr>()
    .map_err(|_| Failure::Invalid)?;
    if destination.is_unspecified() {
        return Err(Failure::Range);
    }
    let source = if sender {
        None
    } else {
        Some(
            text(value, "source_address")?
                .parse::<IpAddr>()
                .map_err(|_| Failure::Invalid)?,
        )
    };
    if source.is_some_and(|s| s.is_multicast() || s.is_ipv4() != destination.is_ipv4()) {
        return Err(Failure::Invalid);
    }
    let udp_port = uint(value, "udp_port", 65535)? as u16;
    if udp_port == 0 {
        return Err(Failure::Range);
    }
    let vlan = optional(value, "vlan")
        .map(|v| {
            let n = v.as_u64().ok_or(Failure::Type)?;
            if !(1..=4094).contains(&n) {
                Err(Failure::Range)
            } else {
                Ok(n as u16)
            }
        })
        .transpose()?;
    let interface = &value["network_interface"];
    let interface = Interface {
        module: text(interface, "module_id")?.into(),
        port: text(interface, "port_id")?.into(),
    };
    if !registry.interface_exists(&interface) {
        return Err(Failure::Unknown);
    }
    Ok(Stream {
        destination,
        source,
        udp_port,
        vlan,
        interface,
    })
}
fn fec(value: &Value) -> Result<Fec, Failure> {
    let columns = uint(value, "length", u32::MAX.into())? as u32;
    let rows = optional(value, "depth")
        .map(|n| {
            n.as_u64()
                .filter(|n| *n > 0 && *n <= u32::MAX.into())
                .map(|n| n as u32)
                .ok_or(Failure::Range)
        })
        .transpose()?;
    let overhead_percent = value["overhead"]
        .as_f64()
        .filter(|n| n.is_finite() && (0.0..=100.0).contains(n))
        .ok_or(Failure::Range)?;
    if columns == 0 {
        return Err(Failure::Range);
    }
    Ok(Fec {
        columns,
        rows,
        skew: boolean(value, "skew")?,
        overhead_percent,
    })
}
fn srt(value: &Value) -> Result<Srt, Failure> {
    let caller = choice(value, "mode", &["caller", "listener"])? == "caller";
    let latency_ms = uint(value, "buffer_latency", u32::MAX.into())? as u32;
    let overhead_percent = uint(value, "overhead_bandwidth", 100)? as u32;
    let encryption = optional(value, "encryption")
        .map(|e| {
            let algorithm = choice(e, "mode", &["AES128", "AES192", "AES256"])?.into();
            let key = text(e, "key")?;
            // Simulator policy: bounded nonempty key. SPEC gives no passphrase
            // length rule, so this is not claimed to validate an actual SRT engine.
            if key.is_empty() || key.len() > 1024 {
                return Err(Failure::Range);
            }
            Ok(Encryption {
                algorithm,
                key: key.into(),
            })
        })
        .transpose()?;
    Ok(Srt {
        caller,
        latency_ms,
        overhead_percent,
        encryption,
    })
}
fn streaming(value: &Value) -> Result<Option<Streaming>, Failure> {
    let fields = object(value)?;
    // A future optional field must not turn an otherwise empty/default streaming
    // object into a malformed partial known configuration.
    if ![
        "qos",
        "ts_packet_length",
        "ts_packets_per_frame",
        "ts_rate",
        "ip_rate",
        "ttl",
    ]
    .iter()
    .any(|key| fields.contains_key(*key))
    {
        return Ok(None);
    }
    let qos = text(value, "qos")?;
    if qos.len() != 8 || !qos.bytes().all(|b| b == b'0' || b == b'1') {
        return Err(Failure::Range);
    }
    let packet_length = uint(value, "ts_packet_length", u16::MAX.into())? as u16;
    if ![188, 204].contains(&packet_length) {
        return Err(Failure::Unsupported);
    }
    let packets_per_frame = uint(value, "ts_packets_per_frame", u16::MAX.into())? as u16;
    let ttl = uint(value, "ttl", 255)? as u8;
    if packets_per_frame == 0 || ttl == 0 {
        return Err(Failure::Range);
    }
    Ok(Some(Streaming {
        dscp_byte: u8::from_str_radix(qos, 2).map_err(|_| Failure::Invalid)?,
        packet_length,
        packets_per_frame,
        transport_rate: uint(value, "ts_rate", u64::MAX)?,
        ip_rate: uint(value, "ip_rate", u64::MAX)?,
        ttl,
    }))
}

fn format(value: &Value) -> Result<Format, Failure> {
    object(value)?;
    let mut result = Format {
        attributes: attributes(optional(value, "attributes").unwrap_or(&json!({})))?,
        ..Format::default()
    };
    if let Some(pairs) = optional(value, "audio_pairs") {
        for (name, pair) in object(pairs)? {
            if ![
                "G1P1", "G1P2", "G2P1", "G2P2", "G3P1", "G3P2", "G4P1", "G4P2",
            ]
            .contains(&name.as_str())
            {
                continue;
            }
            if pair.is_null() {
                continue;
            }
            let codec = choice(
                pair,
                "codec",
                &[
                    "MPEG-1 Layer 2",
                    "AAC_LC",
                    "AAC_HE",
                    "SMPTE 302M",
                    "Dolby E",
                    "Dolby Digital",
                ],
            )?;
            result.audio_pairs.insert(
                name.clone(),
                (codec.into(), uint(pair, "bitrate", u32::MAX.into())? as u32),
            );
        }
    }
    if let Some(video) = optional(value, "video_codec") {
        result.video_codec = Some((
            choice(
                video,
                "codec",
                &[
                    "H.264",
                    "H.265",
                    "MPEG-2",
                    "AVC-I 50",
                    "AVC-I 100",
                    "J2K",
                    "TICO",
                    "JPEG XS",
                ],
            )?
            .into(),
            choice(video, "entropy_encoding", &["CAVLC", "CABAC"])?.into(),
            choice(
                video,
                "gop_struct",
                &["Automatic", "IP", "IPB", "IPBB", "IPBBB", "IPBBBB"],
            )?
            .into(),
            uint(video, "gop_size", u32::MAX.into())? as u32,
        ));
    }
    Ok(result)
}

fn attributes(value: &Value) -> Result<BTreeMap<String, String>, Failure> {
    object(value)?;
    let mut result = BTreeMap::new();
    for (key, max) in [("ptp_domain", 255), ("rtp_payload_type", 127)] {
        if optional(value, key).is_some() {
            result.insert(key.into(), uint(value, key, max)?.to_string());
        }
    }
    if let Some(audio) = optional(value, "audio") {
        object(audio)?;
        if optional(audio, "bit_depth").is_some() {
            result.insert(
                "audio.bit_depth".into(),
                choice(audio, "bit_depth", &["L16", "L20", "L24", "L32", "AM824"])?.into(),
            );
        }
        for field in ["frame_size", "n_channels", "sample_rate", "link_offset"] {
            if optional(audio, field).is_some() {
                let n = uint(audio, field, u32::MAX.into())?;
                if n == 0 && ["n_channels", "sample_rate"].contains(&field) {
                    return Err(Failure::Range);
                }
                result.insert(format!("audio.{field}"), n.to_string());
            }
        }
    }
    if let Some(video) = optional(value, "video") {
        object(video)?;
        let fields: &[(&str, &[&str])] = &[
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
        for (field, allowed) in fields {
            if optional(video, field).is_some() {
                result.insert(
                    format!("video.{field}"),
                    choice(video, field, allowed)?.into(),
                );
            }
        }
    }
    Ok(result)
}

/// Bounded modeled SDP subset: one RTP/AVP L16/L24 PCM audio or MP2T/H264/H265 video
/// section. Audio uses 44.1/48/96 kHz and 1..64 channels (simulator limits).
/// Raw ST 2110 video needs fmtp interpretation and is explicitly unsupported.
/// This validates control-state consistency, not universal SDP conformance.
pub fn validate_sdp(
    sdp: &str,
    stream: &Stream,
    format: &Format,
    advertised_format: Option<&str>,
) -> Result<String, Failure> {
    validate_sdp_with_spare(sdp, stream, None, format, advertised_format)
}

/// RFC 5888 mids identify sections independently of their occurrence order.
/// This simulator maps the first/second DUP group IDs to main/spare respectively;
/// RFC 7104 itself does not define SPEC-23 main/spare or transmission precedence.
/// Exactly two H264/H265 streams with the same payload mapping are supported.
/// Different PTs are legal in RFC 7104, but require per-leg format state we lack.
pub fn validate_sdp_with_spare(
    sdp: &str,
    main: &Stream,
    spare: Option<&Stream>,
    format: &Format,
    advertised_format: Option<&str>,
) -> Result<String, Failure> {
    if sdp.len() > 65536 || sdp.contains('\0') {
        return Err(Failure::Range);
    }
    let mut session = Vec::new();
    let mut sections: Vec<Vec<&str>> = Vec::new();
    for (index, line) in sdp
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
    {
        if index >= 256 || line.len() > 2048 {
            return Err(Failure::Range);
        }
        if line.starts_with("m=") {
            if sections.len() >= 2 {
                return Err(Failure::Unsupported);
            }
            sections.push(Vec::new());
        }
        if let Some(section) = sections.last_mut() {
            // Session-only fields must not silently acquire media scope.
            if ["v=", "o=", "s=", "t=", "a=group:"]
                .iter()
                .any(|p| line.starts_with(p))
            {
                return Err(Failure::Invalid);
            }
            section.push(line);
        } else {
            if line.starts_with("a=mid:") || line.starts_with("a=rtpmap:") {
                return Err(Failure::Invalid);
            }
            session.push(line);
        }
    }
    let mut mids = Vec::new();
    for section in &sections {
        let ids: Vec<_> = section
            .iter()
            .filter_map(|line| line.strip_prefix("a=mid:"))
            .collect();
        if ids.len() > 1 || ids.first().is_some_and(|id| !sdp_token(id)) {
            return Err(Failure::Invalid);
        }
        mids.push(ids.first().copied());
    }
    let groups: Vec<_> = session
        .iter()
        .filter_map(|line| line.strip_prefix("a=group:"))
        .collect();
    let Some(spare) = spare else {
        if !groups.is_empty() || sections.len() != 1 {
            return Err(Failure::Unsupported);
        }
        return validate_single_sdp(sdp, main, format, advertised_format);
    };
    // Two copies of the exact same path do not describe path redundancy. Equal
    // addresses on distinct interfaces remain legitimate isolated-network legs.
    if main == spare {
        return Err(Failure::Unsupported);
    }
    if sections.len() != 2 || groups.len() != 1 || advertised_format != Some("Video") {
        return Err(Failure::Unsupported);
    }
    let group: Vec<_> = groups[0].split_whitespace().collect();
    if group.len() != 3
        || group[0] != "DUP"
        || group[1] == group[2]
        || !group[1..].iter().all(|id| sdp_token(id))
        || mids[0].is_none()
        || mids[1].is_none()
        || mids[0] == mids[1]
    {
        return Err(Failure::Invalid);
    }
    // No session connection/filter inheritance for the two-leg subset: every
    // leg explicitly describes its own destination and optional source filter.
    let header: Vec<_> = session
        .iter()
        .copied()
        .filter(|line| !line.starts_with("a=group:"))
        .collect();
    if header
        .iter()
        .any(|line| !["v=", "o=", "s=", "t="].iter().any(|p| line.starts_with(p)))
    {
        return Err(Failure::Unsupported);
    }
    let mut normalized = format!(
        "{}\r\na=group:DUP {} {}\r\n",
        header.join("\r\n"),
        group[1],
        group[2]
    );
    let mut previous_mapping = None;
    for (id, stream) in group[1..].iter().zip([main, spare]) {
        let index = mids
            .iter()
            .position(|mid| *mid == Some(*id))
            .ok_or(Failure::Mismatch)?;
        let section = &sections[index];
        let text = format!("{}\r\n{}\r\n", header.join("\r\n"), section.join("\r\n"));
        let checked = validate_single_sdp(&text, stream, format, advertised_format)?;
        let mapping = section
            .iter()
            .find_map(|line| line.strip_prefix("a=rtpmap:"))
            .ok_or(Failure::Unsupported)?;
        let (pt, encoding) = mapping.split_once(' ').ok_or(Failure::Invalid)?;
        if !["H264/90000", "H265/90000"].contains(&encoding) {
            return Err(Failure::Unsupported);
        }
        if previous_mapping.is_some_and(|previous| previous != (pt, encoding)) {
            return Err(Failure::Mismatch);
        }
        previous_mapping = Some((pt, encoding));
        // Normalize attribute order and source-filter whitespace as well as m=
        // order. Checks compare independently read state to this canonical form.
        let mut attributes: Vec<_> = checked
            .lines()
            .filter(|line| line.starts_with("a="))
            .collect();
        attributes.sort_unstable();
        normalized.push_str(section[0]);
        normalized.push_str("\r\n");
        normalized.push_str(
            section
                .iter()
                .find(|line| line.starts_with("c="))
                .ok_or(Failure::Invalid)?,
        );
        normalized.push_str("\r\n");
        normalized.push_str(&attributes.join("\r\n"));
        normalized.push_str("\r\n");
    }
    Ok(normalized)
}

fn sdp_token(value: &str) -> bool {
    // RFC 4566 token (used by RFC 5888 identification-tag), ASCII only.
    !value.is_empty() && value.bytes().all(|b| matches!(b, 0x21 | 0x23..=0x27 | 0x2a..=0x2b | 0x2d..=0x2e | 0x30..=0x39 | 0x41..=0x5a | 0x5e..=0x7e))
}

fn validate_single_sdp(
    sdp: &str,
    stream: &Stream,
    format: &Format,
    advertised_format: Option<&str>,
) -> Result<String, Failure> {
    let lines: Vec<_> = sdp
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if lines.first() != Some(&"v=0")
        || ["v=", "o=", "s=", "t="]
            .iter()
            .any(|prefix| lines.iter().filter(|line| line.starts_with(prefix)).count() != 1)
        || !lines.contains(&"t=0 0")
    {
        return Err(Failure::Invalid);
    }
    let media: Vec<_> = lines.iter().filter(|l| l.starts_with("m=")).collect();
    let connections: Vec<_> = lines.iter().filter(|l| l.starts_with("c=")).collect();
    if media.len() != 1 || connections.len() != 1 {
        return Err(Failure::Unsupported);
    }
    let m: Vec<_> = media[0][2..].split_whitespace().collect();
    if m.len() != 4 || !["audio", "video"].contains(&m[0]) || m[2] != "RTP/AVP" {
        return Err(Failure::Unsupported);
    }
    let expected_media = match advertised_format {
        Some("Audio") => "audio",
        Some("Video") => "video",
        _ => return Err(Failure::Unsupported),
    };
    if m[0] != expected_media {
        return Err(Failure::Mismatch);
    }
    if m[1].parse::<u16>().ok() != Some(stream.udp_port) {
        return Err(Failure::Mismatch);
    }
    let payload = m[3].parse::<u8>().map_err(|_| Failure::Invalid)?;
    if payload > 127
        || format
            .attributes
            .get("rtp_payload_type")
            .is_some_and(|s| s != m[3])
    {
        return Err(Failure::Mismatch);
    }
    let c: Vec<_> = connections[0][2..].split_whitespace().collect();
    if c.len() != 3 || c[0] != "IN" || !["IP4", "IP6"].contains(&c[1]) {
        return Err(Failure::Invalid);
    }
    let address_parts: Vec<_> = c[2].split('/').collect();
    let address = address_parts[0]
        .parse::<IpAddr>()
        .map_err(|_| Failure::Invalid)?;
    if address_parts.len() > 1
        && (address_parts.len() != 2
            || !address.is_ipv4()
            || !address.is_multicast()
            || !decimal(address_parts[1]).is_ok_and(|ttl| ttl <= 255))
    {
        return Err(Failure::Unsupported);
    }
    if address != stream.destination || (c[1] == "IP4") != address.is_ipv4() {
        return Err(Failure::Mismatch);
    }
    if format
        .attributes
        .keys()
        .any(|key| key.starts_with("video."))
        || (m[0] == "video"
            && format
                .attributes
                .keys()
                .any(|key| key.starts_with("audio.")))
        || ["audio.frame_size", "audio.link_offset"]
            .iter()
            .any(|key| format.attributes.contains_key(*key))
    {
        // Only the supported audio attributes are cross-checked. Reject supplied
        // attributes we cannot reconcile instead of silently confirming them.
        return Err(Failure::Unsupported);
    }
    let mappings = lines.iter().filter(|l| l.starts_with("a=rtpmap:")).count();
    if mappings > 1 || (mappings == 0 && !(m[0] == "video" && payload == 33)) {
        return Err(Failure::Unsupported);
    }
    if lines
        .iter()
        .filter(|line| line.starts_with("a=source-filter:"))
        .count()
        > 1
    {
        return Err(Failure::Unsupported);
    }
    for line in &lines {
        // JSON v1 object extensions are ignored elsewhere. SDP is a different
        // language: unknown attributes may change transport/encoding semantics,
        // so this deliberately small implementation rejects them explicitly.
        if ![
            "v=",
            "o=",
            "s=",
            "c=",
            "t=",
            "m=",
            "a=rtpmap:",
            "a=source-filter:",
            "a=mid:",
        ]
        .iter()
        .any(|prefix| line.starts_with(prefix))
        {
            return Err(Failure::Unsupported);
        }
        if let Some(mapping) = line.strip_prefix("a=rtpmap:") {
            let (pt, encoding) = mapping.split_once(' ').ok_or(Failure::Invalid)?;
            if pt != m[3] {
                return Err(Failure::Mismatch);
            }
            let fields: Vec<_> = encoding.split('/').collect();
            if !(2..=3).contains(&fields.len()) {
                return Err(Failure::Invalid);
            }
            let clock = decimal(fields[1])?;
            if m[0] == "audio" {
                if !["L16", "L24"].contains(&fields[0]) || ![44100, 48000, 96000].contains(&clock) {
                    return Err(Failure::Unsupported);
                }
                let channels = fields.get(2).map_or(Ok(1), |value| decimal(value))?;
                if !(1..=64).contains(&channels) {
                    return Err(Failure::Range);
                }
                // Static RTP audio payloads have fixed semantics. Dynamic values
                // are 96..127; a random static PT must not be relabeled PCM.
                if !(96..=127).contains(&payload)
                    && !(fields[0] == "L16"
                        && clock == 44100
                        && ((payload == 10 && channels == 2) || (payload == 11 && channels == 1)))
                {
                    return Err(Failure::Mismatch);
                }
                for (key, observed) in [
                    ("audio.bit_depth", fields[0].to_owned()),
                    ("audio.sample_rate", clock.to_string()),
                    ("audio.n_channels", channels.to_string()),
                ] {
                    if format.attributes.get(key).is_some_and(|s| s != &observed) {
                        return Err(Failure::Mismatch);
                    }
                }
            } else if fields.len() != 2
                || !["MP2T", "H264", "H265"].contains(&fields[0])
                || clock != 90000
            {
                return Err(Failure::Unsupported);
            } else if !(96..=127).contains(&payload) && !(fields[0] == "MP2T" && payload == 33) {
                return Err(Failure::Mismatch);
            }
        }
        if let Some(filter) = line.strip_prefix("a=source-filter:") {
            let filter = filter
                .trim_start()
                .strip_prefix("incl ")
                .ok_or(Failure::Unsupported)?;
            let f: Vec<_> = filter.split_whitespace().collect();
            if f.len() != 4
                || f[0] != "IN"
                || f[1] != c[1]
                || f[2].parse::<IpAddr>().ok() != Some(stream.destination)
                || f[3].parse::<IpAddr>().ok() != stream.source
            {
                return Err(Failure::Mismatch);
            }
        }
    }
    let lines: Vec<_> = lines
        .iter()
        .map(|line| {
            if let Some(filter) = line.strip_prefix("a=source-filter:") {
                // Comparison is semantic: permitted whitespace must not produce
                // perpetual drift after the target normalizes a source filter.
                format!(
                    "a=source-filter:{}",
                    filter.split_whitespace().collect::<Vec<_>>().join(" ")
                )
            } else {
                (*line).to_owned()
            }
        })
        .collect();
    Ok(format!("{}\r\n", lines.join("\r\n")))
}

/// Positive decimal SDP clocks/channels; accepting strings merely because a
/// corresponding optional format attribute is absent would confirm nonsense.
fn decimal(value: &str) -> Result<u32, Failure> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Failure::Invalid);
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|number| *number > 0)
        .ok_or(Failure::Range)
}
