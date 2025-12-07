use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use tracing::instrument;

use crate::state::AppState;

/// Relay JSON-RPC request to the configured Ethereum node
///
/// This handler:
/// 1. Receives the raw request body (JSON-RPC payload)
/// 2. Forwards it to the configured node URL
/// 3. Returns the node's response to the client
#[instrument(skip_all, fields(body_size))]
pub async fn paid_relay(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Response {
    tracing::Span::current().record("body_size", body.len());

    tracing::debug!(
        node_url = %state.config.node_url,
        "Relaying request to node"
    );

    // Forward the request to the Ethereum node
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
                format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to connect to node: {}"}},"id":null}}"#, e),
            ).into_response();
        }
    };

    // Check if the node returned an error status
    let status = response.status();
    if !status.is_success() {
        tracing::warn!(
            status = %status,
            "Node returned non-success status"
        );
    }

    // Get the response body from the node
    let response_body = match response.bytes().await {
        Ok(bytes) => bytes,
        Err(e) => {
            tracing::error!(error = %e, "Failed to read response from node");
            return (
                StatusCode::BAD_GATEWAY,
                [(header::CONTENT_TYPE, "application/json")],
                format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"Failed to read node response: {}"}},"id":null}}"#, e),
            ).into_response();
        }
    };

    tracing::debug!(
        response_size = response_body.len(),
        "Successfully relayed request"
    );

    // Return the node's response to the client
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        response_body,
    ).into_response()
}

/// Health check endpoint (not paywalled)
pub async fn health() -> &'static str {
    "OK"
}

