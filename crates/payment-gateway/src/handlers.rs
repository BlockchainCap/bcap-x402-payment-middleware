use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::str::FromStr;
use std::sync::Arc;
use alloy::primitives::{Address, Signature};
use x402_axum::{PaygateProtocol, paygate::{Paygate, PaygateError, VerificationError}};
use x402_rs::{networks::{KnownNetworkEip155, USDC}, proto::{self, v2::{self, ResourceInfo}}, scheme::v2_eip155_exact::V2Eip155Exact, util::Base64Bytes};

use crate::errors::ApiError;
use crate::state::AppState;

/// Top-up amount in USDC for prepayments
const TOPUP_AMOUNT_USDC: f64 = 1.0;

/// Timestamp window in seconds - requests must be within this time
const TIMESTAMP_WINDOW_SECS: u64 = 60;

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

/// Create payment requirements for top-up
fn create_payment_requirements(state: &AppState) -> v2::PriceTag {
    let amount_smallest_unit = (TOPUP_AMOUNT_USDC * 1_000_000.0) as u64;

    // let requirements: PaymentRequirements = PaymentRequirements {
    //     scheme: ExactScheme.to_string(),
    //     network: ChainId::new("eip155", "84532"),
    //     amount: amount_smallest_unit.to_string(),
    //     pay_to: state.config.payment_address.clone(),
    //     asset: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".to_string(),
    //     max_timeout_seconds: 300,
    //     extra: None,
    // };

    V2Eip155Exact::price_tag(
        state.config.payment_address.parse::<Address>().unwrap(),
        USDC::base_sepolia().amount(amount_smallest_unit),
    )
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

fn request_payment(state: &AppState) -> Response {
    // Create payment requirements for top-up
    let price_tag = create_payment_requirements(state);

    // Create X402Paygate to verify and settle payment
    let paygate = Paygate {
        facilitator: state.facilitator.clone(),
        accepts: Arc::new(vec![price_tag]),
        settle_before_execution: true,
        resource: ResourceInfo {
            description: "x402 node rpc".to_string(),
            mime_type: "application/json".to_string(),
            url: format!("http://localhost:{}/relay", state.config.port)
                .parse()
                .unwrap()
        },
    };

    v2::PriceTag::error_into_response(
        PaygateError::Verification(VerificationError::PaymentHeaderRequired(v2::PriceTag::PAYMENT_HEADER_NAME)), 
            &paygate.accepts, 
            &paygate.resource
        )
}

/// Main relay endpoint - handles both payments and authenticated requests
pub async fn relay(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    tracing::Span::current().record("body_size", body.len());

    let payment_required_response = request_payment(&state);

    // check for authentication headers
    let (address, signature, timestamp) = match extract_auth_headers(&headers) {
        Some(auth) => auth,
        None => {
            tracing::debug!("No authentication headers found");
            return ApiError::ApiHeadersMissing.into_response();
        }
    };

    tracing::debug!("Auth extracted. Address: {}, Signature: {}, Timestamp: {}", address, signature, timestamp);

    // Check if signature is in cache
    {
        let mut cache = state.signature_cache.lock().unwrap();
        if cache.is_replay(&signature) {
            tracing::warn!(signature = %signature, "Signature replay detected");
            return (
                StatusCode::UNAUTHORIZED,
                format!("Signature replay detected"),
            ).into_response();
        }
        drop(cache);
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
                drop(cache);
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
            if extract_payment_header(&headers, v2::PriceTag::PAYMENT_HEADER_NAME).is_some() {
                let payment_header_value = process_payment(state.clone(), &headers).await.unwrap();

                state.database.add_balance(&address, TOPUP_AMOUNT_USDC).await.unwrap();
        
                let mut res = relay_to_node(&state, body).await;
        
                res.headers_mut().insert("X-Payment-Response", payment_header_value);
                return res.into_response();
            }

            tracing::info!("Returning payment required response");

            payment_required_response
        }
    }
}

/// Handle payment/deposit request using X402Paygate
async fn process_payment(
    state: Arc<AppState>,
    headers: &HeaderMap
) -> Result<HeaderValue, PaygateError> {
    // Create payment requirements for top-up
    let price_tag = create_payment_requirements(&state);

    // Create X402Paygate to verify and settle payment
    let paygate = Paygate {
        facilitator: state.facilitator.clone(),
        accepts: Arc::new(vec![price_tag]),
        settle_before_execution: true,
        resource: ResourceInfo {
            description: "x402 node rpc".to_string(),
            mime_type: "application/json".to_string(),
            url: format!("http://localhost:{}/relay", state.config.port)
                .parse()
                .unwrap()
        },
    };

    // Extract payment payload from headers
    let header = extract_payment_header(headers, v2::PriceTag::PAYMENT_HEADER_NAME).ok_or(
        VerificationError::PaymentHeaderRequired(v2::PriceTag::PAYMENT_HEADER_NAME),
    )?;
    
    let payment_payload = extract_payment_payload::<v2::PaymentPayload<v2::PaymentRequirements, serde_json::Value>>(header)
        .ok_or(VerificationError::InvalidPaymentHeader)?;

    let verify_request =
        v2::PriceTag::make_verify_request(payment_payload, &paygate.accepts, &paygate.resource)?;

    tracing::debug!("Verify request created. Verify request: {:?}", verify_request);

    let verify_response = paygate.verify_payment(&verify_request).await?;

    tracing::debug!("Verify response received. Verify response: {:?}", verify_response);

    v2::PriceTag::validate_verify_response(verify_response)?;

    let settlement = paygate.settle_payment(&verify_request).await?;

    let header_value = settlement_to_header(settlement)?;

    Ok(header_value)
}

/// Health check endpoint (not paywalled)
pub async fn health() -> &'static str {
    "OK"
}

/// Converts a [`proto::SettleResponse`] into an HTTP header value.
///
/// Returns an error response if conversion fails.
fn settlement_to_header(settlement: proto::SettleResponse) -> Result<HeaderValue, PaygateError> {
    let json =
        serde_json::to_vec(&settlement).map_err(|err| PaygateError::Settlement(err.to_string()))?;
    let payment_header = Base64Bytes::encode(json);
    HeaderValue::from_bytes(payment_header.as_ref())
        .map_err(|err| PaygateError::Settlement(err.to_string()))
}

/// Extracts and deserializes the payment payload from base64-encoded header bytes.
fn extract_payment_payload<T>(header_bytes: &[u8]) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let base64 = Base64Bytes::from(header_bytes).decode().ok()?;
    let value = serde_json::from_slice(base64.as_ref()).ok()?;
    Some(value)
}

/// Extracts the payment header value from the header map.
fn extract_payment_header<'a>(header_map: &'a HeaderMap, header_name: &'a str) -> Option<&'a [u8]> {
    header_map.get(header_name).map(|h| h.as_bytes())
}