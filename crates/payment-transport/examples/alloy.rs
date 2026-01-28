use std::env;

use tracing_subscriber::EnvFilter;
use payment_transport::PaymentTransport;
use alloy::{signers::local::PrivateKeySigner};
use alloy::providers::{ProviderBuilder, Provider};
use dotenvy::dotenv;

#[tokio::main]
async fn main() {
    dotenv().expect("Failed to load .env file");

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Create a signer that will be credited for API requests
    let signer: PrivateKeySigner = env::var("PRIVATE_KEY")
        .expect("PRIVATE_KEY env variable required")
        .parse()
        .unwrap();

    let url = env::var("NODE_URL")
        .expect("NODE_URL env variable required")
        .parse()
        .unwrap();

    println!("Using node URL: {}", url);

    // Create a custom transport layer that embeds the micropayments middleware
    let transport = PaymentTransport::new(url, signer);

    // Create an EVM provider normally, include the transport layer
    let provider = ProviderBuilder::new().connect_with(&transport).await.unwrap();

    // Use the provider normally
    let block_number = provider.get_block_number().await.map_err(|e| {
        eprintln!("provider error: {e:?}");
        e
    }).unwrap();

    println!("Current block number is {:?}", block_number);
}
