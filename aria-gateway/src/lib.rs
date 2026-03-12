//! # aria-gateway
//!
//! L1 Gateway adapters for ARIA-X signal normalization.
//!
//! Inbound signals from external channels (Telegram, CLI, HTTP, etc.)
//! are normalized into `AgentRequest` structs via channel-specific modules.

pub mod adapter;
pub mod auth;
pub mod backpressure;
pub mod cli;
pub mod discord;
pub mod error;
pub mod http_api;
pub mod imessage;
pub mod normalizer;
pub mod ros2;
pub mod slack;
pub mod telegram;
pub mod websocket;
pub mod whatsapp;

pub use adapter::GatewayAdapter;
pub use auth::{AuthManager, RateLimiter};
pub use backpressure::SemanticBackpressure;
pub use error::GatewayError;
pub use telegram::TelegramNormalizer;

pub use cli::CliNormalizer;
pub use discord::DiscordNormalizer;
pub use imessage::IMessageNormalizer;
pub use ros2::{normalize_ros2_message, Ros2Bridge, Ros2StringMessage};
pub use slack::SlackNormalizer;
pub use websocket::WebSocketNormalizer;
pub use whatsapp::WhatsAppNormalizer;

#[cfg(test)]
mod tests {
    use aria_core::{GatewayChannel, MessageContent};

    use super::*;

    #[test]
    fn normalize_telegram_to_agent_request() {
        let payload = r#"{
            "update_id": 123456789,
            "message": {
                "message_id": 42,
                "from": {"id": 99887766, "first_name": "Alice"},
                "chat": {"id": -1001234567890, "type": "supergroup"},
                "text": "Hello ARIA!",
                "date": 1709500000
            }
        }"#;
        let req = TelegramNormalizer::normalize(payload).expect("normalize");
        assert_eq!(req.channel, GatewayChannel::Telegram);
        assert_eq!(req.content, MessageContent::Text("Hello ARIA!".into()));
    }

    #[test]
    fn normalize_whatsapp_payload() {
        let payload =
            r#"{"user_id":"u1","chat_id":42,"text":"ping","timestamp_us":1709500000000000}"#;
        let req = WhatsAppNormalizer::normalize(payload).unwrap();
        assert_eq!(req.channel, GatewayChannel::WhatsApp);
        assert_eq!(req.content, MessageContent::Text("ping".into()));
    }

    #[test]
    fn normalize_discord_payload() {
        let payload = r#"{"author_id":"d1","channel_id":7,"content":"hi","timestamp_us":1}"#;
        let req = DiscordNormalizer::normalize(payload).unwrap();
        assert_eq!(req.channel, GatewayChannel::Discord);
    }

    #[test]
    fn normalize_slack_payload() {
        let payload = r#"{"user":"s1","channel":"C123","text":"hi","timestamp_us":1}"#;
        let req = SlackNormalizer::normalize(payload).unwrap();
        assert_eq!(req.channel, GatewayChannel::Slack);
    }

    #[test]
    fn normalize_websocket_payload() {
        let payload = r#"{"session_id":9,"user_id":"w1","text":"hello","timestamp_us":1}"#;
        let req = WebSocketNormalizer::normalize(payload).unwrap();
        assert_eq!(req.channel, GatewayChannel::WebSocket);
    }

    #[test]
    fn normalize_imessage_payload() {
        let payload = r#"{"sender_id":"im1","thread_id":5,"body":"yo","timestamp_us":1}"#;
        let req = IMessageNormalizer::normalize(payload).unwrap();
        assert_eq!(req.channel, GatewayChannel::IMessage);
    }

    #[test]
    fn normalize_cli_line() {
        let req = CliNormalizer::normalize_line("cli_user", 99, "hello", 1);
        assert_eq!(req.channel, GatewayChannel::Cli);
        assert_eq!(req.content, MessageContent::Text("hello".into()));
    }

    #[test]
    fn error_display() {
        let e = GatewayError::ParseError("bad json".into());
        assert!(format!("{}", e).contains("parse error"));
    }
}
