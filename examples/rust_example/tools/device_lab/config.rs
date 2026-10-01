//! Lab-only transport settings layered over the shared driver configuration.
use crate::config::{Config as DriverConfig, ConfigError};
use std::{collections::BTreeMap, ops::Deref, path::PathBuf, time::Duration};

pub const OPTIONS: &[(&str, &str)] = &[
    ("DRIVER_SOCKET", "/run/rust-example/device.sock"),
    ("DRIVER_CONTROL_SOCKET", "/run/rust-example/control.sock"),
    ("DRIVER_OPERATION_TIMEOUT_MS", "3000"),
];

/// Deliberately no `Debug`: shared configuration contains the bearer credential.
pub struct Config {
    shared: DriverConfig,
    pub socket: PathBuf,
    pub control_socket: PathBuf,
    pub operation_timeout: Duration,
}

impl Deref for Config {
    type Target = DriverConfig;

    fn deref(&self) -> &Self::Target {
        &self.shared
    }
}

impl Config {
    pub fn environment(driver: bool) -> Result<Self, ConfigError> {
        Self::parse(&std::env::vars().collect(), driver)
    }

    pub fn parse(env: &BTreeMap<String, String>, driver: bool) -> Result<Self, ConfigError> {
        let shared = DriverConfig::parse(env, driver)?;
        let get = |name: &str| -> String {
            env.get(name).cloned().unwrap_or_else(|| {
                OPTIONS
                    .iter()
                    .find(|(key, _)| *key == name)
                    .expect("declared lab option")
                    .1
                    .into()
            })
        };
        let path = |name: &'static str| -> Result<PathBuf, ConfigError> {
            let path = PathBuf::from(get(name));
            if !path.is_absolute() {
                return Err(ConfigError(name));
            }
            Ok(path)
        };
        let socket = path("DRIVER_SOCKET")?;
        let control_socket = path("DRIVER_CONTROL_SOCKET")?;
        let millis = get("DRIVER_OPERATION_TIMEOUT_MS")
            .parse::<u64>()
            .map_err(|_| ConfigError("DRIVER_OPERATION_TIMEOUT_MS"))?;
        if !(10..=4000).contains(&millis) {
            return Err(ConfigError("DRIVER_OPERATION_TIMEOUT_MS"));
        }
        let operation_timeout = Duration::from_millis(millis);
        if operation_timeout >= shared.request_timeout
            || shared.request_timeout > shared.ipt_timeout
        {
            return Err(ConfigError("operation < request <= IPT deadline required"));
        }
        if socket == control_socket {
            return Err(ConfigError("device/control sockets must differ"));
        }
        Ok(Self {
            shared,
            socket,
            control_socket,
            operation_timeout,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simulator_needs_no_vip_credentials() {
        assert!(Config::parse(&BTreeMap::new(), false).is_ok());
    }

    #[test]
    fn lab_paths_and_deadlines_remain_validated() {
        for (key, value) in [
            ("DRIVER_SOCKET", "relative"),
            ("DRIVER_CONTROL_SOCKET", "relative"),
            ("DRIVER_CONTROL_SOCKET", "/run/rust-example/device.sock"),
            ("DRIVER_OPERATION_TIMEOUT_MS", "5000"),
            ("DRIVER_OPERATION_TIMEOUT_MS", "9"),
            ("DRIVER_OPERATION_TIMEOUT_MS", "SECRET"),
            ("DRIVER_REQUEST_TIMEOUT_MS", "3000"),
        ] {
            let env = [(key.into(), value.into())].into();
            let error = Config::parse(&env, false).err().unwrap().to_string();
            assert!(!error.contains("SECRET"));
        }
        let env = [
            ("DRIVER_OPERATION_TIMEOUT_MS".into(), "10".into()),
            ("DRIVER_REQUEST_TIMEOUT_MS".into(), "100".into()),
        ]
        .into();
        assert!(Config::parse(&env, false).is_ok());
    }
}
