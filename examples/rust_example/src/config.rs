//! Resolve deployment inputs once. Errors name settings but never echo their values.
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use url::Url;
use uuid::Uuid;

/// Public environment names are also exercised by the deployment contract tests.
pub const OPTIONS: &[(&str, &str)] = &[
    ("DRIVER_CONFIGURATION_ORDER", "senders_first"),
    ("VIDEOIPATH_URL", ""),
    ("VIDEOIPATH_TOKEN", ""),
    ("VIDEOIPATH_NODE_ID", ""),
    ("VIDEOIPATH_CA_BUNDLE", ""),
    ("DRIVER_STATE_DIRECTORY", "/var/lib/rust-example"),
    ("DRIVER_LOCK_DIRECTORY", "/var/lock/videoipath-drivers"),
    ("DRIVER_LOG_LEVEL", "info"),
    ("DRIVER_REQUEST_LIMIT", "16"),
    ("DRIVER_OUTBOUND_LIMIT", "128"),
    ("DRIVER_MAX_FRAME_BYTES", "1048576"),
    ("DRIVER_REQUEST_TIMEOUT_MS", "5000"),
    ("DRIVER_IPT_TIMEOUT_MS", "20000"),
    ("DRIVER_RECONNECT_MIN_MS", "500"),
    ("DRIVER_RECONNECT_MAX_MS", "30000"),
    ("DRIVER_SHUTDOWN_TIMEOUT_MS", "5000"),
];

#[derive(Debug, thiserror::Error)]
#[error("invalid configuration: {0}")]
pub struct ConfigError(pub &'static str);

/// Deliberately no `Debug`: deriving it would disclose the bearer credential.
pub struct Config {
    pub configuration_order: String,
    pub url: Option<Url>,
    pub token: String,
    pub node_id: Option<Uuid>,
    pub ca_bundle: Option<PathBuf>,
    pub state_directory: PathBuf,
    pub lock_directory: PathBuf,
    pub log_level: String,
    pub request_limit: usize,
    pub outbound_limit: usize,
    pub max_frame_bytes: usize,
    pub request_timeout: Duration,
    pub ipt_timeout: Duration,
    pub reconnect_min: Duration,
    pub reconnect_max: Duration,
    pub shutdown_timeout: Duration,
}

impl Config {
    pub fn environment(driver: bool) -> Result<Self, ConfigError> {
        Self::parse(&std::env::vars().collect(), driver)
    }

    pub fn parse(env: &BTreeMap<String, String>, driver: bool) -> Result<Self, ConfigError> {
        let get = |name: &str| -> String {
            env.get(name).cloned().unwrap_or_else(|| {
                OPTIONS
                    .iter()
                    .find(|(key, _)| *key == name)
                    .expect("declared option")
                    .1
                    .into()
            })
        };
        let number = |name: &'static str, min: u64, max: u64| -> Result<u64, ConfigError> {
            let n = get(name).parse::<u64>().map_err(|_| ConfigError(name))?;
            if !(min..=max).contains(&n) {
                return Err(ConfigError(name));
            }
            Ok(n)
        };
        let duration = |name, min, max| number(name, min, max).map(Duration::from_millis);
        let path = |name: &'static str| -> Result<PathBuf, ConfigError> {
            let p = PathBuf::from(get(name));
            if !p.is_absolute() {
                return Err(ConfigError(name));
            }
            Ok(p)
        };
        let token = if driver {
            get("VIDEOIPATH_TOKEN")
        } else {
            String::new()
        };
        let (url, node_id) = if driver {
            // Reject URL userinfo/query/fragment, rather than risk placing a secret in logs or redirects.
            let mut url =
                Url::parse(&get("VIDEOIPATH_URL")).map_err(|_| ConfigError("VIDEOIPATH_URL"))?;
            if url.scheme() != "wss"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
                || url.path() != "/x-driver/v1"
                || url.host_str().is_some_and(|h| h.ends_with(".invalid"))
            {
                return Err(ConfigError("VIDEOIPATH_URL"));
            }
            if token.trim().is_empty()
                || token.contains(['\r', '\n'])
                || token.trim() != token
                || token.contains(['<', '>'])
                || matches!(
                    token.to_lowercase().as_str(),
                    "changeme" | "placeholder" | "token"
                )
            {
                return Err(ConfigError("VIDEOIPATH_TOKEN"));
            }
            let id = Uuid::parse_str(&get("VIDEOIPATH_NODE_ID"))
                .map_err(|_| ConfigError("VIDEOIPATH_NODE_ID"))?;
            if id.is_nil()
                || id == Uuid::max()
                || id.to_string() == "550e8400-e29b-41d4-a716-446655440000"
            {
                return Err(ConfigError("VIDEOIPATH_NODE_ID"));
            }
            url.query_pairs_mut()
                .append_pair("node_id", &id.to_string());
            (Some(url), Some(id))
        } else {
            (None, None)
        };
        let ca = get("VIDEOIPATH_CA_BUNDLE");
        let ca_bundle = if driver && !ca.is_empty() {
            let p = PathBuf::from(ca);
            if !p.is_absolute() || !p.is_file() {
                return Err(ConfigError("VIDEOIPATH_CA_BUNDLE"));
            }
            Some(p)
        } else {
            None
        };
        let log_level = get("DRIVER_LOG_LEVEL");
        if !["error", "warn", "info", "debug", "trace"].contains(&log_level.as_str()) {
            return Err(ConfigError("DRIVER_LOG_LEVEL"));
        }
        let configuration_order = get("DRIVER_CONFIGURATION_ORDER");
        if !["senders_first", "receivers_first", "all"].contains(&configuration_order.as_str()) {
            return Err(ConfigError("DRIVER_CONFIGURATION_ORDER"));
        }
        let config = Self {
            configuration_order,
            url,
            token,
            node_id,
            ca_bundle,
            state_directory: path("DRIVER_STATE_DIRECTORY")?,
            lock_directory: path("DRIVER_LOCK_DIRECTORY")?,
            log_level,
            request_limit: number("DRIVER_REQUEST_LIMIT", 1, 256)? as usize,
            outbound_limit: number("DRIVER_OUTBOUND_LIMIT", 2, 4096)? as usize,
            max_frame_bytes: number("DRIVER_MAX_FRAME_BYTES", 1024, 4 * 1024 * 1024)? as usize,
            request_timeout: duration("DRIVER_REQUEST_TIMEOUT_MS", 100, 7000)?,
            ipt_timeout: duration("DRIVER_IPT_TIMEOUT_MS", 100, 25000)?,
            reconnect_min: duration("DRIVER_RECONNECT_MIN_MS", 100, 30000)?,
            reconnect_max: duration("DRIVER_RECONNECT_MAX_MS", 100, 120000)?,
            shutdown_timeout: duration("DRIVER_SHUTDOWN_TIMEOUT_MS", 100, 10000)?,
        };
        if config.request_timeout > config.ipt_timeout {
            return Err(ConfigError("request <= IPT deadline required"));
        }
        if config.reconnect_min > config.reconnect_max {
            return Err(ConfigError("reconnect minimum exceeds maximum"));
        }
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn env() -> BTreeMap<String, String> {
        [
            ("VIDEOIPATH_URL", "wss://vip.example.test/x-driver/v1"),
            ("VIDEOIPATH_TOKEN", "local-test-only$ # ' credential"),
            ("VIDEOIPATH_NODE_ID", "afe3cf0d-d906-4a89-a495-5f6a989a16c1"),
        ]
        .map(|(k, v)| (k.into(), v.into()))
        .into()
    }
    #[test]
    fn url_identity_and_literal_secret() {
        let c = Config::parse(&env(), true).unwrap();
        assert!(
            c.url
                .unwrap()
                .as_str()
                .ends_with("?node_id=afe3cf0d-d906-4a89-a495-5f6a989a16c1")
        );
        assert_eq!(c.token, "local-test-only$ # ' credential");
    }
    #[test]
    fn invalid_values_fail_without_echo() {
        for (k, v) in [
            ("VIDEOIPATH_URL", "wss://user:SECRET@vip.test/x-driver/v1"),
            ("VIDEOIPATH_TOKEN", "SECRET\r\n"),
            ("DRIVER_REQUEST_LIMIT", "-1"),
            ("DRIVER_LOG_LEVEL", "weird"),
            ("DRIVER_STATE_DIRECTORY", "relative"),
            ("DRIVER_REQUEST_TIMEOUT_MS", "8000"),
        ] {
            let mut e = env();
            e.insert(k.into(), v.into());
            let err = Config::parse(&e, true).err().unwrap().to_string();
            assert!(!err.contains("SECRET"));
        }
    }
    #[test]
    fn non_driver_configuration_needs_no_vip_credentials() {
        assert!(Config::parse(&BTreeMap::new(), false).is_ok());
    }
}
