use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub gateway: GatewayConfig,
    pub polling: PollingConfig,
    pub api: ApiConfig,
    pub storage: StorageConfig,
    pub tou: TouConfig,
    #[serde(default)]
    pub arrays: HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct GatewayConfig {
    pub host: String,
    pub token: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PollingConfig {
    pub interval_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ApiConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct StorageConfig {
    pub db_path: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct TouConfig {
    pub openei_api_key: String,
    pub sdge_rate_label: String,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        Ok(Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("ENPHASE__").split("__"))
            .extract()?)
    }
}
