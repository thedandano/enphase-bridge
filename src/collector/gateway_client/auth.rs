use reqwest::header::SET_COOKIE;
use tracing::{debug, error};

use crate::error::{AppError, GatewayError};

use super::{GatewayClient, parse_session_cookie};

impl GatewayClient {
    /// Exchange the cloud JWT for a local session token via POST /auth/check_jwt.
    /// IQ Gateway firmware 7.x+ requires this session cookie for /ivp/ endpoints.
    pub async fn check_jwt(&mut self) -> Result<(), AppError> {
        let url = format!("{}/auth/check_jwt", self.base_url);
        debug!(event = "check_jwt_request", url = %url);

        let response = self
            .client
            .post(&url)
            .header("Authorization", self.auth_header())
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
}
