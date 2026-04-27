use zeroize::Zeroizing;

use crate::api::middleware::api_key::{generate_api_key, resolve_api_key};
use crate::config::ApiConfig;

/// Fatal errors that terminate startup with a specific exit code.
#[derive(Debug)]
pub enum StartupError {
    /// Gateway token expired; exit code 1.
    TokenExpired,
    /// Configuration is invalid; exit code 2.
    InvalidConfig(String),
    /// Unrecoverable runtime error; exit code 3.
    Runtime(anyhow::Error),
}

impl StartupError {
    pub fn exit_code(&self) -> i32 {
        match self {
            Self::TokenExpired => 1,
            Self::InvalidConfig(_) => 2,
            Self::Runtime(_) => 3,
        }
    }
}

impl std::fmt::Display for StartupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "gateway token expired — update config.toml"),
            Self::InvalidConfig(msg) => write!(f, "configuration error: {msg}"),
            Self::Runtime(e) => write!(f, "{e}"),
        }
    }
}

impl From<anyhow::Error> for StartupError {
    fn from(e: anyhow::Error) -> Self {
        Self::Runtime(e)
    }
}

/// Resolves the API key from config:
/// - auth disabled → `Ok(None)`
/// - key absent / blank → auto-generates, prints to stderr, returns `Ok(Some(_))`
/// - key < 32 chars → `Err(InvalidConfig)`
/// - valid key → `Ok(Some(_))`
///
/// The auto-generated key is printed to stderr (not via tracing) to keep it
/// out of structured log sinks that may forward to external aggregators.
pub fn resolve_active_api_key(cfg: &ApiConfig) -> Result<Option<Zeroizing<String>>, StartupError> {
    if !cfg.require_auth {
        return Ok(None);
    }

    match resolve_api_key(cfg.api_key.clone()) {
        Err(msg) => Err(StartupError::InvalidConfig(msg)),
        Ok(None) => {
            let key = Zeroizing::new(generate_api_key());
            // Print directly to stderr — not via tracing — so the secret stays
            // out of any tracing subscriber forwarding to Loki, Datadog, etc.
            eprintln!("[enphase-bridge] API_KEY_GENERATED: {}", *key);
            tracing::warn!(
                event = "API_KEY_GENERATED",
                message = "Auto-generated API key written to stderr. Set api_key in config.toml for a stable key."
            );
            tracing::warn!(
                event = "api_key_ephemeral",
                message = "This key changes on every restart."
            );
            Ok(Some(key))
        }
        Ok(Some(key)) => {
            tracing::info!(event = "api_auth_enabled", key_len = key.len());
            Ok(Some(Zeroizing::new(key)))
        }
    }
}
