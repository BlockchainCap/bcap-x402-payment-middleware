use std::task::{self};

use alloy::transports::TransportErrorKind;
use tower::Service;
use tracing::{debug_span, Instrument};

use alloy_transport::{BoxTransport, Transport, TransportConnect, TransportError, TransportFut, TransportResult};
use alloy_json_rpc::{RequestPacket, ResponsePacket};
use reqwest_middleware::ClientWithMiddleware;

#[derive(Clone)]
pub struct PaymentTransport {
    client: ClientWithMiddleware,
    url: reqwest::Url,
}

impl PaymentTransport {
    pub fn new(client: ClientWithMiddleware, url: reqwest::Url) -> Self {
        Self { client, url }
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
        // x402 middleware lives *inside* self.client. By the time this returns,
        // any 402 -> pay -> retry dance should already be handled.
        let resp = self
            .client
            .post(self.url.clone())
            .body(serde_json::to_string(&req).unwrap())
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

