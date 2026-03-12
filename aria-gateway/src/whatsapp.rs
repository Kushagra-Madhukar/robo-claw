use aria_core::{AgentRequest, GatewayChannel};
use serde::Deserialize;

use crate::{normalizer::build_text_request, GatewayError};

#[derive(Debug, Deserialize)]
struct WhatsAppPayload {
    user_id: String,
    chat_id: u64,
    text: String,
    timestamp_us: u64,
}

pub struct WhatsAppNormalizer;

impl WhatsAppNormalizer {
    pub fn normalize(json: &str) -> Result<AgentRequest, GatewayError> {
        let payload: WhatsAppPayload =
            serde_json::from_str(json).map_err(|e| GatewayError::ParseError(e.to_string()))?;
        Ok(build_text_request(
            GatewayChannel::WhatsApp,
            payload.user_id,
            payload.chat_id,
            payload.chat_id,
            payload.text,
            payload.timestamp_us,
        ))
    }
}
