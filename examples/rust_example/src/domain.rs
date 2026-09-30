//! Vendor-operation seam used by the handlers in `application.rs`.
//! Start at [`Device`] when replacing the simulator with a physical device API.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, future::Future, net::IpAddr};

pub type Values = BTreeMap<String, Value>;

/// Resolve parameters by (location, local ID), never the local ID alone.
/// Root is JSON null; module and port locations match the advertised structure.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Location(pub Option<ModuleLocation>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ModuleLocation {
    pub module_id: String,
    #[serde(default)]
    pub port_location: Option<PortLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortLocation {
    #[serde(rename = "type")]
    pub kind: PortKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    Sender,
    Receiver,
    Generic,
    NetworkInterface,
}

impl Location {
    pub fn root() -> Self {
        Self(None)
    }
    pub fn module(module: impl Into<String>) -> Self {
        Self(Some(ModuleLocation {
            module_id: module.into(),
            port_location: None,
        }))
    }
    pub fn port(module: impl Into<String>, kind: PortKind, id: impl Into<String>) -> Self {
        Self(Some(ModuleLocation {
            module_id: module.into(),
            port_location: Some(PortLocation {
                kind,
                id: id.into(),
            }),
        }))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Sender,
    Receiver,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PortKey {
    pub module_id: String,
    pub port_id: String,
    pub sub_index: Option<u32>,
    pub direction: Direction,
}

/// Fixed messages never interpolate an API error, supplied value, SDP or SRT key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    #[error("Invalid request")]
    Invalid,
    #[error("Unsupported capability")]
    Unsupported,
    #[error("Unknown parameter or resource")]
    Unknown,
    #[error("Read-only parameter")]
    ReadOnly,
    #[error("Write-only parameter")]
    WriteOnly,
    #[error("Invalid value type")]
    Type,
    #[error("Out of range")]
    Range,
    #[error("Device unavailable")]
    Unavailable,
    #[error("Device operation timed out")]
    Timeout,
    #[error("Device outcome is uncertain")]
    Ambiguous,
    #[error("Device state does not match")]
    Mismatch,
    #[error("Capacity exceeded")]
    Overloaded,
    #[error("Internal invariant failed")]
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Notification,
    Minor,
    Warning,
    Major,
    Critical,
}
fn service_affecting_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alarm {
    pub id: String,
    #[serde(default)]
    pub location: Location,
    pub details: String,
    pub severity: Severity,
    #[serde(default = "service_affecting_default")]
    pub service_affecting: bool,
}

/// Driver-originated changes are returned to the session, which delivers the
/// correlated response first. Independent local changes arrive via the adapter.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Values { location: Location, values: Values },
    Alarm { alarm: Alarm },
    AlarmCleared { location: Location, id: String },
    PollAlarms,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interface {
    pub module: String,
    pub port: String,
}

/// Main and spare paths share one normalized model. Sender source is absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stream {
    pub destination: IpAddr,
    pub source: Option<IpAddr>,
    pub udp_port: u16,
    pub vlan: Option<u16>,
    pub interface: Interface,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fec {
    pub columns: u32,
    pub rows: Option<u32>,
    pub skew: bool,
    pub overhead_percent: f64,
}

/// Deliberately redacts key material from Debug; serialization is for the private
/// simulator transport only and must never be used as a diagnostic dump.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Encryption {
    pub algorithm: String,
    pub key: String,
}
impl std::fmt::Debug for Encryption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Encryption([redacted])")
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Srt {
    pub caller: bool,
    pub latency_ms: u32,
    pub overhead_percent: u32,
    pub encryption: Option<Encryption>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Streaming {
    pub dscp_byte: u8,
    pub packet_length: u16,
    pub packets_per_frame: u16,
    pub transport_rate: u64,
    pub ip_rate: u64,
    pub ttl: u8,
}

/// Fields supported by this example's format mapping. Adapt both the request
/// mapping and vendor readback together so configuration checks compare like state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Format {
    pub attributes: BTreeMap<String, String>,
    pub audio_pairs: BTreeMap<String, (String, u32)>,
    pub video_codec: Option<(String, String, String, u32)>,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaState {
    pub main: Stream,
    pub spare: Option<Stream>,
    pub format: Format,
    pub fec: Option<Fec>,
    pub fec_enabled: bool,
    pub srt: Option<Srt>,
    pub streaming: Option<Streaming>,
    pub partner: Option<PortKey>,
    /// Normalized supported SDP lines, not the original request/frame.
    pub sdp: Option<String>,
}
impl std::fmt::Debug for MediaState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // SDP and transport configuration can contain private addressing or
        // credentials. Log capability presence, never the control payload.
        f.debug_struct("MediaState")
            .field("has_spare", &self.spare.is_some())
            .field("has_fec", &self.fec.is_some())
            .field("has_srt", &self.srt.is_some())
            .field("has_sdp", &self.sdp.is_some())
            .finish_non_exhaustive()
    }
}

/// Six operations chosen for this sample, not a trait mandated by SPEC-23.
/// Trim or split this seam for a smaller device and advertise only its capabilities;
/// unsupported requests must still receive an error rather than fake success.
/// Serialize conflicting vendor operations. Return observed state, and leave causal
/// value events to `application::set_values` so the session can send its response first.
pub trait Device: Send + Sync {
    /// Fetch current readable values for one advertised location from the vendor API.
    /// Use one coherent snapshot for related sensors; do not substitute defaults or
    /// stale data on failure. `application::dispatch` selects IDs for get_dynamic_values
    /// and turns a failed read into a correlated error, not a per-ID error object.
    fn read_values(
        &self,
        location: Location,
    ) -> impl Future<Output = Result<Values, Failure>> + Send;
    /// Set the addressed vendor parameter and return its observed value after readback.
    /// `application::set_values` validates input, records failures, and reports actual
    /// changes even when the device normalizes or only partly applies a request.
    /// For a write-only action, confirm acceptance without inventing readable state.
    /// If transmission may have occurred, report ambiguity instead of retrying; do
    /// not independently publish this write's causal event before its response.
    fn set_value(
        &self,
        location: Location,
        id: String,
        value: Value,
    ) -> impl Future<Output = Result<Value, Failure>> + Send;
    /// Read the full active vendor alarm list for the alarms branch in application::dispatch.
    /// Missing entries clear existing alarms. Return an error on read failure;
    /// an empty list means none active or intentionally unsupported alarm reporting.
    fn alarms(&self) -> impl Future<Output = Result<Vec<Alarm>, Failure>> + Send;
    /// Fetch SDP for the addressed sender in application::dispatch's get_sender_sdp branch.
    /// Some(text) is available SDP; None is temporary absence (disabled or pending).
    /// Unsupported capability or a failed vendor read is an error, not None.
    fn sender_sdp(
        &self,
        key: PortKey,
    ) -> impl Future<Output = Result<Option<String>, Failure>> + Send;
    /// Apply transport/format state through the vendor API; None must disable the port.
    /// `application::set_ipt` and the check_ipt_configuration branch read back afterward.
    /// Report unsupported requirements, partial failure, or an uncertain transmitted
    /// outcome truthfully; successful API acceptance alone does not prove device state.
    fn configure(
        &self,
        key: PortKey,
        state: Option<MediaState>,
    ) -> impl Future<Output = Result<(), Failure>> + Send;
    /// Read actual port enablement and supported transport/format fields from the device.
    /// Normalize them like protocol::media_state; None means observed disabled state,
    /// not unavailable data. Never echo a saved request. application::set_ipt verifies
    /// writes here; check_ipt_configuration uses the same read to detect and repair drift.
    fn configuration(
        &self,
        key: PortKey,
    ) -> impl Future<Output = Result<Option<MediaState>, Failure>> + Send;
}
