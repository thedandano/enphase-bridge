mod auth;
mod inverters;
mod meters;

use reqwest::Client;
use serde::Deserialize;

// Re-export public symbols so existing call sites (tests, recompute, etc.) remain unchanged.
pub use meters::{ChannelReading, MeterReadings, extract_cumulatives_from_json};

/// Extract the sessionId value from a Set-Cookie header value.
/// Expects format: "sessionId=<value>; attr; attr"
pub fn parse_session_cookie(header_value: &str) -> Option<String> {
    header_value
        .split(';')
        .next()
        .and_then(|s| s.trim().strip_prefix("sessionId="))
        .map(str::to_owned)
}

/// HTTP timeout for all requests to the IQ Gateway.
const HTTP_TIMEOUT_SECS: u64 = 10;

/// `measurementType` value for the bidirectional net-consumption meter.
pub(super) const NET_CONSUMPTION_MEASUREMENT_TYPE: &str = "net-consumption";

/// Expected `state` value for an active meter.
pub(super) const METER_STATE_ENABLED: &str = "enabled";

pub struct GatewayClient {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) client: Client,
    pub(super) session_id: Option<String>,
}

impl GatewayClient {
    pub fn new(host: String, token: String) -> Self {
        // Self-signed TLS cert on the IQ Gateway — accept invalid certs for this client only.
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
            .build()
            .expect("failed to build gateway HTTP client");

        let base_url = if host.starts_with("http") {
            host
        } else {
            format!("https://{}", host)
        };

        Self {
            base_url,
            token,
            client,
            session_id: None,
        }
    }

    pub(super) fn cookie_header(&self) -> Option<String> {
        self.session_id.as_ref().map(|id| format!("sessionId={id}"))
    }

    pub(super) fn auth_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

/// Deserializable meter info from GET /ivp/meters — used by probe_meters.
#[derive(Debug, Deserialize)]
pub(super) struct MeterInfo {
    pub eid: u64,
    #[serde(rename = "measurementType")]
    pub measurement_type: String,
    pub state: String,
}
