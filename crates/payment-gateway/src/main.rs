mod config;
mod database;
mod handlers;
mod signature_cache;
mod state;

use axum::{routing::{get, post}, Router};
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use config::Config;
use state::AppState;

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bcap_x402_middleware=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
    
    tracing::info!("Starting x402 Prepayment RPC Gateway");

    // Load configuration
    let config = Config::load().expect("Failed to load configuration");
    tracing::info!(
        node_url = %config.node_url,
        port = config.port,
        price_per_request = config.price_per_request,
        database_path = %config.database_path,
        payment_address = %config.payment_address,
        "Configuration loaded"
    );

    // Create application state (opens database)
    let state = Arc::new(AppState::new(config.clone()));

    tracing::info!(
        facilitator = %config.facilitator_url,
        "Prepayment system initialized"
    );

    // Build router - single endpoint, no x402 layer
    let app = Router::new()
        // Health check endpoint
        .route("/health", get(handlers::health))
        // Main relay endpoint - handles authentication and payments
        .route("/relay", post(handlers::relay))
        .with_state(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!(
        address = %listener.local_addr().unwrap(),
        "Server listening"
    );

    axum::serve(listener, app).await.unwrap();
}

