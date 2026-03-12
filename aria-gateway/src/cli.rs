use aria_core::{AgentRequest, GatewayChannel};

use crate::normalizer::build_text_request;

pub struct CliNormalizer;

impl CliNormalizer {
    pub fn normalize_line(
        user_id: &str,
        session_seed: u64,
        line: &str,
        timestamp_us: u64,
    ) -> AgentRequest {
        build_text_request(
            GatewayChannel::Cli,
            user_id.to_string(),
            session_seed,
            session_seed,
            line.to_string(),
            timestamp_us,
        )
    }
}
