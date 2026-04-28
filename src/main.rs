mod api;
mod auth;
mod collector;
mod config;
mod error;
mod inverter;
mod storage;
mod tou;
mod trueup;

use tracing::info;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("enphase_bridge=info".parse()?),
        )
        .with_target(true)
        .with_current_span(false)
        .init();

    info!(event = "startup", version = env!("CARGO_PKG_VERSION"));

    let config = config::Config::load()?;
    let pool = storage::db::connect(&config.storage.db_path).await?;
    info!(event = "storage_ready", db_path = %config.storage.db_path);

    let token_manager = auth::token_manager::TokenManager::new(config.gateway.token.clone());
    if token_manager.is_expired() {
        tracing::error!(
            event = "auth_error",
            reason = "token expired — update config.toml"
        );
        std::process::exit(1);
    }
    if token_manager.is_near_expiry(std::time::Duration::from_secs(30 * 24 * 3600)) {
        tracing::warn!(
            event = "token_near_expiry",
            message = "token expires within 30 days"
        );
    }

    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let api_state = api::server::AppState {
        pool: pool.clone(),
        token_expires_at: token_manager.expiry_timestamp(),
        started_at,
        arrays: config.arrays.clone(),
        tou_api_key: config.tou.openei_api_key.clone(),
        tou_rate_label: config.tou.sdge_rate_label.clone(),
    };

    let gateway_client = collector::gateway_client::GatewayClient::new(
        config.gateway.host.clone(),
        config.gateway.token.clone(),
    );

    let scheduler = collector::scheduler::Scheduler::new(
        gateway_client,
        pool.clone(),
        config.polling.interval_secs,
    );

    let api_host = config.api.host.clone();
    let api_port = config.api.port;

    info!(
        event = "daemon_start",
        interval_secs = config.polling.interval_secs,
        api_addr = format!("{}:{}", api_host, api_port),
    );

    // Run API server and polling scheduler concurrently
    tokio::select! {
        result = api::server::serve(api_state, &api_host, api_port) => {
            if let Err(e) = result {
                tracing::error!(event = "api_server_error", error = %e);
            }
        }
        _ = scheduler.run() => {}
    }

    Ok(())
}
