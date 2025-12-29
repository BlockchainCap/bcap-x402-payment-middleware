use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use std::str::FromStr;
use std::sync::Arc;
use tracing::instrument;
use serde_json::json;
use alloy::primitives::{Address, Signature};
use x402_axum::layer::X402Paygate;
use x402_rs::types::{EvmAddress, MixedAddress, PaymentRequiredResponse, PaymentRequirements, Scheme, TokenAmount, X402Version};
use x402_rs::network::Network;
use once_cell::sync::Lazy;

use crate::state::AppState;

/// Top-up amount in USDC for prepayments
const TOPUP_AMOUNT_USDC: f64 = 1.0;

/// Timestamp window in seconds - requests must be within this time
const TIMESTAMP_WINDOW_SECS: u64 = 60;

static ERR_PAYMENT_HEADER_REQUIRED: Lazy<String> =
    Lazy::new(|| "X-PAYMENT header is required".to_string());
    
/// Extract authentication headers from request
/// Returns (address, signature, timestamp) if all headers are present
fn extract_auth_headers(headers: &HeaderMap) -> Option<(String, String, u64)> {
    let address = headers.get("x-auth-address")?.to_str().ok()?.to_string();
    let signature = headers.get("x-auth-signature")?.to_str().ok()?.to_string();
    let timestamp = headers.get("x-auth-timestamp")?
        .to_str().ok()?
        .parse::<u64>().ok()?;
    
    Some((address, signature, timestamp))
}

/// Check if request has an X-Payment header (indicates payment attempt)
fn has_payment_header(headers: &HeaderMap) -> bool {
    headers.contains_key("X-Payment")
}

/// Create payment requirements for top-up
fn create_payment_requirements(state: &AppState) -> Vec<PaymentRequirements> {
    let amount_smallest_unit = (TOPUP_AMOUNT_USDC * 1_000_000.0) as u64;
    
    vec![PaymentRequirements {
        scheme: Scheme::Exact,
        network: Network::BaseSepolia,
        max_amount_required: TokenAmount::from(amount_smallest_unit),
        resource: format!("http://localhost:{}/relay", state.config.port)
            .parse()
            .unwrap(),
        description: "Top up your RPC access balance with $1 USDC".to_string(),
        mime_type: "application/json".to_string(),
        pay_to: MixedAddress::Evm(EvmAddress::from_str(&state.config.payment_address).unwrap()),
        max_timeout_seconds: 300,
        asset: MixedAddress::Evm(EvmAddress::from_str("0x036CbD53842c5426634e7929541eC2318f3dCF7e").unwrap()),
        extra: Some(json!({
            "name": "USDC",
            "version": "2"
        })),
        output_schema: None,
    }]
}

/// Verify cryptographic signature and timestamp
fn verify_signature(
    address: &str,
    signature: &str,
    timestamp: u64,
    body: &[u8],
) -> Result<(), String> {
    // Check timestamp is within acceptable window
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    if now.abs_diff(timestamp) > TIMESTAMP_WINDOW_SECS {
        return Err(format!(
            "Timestamp outside window: {} seconds drift",
            now.abs_diff(timestamp)
        ));
    }

    // Reconstruct the message that was signed
    // Format: address + timestamp + body_hash
    let body_hash = alloy::primitives::keccak256(body);
    let message = format!("{}{}{}", address, timestamp, hex::encode(body_hash));
    let message_hash = alloy::primitives::keccak256(message.as_bytes());

    // Parse and verify signature
    let sig = Signature::from_str(signature)
        .map_err(|e| format!("Invalid signature format: {}", e))?;

    let recovered_address = sig.recover_address_from_prehash(&message_hash)
        .map_err(|e| format!("Failed to recover address: {}", e))?;

    let claimed_address = address.parse::<Address>()
        .map_err(|e| format!("Invalid address format: {}", e))?;

    if recovered_address != claimed_address {
        return Err("Signature verification failed: address mismatch".to_string());
    }

    Ok(())
}

/// Return 402 Payment Required with x402 payment requirements
fn request_payment(state: &AppState) -> Response {
    let payment_required_response = PaymentRequiredResponse {
        error: ERR_PAYMENT_HEADER_REQUIRED.clone(),
        accepts: create_payment_requirements(state),
        x402_version: X402Version::V1,
    };

    (
        StatusCode::PAYMENT_REQUIRED,
        [(header::CONTENT_TYPE, "application/json")],
        serde_json::to_string(&payment_required_response).unwrap(),
    ).into_response()
}

/// Forward request to RPC node
async fn relay_to_node(state: &AppState, body: Bytes) -> Response {
    let response = match state
        .client
        .post(&state.config.node_url)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!(error = %e, "Failed to relay request to node");
            return (
                StatusCode::BAD_GATEWAY,
                [(header::CONTENT_TYPE, "application/json")],
                format!(
                    r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to connect to node: {}"}},"id":null}}"#,
                    e
                ),
            ).into_response();
        }
    };

    let status = response.status();
    let response_body = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(error = %e, "Failed to read response from node");
            return (
                StatusCode::BAD_GATEWAY,
                [(header::CONTENT_TYPE, "application/json")],
                format!(
                    r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to read node response: {}"}},"id":null}}"#,
                    e
                ),
            ).into_response();
        }
    };

    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        response_body,
    ).into_response()
}

/// Main relay endpoint - handles both payments and authenticated requests
#[instrument(skip_all, fields(body_size))]
pub async fn relay(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    tracing::Span::current().record("body_size", body.len());

    // Check if this is a payment/top-up request (has X-Payment header)
    if has_payment_header(&headers) {
        return handle_payment_with_paygate(state, headers, body).await;
    }

    // Not a payment - check for authentication headers
    let (address, signature, timestamp) = match extract_auth_headers(&headers) {
        Some(auth) => auth,
        None => {
            tracing::debug!("No authentication headers found");
            return request_payment(&state);
        }
    };

    // Check if signature has been used before (replay attack)
    {
        let mut cache = state.signature_cache.lock().unwrap();
        if cache.is_replay(&signature) {
            tracing::warn!(
                address = %address,
                signature = %signature,
                "Replay detected"
            );
            return (
                StatusCode::UNAUTHORIZED,
                "Replay detected: signature already used",
            ).into_response();
        }
    }

    // Verify signature
    if let Err(e) = verify_signature(&address, &signature, timestamp, &body) {
        tracing::warn!(
            address = %address,
            error = %e,
            "Signature verification failed"
        );
        return (
            StatusCode::UNAUTHORIZED,
            format!("Authentication failed: {}", e),
        ).into_response();
    }

    // Check user balance
    let price = state.config.price_per_request;
    
    match state.database.deduct_balance(&address, price, timestamp).await {
        Ok(remaining_balance) => {
            // Add signature to cache to prevent replay
            {
                let mut cache = state.signature_cache.lock().unwrap();
                cache.add(&signature);
            }

            tracing::info!(
                address = %address,
                deducted = price,
                remaining = remaining_balance,
                "Request authorized, balance deducted"
            );

            // Forward to RPC node
            relay_to_node(&state, body).await
        }
        Err(e) => {
            tracing::info!(
                address = %address,
                error = %e,
                required = price,
                "Insufficient balance or database error"
            );
            request_payment(&state)
        }
    }
}

/// Handle payment/deposit request using X402Paygate
async fn handle_payment_with_paygate(
    state: Arc<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    // Create payment requirements for top-up
    let payment_requirements = create_payment_requirements(&state);
    
    // Create X402Paygate to verify and settle payment
    let paygate = X402Paygate {
        facilitator: state.facilitator.clone(),
        payment_requirements: Arc::new(payment_requirements),
        settle_before_execution: false, // Settle after we add balance
    };

    // Extract and verify payment
    let payment_payload = match paygate.extract_payment_payload(&headers).await {
        Ok(payload) => payload,
        Err(err) => {
            tracing::warn!("Payment extraction failed");
            return err.into_response();
        }
    };

    // Verify payment with facilitator
    let verify_request = match paygate.verify_payment(payment_payload).await {
        Ok(request) => request,
        Err(err) => {
            tracing::warn!("Payment verification failed");
            return err.into_response();
        }
    };

    // Extract user address and amount from verified payment
    // Convert PaymentPayload to JSON to extract fields
    let payment_json = match serde_json::to_value(&verify_request.payment_payload) {
        Ok(json) => json,
        Err(e) => {
            tracing::error!("Failed to serialize payment payload: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                "Invalid payment format",
            ).into_response();
        }
    };
    
    // Extract from address - the payment payload should have an EVM authorization
    let user_address = payment_json
        .get("payload")
        .and_then(|p| p.get("authorization"))
        .and_then(|auth| auth.get("from"))
        .and_then(|from| from.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    if user_address.is_empty() {
        tracing::error!("Failed to extract user address from payment");
        return (
            StatusCode::BAD_REQUEST,
            "Invalid payment format",
        ).into_response();
    }

    // Extract amount
    let amount_raw = payment_json
        .get("payload")
        .and_then(|p| p.get("authorization"))
        .and_then(|auth| auth.get("value"))
        .and_then(|val| val.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "0".to_string());

    // Convert from string to u64 to f64 USDC (6 decimals)
    let amount_usdc = amount_raw.parse::<u64>()
        .map(|v| v as f64 / 1_000_000.0)
        .unwrap_or(0.0);

    tracing::info!(
        address = %user_address,
        amount = amount_usdc,
        "Payment verified, settling and adding to balance"
    );

    // Settle payment on-chain
    match paygate.settle_payment(&verify_request).await {
        Ok(_settlement) => {
            tracing::info!(
                address = %user_address,
                "Payment settled successfully"
            );

            // Add balance to user account
            match state.database.add_balance(&user_address, amount_usdc).await {
                Ok(new_balance) => {
                    tracing::info!(
                        address = %user_address,
                        new_balance = new_balance,
                        "Balance updated successfully"
                    );

                    // Deduct the price for this request
                    let price = state.config.price_per_request;
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    if let Err(e) = state.database.deduct_balance(&user_address, price, timestamp).await {
                        tracing::error!(
                            address = %user_address,
                            error = %e,
                            "Failed to deduct balance after deposit"
                        );
                    }

                    // Process the original request
                    relay_to_node(&state, body).await
                }
                Err(e) => {
                    tracing::error!(
                        address = %user_address,
                        error = %e,
                        "Failed to add balance"
                    );
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Failed to process payment: {}", e),
                    ).into_response()
                }
            }
        }
        Err(err) => {
            tracing::error!("Payment settlement failed");
            err.into_response()
        }
    }
}

/// Health check endpoint (not paywalled)
pub async fn health() -> &'static str {
    "OK"
}
