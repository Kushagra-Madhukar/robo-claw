use aria_core::{AgentRequest, GatewayChannel};
use serde::Deserialize;

use crate::{normalizer::build_text_request, GatewayError};

#[derive(Debug, Deserialize)]
struct WebSocketPayload {
    session_id: u64,
    user_id: String,
    text: String,
    timestamp_us: u64,
}

pub struct WebSocketNormalizer;

impl WebSocketNormalizer {
    pub fn normalize(json: &str) -> Result<AgentRequest, GatewayError> {
        let payload: WebSocketPayload =
            serde_json::from_str(json).map_err(|e| GatewayError::ParseError(e.to_string()))?;
        Ok(build_text_request(
            GatewayChannel::WebSocket,
            payload.user_id,
            payload.session_id,
            payload.session_id,
            payload.text,
            payload.timestamp_us,
        ))
    }
}
