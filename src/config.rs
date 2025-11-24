use std::path::Path;

use anyhow::Context;
use serde::Deserialize;
use tracing::info;

use crate::grpc::GrpcConfig;

#[derive(Deserialize)]
pub struct Config {
    pub grpc: Vec<GrpcConfig>,
    pub arb_programs: Vec<String>,
    pub ttl: TtlConfig,
}

#[derive(Deserialize)]
pub struct TtlConfig {
    pub transaction_ttl_secs: u64,
    pub ttl_interval_secs: u64,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path.as_ref())
            .context("Failed to read config file")?;

        let config = if path.as_ref().extension().and_then(|s| s.to_str()) == Some("json") {
            serde_json::from_str(&content).context("Failed to parse JSON config")?
        } else {
            toml::from_str(&content).context("Failed to parse TOML config")?
        };

        let config_path = std::fs::canonicalize(&path)
            .unwrap_or_else(|_| path.as_ref().to_path_buf());

        info!("Loaded config from {}", config_path.display());

        Ok(config)
    }
}
