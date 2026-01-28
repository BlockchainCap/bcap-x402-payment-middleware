use std::env;
use std::str::FromStr;
use std::sync::Arc;
use std::task::{self, Context, Poll};

use alloy::primitives::Address;
use reqwest::Client;
use tracing_subscriber::EnvFilter;
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use payment_transport::PaymentTransport;
use alloy::{signers::local::PrivateKeySigner, transports::TransportErrorKind};
use alloy::providers::ProviderBuilder;
use tower::Service;
use tracing::{debug, info, debug_span, trace, Instrument};
use alloy::providers::Provider;
use tokio::time::Instant;
use alloy::primitives::FixedBytes;
use alloy::primitives::BlockHash;
use x402_rs::scheme::v2_eip155_exact::client::V2Eip155ExactClient;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // Create a signer that will be credited for API requests
    let signer: PrivateKeySigner = env::var("PRIVATE_KEY")
        .expect("PRIVATE_KEY env variable required")
        .parse()
        .unwrap();

    // Create a custom transport layer that embeds the micropayments middleware
    let transport = PaymentTransport::new("http://localhost:3000/relay".parse().unwrap(), signer);

    // Create an EVM provider normally, include the transport layer
    let provider = ProviderBuilder::new().connect_with(&transport).await.unwrap();

    // Use the provider normally
    let block_number = provider.get_block_number().await.map_err(|e| {
        eprintln!("provider error: {e:?}");
        e
    }).unwrap();

    println!("Current block number is {:?}", block_number);
}
