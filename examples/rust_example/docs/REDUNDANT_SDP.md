# Main/spare SDP walkthrough

Version 0.2.0 adds a bounded **H.264/H.265 two-stream DUP** example to the existing
single-stream SDP subset. The simulator stores control state; it does not emit
duplicated packets, recover loss, or implement an ST 2022-7 media engine.

## Contract and ownership

| Boundary | Responsibility |
| --- | --- |
| Protocol codec | Validate SDP groups/sections against explicit main/spare addresses, ports and supported format; reject before applying |
| Capability handler | Apply normalized desired state, read back, compare; no retries added |
| Simulator | Own both legs in one `MediaState` observation; generate truthful SDP or return unsupported |
| Session | Preserve request correlation/deadline; no new wire messages or async SDP token |

Grouping follows [RFC 7104](https://www.rfc-editor.org/rfc/rfc7104), using the
media IDs and session-level groups defined by
[RFC 5888](https://www.rfc-editor.org/rfc/rfc5888). SDP field scope follows
[RFC 8866](https://www.rfc-editor.org/rfc/rfc8866).

**Local policy:** first/second IDs in the `DUP` group map to the explicit
main/spare configuration fields. The textual occurrence order of the media
sections does not determine that mapping. RFC 7104 does not define SPEC main/spare
semantics: this mapping must be checked against the intended backend. The two legs
must use the same H264/H265 encoding, 90000 Hz clock and dynamic payload type in
96–127. RFC 7104 permits some differing payload types; this simulator intentionally
rejects them because its domain has one shared format description.

## 1. Configure the sender

Send through the handlers in the offline walkthrough, a test backend or a future
VideoIPath integration. The two interface IDs already exist in the structure.

```json
{"message_id":50,"message_type":"set_ipt_configurations","data":{"module_id":"module-1","sender_configs":[{"id":"tx-1","transceiver_type":"sender","enabled_config":{"main_ip_destination":{"address":"239.1.1.1","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},"spare_ip_destination":{"address":"239.2.1.1","udp_port":5006,"network_interface":{"module_id":"module-1","port_id":"net-spare"}},"format_info":{"video_codec":{"codec":"H.264","entropy_encoding":"CABAC","gop_struct":"Automatic","gop_size":1},"attributes":{"rtp_payload_type":111}}}}]}}
```

```json
{"message_id":50,"message_type":"set_ipt_configurations","data":{"sender_config_errors":[],"receiver_config_errors":[]}}
```

## 2. Read the paired SDP

```json
{"message_id":51,"message_type":"get_sender_sdp","data":{"module_id":"module-1","port_id":"tx-1"}}
```

```json
{"message_id":51,"message_type":"get_sender_sdp","data":"v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=Modeled RTP session\r\nt=0 0\r\na=group:DUP main spare\r\nm=video 5004 RTP/AVP 111\r\nc=IN IP4 239.1.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:main\r\nm=video 5006 RTP/AVP 111\r\nc=IN IP4 239.2.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:spare\r\n"}
```

The SDP origin is a modeled identifier, not the sender's physical source address.
The generated SDP omits source filters because the sender domain does not contain
observed source IPs. Do not infer real transmission from the illustrative origin.

## 3. Apply it to the receiver

The explicit sources below are modeled addresses for the two known interfaces.
When a supplied media-level inclusive `source-filter` is present, the parser
requires it to match the corresponding explicit source and destination.

```json
{"message_id":52,"message_type":"set_ipt_configurations","data":{"module_id":"module-1","receiver_configs":[{"id":"rx-1","transceiver_type":"receiver","enabled_config":{"main_ip_source":{"source_address":"192.0.2.10","destination_address":"239.1.1.1","udp_port":5004,"network_interface":{"module_id":"module-1","port_id":"net-main"}},"spare_ip_source":{"source_address":"198.51.100.10","destination_address":"239.2.1.1","udp_port":5006,"network_interface":{"module_id":"module-1","port_id":"net-spare"}},"format_attributes":{"rtp_payload_type":111},"sdp":"v=0\r\no=- 1 1 IN IP4 127.0.0.1\r\ns=Modeled RTP session\r\nt=0 0\r\na=group:DUP main spare\r\nm=video 5004 RTP/AVP 111\r\nc=IN IP4 239.1.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:main\r\nm=video 5006 RTP/AVP 111\r\nc=IN IP4 239.2.1.1/64\r\na=rtpmap:111 H264/90000\r\na=mid:spare\r\n"}}]}}
```

```json
{"message_id":52,"message_type":"set_ipt_configurations","data":{"sender_config_errors":[],"receiver_config_errors":[]}}
```

Both legs are stored and read back together. Reordering the SDP media sections or
attributes normalizes to the same observation; a configuration check must not
report drift solely for those order changes. Changing group ID order changes the
local main/spare mapping and must still match the explicit path fields.

## 4. Check, repair and disable

Use the receiver configuration above in `check_ipt_configuration.data.config`.
`set_again: false` compares actual simulator state; `true` reapplies and reads back
only on drift. A failure to apply/confirm is a correlated error, not unconditional
success. The automated test independently removes state and exercises this path.

```json
{"message_id":53,"message_type":"set_ipt_configurations","data":{"module_id":"module-1","sender_configs":[{"id":"tx-1","transceiver_type":"sender","enabled_config":null}],"receiver_configs":[{"id":"rx-1","transceiver_type":"receiver","enabled_config":null}]}}
```

Disabling removes both legs for that resource; sender SDP then returns null.
For a single-stream/highest-resilience control-state example, omit the spare and
use one SDP media section without a group. No automatic failover is simulated.

## Negative cases and bounds

| Input | Outcome |
| --- | --- |
| Missing, duplicate or unknown group/mid ID; session-scoped mid | Rejected before apply |
| Wrong spare destination, UDP port or source filter | Per-port configuration failure; existing target state preserved |
| Mismatched encoding/clock/payload; unsupported group type | Rejected rather than guessing duplicate semantics |
| Exact duplicate full main/spare path | Rejected; equal destinations on distinct interfaces can remain valid |
| More than two media sections, inherited group connection/filter | Outside this subset; rejected |
| SRT/FEC with SDP; raw video/fmtp; audio DUP; inline keys/crypto | Unsupported here, not a claim those standard combinations are invalid |
| Over 64 KiB, 256 nonempty lines or 2 KiB per line | Bounded parser rejection |

Run the actual simulator round trips, mutation failures, normalization, IPv4/IPv6,
drift/repair and concurrent coherent-snapshot checks:

```sh
cargo test --locked --test demo_device
# Optional independent-transport, IPv4/IPv6 and fault cases:
cargo test --locked --features device-lab --test sdp_redundancy
```

This is local document-derived behavior. Full ST 2110 format/fmtp/PTP modeling,
physical redundancy and release-specific SDP acceptance are separate gates.
