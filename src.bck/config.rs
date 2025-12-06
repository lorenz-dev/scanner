use std::fs;

use anyhow::Result;
use serde::Deserialize;
use tracing::info;

#[derive(Deserialize)]
pub struct ScannerConfig {
    pub arb_programs: Vec<String>,
    pub base_asset: String,
    pub grpc: GrpcConfig,
    pub transaction_ttl_sec: u64,
}

impl Default for ScannerConfig {
    fn default() -> Self {
        Self {
            arb_programs: vec![],
            base_asset: String::from("So11111111111111111111111111111111111111112"),
            grpc: GrpcConfig::default(),
            transaction_ttl_sec: 60,
        }
    }
}

#[derive(Deserialize, Clone)]
pub struct GrpcEndpoint {
    pub url: String,
    pub x_token: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct GrpcConfig {
    #[serde(default)]
    pub endpoints: Vec<GrpcEndpoint>,
}

impl ScannerConfig {
    pub fn from_toml_file(path: String) -> Result<Self> {
        let content = fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&content)?;
        info!("Loaded config from {}", path);
        Ok(config)
    }
}
