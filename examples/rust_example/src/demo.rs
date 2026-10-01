//! Small in-memory stand-in for your device's operations, not a device simulator API.
//!
//! Read this beside `domain::Device`: replace the state accesses with vendor calls.
//! State is separate from request DTOs so readback and drift checks are meaningful
//! within the demo, but no operation here proves physical media or hardware behavior.
use crate::{
    domain::*,
    registry::{Access, Registry},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub struct DemoDevice {
    registry: Arc<Registry>,
    state: Mutex<State>,
}

struct State {
    values: BTreeMap<Location, Values>,
    configurations: BTreeMap<PortKey, MediaState>,
    alarm: Option<Alarm>,
    actions: u64,
}

impl DemoDevice {
    /// Model availability only. A vendor adapter must check real device contact.
    pub fn healthy(&self) -> bool {
        !self.state.is_poisoned()
    }

    pub fn new(registry: Arc<Registry>) -> Self {
        let values = registry
            .locations()
            .into_iter()
            .map(|location| (location.clone(), registry.defaults(&location)))
            .collect();
        Self {
            registry,
            state: Mutex::new(State {
                values,
                configurations: BTreeMap::new(),
                alarm: None,
                actions: 0,
            }),
        }
    }

    /// Deliberately generated demo input: a signal alternates present/absent.
    /// The executable calls this every ten seconds while connected. Replace this
    /// teaching clock with actual vendor events, NOT a second sensor poll loop.
    /// Only a read-only value changes here, so it cannot race a writable-value echo.
    pub fn tick(&self) -> Result<Vec<Event>, Failure> {
        let mut state = self.state.lock().map_err(|_| Failure::Internal)?;
        let values = state
            .values
            .get_mut(&Location::root())
            .ok_or(Failure::Internal)?;
        let present = !values["signal_present"]
            .as_bool()
            .ok_or(Failure::Internal)?;
        values.insert("signal_present".into(), json!(present));
        let change = Event::Values {
            location: Location::root(),
            values: [("signal_present".into(), json!(present))].into(),
        };
        let alarm_event = if present {
            state.alarm = None;
            Event::AlarmCleared {
                location: Location::root(),
                id: "Demo signal absent".into(),
            }
        } else {
            let alarm = Alarm {
                id: "Demo signal absent".into(),
                location: Location::root(),
                details: "Teaching clock input; not a physical fault".into(),
                severity: Severity::Warning,
                service_affecting: false,
            };
            state.alarm = Some(alarm.clone());
            Event::Alarm { alarm }
        };
        Ok(vec![change, alarm_event])
    }

    /// Actions are not readable parameter state. This counter lets tests prove
    /// that two explicit invocations are two actions, not one deduplicated set.
    pub fn action_count(&self) -> Result<u64, Failure> {
        Ok(self.state.lock().map_err(|_| Failure::Internal)?.actions)
    }
}

impl Device for DemoDevice {
    async fn read_values(&self, location: Location) -> Result<Values, Failure> {
        let state = self.state.lock().map_err(|_| Failure::Internal)?;
        let mut values = state
            .values
            .get(&location)
            .cloned()
            .ok_or(Failure::Unknown)?;
        values.retain(|id, _| {
            self.registry
                .parameter(&location, id)
                .is_some_and(|p| p.access != Access::WriteOnly)
        });
        Ok(values)
    }

    async fn set_value(
        &self,
        location: Location,
        id: String,
        value: Value,
    ) -> Result<Value, Failure> {
        let parameter = self
            .registry
            .parameter(&location, &id)
            .ok_or(Failure::Unknown)?;
        if parameter.access == Access::ReadOnly {
            return Err(Failure::ReadOnly);
        }
        parameter.validate(&value)?;
        let mut state = self.state.lock().map_err(|_| Failure::Internal)?;
        if parameter.access == Access::WriteOnly {
            state.actions = state.actions.checked_add(1).ok_or(Failure::Overloaded)?;
            return Ok(value); // Action acknowledgement, not fabricated readable state.
        }
        let values = state.values.get_mut(&location).ok_or(Failure::Unknown)?;
        values.insert(id.clone(), value);
        // On real hardware, query after applying. Handlers compare this observation
        // with the requested value and return the operation-specific failure map.
        // Do not publish here: the session sends the response before causal changes.
        Ok(values[&id].clone())
    }

    async fn alarms(&self) -> Result<Vec<Alarm>, Failure> {
        Ok(self
            .state
            .lock()
            .map_err(|_| Failure::Internal)?
            .alarm
            .iter()
            .cloned()
            .collect())
    }

    async fn sender_sdp(&self, key: PortKey) -> Result<Option<String>, Failure> {
        let port = self.registry.port(&key)?;
        if key.direction != Direction::Sender || port["topology_info"]["sdp_support"] != true {
            return Err(Failure::Unsupported);
        }
        self.state
            .lock()
            .map_err(|_| Failure::Internal)?
            .configurations
            .get(&key)
            .map(crate::sdp::sender_sdp)
            .transpose()
    }

    async fn configure(&self, key: PortKey, requested: Option<MediaState>) -> Result<(), Failure> {
        let port = self.registry.port(&key)?;
        if port["topology_info"].is_null() {
            return Err(Failure::Unsupported);
        }
        let mut state = self.state.lock().map_err(|_| Failure::Internal)?;
        match requested {
            Some(observed) => {
                // Local capacity policy, not a SPEC limit. Domain configuration is
                // already validated by the codec; real hardware applies it here.
                if !state.configurations.contains_key(&key) && state.configurations.len() >= 128 {
                    return Err(Failure::Overloaded);
                }
                state.configurations.insert(key, observed);
            }
            None => {
                state.configurations.remove(&key);
            } // Null means disable.
        }
        Ok(())
    }

    async fn configuration(&self, key: PortKey) -> Result<Option<MediaState>, Failure> {
        self.registry.port(&key)?;
        // Independent state access makes a local change visible to a later check.
        // Replace it with vendor readback, never a cache of the last SPEC request.
        Ok(self
            .state
            .lock()
            .map_err(|_| Failure::Internal)?
            .configurations
            .get(&key)
            .cloned())
    }
}
