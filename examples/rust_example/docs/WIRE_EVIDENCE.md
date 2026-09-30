# Remaining wire evidence

The implementation deliberately does not guess these two payload contracts.
Local tests can prove rejection/absence policy, but cannot supply missing backend
semantics. Neither item is resolved by a generic JSON envelope schema.

| Open item | Established behavior | Missing evidence |
| --- | --- | --- |
| `main_interface_ip_overrides`, `spare_interface_ip_overrides` | Absent/null default; overrides take precedence over routed addresses | Populated container/member types and a release-matched accepted structure |
| Selected `resource_partner` for a `tx-tx` link | Structure uses `type: "tx-tx"` with sender endpoints; existing partner example explicitly identifies a receiver | Actual sender-partner selection rule and request, including overlapping sender/receiver IDs |

## Review performed

On 2026-09-30, the supplied documentation, repository guide, Python example, Rust
fixtures and vendor placeholders were rechecked. They contain neither populated
override examples nor a backend-generated `tx-tx` partner request. The available
positive structure fixture omits overrides and has no links. Its provenance lacks
a release/SHA. Existing invented override values are **negative tests**, not
evidence of a supported alternative representation.

The later documented baseline uses `/x-driver/v1`, array-based IPT/error collections,
`port_id`, and `async_values_changed`. Older map-based collections, `sender_id`, or
`/extDriver/v1` are not aliases to add speculatively. No official external JSON
Schema or release-matched backend decoder was supplied.

`rx-tx` selection is sufficiently documented: a sender names its chosen receiver
using `{module_id, port_id, sub_index}`. The example now advertises two such choices
and tests independent target readback. Its structural `tx-tx` tests validate only
endpoint references; no such link is advertised by the runtime fixture.

## Minimal evidence needed to unblock implementation

An operator or maintainer can supply a **sanitized** evidence package later, without
granting this driver northbound API access:

| Artifact | Required information |
| --- | --- |
| Version record | Exact VideoIPath release and backend SHA; relevant source/decoder version |
| Override model | Type definition or accepted populated `get_structure`, both main/spare variants, absent/null/empty behavior |
| Partner model | Backend-generated sender configuration for a selectable `tx-tx` route, disable/check forms and sub-index handling |
| Ambiguity case | A sender and receiver sharing one list-scoped ID; how the partner direction is determined |
| Acceptance record | Which decoder accepted/rejected each shape; Inventory and catalog result where available |

Remove tokens, cookies, device credentials, SRT keys, sensitive SDP fields and
private addresses/labels where necessary **before sharing**. Preserve the structural
types and reference relationships when replacing values. Never paste real token
output into chat or a fixture. A manually authored example without target provenance
is useful for discussion, not a compatibility result.

Until that evidence exists, populated overrides and unsupported selected partners
remain explicit errors. The [coverage ledger](COVERAGE.md) and
[target acceptance checklist](../VALIDATION.md#future-videoipath-acceptance) track
this separately from local implementation progress. Full ST 2110 SDP, real media,
and live Inventory/Panel commissioning are also not implied by the local fixtures.
