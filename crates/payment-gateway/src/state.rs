use crate::config::Config;
use reqwest::Client;
use std::time::Duration;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// HTTP client for relaying requests to the node
    pub client: Client,

    /// Application configuration
    pub config: Config,
}

impl AppState {
    /// Create new application state with configured HTTP client
    pub fn new(config: Config) -> Self {
        // Configure HTTP client with reasonable defaults for RPC relay
        let client = Client::builder()
            // Connection timeout for establishing connection to node
            .connect_timeout(Duration::from_secs(10))
            // Request timeout - some RPC calls can take longer
            .timeout(Duration::from_secs(30))
            // Enable connection pooling for better performance
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to build HTTP client");

        Self { client, config }
    }
}

