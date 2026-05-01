use crate::error::{AppError, GatewayError};
use crate::inverter::snapshot::{InverterReport, parse_snapshots};
use crate::storage::models::MicroinverterSnapshot;
use reqwest::Client;
use reqwest::header::SET_COOKIE;
use serde::Deserialize;
use tracing::{debug, error, info, instrument, warn};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MeterReadings {
    pub production_w_now: f64,
    pub consumption_w_now: f64,
    pub grid_w_now: f64,
    /// Lifetime Wh produced by solar (actEnergyDlvd on production meter, EID 704643328)
    pub production_cum_wh: f64,
    /// Lifetime Wh imported from grid (actEnergyDlvd on net-consumption meter, EID 704643584)
    pub grid_import_cum_wh: f64,
    /// Lifetime Wh exported to grid (actEnergyRcvd on net-consumption meter, EID 704643584)
    pub grid_export_cum_wh: f64,
    /// Raw response body from the same HTTP request that produced the cumulative fields.
    /// Must never be reused across poll ticks.
    pub raw_json: String,
    /// Per-channel readings extracted from meters that have channels
    pub channel_readings: Vec<ChannelReading>,
}

#[derive(Debug, Clone)]
pub struct ChannelReading {
    pub meter_eid: u64,
    pub channel_eid: u64,
    pub active_power: f64,
    pub act_energy_dlvd: f64,
    pub act_energy_rcvd: f64,
}

const EID_PRODUCTION: u64 = 704643328;
const EID_CONSUMPTION: u64 = 704643584;
const EID_NET: u64 = 1023410688;

#[derive(Debug, Clone, Deserialize)]
pub struct MeterChannel {
    pub eid: u64,
    #[serde(rename = "activePower", default)]
    pub active_power: f64,
    #[serde(rename = "actEnergyDlvd", default)]
    pub act_energy_dlvd: f64,
    #[serde(rename = "actEnergyRcvd", default)]
    pub act_energy_rcvd: f64,
}

#[derive(Debug, Deserialize)]
struct MeterObject {
    eid: u64,
    #[serde(rename = "activePower", default)]
    active_power: f64,
    #[serde(rename = "actEnergyDlvd", default)]
    act_energy_dlvd: f64,
    #[serde(rename = "actEnergyRcvd", default)]
    act_energy_rcvd: f64,
    #[serde(default)]
    channels: Option<Vec<MeterChannel>>,
}

#[derive(Debug, Deserialize)]
struct MeterInfo {
    eid: u64,
    #[serde(rename = "measurementType")]
    measurement_type: String,
    state: String,
}

pub struct GatewayClient {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) client: Client,
    session_id: Option<String>,
}

impl GatewayClient {
    pub fn new(host: String, token: String) -> Self {
        // Self-signed TLS cert on the IQ Gateway — accept invalid certs for this client only.
        let client = Client::builder()
            .danger_accept_invalid_certs(true)
            .timeout(std::time::Duration::from_secs(10))
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

    /// Exchange the cloud JWT for a local session token via POST /auth/check_jwt.
    /// IQ Gateway firmware 7.x+ requires this session cookie for /ivp/ endpoints.
    pub async fn check_jwt(&mut self) -> Result<(), AppError> {
        let url = format!("{}/auth/check_jwt", self.base_url);
        debug!(event = "check_jwt_request", url = %url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| AppError::Gateway(GatewayError::Request(e)))?;

        let status = response.status();
        if !status.is_success() {
            error!(event = "session_auth_failed", status = %status);
            return Err(AppError::Gateway(GatewayError::Unauthorized));
        }

        self.session_id = response
            .headers()
            .get(SET_COOKIE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_session_cookie);

        debug!(
            event = "session_acquired",
            has_session = self.session_id.is_some()
        );
        Ok(())
    }

    /// Probe GET /ivp/meters to validate that the net-consumption meter is present and enabled.
    /// Called once at scheduler startup after check_jwt(); halts if the required meter is absent.
    pub async fn probe_meters(&mut self) -> Result<(), AppError> {
        let url = format!("{}/ivp/meters", self.base_url);
        debug!(event = "probe_meters_request", url = %url);

        let mut req = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));

        if let Some(cookie) = self.cookie_header() {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            error!(event = "probe_meters_failed", error = %e);
            AppError::Gateway(GatewayError::Request(e))
        })?;

        if !response.status().is_success() {
            return Err(AppError::Gateway(GatewayError::Unreachable(format!(
                "meters probe returned HTTP {}",
                response.status()
            ))));
        }

        let meters: Vec<MeterInfo> = response.json().await.map_err(|e| {
            error!(event = "probe_meters_parse_error", error = %e);
            AppError::Gateway(GatewayError::MalformedResponse(e.to_string()))
        })?;

        let net_cons = meters
            .iter()
            .find(|m| m.measurement_type == "net-consumption");

        match net_cons {
            None => {
                let seen: Vec<&str> = meters.iter().map(|m| m.measurement_type.as_str()).collect();
                error!(
                    event = "required_meter_absent",
                    meter_type = "net-consumption",
                    seen_types = ?seen
                );
                Err(AppError::Gateway(GatewayError::MissingMeter(
                    "net-consumption".to_string(),
                )))
            }
            Some(m) if m.state != "enabled" => {
                error!(
                    event = "meter_disabled",
                    meter_type = "net-consumption",
                    state = %m.state
                );
                Err(AppError::Gateway(GatewayError::MissingMeter(format!(
                    "net-consumption meter is {} (not enabled)",
                    m.state
                ))))
            }
            Some(m) => {
                info!(
                    event = "meters_discovered",
                    net_consumption_eid = m.eid,
                    net_consumption_state = %m.state
                );
                Ok(())
            }
        }
    }

    fn cookie_header(&self) -> Option<String> {
        self.session_id.as_ref().map(|id| format!("sessionId={id}"))
    }

    #[instrument(skip(self), fields(endpoint = "/ivp/meters/readings"))]
    pub async fn get_meter_readings(&mut self) -> Result<MeterReadings, AppError> {
        match self.request_meter_readings().await {
            Err(AppError::Gateway(GatewayError::Unauthorized)) => {
                self.check_jwt().await?;
                self.request_meter_readings().await
            }
            other => other,
        }
    }

    async fn request_meter_readings(&self) -> Result<MeterReadings, AppError> {
        let url = format!("{}/ivp/meters/readings", self.base_url);
        debug!(event = "gateway_request", url = %url);

        let mut req = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token));

        if let Some(cookie) = self.cookie_header() {
            req = req.header("Cookie", cookie);
        }

        let response = req.send().await.map_err(|e| {
            error!(event = "gateway_request_failed", error = %e);
            AppError::Gateway(GatewayError::Request(e))
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(AppError::Gateway(GatewayError::Unauthorized));
        }
        if !status.is_success() {
            error!(event = "gateway_error_status", status = %status);
            return Err(AppError::Gateway(GatewayError::Unreachable(format!(
                "gateway returned HTTP {}",
                status
            ))));
        }

        let body = response.text().await.map_err(|e| {
            error!(event = "gateway_parse_error", error = %e);
            AppError::Gateway(GatewayError::MalformedResponse(e.to_string()))
        })?;

        extract_cumulatives_from_json(&body)
    }

    #[instrument(skip(self), fields(endpoint = "/api/v1/production/inverters"))]
    pub async fn get_inverter_snapshots(
        &self,
        window_start: i64,
    ) -> Result<Vec<MicroinverterSnapshot>, AppError> {
        let url = format!("{}/api/v1/production/inverters", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| AppError::Gateway(GatewayError::Request(e)))?;

        if !response.status().is_success() {
            return Err(AppError::Gateway(GatewayError::Unreachable(format!(
                "inverters endpoint returned HTTP {}",
                response.status()
            ))));
        }

        let reports: Vec<InverterReport> = response
            .json()
            .await
            .map_err(|e| AppError::Gateway(GatewayError::MalformedResponse(e.to_string())))?;

        Ok(parse_snapshots(reports, window_start))
    }
}

/// Parse a raw `/ivp/meters/readings` JSON response body into `MeterReadings`.
/// This is the pure extraction logic decoupled from the HTTP transport.
pub fn extract_cumulatives_from_json(raw: &str) -> Result<MeterReadings, AppError> {
    let meters: Vec<MeterObject> = serde_json::from_str(raw).map_err(|e| {
        error!(event = "gateway_parse_error", error = %e);
        AppError::Gateway(GatewayError::MalformedResponse(e.to_string()))
    })?;

    let prod = meters.iter().find(|m| m.eid == EID_PRODUCTION);
    let cons = meters
        .iter()
        .find(|m| m.eid == EID_CONSUMPTION)
        .ok_or_else(|| {
            AppError::Gateway(GatewayError::MissingMeter("net-consumption".to_string()))
        })?;
    // EID_NET is undocumented; used only for optional real-time grid_w_now (display only, not window math)
    let net = meters.iter().find(|m| m.eid == EID_NET);

    let mut channel_readings = Vec::new();
    for m in &meters {
        match &m.channels {
            None => {
                warn!(event = "channels_absent", meter_eid = m.eid);
            }
            Some(channels) => {
                for ch in channels {
                    channel_readings.push(ChannelReading {
                        meter_eid: m.eid,
                        channel_eid: ch.eid,
                        active_power: ch.active_power,
                        act_energy_dlvd: ch.act_energy_dlvd,
                        act_energy_rcvd: ch.act_energy_rcvd,
                    });
                }
            }
        }
    }

    Ok(MeterReadings {
        production_w_now: prod.map(|m| m.active_power).unwrap_or(0.0),
        // Consumption activePower is negative in Enphase convention — negate to positive watts
        consumption_w_now: -cons.active_power,
        grid_w_now: net.map(|m| m.active_power).unwrap_or(0.0),
        production_cum_wh: prod.map(|m| m.act_energy_dlvd).unwrap_or(0.0),
        grid_import_cum_wh: cons.act_energy_dlvd,
        grid_export_cum_wh: cons.act_energy_rcvd,
        raw_json: raw.to_string(),
        channel_readings,
    })
}

/// Extract the sessionId value from a Set-Cookie header value.
/// Expects format: "sessionId=<value>; attr; attr"
pub fn parse_session_cookie(header_value: &str) -> Option<String> {
    header_value
        .split(';')
        .next()
        .and_then(|s| s.trim().strip_prefix("sessionId="))
        .map(str::to_owned)
}
