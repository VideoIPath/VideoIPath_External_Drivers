//! Edit [`Registry::new`] to describe your device for get_structure.
//! Parameter definitions and runtime validation share this catalog; vendor reads
//! and writes use the same (location, local ID) identities in `application.rs`.
use crate::domain::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
    WriteOnly,
}

#[derive(Debug, Clone)]
pub enum ValueType {
    Boolean,
    Choices(Vec<String>),
    Number {
        range: Option<(f64, f64, Option<f64>)>,
        unit: Option<String>,
        integer: bool,
    },
}

#[derive(Debug, Clone)]
pub struct Parameter {
    pub label: String,
    pub description: String,
    pub value_type: ValueType,
    pub access: Access,
    pub gpi: bool,
    pub gpo: bool,
    pub default: Value,
    pub advertised: bool,
}

impl Parameter {
    /// JSON booleans are never numbers; step alignment uses a small relative
    /// tolerance solely for floating-point representation, not device rounding.
    pub fn validate(&self, value: &Value) -> Result<(), Failure> {
        match &self.value_type {
            ValueType::Boolean if value.is_boolean() => Ok(()),
            ValueType::Choices(choices) => match value.as_str() {
                Some(v) if choices.iter().any(|c| c == v) => Ok(()),
                Some(_) => Err(Failure::Range),
                None => Err(Failure::Type),
            },
            ValueType::Number { range, integer, .. } => {
                let n = value.as_f64().ok_or(Failure::Type)?;
                if !n.is_finite() || (*integer && !value.is_i64() && !value.is_u64()) {
                    return Err(Failure::Type);
                }
                if let Some((low, high, step)) = range {
                    if n < *low || n > *high {
                        return Err(Failure::Range);
                    }
                    if let Some(step) = step {
                        let ticks = (n - low) / step;
                        if (ticks - ticks.round()).abs() > 1e-9 * ticks.abs().max(1.0) {
                            return Err(Failure::Range);
                        }
                    }
                }
                Ok(())
            }
            _ => Err(Failure::Type),
        }
    }

    /// Discovery metadata, not current state or a command to apply `default`.
    /// Match access and GPIO flags to vendor operations you can actually support.
    pub fn definition(&self) -> Value {
        let type_info = match &self.value_type {
            ValueType::Boolean => json!({"type":"boolean"}),
            ValueType::Choices(values) => json!({"type":"string","values":values}),
            ValueType::Number { range, unit, .. } => {
                json!({"type":"number", "range":range.map(|(from,to,step)| json!({"from":from,"to":to,"step":step,"unit":unit}))})
            }
        };
        let value_info = match self.access {
            Access::ReadOnly => json!({"type":"read_only","gpi_capable":self.gpi}),
            Access::ReadWrite => {
                json!({"type":"read_write","gpi_capable":self.gpi,"gpo_capable":self.gpo})
            }
            Access::WriteOnly => json!({"type":"write_only"}),
        };
        json!({"label":self.label,"description":self.description,"type_info":type_info,"value_info":value_info})
    }
}

#[derive(Debug, Clone)]
pub struct Registry {
    parameters: BTreeMap<(Location, String), Parameter>,
    structure: Value,
}
impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// Replace this sample catalog with the target's actual capabilities and limits.
    /// Keep the physical device's node UUID across restarts; choose module/port IDs
    /// from stable slots or vendor identities, unique within each list. Labels can
    /// change; IDs must not shift when another element is inserted or removed.
    pub fn new() -> Self {
        let mut registry = Self {
            parameters: BTreeMap::new(),
            structure: Value::Null,
        };
        let root = Location::root();
        let numeric = |label: &str, low, high, step, integer, default| Parameter {
            label: label.into(),
            description: "Modeled simulator parameter".into(),
            value_type: ValueType::Number {
                range: Some((low, high, Some(step))),
                unit: None,
                integer,
            },
            access: Access::ReadWrite,
            gpi: false,
            gpo: false,
            default,
            advertised: true,
        };
        registry.insert(
            root.clone(),
            "level",
            numeric("Level", 0.0, 100.0, 1.0, true, json!(50)),
        );
        // To adapt audio_gain, change its real range/step/unit here and map this
        // root ID in Device::read_values/set_value. The handlers share this
        // validation and compare the write's observed result with the request.
        let mut gain = numeric("Audio gain", -20.0, 20.0, 0.5, false, json!(0.0));
        if let ValueType::Number { unit, .. } = &mut gain.value_type {
            *unit = Some("dB".into());
        }
        registry.insert(root.clone(), "audio_gain", gain);
        let boolean = Parameter {
            label: "Enabled".into(),
            description: "Modeled state with local events".into(),
            value_type: ValueType::Boolean,
            access: Access::ReadWrite,
            gpi: true,
            gpo: true,
            default: json!(true),
            advertised: true,
        };
        registry.insert(root.clone(), "enabled", boolean.clone());
        let mut signal = boolean.clone();
        signal.label = "Signal present".into();
        signal.access = Access::ReadOnly;
        signal.gpo = false;
        registry.insert(root.clone(), "signal_present", signal);
        let mut mute = boolean.clone();
        mute.label = "Mute".into();
        mute.gpi = false;
        mute.default = json!(false);
        registry.insert(root.clone(), "mute", mute);
        let mut trigger = boolean.clone();
        trigger.access = Access::WriteOnly;
        trigger.gpi = false;
        trigger.label = "Trigger action".into();
        trigger.default = json!(false);
        registry.insert(root.clone(), "trigger", trigger);
        registry.insert(
            root.clone(),
            "mode",
            Parameter {
                label: "Mode".into(),
                description: "Enumerated state".into(),
                value_type: ValueType::Choices(vec!["standby".into(), "active".into()]),
                access: Access::ReadWrite,
                gpi: false,
                gpo: false,
                default: json!("standby"),
                advertised: true,
            },
        );
        // Read-only sensors use ordinary get_dynamic_values. Fetch related vendor
        // readings together in Device::read_values; no extra polling is needed.
        let mut sensor = numeric("Temperature", -40.0, 125.0, 0.1, false, json!(21.0));
        sensor.access = Access::ReadOnly;
        registry.insert(root.clone(), "temperature", sensor);
        registry.insert(
            root.clone(),
            "unbounded",
            Parameter {
                label: "Unbounded numeric sample".into(),
                description: "No optional range; runtime requires finite numbers".into(),
                value_type: ValueType::Number {
                    range: None,
                    unit: None,
                    integer: false,
                },
                access: Access::ReadWrite,
                gpi: false,
                gpo: false,
                default: json!(0),
                advertised: true,
            },
        );
        // A tiny explicit namespace demonstrates unadvertised addresses without
        // accepting arbitrary OIDs, paths or URLs from an untrusted frame.
        for id in ["sim.user.1", "sim.user.2"] {
            let mut hidden = numeric(
                "Unadvertised scratch value",
                0.0,
                100.0,
                1.0,
                true,
                json!(0),
            );
            hidden.advertised = false;
            registry.insert(root.clone(), id, hidden);
        }
        for location in [
            Location::module("module-1"),
            Location::port("module-1", PortKind::Sender, "tx-1"),
            Location::port("module-1", PortKind::Receiver, "rx-1"),
            Location::port("module-1", PortKind::Receiver, "rx-audio"),
            Location::port("module-1", PortKind::Receiver, "rx-alt"),
            Location::port("module-1", PortKind::Generic, "fan-1"),
            Location::port("module-1", PortKind::NetworkInterface, "net-main"),
            Location::port("module-1", PortKind::NetworkInterface, "net-spare"),
            Location::module("module-2"),
            Location::port("module-2", PortKind::NetworkInterface, "net-aux"),
        ] {
            registry.insert(location, "enabled", boolean.clone());
        }
        let interface = |module: &str, id: &str, address: Value| json!({"type":"single","id":id,"label":id,"known_address":address,"dynamic_values":registry.definitions(&Location::port(module,PortKind::NetworkInterface,id))});
        // Sender sdp_support promises SDP reads; receiver sdp_support promises
        // SDP application. Match format_type and these flags to your device and
        // protocol::media_state, whose accepted SDP subset is sample-specific.
        let transceiver = |id: &str, kind, format: Value, topology: bool| json!({"id":id,"label":id,"dynamic_values":registry.definitions(&Location::port("module-1",kind,id)),"topology_info":if topology {json!({"format_type":format,"format_id":null,"sdp_support":id=="tx-1" || id=="rx-1" || id=="rx-audio","sips":true,"interface_options":[{"module_id":"module-1","port_id":"net-main"},{"module_id":"module-1","port_id":"net-spare"},{"module_id":"module-2","port_id":"net-aux"}]})} else {Value::Null}});
        let alternate = transceiver("rx-alt", PortKind::Receiver, json!("Video"), true);
        registry.structure = json!({"label":"Rust control-plane simulator","version":"0.1.0","product_name":"SPEC-23 teaching device","configuration_order":"senders_first","dynamic_values":registry.definitions(&root),"resource_links":[{"type":"rx-tx","from":{"module_id":"module-1","port_id":"rx-1"},"to":{"module_id":"module-1","port_id":"tx-1"}}],"modules":[
            {"id":"module-1","label":"Media module","dynamic_values":registry.definitions(&Location::module("module-1")),"senders":[transceiver("tx-1",PortKind::Sender,json!("Video"),true),transceiver("tx-audio",PortKind::Sender,json!("Audio"),true),transceiver("tx-asi",PortKind::Sender,json!("ASI"),true),transceiver("tx-anc",PortKind::Sender,json!("Ancillary"),true),transceiver("tx-unknown",PortKind::Sender,Value::Null,true),transceiver("tx-monitor",PortKind::Sender,Value::Null,false)],"receivers":[transceiver("rx-1",PortKind::Receiver,json!("Video"),true),transceiver("rx-audio",PortKind::Receiver,json!("Audio"),true)],"generic_ports":[{"id":"fan-1","label":"Fan","dynamic_values":registry.definitions(&Location::port("module-1",PortKind::Generic,"fan-1"))}],"network_interfaces":[{"type":"redundant_pair","interface_1":interface("module-1","net-main",json!("192.0.2.10")),"interface_2":interface("module-1","net-spare",json!("198.51.100.10"))}]},
            {"id":"module-2","label":"Auxiliary interfaces","dynamic_values":registry.definitions(&Location::module("module-2")),"senders":[],"receivers":[],"generic_ports":[],"network_interfaces":[interface("module-2","net-aux",Value::Null)]}
        ]});
        registry.structure["version"] = json!(env!("CARGO_PKG_VERSION"));
        registry.structure["modules"][0]["senders"][0]["topology_info"]["format_id"] =
            json!("raw-video");
        registry.structure["modules"][0]["receivers"][0]["topology_info"]["format_id"] =
            json!("raw-video");
        // Add resource links only for real internal routing choices. A sender's
        // resource_partner selects one of these receiver-to-sender paths.
        let mut alternate = alternate;
        alternate["topology_info"]["format_id"] = json!("raw-video");
        registry.structure["modules"][0]["receivers"]
            .as_array_mut()
            .expect("receiver list")
            .push(alternate);
        registry.structure["resource_links"]
            .as_array_mut()
            .expect("resource links")
            .push(json!({
                "type":"rx-tx","from":{"module_id":"module-1","port_id":"rx-alt"},
                "to":{"module_id":"module-1","port_id":"tx-1"}
            }));
        registry.structure["modules"][1]["label"] = json!("Auxiliary Console 42");
        registry
    }

    /// Configuration order is a startup choice, not a live structure mutation.
    pub fn with_configuration_order(mut self, order: &str) -> Result<Self, Failure> {
        if !["senders_first", "receivers_first", "all"].contains(&order) {
            return Err(Failure::Invalid);
        }
        self.structure["configuration_order"] = json!(order);
        Ok(self)
    }
    fn insert(&mut self, location: Location, id: &str, parameter: Parameter) {
        assert!(
            self.parameters
                .insert((location, id.into()), parameter)
                .is_none(),
            "duplicate catalog identity"
        );
    }
    pub fn structure(&self) -> Value {
        self.structure.clone()
    }
    pub fn validate(&self) -> Result<(), Failure> {
        validate_structure(&self.structure)
    }
    pub fn parameter(&self, location: &Location, id: &str) -> Option<&Parameter> {
        self.parameters.get(&(location.clone(), id.into()))
    }
    pub fn locations(&self) -> Vec<Location> {
        self.parameters
            .keys()
            .map(|(l, _)| l.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }
    /// Simulator initialization values, not physical readback or automatic resets.
    pub fn defaults(&self, location: &Location) -> Values {
        self.parameters
            .iter()
            .filter(|((l, _), _)| l == location)
            .map(|((_, id), p)| (id.clone(), p.default.clone()))
            .collect()
    }
    pub fn definitions(&self, location: &Location) -> BTreeMap<String, Value> {
        self.parameters
            .iter()
            .filter(|((l, _), p)| l == location && p.advertised)
            .map(|((_, id), p)| (id.clone(), p.definition()))
            .collect()
    }
    pub fn known_location(&self, location: &Location) -> bool {
        match &location.0 {
            None => true,
            Some(l) => self.structure["modules"].as_array().is_some_and(|modules| {
                modules.iter().any(|m| {
                    m["id"] == l.module_id
                        && l.port_location.as_ref().is_none_or(|p| match p.kind {
                            PortKind::NetworkInterface => {
                                interfaces(m).iter().any(|i| i["id"] == p.id)
                            }
                            _ => m[port_list(p.kind)]
                                .as_array()
                                .is_some_and(|a| a.iter().any(|v| v["id"] == p.id)),
                        })
                })
            }),
        }
    }
    /// Resolve a stable port and optional stream instance. The 64-instance bound
    /// is this simulator's capacity, not a SPEC-23 limit; null is a normal single port.
    pub fn port(&self, key: &PortKey) -> Result<Value, Failure> {
        if key.sub_index.is_some_and(|n| n == 0 || n > 64) {
            return Err(Failure::Range);
        }
        let list = if key.direction == Direction::Sender {
            "senders"
        } else {
            "receivers"
        };
        self.structure["modules"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|m| m["id"] == key.module_id)
            .and_then(|m| m[list].as_array())
            .into_iter()
            .flatten()
            .find(|p| p["id"] == key.port_id)
            .cloned()
            .ok_or(Failure::Unknown)
    }
    pub fn interface_exists(&self, interface: &Interface) -> bool {
        self.known_location(&Location::port(
            &interface.module,
            PortKind::NetworkInterface,
            &interface.port,
        ))
    }
}
fn port_list(kind: PortKind) -> &'static str {
    match kind {
        PortKind::Sender => "senders",
        PortKind::Receiver => "receivers",
        PortKind::Generic => "generic_ports",
        PortKind::NetworkInterface => "network_interfaces",
    }
}
fn interfaces(module: &Value) -> Vec<&Value> {
    module["network_interfaces"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|i| {
            if i["type"] == "redundant_pair" {
                vec![&i["interface_1"], &i["interface_2"]]
            } else {
                vec![i]
            }
        })
        .collect()
}

/// Startup structural validation is independent of runtime device I/O. Optional
/// arrays follow the documented empty default; missing references fail closed.
pub fn validate_structure(structure: &Value) -> Result<(), Failure> {
    if !structure["label"].is_string() || !structure["version"].is_string() {
        return Err(Failure::Invalid);
    }
    validate_element(structure, &["modules", "resource_links"])?;
    if let Some(order) = structure.get("configuration_order")
        && !["senders_first", "receivers_first", "all"]
            .iter()
            .any(|v| order == v)
    {
        return Err(Failure::Invalid);
    }
    let mut modules = BTreeSet::new();
    let mut network = BTreeSet::new();
    let mut vertices = BTreeSet::new();
    for module in structure["modules"].as_array().into_iter().flatten() {
        validate_element(
            module,
            &[
                "senders",
                "receivers",
                "generic_ports",
                "network_interfaces",
            ],
        )?;
        let mid = module["id"].as_str().ok_or(Failure::Invalid)?;
        if !modules.insert(mid) {
            return Err(Failure::Invalid);
        }
        let mut ids = BTreeSet::new();
        for interface in interfaces(module) {
            validate_element(interface, &[])?;
            let id = interface["id"].as_str().ok_or(Failure::Invalid)?;
            if interface["type"] != "single" || !ids.insert(id) {
                return Err(Failure::Invalid);
            }
            if let Some(address) = interface.get("known_address").filter(|v| !v.is_null()) {
                address
                    .as_str()
                    .ok_or(Failure::Invalid)?
                    .parse::<std::net::IpAddr>()
                    .map_err(|_| Failure::Invalid)?;
            }
            network.insert((mid, id));
        }
        for list in ["senders", "receivers", "generic_ports"] {
            let mut ids = BTreeSet::new();
            for port in module[list].as_array().into_iter().flatten() {
                validate_element(port, &[])?;
                let id = port["id"].as_str().ok_or(Failure::Invalid)?;
                if !ids.insert(id) {
                    return Err(Failure::Invalid);
                }
                if !port["topology_info"].is_null() {
                    vertices.insert((mid, id, list));
                }
            }
        }
    }
    for module in structure["modules"].as_array().into_iter().flatten() {
        for list in ["senders", "receivers"] {
            for port in module[list].as_array().into_iter().flatten() {
                let topology = &port["topology_info"];
                if !topology.is_null() {
                    if !topology.is_object() {
                        return Err(Failure::Invalid);
                    }
                    if topology
                        .get("interface_options")
                        .is_some_and(|v| !v.is_array())
                    {
                        return Err(Failure::Invalid);
                    }
                    for flag in ["sdp_support", "sips"] {
                        if topology.get(flag).is_some_and(|v| !v.is_boolean()) {
                            return Err(Failure::Invalid);
                        }
                    }
                    if topology
                        .get("format_id")
                        .is_some_and(|value| !value.is_null() && !value.is_string())
                    {
                        return Err(Failure::Invalid);
                    }
                    // The reference does not specify populated IP-override shapes.
                    for name in [
                        "main_interface_ip_overrides",
                        "spare_interface_ip_overrides",
                    ] {
                        if topology.get(name).is_some_and(|v| !v.is_null()) {
                            return Err(Failure::Unsupported);
                        }
                    }
                }
                if let Some(format) = topology.get("format_type").filter(|v| !v.is_null())
                    && !["Video", "Audio", "ASI", "Ancillary"]
                        .iter()
                        .any(|v| format == v)
                {
                    return Err(Failure::Invalid);
                }
                for reference in topology["interface_options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                {
                    let pair = (
                        reference["module_id"].as_str().ok_or(Failure::Invalid)?,
                        reference["port_id"].as_str().ok_or(Failure::Invalid)?,
                    );
                    if !network.contains(&pair) {
                        return Err(Failure::Unknown);
                    }
                }
            }
        }
    }
    for link in structure["resource_links"].as_array().into_iter().flatten() {
        let from_list = match link["type"].as_str() {
            Some("rx-tx") => "receivers",
            Some("tx-tx") => "senders",
            _ => return Err(Failure::Invalid),
        };
        for (side, list) in [("from", from_list), ("to", "senders")] {
            if !vertices.contains(&(
                link[side]["module_id"].as_str().ok_or(Failure::Invalid)?,
                link[side]["port_id"].as_str().ok_or(Failure::Invalid)?,
                list,
            )) {
                return Err(Failure::Unknown);
            }
        }
    }
    Ok(())
}

fn validate_element(element: &Value, arrays: &[&str]) -> Result<(), Failure> {
    if !element.is_object() || !element["label"].is_string() {
        return Err(Failure::Invalid);
    }
    for name in arrays {
        if element.get(name).is_some_and(|v| !v.is_array()) {
            return Err(Failure::Invalid);
        }
    }
    if let Some(definitions) = element.get("dynamic_values") {
        for definition in definitions.as_object().ok_or(Failure::Invalid)?.values() {
            validate_definition(definition)?;
        }
    }
    Ok(())
}

/// Validate the complete advertised metadata map, including absent/null optional
/// ranges. This is derived local validation, not a backend decoder substitute.
pub fn validate_definition(definition: &Value) -> Result<(), Failure> {
    if !definition["label"].is_string() {
        return Err(Failure::Invalid);
    }
    let kind = &definition["type_info"];
    match kind["type"].as_str() {
        Some("boolean") => {}
        Some("string") => {
            let values = kind["values"].as_array().ok_or(Failure::Invalid)?;
            if values.iter().any(|v| !v.is_string()) {
                return Err(Failure::Invalid);
            }
        }
        Some("number") => {
            if let Some(range) = kind.get("range").filter(|r| !r.is_null()) {
                let low = range["from"].as_f64().ok_or(Failure::Invalid)?;
                let high = range["to"].as_f64().ok_or(Failure::Invalid)?;
                if !low.is_finite() || !high.is_finite() || low > high {
                    return Err(Failure::Range);
                }
                if let Some(step) = range.get("step").filter(|s| !s.is_null())
                    && !step.as_f64().is_some_and(|s| s.is_finite() && s > 0.0)
                {
                    return Err(Failure::Range);
                }
                if range
                    .get("unit")
                    .is_some_and(|s| !s.is_null() && !s.is_string())
                {
                    return Err(Failure::Type);
                }
            }
        }
        _ => return Err(Failure::Unsupported),
    }
    let access = &definition["value_info"];
    if !["read_only", "read_write", "write_only"]
        .iter()
        .any(|a| access["type"] == *a)
    {
        return Err(Failure::Invalid);
    }
    for flag in ["gpi_capable", "gpo_capable"] {
        if access.get(flag).is_some_and(|v| !v.is_boolean()) {
            return Err(Failure::Type);
        }
    }
    Ok(())
}
