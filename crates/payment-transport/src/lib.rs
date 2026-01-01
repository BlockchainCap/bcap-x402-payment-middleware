use std::task::{self};

use alloy::transports::TransportErrorKind;
use alloy::signers::{Signer, local::PrivateKeySigner};
use tower::Service;
use tracing::{debug_span, Instrument};

use alloy_transport::{BoxTransport, Transport, TransportConnect, TransportError, TransportFut, TransportResult};
use alloy_json_rpc::{RequestPacket, ResponsePacket};
use reqwest_middleware::ClientWithMiddleware;

#[derive(Clone)]
pub struct PaymentTransport {
    client: ClientWithMiddleware,
    url: reqwest::Url,
    signer: PrivateKeySigner,
}

impl PaymentTransport {
    pub fn new(client: ClientWithMiddleware, url: reqwest::Url, signer: PrivateKeySigner) -> Self {
        Self { client, url, signer }
    }
}

impl Service<RequestPacket> for PaymentTransport {
    type Response = ResponsePacket;
    type Error = TransportError;
    type Future = TransportFut<'static>;

    #[inline]
    fn poll_ready(&mut self, _cx: &mut task::Context<'_>) -> task::Poll<Result<(), Self::Error>> {
        // `reqwest` always returns `Ok(())`.
        task::Poll::Ready(Ok(()))
    }

    #[inline]
    fn call(&mut self, req: RequestPacket) -> Self::Future {
        let this = self.clone();
        let span = debug_span!("ReqwestTransport", url = %this.url);
        Box::pin(this.do_reqwest(req).instrument(span))
    }
}

impl PaymentTransport {
    async fn do_reqwest(self, req: RequestPacket) -> TransportResult<ResponsePacket> {
        // Serialize request body
        let body = serde_json::to_string(&req).unwrap();
        let body_bytes = body.as_bytes();
        
        // Generate authentication headers
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let address = self.signer.address();
        
        // Sign: address + timestamp + keccak256(body)
        let body_hash = alloy::primitives::keccak256(body_bytes);
        let message = format!("{}{}{}", address, timestamp, hex::encode(body_hash));
        let message_hash = alloy::primitives::keccak256(message.as_bytes());
        
        let signature = self.signer
            .sign_hash(&message_hash)
            .await
            .map_err(|e| TransportErrorKind::custom(e))?;

        tracing::debug!(
            address = %address,
            timestamp = timestamp,
            "Authenticated request"
        );

        // x402 middleware lives *inside* self.client. By the time this returns,
        // any 402 -> pay -> retry dance should already be handled.
        let resp = self
            .client
            .post(self.url.clone())
            .header("X-Auth-Address", address.to_string())
            .header("X-Auth-Signature", signature.to_string())
            .header("X-Auth-Timestamp", timestamp.to_string())
            .body(body)
            .send()
            .await
            .map_err(TransportErrorKind::custom)?;

        let status = resp.status();
        let body = resp.bytes().await.map_err(TransportErrorKind::custom)?;

        if !status.is_success() {
            // At this point, non-2xx is *not* x402 — it's a genuine error.
            return Err(TransportErrorKind::http_error(
                status.as_u16(),
                String::from_utf8_lossy(&body).into_owned(),
            ));
        }

        serde_json::from_slice(&body)
            .map_err(|err| TransportError::deser_err(err, String::from_utf8_lossy(&body)))
    }
}

impl TransportConnect for PaymentTransport {
    fn is_local(&self) -> bool {
        false
    }

    async fn get_transport(&self) -> Result<BoxTransport, TransportError> {
        Ok(self.clone().boxed())
    }
}

