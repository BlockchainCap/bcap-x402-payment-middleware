# x402 EVM Middleware

A lightweight gateway that enables Ethereum full node operators to monetize their RPC endpoints using the [x402](https://x402.org) payment protocol.

## Problem

There are ~13k+ Ethereum nodes participating in the P2P layer, yet very few expose public RPC endpoints. Node operators have no natural incentive to share their resources, while developers remain dependent on a handful of centralized RPC providers.

## Solution

This middleware sits in front of your EVM node, intercepting requests and requiring x402-compatible micropayments before forwarding them. It's a plug-and-play sidecar that requires minimal setup—optionally place it behind a reverse proxy like nginx for production use.

```
Client → [x402 Middleware] → EVM Node
```

## Crates

- **`payment-gateway`** — Axum-based service that validates x402 payments and relays paid requests to the underlying node
- **`x402-transport`** — Alloy-compatible HTTP transport for clients to seamlessly interact with x402-protected RPC endpoints

## Quick Start

### Running the Gateway

```bash
cp crates/payment-gateway/config.toml.example crates/payment-gateway/config.toml
# Edit config.toml with your node URL and payment address
cargo run -p payment-gateway
```

### Client Usage

```rust
use x402_transport::PaymentTransport;
use x402_reqwest::ClientExt;
use alloy::providers::ProviderBuilder;
use alloy::providers::Provider;

let signer: PrivateKeySigner =
        "0x...".parse().unwrap();
    
let reqwest_client_builder = Client::new()
    .with_payments(signer.clone())
    .build();

let transport = PaymentTransport::new(reqwest_client_builder, "<RPC_URL/relay>".parse().unwrap(), signer);
let provider = ProviderBuilder::new().connect_with(&transport).await.unwrap();

let chain_id = provider.get_chain_id().await?;
```

## Configuration

| Option | Description |
|--------|-------------|
| `node_url` | URL of your Ethereum node |
| `price_per_request` | Price per RPC call in USDC |
| `port` | Port to bind the middleware |
| `facilitator_url` | x402 facilitator endpoint |
| `payment_address` | Address to receive payments |

## License

MIT
