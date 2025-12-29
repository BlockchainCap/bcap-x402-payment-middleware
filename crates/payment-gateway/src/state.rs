use crate::config::Config;
use crate::database::DatabaseTrait;
use crate::signature_cache::SignatureCache;
use reqwest::Client;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use x402_axum::facilitator_client::FacilitatorClient;

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// HTTP client for relaying requests to the node
    pub client: Client,

    /// Application configuration
    pub config: Config,

    /// Database for persistent user balances (trait object for flexibility)
    pub database: Arc<dyn DatabaseTrait>,

    /// In-memory signature cache for replay attack prevention
    pub signature_cache: Arc<Mutex<SignatureCache>>,

    /// X402 facilitator client for payment verification and settlement
    pub facilitator: Arc<FacilitatorClient>,
}

impl AppState {
    /// Create new application state with configured HTTP client and database
    pub fn new(config: Config, database: Arc<dyn DatabaseTrait>) -> Self {
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

        // Initialize signature cache
        let signature_cache = SignatureCache::new();

        // Initialize X402 facilitator client
        let facilitator = FacilitatorClient::try_from(config.facilitator_url.as_str())
            .expect("Failed to create facilitator client");

        Self {
            client,
            config,
            database,
            signature_cache: Arc::new(Mutex::new(signature_cache)),
            facilitator: Arc::new(facilitator),
        }
    }
}

