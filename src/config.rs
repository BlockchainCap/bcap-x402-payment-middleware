use serde::Deserialize;
use std::env;
use std::fs;
use std::path::Path;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Failed to read config file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

/// Settings loaded from config.toml
#[derive(Debug, Deserialize)]
struct TomlConfig {
    node_url: String,
    price_per_request: f64,
    port: u16,
    facilitator_url: String,
}

/// Complete application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// URL of the Ethereum node to relay requests to
    pub node_url: String,

    /// Price per RPC request in USDC
    pub price_per_request: f64,

    /// Port to bind the server to
    pub port: u16,

    /// x402 facilitator URL
    pub facilitator_url: String,

    /// EVM address to receive payments
    pub payment_address: String,
}

impl Config {
    /// Load configuration from .env and config.toml
    pub fn load() -> Result<Self, ConfigError> {
        // Load .env file (secrets)
        dotenvy::dotenv().ok();

        // Load payment address from environment
        let payment_address = env::var("PAYMENT_ADDRESS")
            .map_err(|_| ConfigError::MissingEnvVar("PAYMENT_ADDRESS".to_string()))?;

        // Validate payment address format (basic check for 0x prefix and length)
        if !payment_address.starts_with("0x") || payment_address.len() != 42 {
            return Err(ConfigError::Invalid(
                "PAYMENT_ADDRESS must be a valid EVM address (0x... with 42 characters)".to_string(),
            ));
        }

        // Load config.toml (settings)
        let config_path = env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".to_string());
        let toml_config = Self::load_toml(&config_path)?;

        // Validate node URL
        if toml_config.node_url.is_empty() {
            return Err(ConfigError::Invalid("node_url cannot be empty".to_string()));
        }

        // Validate price
        if toml_config.price_per_request < 0.0 {
            return Err(ConfigError::Invalid(
                "price_per_request cannot be negative".to_string(),
            ));
        }

        Ok(Config {
            node_url: toml_config.node_url,
            price_per_request: toml_config.price_per_request,
            port: toml_config.port,
            facilitator_url: toml_config.facilitator_url,
            payment_address,
        })
    }

    fn load_toml(path: &str) -> Result<TomlConfig, ConfigError> {
        let path = Path::new(path);
        let contents = fs::read_to_string(path)?;
        let config: TomlConfig = toml::from_str(&contents)?;
        Ok(config)
    }
}

