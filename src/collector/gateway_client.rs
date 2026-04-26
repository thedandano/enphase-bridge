use crate::error::{AppError, GatewayError};
use crate::inverter::snapshot::{InverterReport, parse_snapshots};
use crate::storage::models::MicroinverterSnapshot;
use reqwest::Client;
use serde::Deserialize;
use tracing::{debug, error, instrument};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct MeterReadings {
    pub production_w_now: f64,
    pub consumption_w_now: f64,
    pub grid_w_now: f64,
    /// Lifetime Wh produced by solar (actEnergyDlvd on production meter)
    pub production_cum_wh: f64,
    /// Lifetime Wh consumed by home (actEnergyDlvd on consumption meter)
    pub consumption_cum_wh: f64,
}

const EID_PRODUCTION: u64 = 704643328;
const EID_CONSUMPTION: u64 = 704643584;
const EID_NET: u64 = 1023410688;

#[derive(Debug, Deserialize)]
struct MeterObject {
    eid: u64,
    #[serde(rename = "activePower", default)]
    active_power: f64,
    #[serde(rename = "actEnergyDlvd", default)]
    act_energy_dlvd: f64,
}

pub struct GatewayClient {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) client: Client,
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
        }
    }

    #[instrument(skip(self), fields(endpoint = "/ivp/meters/readings"))]
    pub async fn get_meter_readings(&self) -> Result<MeterReadings, AppError> {
        let url = format!("{}/ivp/meters/readings", self.base_url);
        debug!(event = "gateway_request", url = %url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| {
                error!(event = "gateway_request_failed", error = %e);
                AppError::Gateway(GatewayError::Request(e))
            })?;

        let status = response.status();
        if !status.is_success() {
            error!(event = "gateway_error_status", status = %status);
            return Err(AppError::Gateway(GatewayError::Unreachable(format!(
                "gateway returned HTTP {}",
                status
            ))));
        }

        let meters: Vec<MeterObject> = response.json().await.map_err(|e| {
            error!(event = "gateway_parse_error", error = %e);
            AppError::Gateway(GatewayError::MalformedResponse(e.to_string()))
        })?;

        let prod = meters.iter().find(|m| m.eid == EID_PRODUCTION);
        let cons = meters.iter().find(|m| m.eid == EID_CONSUMPTION);
        let net = meters.iter().find(|m| m.eid == EID_NET);

        Ok(MeterReadings {
            production_w_now: prod.map(|m| m.active_power).unwrap_or(0.0),
            // Consumption activePower is negative in Enphase convention — negate to positive watts
            consumption_w_now: cons.map(|m| -m.active_power).unwrap_or(0.0),
            grid_w_now: net.map(|m| m.active_power).unwrap_or(0.0),
            production_cum_wh: prod.map(|m| m.act_energy_dlvd).unwrap_or(0.0),
            consumption_cum_wh: cons.map(|m| m.act_energy_dlvd).unwrap_or(0.0),
        })
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
