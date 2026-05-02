use tracing::instrument;

use crate::error::{AppError, GatewayError};
use crate::inverter::snapshot::{InverterReport, parse_snapshots};
use crate::storage::models::MicroinverterSnapshot;

use super::GatewayClient;

impl GatewayClient {
    #[instrument(skip(self), fields(endpoint = "/api/v1/production/inverters"))]
    pub async fn get_inverter_snapshots(
        &self,
        window_start: i64,
    ) -> Result<Vec<MicroinverterSnapshot>, AppError> {
        let url = format!("{}/api/v1/production/inverters", self.base_url);

        let response = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
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
