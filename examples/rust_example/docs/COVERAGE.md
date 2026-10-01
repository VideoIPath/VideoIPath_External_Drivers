# Capability and evidence ledger

This ledger covers the later documented SPEC-23 v1 capabilities and important
variants. Stable row IDs identify gaps as well as implemented behavior. A parsed
field is not necessarily an applied capability, and a local test is not target
acceptance. The example models control-plane state, not media.

**Target status for every row: deferred.** No live VideoIPath instance, exact
release/backend SHA, formal external JSON Schema or accepted release fixture was
supplied. The reference material describes human-readable payloads; these Rust
models are derived from them. Actual check execution belongs in
[VALIDATION.md](../VALIDATION.md). Test references below mean test code exists and
has the stated scope, not that this document certifies its latest execution.

## How to read the tables

**0.3 layout:** the default driver uses [DemoDevice](../src/demo.rs), not a second
service. API handlers, registry and wire coverage remain shared. T10, T18, T19,
T21, the transport portions of T22, and T23 require `--features device-lab` and
exercise **optional test equipment**, not the default executable. Their prior
measurements do not qualify the lean driver. T24/T25 cover the new default path.

| Mark | Meaning |
| --- | --- |
| **M** | Modeled application/behavior exists in current source; no physical-media claim |
| **P** | Parsing, validation or metadata only; not complete behavioral coverage |
| **G** | Missing implementation, fixture, or important variant |
| **B** | Blocked by a concrete missing wire-contract/target evidence question |
| **O** | Outside this driver's scope |

Source abbreviations are links: [R = registry](../src/registry.rs),
[P = protocol](../src/protocol.rs), [A = application](../src/application.rs),
[D = domain](../src/domain.rs), [U = optional Unix adapter](../tools/device_lab/client.rs),
[S = optional simulator](../tools/device_lab/simulator.rs), [W = session](../src/session.rs),
[M = main/supervisor](../src/main.rs), [C = config](../src/config.rs),
[TLS](../src/tls.rs), [L = node lock](../src/identity.rs).

Runnable teaching entry points: [offline API walkthrough](SCENARIOS.md),
[wire frames](PROTOCOL.md), and [extension checks](EXTENDING.md#evidence-to-add-with-an-extension).
`cargo run --locked --example api_walkthrough` supplies complete requests directly
to handlers without a backend. The connected driver receives them over WebSocket.

### Test index

For a first API lesson, start with
[`readme_gain_frames_follow_observed_device_state`](../tests/documented_frames.rs):
it runs the README's read/write/notification frames against in-memory device state
and checks that an unchanged write produces no second change.
The index below is for deeper coverage review, not required reading before adapting
[one real parameter](EXTENDING.md#first-increment-one-real-parameter).

Names refer to exact test functions. Several rows intentionally share a test;
their gap column narrows what that test actually establishes.

| Key | Test location / functions |
| --- | --- |
| T1 | [capabilities.rs](../tests/capabilities.rs): `decode_unknown_fields_and_usable_correlations`, `unsupported_and_malformed_requests_are_sanitized_correlated_errors` |
| T2 | [capabilities.rs](../tests/capabilities.rs): `catalog_all_scopes_complete_maps_and_defaults`, `structure_duplicates_missing_refs_and_orders` |
| T3 | [capabilities.rs](../tests/capabilities.rs): `reads_selected_empty_unknown_hidden_and_snapshot_failure` |
| T4 | [capabilities.rs](../tests/capabilities.rs): `partial_set_types_access_steps_changed_only_and_action`, `scope_identity_and_uncertain_write_readback` |
| T5 | [capabilities.rs](../tests/capabilities.rs): `alarms_all_severities_defaults_authoritative_clear_and_events` |
| T6 | [capabilities.rs](../tests/capabilities.rs): `ipt_enable_disable_compare_drift_repair_and_failures`, `ipt_partial_errors_duplicate_pairs_and_multi_vertex` |
| T7 | [capabilities.rs](../tests/capabilities.rs): `sender_sdp_null_string_unsupported_and_unknown` |
| T8 | [capabilities.rs](../tests/capabilities.rs): `fec_srt_streaming_and_format_validation_normalization` |
| T9 | [capabilities.rs](../tests/capabilities.rs): `redundancy_partner_refs_and_receiver_sdp_cross_checks` |
| T10 | [device_lifecycle.rs](../tests/device_lifecycle.rs): state/restart, scoped values/actions/drift, alarms, rejection/disconnect, immediate replies/events, cancellation/no replay, timeout/waiters, socket permissions, partial-frame handling, admission, oversize frames, canceled pre-apply generations, escaped alarm capacity, truthful sender SDP and per-parameter delayed-event freshness |
| T11 | [session_lifecycle.rs](../tests/session_lifecycle.rs): idle/no ping, application traffic vs pong/events, exact keepalive, unhealthy target, overload, slow writer, cancellation, response/event order, panic/deadline, envelope/IDs and log redaction |
| T12 | [tls_session.rs](../tests/tls_session.rs): custom CA/header/URL, untrusted CA, hostname mismatch, HTTP 401, transient classification and sanitized invalid CA |
| T13 | [config.rs](../src/config.rs), [identity.rs](../src/identity.rs), [deployment_contract.rs](../tests/deployment_contract.rs): configuration, local singleton, environment/packaging contract |
| T14 | [capabilities.rs](../tests/capabilities.rs): `optional_metadata_is_validated_as_a_complete_map`, `documented_format_enum_choices_are_modeled_not_just_deserialized`, `srt_modes_encryption_and_network_negative_cases`, `empty_arrays_defaults_and_batch_boundaries_do_not_apply_invalid_requests` |
| T15 | [capabilities.rs](../tests/capabilities.rs): `full_width_json_integer_ids_keep_success_and_error_correlation`, `configuration_orders_are_observed_at_the_device_boundary`, `gpio_access_variants_and_every_scope_route_to_the_canonical_identity`, `advertised_media_and_non_null_format_resource_references_stay_consistent` |
| T16 | [capabilities.rs](../tests/capabilities.rs): `receiver_sdp_validation_does_not_depend_on_optional_attributes`, `sdp_supported_forms_and_explicitly_unsupported_semantics`, `unknown_json_object_fields_are_ignored_without_weakening_known_field_validation` |
| T17 | [session.rs](../src/session.rs): `private_revision_never_leaks_onto_spec_wire`, `stale_delayed_event_retires_instead_of_overwriting_confirmed_state`, `mutating_requests_hold_the_lane_through_causal_delivery`, `reconnect_jitter_is_bounded_and_saturates` |
| T18 | [native_end_to_end.rs](../tests/native_end_to_end.rs): `native_driver_tls_simulator_control_restart_and_sequential_smoke` — actual driver binary, ephemeral TLS backend, independent simulator, sanitized traces, singleton/restart and 500 keepalives plus 500 fresh reads |
| T19 | [load.rs](../tests/load.rs): `full_simulator_capacity_bulk_apply_and_repeated_cyclic_checks` — 128 modeled resources, ten check cycles, overflow rejection preserving existing state |
| T20 | [boundaries.rs](../tests/boundaries.rs): all eight audio-pair slots, integer endpoints/neighbours, null/default distinctions, streaming/FEC matrix, SRT modes/key-size/transport bounds, VLAN/UDP and indices 0/1/64/65 |
| T21 | [resource_choices.rs](../tests/resource_choices.rs): structural duplicate/link-scoped IDs; two receiver partners applied, independently read back, checked and disabled; unsupported selection preserves previous state |
| T22 | [sdp_redundancy.rs](../tests/sdp_redundancy.rs): paired H264/H265 generation and receive/readback, IPv4/IPv6, mid/group mapping, section normalization, drift/repair, coherent snapshots and negative cases; [documented_frames.rs](../tests/documented_frames.rs) checks walkthrough configurations |
| T23 | [soak.rs](../tests/soak.rs): automatic two-cycle regression and ignored elapsed workload; actual driver/simulator children, loopback TLS, reads/writes/events/alarms/IPT, backend and target recoveries, bounded histogram/RSS checks; [measured scope](SOAK.md) |
| T24 | [demo_device.rs](../tests/demo_device.rs): default in-memory adapter through API requests, readback, scoped values/actions, partial errors, alarm events/snapshots, SDP and IPT drift/repair; no physical-device claim |
| T25 | [native_demo.rs](../tests/native_demo.rs): actual lean binary with local TLS; authentication, response/event ordering, teaching signal/alarm, reconnect persistence, restart reset, shutdown and redaction |

## Transport, envelope and compatibility

| ID | Capability / variants | Implementation and example | Local tests and remaining gaps |
| --- | --- | --- | --- |
| TR-01 | Client WebSocket; lower-case `/x-driver/v1`; virtual IP; HTTP 101 | **M** C/TLS/M; [transport](PROTOCOL.md#transport-and-envelope) | T12 real loopback TLS/WS; target virtual-IP deployment deferred |
| TR-02 | Bearer header; 401 invalid auth; token absent from URL | **M** C/TLS/W | T12 positive/header and negative 401; no real token used |
| TR-03 | System trust, custom CA, certificate/hostname checks | **M** TLS; [ownership](../DESIGN.md#ownership-and-failure-rules) | T12 trust success, untrusted CA, hostname mismatch, invalid material |
| TR-04 | Stable UUID across reconnect/restart; same-host singleton | **M** C/L/M; provisioned identity | T13 lock/config; T18 actual duplicate-process rejection and identity-preserving restart; no cross-host ownership claim |
| TR-05 | Revocation closes shared-token sessions; duplicate-node takeover | **G** backend behavior, not implemented by driver | No target fixture; local singleton cannot prove backend takeover/revocation |
| TR-06 | Transient reconnect; capped backoff/jitter; permanent failure | **M** M/W; readiness cleared before backoff | T12 classification, T11 generations, T17 bounded/saturating jitter and T18 process restart; not a full real-backend outage/revocation matrix |
| EN-01 | Integer IDs and exact response correlation | **M** P/A/W; i128 DTO preserves both i64/u64 JSON IDs; [frames](PROTOCOL.md) | T1/T11/T15 full-width success/error and malformed-frame correlation; monotonicity/duplicate-ID enforcement absent |
| EN-02 | Null async IDs; successful type echo; correlated error string | **M** P/A/W | T1/T4/T5/T11; null request IDs rejected except incoming errors |
| EN-03 | Incoming `error` logged without reply, including null ID | **M** W | T11 log/error-loop tests; sanitized payload omission |
| EN-04 | Unknown request type, malformed required fields | **M** P/A/W | T1/T11 positive correlation and invalid cases |
| EN-05 | Invalid JSON, binary frames, unusable IDs, oversize | **M** W/U | T1/T10/T11 rejection and retirement |
| EN-06 | Unknown fields ignored; default arrays/maps and optional values | **P/M** Serde and P translators | T1/T8/T14/T16 nested extra fields, empty streaming extensions, empty arrays and optional ranges; not every field/default combination |
| EN-07 | Future enum cases, superset evolution, older payload forms | **P/G** known enum cases accepted; unknown known-field enums rejected | No speculative `/extDriver/v1`, map-IPT, `sender_id` or `dynamic_values` aliases; release-specific compatibility fixture missing |
| EN-08 | Async universal errors | **G** receive-side silence exists; no driver-originated async-error facility | SPEC says backend ignores these; local diagnostics remain authoritative |
| KA-01 | Exact keepalive echo including extra fields | **M** W; [keepalive frame](PROTOCOL.md#transport-and-envelope) | T11 and session unit test use original text |
| KA-02 | Keepalive independent of stalled handler; unhealthy target closes | **M** W/M; default checks model availability, optional U has a transport probe | T11 failure/loss fixtures; T25 default native path; a physical adapter must supply actual contact health |
| KA-03 | 60-second inbound idle; no autonomous pings | **M** W | T11 paused-clock tests; outbound events and transport pong do not reset idle |
| RT-01 | 10-second ordinary / 30-second IPT backend ceilings | **M** local work defaults 5/20 s, separate bounded delivery | T11 operation deadlines; not one absolute end-to-end budget; no performance guarantee |
| RT-02 | Bounded writer, handlers, target operations, events; teardown | **M** W/U/S/M | T10/T11 saturation/cancellation, T17 transaction lane, T18 native smoke, T19 capacity workload; long-duration soak and quantitative queue/memory baseline remain separate |

## Structure and topology metadata

| ID | Capability / variants | Implementation and example | Local tests and remaining gaps |
| --- | --- | --- | --- |
| ST-01 | Root `label`, `version`, `product_name`, optional `access_url` | **M/P** R emits first three; version comes from package metadata; access URL omitted | T2/T15/T18 response/version; non-null access URL and complete default-decoding fixture absent |
| ST-02 | Modules, labels, stable list-scoped IDs, empty lists/maps | **M** R fixed two-module registry; [structure](PROTOCOL.md#structure-and-identity) | T2/T14 validate metadata/maps/array shapes and duplicates; T15 display label; T18/T25 restart identity; not a formal schema |
| ST-03 | `senders_first`, `receivers_first`, `all` | **M** C/R/A startup `DRIVER_CONFIGURATION_ORDER`; all three applied | T15 observes device call order and invalid option; T18 receivers-first binary configuration. `all` is sequential, not atomic/parallel; cross-module scheduling remains backend-owned |
| ST-04 | Sender/receiver `id`, `label`, `dynamic_values`, topology | **M** R | T2 generated map/scopes; no configurable structure selector |
| ST-05 | Format type `Video`, `Audio`, `ASI`, `Ancillary`, null | **P/M** R includes each; P stores accepted control fields | T2 fixture validation; format-specific transport/media behavior is not modeled |
| ST-06 | `topology_info: null` means no graph vertex | **M/P** R `tx-monitor`; P rejects IPT | T2 invalid resource endpoint test; backend vertex absence needs target evidence |
| ST-07 | `format_id` null/non-null; default resource identity derivation | **P/M** `raw-video` on tx-1/rx-1, null on other fixture ports | T15 verifies link endpoint format IDs and invalid type; backend resource-ID derivation still unverified |
| ST-08 | Sender SDP read flag / receiver SDP set flag; false default | **M, limited** R/P/A/S on `tx-1`/`rx-1`/`rx-audio` | T7/T9/T10/T15/T16/T22; truthful single-stream and paired H264/H265 DUP subset; full ST 2110 unsupported |
| ST-09 | `sips` true/false/default; interface options | **P/M** R advertises true for routable ports; P accepts main/spare | T9 spare validation; no packet protection or exhaustive false/default fixture |
| ST-10 | Main/spare IP overrides and precedence | **B** non-null overrides explicitly rejected, not guessed | T14 negative invented shape. **Missing concrete wire shape** in supplied SPEC text; obtain target DTO/schema or accepted frame before implementation |
| ST-11 | Generic port with dynamic values and no routing vertex | **M/P** R `fan-1` | T2 scope read; backend topology result deferred |
| NI-01 | Single interface: type/id/label/address/dynamic values | **M/P** R `net-aux` and pair members | T2 validates fixture; malformed-interface schema coverage incomplete |
| NI-02 | `redundant_pair`, `interface_1`, `interface_2` | **P** R main/spare pair | T2 duplicate member rejection; Highest Resilience backend routing not tested |
| NI-03 | Known address and null default | **P** R documentation-range IPv4 plus null auxiliary address | T2; manual topology/override routing prerequisites not simulated |
| NI-04 | Cross-module references, missing references, duplicate IDs | **M** R/P | T2 valid cross-module structure and negative references/duplicates; routed acceptance deferred |

## Dynamic values, sensors and GPIO

| ID | Capability / variants | Implementation and example | Local tests and remaining gaps |
| --- | --- | --- | --- |
| DV-01 | Root/module/sender/receiver/generic/interface locations | **M** D/R/A/S; [structured frames](PROTOCOL.md#structure-and-identity) | T2/T15 reads/writes/events across every registered scope, including both receiver types; T4/T10 scoped independence |
| DV-02 | Repeated local IDs at different locations | **M** R repeated `enabled` | T15 confirms untouched siblings for each scoped write; T4/T10 focused cases |
| DV-03 | Definition label/description; enumerated string required `values` | **M/P** R `mode` and complete-map validator | T2/T4 positive choice; T14 missing/non-string enum values rejected; empty-choice runtime policy not exercised; unrestricted strings not implemented |
| DV-04 | Boolean type; strict rejection of numeric/string substitutions | **M** R | T4/T10 negative type cases |
| DV-05 | Number integer/fractional; finite bounds, from/to/step/unit | **M** R `level`, `audio_gain`, `temperature`; integer-only values reject float-form JSON | T2/T4 valid defaults, unit and step/type failures; floating-point boundary matrix incomplete |
| DV-06 | Optional number range absent / null / populated | **P/M** R emits null/full ranges; validates all documented optional forms | T14 complete maps with absent/null range and absent/null step/unit; rejects partial/inverted range and zero step; target decoder still deferred |
| DV-07 | Read-only with GPI false/true | **M** temperature GPI false; `signal_present` GPI true | T15 verifies metadata and backend write rejection; independent control changes remain observable |
| DV-08 | Read/write with GPI/GPO combinations | **M/P** enabled true/true, numeric values false/false, mute false/true | T15 mute/enable metadata and changes; read/write true/false fixture remains missing |
| DV-09 | Write-only; assumed GPO; no readback | **M** trigger invocation counter | T3/T4/T10; explicit read fails whole request, action emits no value event |
| DV-10 | Requested IDs; unknown omitted; no IDs; empty IDs | **M** A one snapshot; [read frames](PROTOCOL.md#dynamic-values) | T2/T3. Empty means none locally; target confirmation needed. Hidden values excluded from all-values read |
| DV-11 | Fresh coherent sensor snapshot and read failure | **M** A/U; no sensor poll loop | T3 failure rather than stale sample; no physical sensor |
| DV-12 | Complete successful write and changed-only notification | **M** A/R/S | T4/T10; response-before-causal-event T11 |
| DV-13 | Mixed per-ID failures: unknown, type, range, access | **M** A direct map | T4/T10 positive/negative cases; no `failures` wrapper |
| DV-14 | Normalized values, uncertain outcome and observed change | **M, partial** A reports mismatch and can reconcile failed set via read; W serializes backend mutations | T4 ambiguous applied write/no retry; T10 pre-apply cancellation and stale observations; general vendor normalization/history not modeled |
| DV-15 | Canonical `async_values_changed`, null ID, scoped values | **M** P/A/U/S/W | T4/T10/T11; never emits guide-simulator alias `dynamic_values` |
| DV-16 | Changed-only dedup and causal ordering | **M, bounded** mutation RwLock held through causal delivery; private per-ID write floors checked at actual event dispatch | T4/T10/T11/T17 stale relay, scope isolation, reads not advancing floors, wire metadata stripping and mutation ordering; T18 actual independent event trace. Stale/concurrent conflicts reconnect; no full event-history reconciliation |
| DV-17 | Event bursts/backpressure, GPI delivery | **M, partial** subscribe before handshake; bounded queues retire on lag/overflow | T11 event saturation; T10 unrelated GPI/alarm observations not suppressed by value floors; full target consumer/burst-recovery acceptance deferred |
| DV-18 | Unadvertised addresses | **M** only `sim.user.1` / `sim.user.2` validated namespace | T3 hidden read; unknown writes T4. No generic URL/OID/file access |
| DV-19 | Parameter/GPIO catalog vs runtime consumer projection | **B/G** canonical structure/runtime implemented | No target collector/Panel decoder or saved-reference fixture; metadata acceptance is a separate gate |

## Alarms

| ID | Capability / variants | Implementation and example | Local tests and remaining gaps |
| --- | --- | --- | --- |
| AL-01 | Full snapshot, empty snapshot, missing-from-poll clears | **M** A/DemoDevice/S; [alarm exercise](SCENARIOS.md#observe-unsolicited-updates-when-connected) | T5/T10/T24/T25 snapshots and removal; target's clear processing deferred |
| AL-02 | Scoped identity `id + location`, details | **M** D/S keyed pair | T10 root lifecycle; docs scoped interface example; repeated alarm IDs across every scope not exhaustively tested |
| AL-03 | notification/minor/warning/major/critical | **M** D | T5 all enum values; invalid severity rejection follows Serde but dedicated negative fixture missing |
| AL-04 | Service affecting true/false/default true | **M** D | T5 default true; false application/consumer effect needs explicit test |
| AL-05 | Async raise/refresh and clear | **M** S/P; equal refresh deduplicated | T5/T10 raise/clear; changed-details refresh variant not explicitly covered |
| AL-06 | Async poll trigger or polling-only alternative | **M/P** `poll-alarms`; snapshot request supported | T5/T10 token; no mode selector or backend one-second scheduling test |
| AL-07 | Snapshot failure and significant transition overload | **M, partial** errors propagated; invalid/duplicate snapshot identities rejected; escaped-byte capacity checked before mutation | T10 oversize alarm admission preserves previous pollable snapshot/revision; T11 generic event saturation; target transition recovery remains deferred |

## SDP, IPT and resources

| ID | Capability / variants | Implementation and example | Local tests and remaining gaps |
| --- | --- | --- | --- |
| SD-01 | Sender request `module_id`, `port_id`; string/null/error | **M** A/S; [SDP frames](PROTOCOL.md#ipt-configuration-and-sdp) | T7 fake handler and T10 actual simulator; pending-generation state not distinct |
| SD-02 | Single/dual-stream sender SDP | **M** S supports single/paired H.264/H.265 with explicit dynamic payload and DUP mids; rejects FEC/SRT and unrepresentable format state | T10/T22 actual codec/payload/TTL, IPv4/IPv6 paired round trips; constrained modeled SDP, not a media-duplication engine |
| SD-03 | Receiver SDP apply, unsupported/invalid rejection | **M, limited** P one RTP/AVP L16/L24 audio section on rx-audio, H264/H265/MP2T video on rx-1 | T9/T16 positive forms; actual simulator sender-to-receiver H264 SDP/readback round trip; malformed codec/clock/channels, media-type mismatch, payload rules and unsupported syntax |
| SD-04 | SDP source/format consistency | **M, bounded** address/UDP/payload/audio/source-filter checks, CRLF/group-section normalization | T9/T16/T22; first/second group ID maps to main/spare as simulator policy; raw video/fmtp, arbitrary groups, audio DUP and SRT/FEC+SDP remain unsupported |
| IP-01 | Module batch arrays; sender and receiver; empty default arrays | **M** P/A | T6/T14 mixed/empty/omitted arrays; [batch frames](PROTOCOL.md#partial-failure-and-advanced-forms) |
| IP-02 | Per-config error arrays `id`, `sub_index`, `error`; duplicates ignored | **M** A | T6 mixed success/failure and duplicates |
| IP-03 | Whole-request malformed identity/module/capacity failure | **M** A bound 256 | T1/T14 malformed identity and over-limit batch rejected before application; unknown-module batch needs a dedicated test |
| IP-04 | `transceiver_type`, ID, null/default sub-index, null disable | **M** P/A/S | T6 enable/disable/multiplicity; invalid direction not exhaustively tested |
| IP-05 | Sender main/spare destination, address, UDP, VLAN, interface | **M** P/S | T6 multicast/main, T8 unicast, T9 spare, T14 invalid address/UDP/VLAN/interface; IPv6 and complete boundary matrix remain gaps |
| IP-06 | Receiver main/spare source, source/destination addresses, UDP/VLAN/interface | **M** P/S | T6/T9 main; receiver-spare/source-family/invalid-VLAN test matrix incomplete |
| IP-07 | Sender format_info and receiver format_attributes | **M/P** normalized retained fields | T8/T9/T14/T16 explicit retained enum values and bounded SDP consistency; full enumeration ledger below; no encoding |
| IP-08 | Receiver fec_enabled, srt, sdp, informational source_meta | **M/P** FEC flag/SRT/SDP modeled; source_meta ignored | T9/T16/T22 bounded SDP including spare; SDP plus SRT/FEC rejected; T20 non-SDP SRT bounds/modes; no provenance display |
| IP-09 | Sender streaming: QoS, TS packet length/count/rate, IP rate, TTL | **M/P** P eight-bit QoS; packet 188/204; positive count/TTL | T8/T20 both packet lengths, exact integer endpoints, malformed types and immediate out-of-range cases |
| IP-10 | FEC length/depth/skew/overhead; single/matrix | **M/P** P/S; depth null vs positive; overhead 0–100 | T8/T20 null/single/matrix, both skew values, full integer endpoints and 0/100 overhead with negative neighbours; no FEC packets |
| IP-11 | SRT caller/listener, buffer latency, overhead bandwidth | **M/P** P/S; multicast rejected | T14/T20 both modes/directions, latency and overhead endpoints/neighbours, invalid types and multicast rejection |
| IP-12 | SRT encryption absent / AES128 / AES192 / AES256 and key | **M/P** P/D; bounded nonempty key, redacted Debug | T14 all algorithms; T20 synthetic key lengths 0/1/1024/1025 in both directions/modes; simulator policy, not real SRT passphrase/media validation |
| CK-01 | Check actual device state, supported-field normalization | **M** A/U/S | T6 fake drift, T10 independent simulator drift, T18 actual binary repair trace and T19 repeated full-capacity checks; never unconditional true |
| CK-02 | set_again false/true; failed repair; failed reread match | **M** A | T6 false, successful repair, rejected/ignored apply; T18 binary drift/repair; transport readback failure matrix incomplete |
| CK-03 | Backend cyclic schedule (minute / minimum second spacing) | **O** backend owns scheduling | No driver poll loop; target scheduling unverified |
| RL-01 | `rx-tx`, valid endpoints with topology, chosen partner | **M** R two choices (rx-1/rx-alt → tx-1); P validates selected receiver | T2/T9 refs; T21 real Unix adapter applies and independently reads back each indexed choice; no physical internal routing |
| RL-02 | Choice links, duplicates, format-derived resource IDs | **M/P** choices share raw-video endpoint format IDs; duplicate links validate | T15/T21 identity/reference/duplicate tests; target owns graph/resource-ID derivation, not calculated locally |
| RL-03 | `tx-tx` links and selected resource_partner semantics | **P/B** validator accepts link kind; runtime partner is receiver-only | **Ambiguous partner semantics:** SPEC shows receiver-shaped partner and newer sender-to-sender links without a complete selection rule. No fabricated extension |
| MV-01 | Null single segment vs sub-index 1..N; independent lifecycle | **M** R limits N to 64; S keyed state; disable removes key | T6/T15/T19 resource lifecycle/capacity; T20 explicit null/0/1/64/65/type boundaries; T21 partner choice and independent removal |
| MV-02 | Inventory multiplicity setup and dynamic physical resources | **G/O** topology setup external; simulator creates/removes state entries only | No VideoIPath topology automation, physical allocation or sender-SDP sub-index extension |

## Format field and enum inventory

All rows below refer to [protocol.rs](../src/protocol.rs) `format` / `attributes`
and [domain.rs](../src/domain.rs) `Format`. Values are validated and retained in
modeled state; this is **P/M control-state coverage**, never a codec/media claim.
T8/T9 provide representative fixtures; T14 explicitly checks retained enum values,
including video-codec × entropy × GOP combinations. These are domain-model tests,
not codec execution or acceptance of those encodings in the limited SDP subset.
No row has target acceptance.

| ID | Documented fields / cases | Local evidence and gap |
| --- | --- | --- |
| FM-01 | `audio_pairs`: G1P1, G1P2, G2P1, G2P2, G3P1, G3P2, G4P1, G4P2; each null or `{codec, bitrate}` | T20 all eight populated and individually nulled, bitrate endpoints/neighbours/types; unknown pair keys ignored |
| FM-02 | Audio codecs: MPEG-1 Layer 2, AAC_LC, AAC_HE, SMPTE 302M, Dolby E, Dolby Digital | T14 checks every codec retained in G1P1; unsigned bitrate retained without encoding |
| FM-03 | `video_codec.codec`: H.264, H.265, MPEG-2, AVC-I 50, AVC-I 100, J2K, TICO, JPEG XS | T14 checks every codec with both entropy choices and all GOP structures; only H.264/H.265 have sender SDP support |
| FM-04 | `entropy_encoding`: CAVLC, CABAC | T14 checks both in the codec/entropy/GOP combination matrix |
| FM-05 | `gop_struct`: Automatic, IP, IPB, IPBB, IPBBB, IPBBBB; `gop_size` | T14 all structures; T20 zero/u32 maximum retained and adjacent overflow, negative and boolean rejected |
| FM-06 | Audio `bit_depth`: L16, L20, L24, L32, AM824 | T14 checks all retained values; receiver SDP supports only L16/L24 |
| FM-07 | Audio `frame_size`, `n_channels`, `sample_rate`, `link_offset`; nullable optional values | T20 null/absent, each minimum/u32 maximum, overflow/negative/bool/fraction; channel/rate zero rejected; modeled state, not physical audio constraints |
| FM-08 | Video `colorimetry`: BT601, BT709, BT2020, BT2100 | T14 checks each retained value and rejects an unknown case |
| FM-09 | Video `bit_depth`: 8, 10, 12, 16, 16f (strings) | T14 checks each retained value and rejects an unknown case |
| FM-10 | `exact_framerate`: 23.97, 24, 25, 29.97, 30, 47.95, 48, 50, 59.94, 60 (strings) | T14 checks each documented spelling and rejects an unknown case |
| FM-11 | `packed_mode`: 2110GPM, 2110BPM | T14 checks both retained values and rejects an unknown case |
| FM-12 | `resolution`: 480i, 576i, 720p, 1080p, 1080i, 1080p 2k, 2160p, 2160p 4k | T14 checks each retained value and rejects an unknown case |
| FM-13 | `sampling`: 4:2:2, 4:2:0, 4:4:4 | T14 checks each retained value and rejects an unknown case |
| FM-14 | `tcs`: SDR, PQ, HLG, SLOG3 | T14 checks each retained value and rejects an unknown case |
| FM-15 | `tp`: 2110TPNL, 2110TPN, 2110TPW | T14 checks each retained value and rejects an unknown case |
| FM-16 | `ptp_domain`, `rtp_payload_type` | Local bounds 0–255 / 0–127; T8/T20 exact endpoints and max+1 rejection; no PTP or RTP engine |
| FM-17 | Empty/default format objects; irrelevant/unknown fields ignored | T6 empty format, T8/T16 nested extra-field normalization and strict known-field rejection; absent/null/value combinations not exhaustive for every nested field |

## Explicit boundaries and next evidence

| ID | Boundary | Disposition |
| --- | --- | --- |
| NG-01 | Switch programming / network configuration calculation | **O** not part of this proxy |
| NG-02 | VideoIPath connections, endpoint groups, database access or another driver's state | **O** no core/northbound automation in the driver |
| NG-03 | Firmware upgrade, backup/restore, maintenance and generic filesystem access | **O** not exposed |
| NG-04 | New protocol metrics messages, arbitrary custom parameter schemas | **O** sensors use normal dynamic values |
| NG-05 | Automatic active/active, leader election, management-interface redundancy | **O** singleton deployment; external supervision only |
| NG-06 | Physical media, FEC protection, encryption, codec quality or throughput | **O** simulator state only |
| EV-01 | Official schema or target-release decoder/capture | **B** obtain exact release/SHA and accepted/rejected sanitized frames |
| EV-02 | Discovery → Inventory → collector catalog → reads/writes → Panel → async | **B/G** commission each layer separately; local green tests cannot replace it |
| EV-03 | All supported ranges/enums/default forms and concurrency combinations | **G** explicit enum/metadata/order/scope matrices, native smoke and capacity workloads exist; remaining boundary and history combinations are not claimed complete |

The immediate clarification questions are narrow: what are the exact non-null
main/spare IP-override payload types, and how does the intended release identify
a selected partner for a `tx-tx` link? Also confirm empty-ID read semantics and
complete optional-metadata conversion on that release. Keep answers as sanitized,
version-scoped fixtures and update the relevant rows rather than adding guessed
wire aliases or broad claims about API maturity.

See [remaining wire evidence](WIRE_EVIDENCE.md) for the completed local search,
the deliberate rejection boundaries, and the minimal sanitized evidence needed.
