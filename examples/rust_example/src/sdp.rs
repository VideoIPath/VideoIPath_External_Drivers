//! SDP projection shared by the in-memory demo and optional device lab.
use crate::domain::{Failure, MediaState};

/// Explicit H.264/H.265 RTP, with optional two-leg RFC 7104 DUP grouping.
/// Unsupported state is an error, never a plausible but contradictory template.
pub(crate) fn sender_sdp(state: &MediaState) -> Result<String, Failure> {
    if state.srt.is_some()
        || state.fec.is_some()
        || state.fec_enabled
        || state.sdp.is_some()
        || state.main.source.is_some()
        || state
            .spare
            .as_ref()
            .is_some_and(|stream| stream.source.is_some())
        || state.spare.as_ref() == Some(&state.main)
        || !state.format.audio_pairs.is_empty()
        || state
            .format
            .attributes
            .keys()
            .any(|key| key != "rtp_payload_type")
    {
        return Err(Failure::Unsupported);
    }
    let encoding = match state
        .format
        .video_codec
        .as_ref()
        .map(|codec| codec.0.as_str())
    {
        Some("H.264") => "H264",
        Some("H.265") => "H265",
        _ => return Err(Failure::Unsupported),
    };
    let payload = state
        .format
        .attributes
        .get("rtp_payload_type")
        .and_then(|value| value.parse::<u8>().ok())
        .filter(|value| (96..=127).contains(value))
        .ok_or(Failure::Unsupported)?;
    let mut sdp =
        "v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=Modeled RTP session\r\nt=0 0\r\n".to_owned();
    if state.spare.is_some() {
        // Mids are SDP identifiers, not added SPEC fields. Group order is the
        // documented main/spare mapping; section occurrence order is irrelevant.
        sdp.push_str("a=group:DUP main spare\r\n");
    }
    for (stream, mid) in std::iter::once((&state.main, "main"))
        .chain(state.spare.iter().map(|stream| (stream, "spare")))
    {
        let family = if stream.destination.is_ipv4() {
            "IP4"
        } else {
            "IP6"
        };
        let address = match stream.destination {
            std::net::IpAddr::V4(ip) if ip.is_multicast() => {
                format!("{ip}/{}", state.streaming.as_ref().map_or(64, |s| s.ttl))
            }
            address => address.to_string(),
        };
        sdp.push_str(&format!(
            "m=video {} RTP/AVP {payload}\r\nc=IN {} {}\r\na=rtpmap:{payload} {encoding}/90000\r\n",
            stream.udp_port, family, address,
        ));
        if state.spare.is_some() {
            sdp.push_str(&format!("a=mid:{mid}\r\n"));
        }
    }
    Ok(sdp)
}
