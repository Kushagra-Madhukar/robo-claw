use aria_core::{AgentRequest, GatewayChannel, MessageContent};

/// Build a normalized `AgentRequest` from channel inputs.
pub fn build_text_request(
    channel: GatewayChannel,
    user_id: String,
    session_seed: u64,
    request_seed: u64,
    text: String,
    timestamp_us: u64,
) -> AgentRequest {
    let mut request_id = [0u8; 16];
    request_id[0..8].copy_from_slice(&request_seed.to_le_bytes());
    request_id[8..16].copy_from_slice(&timestamp_us.to_le_bytes());

    let mut session_id = [0u8; 16];
    session_id[0..8].copy_from_slice(&session_seed.to_le_bytes());

    AgentRequest {
        request_id,
        session_id,
        channel,
        user_id,
        content: MessageContent::Text(text),
        timestamp_us,
    }
}
