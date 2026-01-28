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

    // testing signer
    // let signer: PrivateKeySigner = env::var("PRIVATE_KEY").expect("PRIVATE_KEY env required").parse().unwrap();

    let mut x402_client = X402Client::new();
    let signer: PrivateKeySigner = env::var("PRIVATE_KEY")
            .expect("PRIVATE_KEY env variable required")
            .parse()
            .unwrap();
    
    // Register eip155 "exact" scheme
    {
        println!("Using EVM signer address: {:?}", signer.clone().address());
        let signer = Arc::new(signer.clone());
        x402_client = x402_client
            .register(V2Eip155ExactClient::new(signer.clone()));
        println!("Enabled eip155 exact scheme")
    };

    let client = Client::new().with_payments(x402_client).build();
    let transport = PaymentTransport::new(client, "http://localhost:3000/relay".parse().unwrap(), signer);
    let provider = ProviderBuilder::new().connect_with(&transport).await.unwrap();

    // let provider = ProviderBuilder::new().connect_http("https://ethereum-rpc.publicnode.com".parse().unwrap());
    // Average duration with payment: 0.14583168316831682s
    let mut average_duration: i32 = 0;
    let iterations = 105;
    for _i in 0..iterations {
        // compute response time
        let start = Instant::now();
        info!("Getting balance. Start time: {:?}", start);
        let response = provider.get_block_by_hash(BlockHash::from_str("0x26a6a51b13e7ea2af8008035af560f1d6f49fb00e8318a266d1f6bbec9ac7199").unwrap()).await.map_err(|e| {
            eprintln!("provider error: {e:?}");
            e
        }).unwrap();
        let end = Instant::now();
        let duration = end.duration_since(start);
        info!("Response time: {:?}. Transaction: {:?}", duration, response.unwrap().header);
        average_duration = average_duration.saturating_add(duration.as_millis() as i32);
    }
    info!("Average duration: {:?}s", average_duration as f64 / iterations as f64 / 1000.0);
}
