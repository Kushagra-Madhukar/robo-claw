use aria_core::{AgentRequest, GatewayChannel};
use serde::Deserialize;

use crate::{normalizer::build_text_request, GatewayError};

#[derive(Debug, Deserialize)]
struct DiscordPayload {
    author_id: String,
    channel_id: u64,
    content: String,
    timestamp_us: u64,
}

pub struct DiscordNormalizer;

impl DiscordNormalizer {
    pub fn normalize(json: &str) -> Result<AgentRequest, GatewayError> {
        let payload: DiscordPayload =
            serde_json::from_str(json).map_err(|e| GatewayError::ParseError(e.to_string()))?;
        Ok(build_text_request(
            GatewayChannel::Discord,
            payload.author_id,
            payload.channel_id,
            payload.channel_id,
            payload.content,
            payload.timestamp_us,
        ))
    }
}
