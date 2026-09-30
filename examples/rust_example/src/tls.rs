//! TLS setup happens before background tasks; diagnostics never format HTTP errors/headers.
use crate::config::{Config, ConfigError};
use rustls::{ClientConfig, RootCertStore};
use std::{io::BufReader, sync::Arc};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{HeaderValue, Request},
};

pub fn connector(config: &Config) -> Result<Arc<ClientConfig>, ConfigError> {
    let mut roots = RootCertStore::empty();
    if let Some(path) = &config.ca_bundle {
        let file = std::fs::File::open(path).map_err(|_| ConfigError("VIDEOIPATH_CA_BUNDLE"))?;
        let certs = rustls_pemfile::certs(&mut BufReader::new(file))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ConfigError("VIDEOIPATH_CA_BUNDLE"))?;
        if certs.is_empty() {
            return Err(ConfigError("VIDEOIPATH_CA_BUNDLE contains no certificates"));
        }
        for cert in certs {
            roots
                .add(cert)
                .map_err(|_| ConfigError("VIDEOIPATH_CA_BUNDLE"))?;
        }
    } else {
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        if roots.is_empty() {
            return Err(ConfigError("system TLS trust store is empty"));
        }
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|_| ConfigError("TLS protocol configuration"))?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(tls))
}

pub fn request(config: &Config) -> Result<Request<()>, ConfigError> {
    let mut request = config
        .url
        .as_ref()
        .ok_or(ConfigError("VIDEOIPATH_URL"))?
        .as_str()
        .into_client_request()
        .map_err(|_| ConfigError("VIDEOIPATH_URL"))?;
    let mut bearer = HeaderValue::from_str(&format!("Bearer {}", config.token))
        .map_err(|_| ConfigError("VIDEOIPATH_TOKEN"))?;
    bearer.set_sensitive(true);
    request.headers_mut().insert("Authorization", bearer);
    Ok(request)
}
