mod config;
mod handlers;
mod state;

use axum::{routing::{get, post}, Router};
use std::str::FromStr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use x402_axum::X402Middleware;
use x402_rs::network::{Network, USDCDeployment};
use x402_rs::types::{EvmAddress, MixedAddress};
use x402_axum::IntoPriceTag;

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
    
    tracing::debug!("Debug logging is enabled");
    tracing::info!("Starting x402 EVM RPC Middleware");

    // Load configuration
    let config = Config::load().expect("Failed to load configuration");
    tracing::info!(
        node_url = %config.node_url,
        port = config.port,
        price = config.price_per_request,
        "Configuration loaded"
    );

    // Create application state
    let state = Arc::new(AppState::new(config.clone()));

    // Setup x402 middleware
    let x402 = X402Middleware::try_from(config.facilitator_url.as_str())
        .expect("Failed to create x402 middleware");

    // Configure USDC deployment for Base Sepolia
    let usdc_deployment = USDCDeployment::by_network(Network::BaseSepolia);

    // Parse payment address
    let payment_address = MixedAddress::Evm(
        EvmAddress::from_str(&config.payment_address)
            .expect("Invalid payment address"),
    );

    // Configure price tag
    let x402_configured = x402.with_price_tag(
        usdc_deployment
            .amount(config.price_per_request)
            .pay_to(payment_address)
            .build()
            .expect("Failed to configure price tag"),
    );

    tracing::info!(
        facilitator = %config.facilitator_url,
        price_usdc = config.price_per_request,
        payment_address = %config.payment_address,
        "x402 middleware configured for Base Sepolia"
    );

    // Build router
    let app = Router::new()
        // Health check - not paywalled
        .route("/health", get(handlers::health))
        // Paid relay endpoint - protected by x402
        .route(
            "/paid-relay",
            post(handlers::paid_relay).layer(x402_configured),
        )
        .with_state(state);

    // Start server
    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind to address");

    tracing::info!("Listening on {}", listener.local_addr().unwrap());

    axum::serve(listener, app).await.unwrap();
}

