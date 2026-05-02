use serde::Deserialize;
use tracing::{debug, error, info, instrument, warn};

use crate::error::{AppError, GatewayError};

use super::{GatewayClient, METER_STATE_ENABLED, MeterInfo, NET_CONSUMPTION_MEASUREMENT_TYPE};

const EID_PRODUCTION: u64 = 704643328;
/// Net-consumption meter EID — documented in Enphase IQ Gateway Local APIs tech brief (Jan 2023).
/// Grid import (`actEnergyDlvd`) and grid export (`actEnergyRcvd`) are both read from this meter.
const EID_NET_CONSUMPTION: u64 = 704643584;
/// Undocumented net meter EID — used only for optional real-time grid_w_now (display, not window math).
const EID_NET: u64 = 1023410688;

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

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct MeterChannel {
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

impl GatewayClient {
    /// Probe GET /ivp/meters to validate that the net-consumption meter is present and enabled.
    /// Called once at scheduler startup after check_jwt(); halts if the required meter is absent.
    pub async fn probe_meters(&mut self) -> Result<(), AppError> {
        let url = format!("{}/ivp/meters", self.base_url);
        debug!(event = "probe_meters_request", url = %url);

        let mut req = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header());

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
            .find(|m| m.measurement_type == NET_CONSUMPTION_MEASUREMENT_TYPE);

        match net_cons {
            None => {
                let seen: Vec<&str> = meters.iter().map(|m| m.measurement_type.as_str()).collect();
                error!(
                    event = "required_meter_absent",
                    meter_type = NET_CONSUMPTION_MEASUREMENT_TYPE,
                    seen_types = ?seen
                );
                Err(AppError::Gateway(GatewayError::MissingMeter(
                    NET_CONSUMPTION_MEASUREMENT_TYPE.to_string(),
                )))
            }
            Some(m) if m.state != METER_STATE_ENABLED => {
                error!(
                    event = "meter_disabled",
                    meter_type = NET_CONSUMPTION_MEASUREMENT_TYPE,
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
            .header("Authorization", self.auth_header());

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
        .find(|m| m.eid == EID_NET_CONSUMPTION)
        .ok_or_else(|| {
            AppError::Gateway(GatewayError::MissingMeter(
                NET_CONSUMPTION_MEASUREMENT_TYPE.to_string(),
            ))
        })?;
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
