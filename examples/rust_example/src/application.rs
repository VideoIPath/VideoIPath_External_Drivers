//! Start at `dispatch` to follow each SPEC-23 request into a device operation.
//! `set_values` and `set_ipt` show partial failures and physical readback;
//! the session sends the returned response before its causal change events.
use crate::{
    domain::*,
    protocol::{self, *},
    registry::{Access, Registry},
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub struct Outcome {
    pub response: Value,
    pub changes: Vec<Event>,
}
impl std::fmt::Debug for Outcome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Outcome")
            .field("message_id", &self.response["message_id"])
            .field("deferred_events", &self.changes.len())
            .finish_non_exhaustive()
    }
}

/// Keepalive and incoming error are session responsibilities, not device calls.
/// No handler retries: even an apparently idempotent operation can have an
/// ambiguous physical outcome after cancellation or transport loss.
pub async fn handle<D: Device>(device: &D, registry: &Registry, request: Request) -> Outcome {
    let mut changes = Vec::new();
    let result = dispatch(device, registry, &request, &mut changes).await;
    Outcome {
        response: match result {
            Ok(data) => protocol::response(&request, data),
            Err(error) => error_frame(request.message_id, error),
        },
        changes,
    }
}

async fn dispatch<D: Device>(
    device: &D,
    registry: &Registry,
    request: &Request,
    changes: &mut Vec<Event>,
) -> Result<Value, Failure> {
    match request.message_type.as_str() {
        "get_structure" => {
            // Publish definitions so VideoIPath can discover controls and ports.
            // Current parameter values belong in get_dynamic_values, not here.
            registry.validate()?;
            Ok(registry.structure())
        }
        "get_dynamic_values" => {
            let data: GetValues = decode(&request.data)?;
            if !registry.known_location(&data.location) {
                return Err(Failure::Unknown);
            }
            if data.ids.as_ref().is_some_and(|ids| ids.len() > MAX_ITEMS) {
                return Err(Failure::Overloaded);
            }
            if data.ids.as_ref().is_some_and(|ids| {
                ids.iter().any(|id| {
                    registry
                        .parameter(&data.location, id)
                        .is_some_and(|p| p.access == Access::WriteOnly)
                })
            }) {
                return Err(Failure::WriteOnly);
            }
            // Read related sensors once, then select requested known IDs. Unknown
            // IDs are omitted; a failed device read becomes a correlated error.
            // No IDs selects advertised readable values in this sample. An empty
            // list selects none here; verify that ambiguous edge with your target.
            let observed = device.read_values(data.location.clone()).await?;
            let selected: Values = observed
                .into_iter()
                .filter(|(id, _)| {
                    registry.parameter(&data.location, id).is_some_and(|p| {
                        p.access != Access::WriteOnly
                            && data
                                .ids
                                .as_ref()
                                .map_or(p.advertised, |ids| ids.contains(id))
                    })
                })
                .collect();
            for (id, value) in &selected {
                registry
                    .parameter(&data.location, id)
                    .ok_or(Failure::Internal)?
                    .validate(value)?;
            }
            Ok(json!(selected))
        }
        "set_dynamic_values" => set_values(device, registry, decode(&request.data)?, changes).await,
        "alarms" => {
            // This is the complete active snapshot, not a delta: omitted alarms
            // are cleared. A failed read must not become an empty successful list.
            let alarms = device.alarms().await?;
            let mut identities = BTreeSet::new();
            for alarm in &alarms {
                if alarm.id.is_empty()
                    || !registry.known_location(&alarm.location)
                    || !identities.insert((&alarm.location, &alarm.id))
                {
                    return Err(Failure::Internal);
                }
            }
            Ok(json!(alarms))
        }
        "get_sender_sdp" => {
            // Return a string when available, null when temporarily absent, and
            // an error for unsupported SDP or a failed device read.
            let data: SdpRequest = decode(&request.data)?;
            let key = PortKey {
                module_id: data.module_id,
                port_id: data.port_id,
                sub_index: None,
                direction: Direction::Sender,
            };
            let port = registry.port(&key)?;
            if port["topology_info"]["sdp_support"] != true {
                return Err(Failure::Unsupported);
            }
            Ok(json!(device.sender_sdp(key).await?))
        }
        "set_ipt_configurations" => set_ipt(device, registry, decode(&request.data)?).await,
        "check_ipt_configuration" => {
            // Compare supported fields with actual device state, not the last
            // request. On drift, set_again permits reapply followed by readback;
            // failed reapply/readback is an error, not a successful comparison.
            let data: IptCheck = decode(&request.data)?;
            let direction: Direction = decode(
                data.config
                    .get("transceiver_type")
                    .ok_or(Failure::Invalid)?,
            )?;
            let key = config_key(&data.module_id, &data.config, direction)?;
            let desired = media_state(registry, &key, &data.config)?;
            if device.configuration(key.clone()).await? == desired {
                return Ok(json!(true));
            }
            if !data.set_again {
                return Ok(json!(false));
            }
            device.configure(key.clone(), desired.clone()).await?;
            if device.configuration(key).await? != desired {
                return Err(Failure::Mismatch);
            }
            Ok(json!(true))
        }
        _ => Err(Failure::Unsupported),
    }
}

// Process each value independently: response data is the direct ID-to-error map,
// with {} meaning success. Readback can reveal changes even after a failed write;
// defer those observed changes until the session has delivered the response.
async fn set_values<D: Device>(
    device: &D,
    registry: &Registry,
    data: SetValues,
    changes: &mut Vec<Event>,
) -> Result<Value, Failure> {
    if !registry.known_location(&data.location) {
        return Err(Failure::Unknown);
    }
    if data.values.len() > MAX_ITEMS {
        return Err(Failure::Overloaded);
    }
    let mut before = device.read_values(data.location.clone()).await?;
    let mut failed = serde_json::Map::new();
    let mut changed = Values::new();
    for (id, value) in data.values {
        let Some(parameter) = registry.parameter(&data.location, &id) else {
            failed.insert(id, json!("Unknown parameter"));
            continue;
        };
        let valid = if parameter.access == Access::ReadOnly {
            Err(Failure::ReadOnly)
        } else {
            parameter.validate(&value)
        };
        if let Err(error) = valid {
            failed.insert(id, json!(error.to_string()));
            continue;
        }
        let result = device
            .set_value(data.location.clone(), id.clone(), value.clone())
            .await;
        // An action has no truthful readback or value-change event. Success is
        // adapter acknowledgement, never an invented persistent trigger state.
        if parameter.access == Access::WriteOnly {
            if let Err(error) = result {
                failed.insert(id, json!(error.to_string()));
            }
            continue;
        }
        let observed = match result {
            Ok(observed) => {
                if parameter.validate(&observed).is_err() {
                    failed.insert(id.clone(), json!(Failure::Internal.to_string()));
                    None
                } else {
                    if observed != value {
                        failed.insert(id.clone(), json!(Failure::Mismatch.to_string()));
                    }
                    Some(observed)
                }
            }
            Err(error) => {
                failed.insert(id.clone(), json!(error.to_string()));
                // Reconcile uncertainty if the adapter still has a usable read
                // channel. Never retry the write or claim the old value is fresh.
                device
                    .read_values(data.location.clone())
                    .await
                    .ok()
                    .and_then(|mut v| v.remove(&id))
                    .filter(|v| parameter.validate(v).is_ok())
            }
        };
        if let Some(observed) = observed {
            if before.get(&id) != Some(&observed) {
                changed.insert(id.clone(), observed.clone());
            }
            before.insert(id, observed);
        }
    }
    if !changed.is_empty() {
        changes.push(Event::Values {
            location: data.location,
            values: changed,
        });
    }
    Ok(Value::Object(failed))
}

// Report addressable failures in sender_config_errors/receiver_config_errors;
// absent pairs succeeded. Invalid batch/module/identity errors fail the request.
// A null enabled_config becomes None: apply and verify a disable, not a no-op.
async fn set_ipt<D: Device>(
    device: &D,
    registry: &Registry,
    data: IptBatch,
) -> Result<Value, Failure> {
    if data.sender_configs.len() + data.receiver_configs.len() > MAX_ITEMS {
        return Err(Failure::Overloaded);
    }
    if !registry.known_location(&Location::module(&data.module_id)) {
        return Err(Failure::Unknown);
    }
    // Establish error-item identities before applying anything. A malformed ID
    // cannot be represented in the documented per-port error shape.
    for config in data.sender_configs.iter().chain(&data.receiver_configs) {
        if !config["id"].is_string()
            || config.get("sub_index").is_some_and(|v| {
                !v.is_null() && v.as_u64().and_then(|n| u32::try_from(n).ok()).is_none()
            })
        {
            return Err(Failure::Invalid);
        }
    }
    let mut sender_errors = Vec::new();
    let mut receiver_errors = Vec::new();
    // Cross-module ordering belongs to the backend. Within a batch the simulator
    // applies the selected order. `all` accepts both directions in one request;
    // it does not promise atomic or parallel physical-device application.
    let mut groups = [
        (Direction::Sender, data.sender_configs, &mut sender_errors),
        (
            Direction::Receiver,
            data.receiver_configs,
            &mut receiver_errors,
        ),
    ];
    if registry.structure()["configuration_order"] == "receivers_first" {
        groups.reverse();
    }
    for (direction, configs, errors) in groups {
        let mut seen = BTreeSet::new();
        for config in configs {
            let identity = (
                config["id"].as_str().unwrap_or_default().to_owned(),
                config.get("sub_index").and_then(Value::as_u64),
            );
            if !seen.insert(identity) {
                continue;
            }
            let result = async {
                let key = config_key(&data.module_id, &config, direction)?;
                let desired = media_state(registry, &key, &config)?;
                device.configure(key.clone(), desired.clone()).await?;
                if device.configuration(key).await? != desired {
                    return Err(Failure::Mismatch);
                }
                Ok::<_, Failure>(())
            }
            .await;
            if let Err(error) = result {
                errors.push(json!({"id":config["id"],"sub_index":config.get("sub_index").unwrap_or(&Value::Null),"error":error.to_string()}));
            }
        }
    }
    Ok(json!({"sender_config_errors":sender_errors,"receiver_config_errors":receiver_errors}))
}
