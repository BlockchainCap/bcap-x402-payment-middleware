# x402 EVM Middleware

A lightweight gateway that enables Ethereum full node operators to monetize their RPC endpoints using the [x402](https://x402.org) payment protocol with prepaid balances.

## Problem

There are ~13k+ Ethereum nodes participating in the P2P layer, yet very few expose public RPC endpoints. Node operators have no natural incentive to share their resources, while developers remain dependent on a handful of centralized RPC providers.

## Solution

This middleware sits in front of your EVM node using a **prepayment model**. Users deposit USDC once (e.g., $1) and consume it across multiple RPC requests. The gateway tracks balances persistently and uses cryptographic signatures for stateless authentication.

```
Client → [x402 Gateway + Balance Tracking] → EVM Node
```

### How It Works

1. **First Request**: Client sends request without balance → Receives `402 Payment Required`
2. **Deposit**: Client sends x402 payment (e.g., $1 USDC) → Balance added to account
3. **Subsequent Requests**: Client authenticates with signature → Balance checked → Request forwarded
4. **Top-up**: When balance runs low, client receives `402` and deposits again

## Architecture

### Prepayment System
- **RocksDB Storage**: Persistent balance tracking per Ethereum address
- **Signature Cache**: In-memory replay attack prevention (60-second window)
- **X402 Integration**: Uses x402 Paygate for payment verification and settlement
- **Stateless Auth**: ECDSA signatures prove identity on each request (no sessions)

### Crates

- **`payment-gateway`** — Axum-based service with balance tracking, signature authentication, and x402 settlement
- **`x402-transport`** — Alloy-compatible HTTP transport that automatically signs requests and handles payments

## Quick Start

### Running the Gateway

```bash
# Copy and configure
cp crates/payment-gateway/config.toml.example crates/payment-gateway/config.toml

# Set your payment address in .env
echo "PAYMENT_ADDRESS=0xYourAddress" > crates/payment-gateway/.env

# Edit config.toml with your node URL and pricing
# Run the gateway
cargo run -p payment-gateway
```

The gateway will:
- Create a RocksDB database for balance storage
- Listen on port 3000 (configurable)
- Accept payments via the x402 facilitator
- Verify signatures and track balances

### Client Usage

```rust
use x402_transport::PaymentTransport;
use x402_reqwest::ClientExt;
use alloy::providers::ProviderBuilder;
use alloy::providers::Provider;
use alloy::signers::local::PrivateKeySigner;

// Your signing key (same key pays and authenticates)
let signer: PrivateKeySigner = "0x...".parse().unwrap();
    
// Create x402-enabled HTTP client
let reqwest_client = Client::new()
    .with_payments(signer.clone())
    .build();

// Create transport with signer for authentication
let transport = PaymentTransport::new(
    reqwest_client, 
    "http://localhost:3000/relay".parse().unwrap(), 
    signer
);

// Use with Alloy provider
let provider = ProviderBuilder::new()
    .connect_with(&transport)
    .await
    .unwrap();

// Make requests - automatically authenticated and paid
let chain_id = provider.get_chain_id().await?;
```

## Configuration

### config.toml

| Option | Description | Example |
|--------|-------------|---------|
| `node_url` | URL of your Ethereum node | `https://ethereum-rpc.publicnode.com` |
| `price_per_request` | Price per RPC call in USDC | `0.000001` (1 micro-USDC) |
| `port` | Port to bind the middleware | `3000` |
| `facilitator_url` | x402 facilitator endpoint | `https://x402.org/facilitator` |
| `database_path` | Path to RocksDB database | `./data/gateway.db` |

### Environment Variables (.env)

| Variable | Description |
|----------|-------------|
| `PAYMENT_ADDRESS` | Your Ethereum address to receive payments (required) |

## How Pricing Works

1. **Top-up Amount**: Hardcoded at **$1 USDC** per deposit
2. **Per-Request Cost**: Configured in `config.toml` (e.g., `0.000001` USDC)
3. **Balance Tracking**: Each request deducts `price_per_request` from user's balance
4. **Persistent Storage**: Balances stored in RocksDB, survive restarts

**Example**: With `price_per_request = 0.000001`, a $1 deposit = **1,000,000 requests**

## Security Features

- **Replay Attack Prevention**: Signature cache blocks duplicate requests (60s window)
- **Timestamp Validation**: Requests must be within 60 seconds of current time
- **Cryptographic Authentication**: ECDSA signature verification on every request
- **On-Chain Settlement**: x402 payments settled via facilitator before balance credit
- **Persistent Balances**: RocksDB ensures balances survive server restarts

## Client Behavior

The client automatically:
1. **Signs every request** with private key (proves identity)
2. **Handles 402 responses** by creating x402 payment
3. **Retries after payment** to complete the original request
4. **No state management** required - completely stateless from client perspective

## License

MIT
