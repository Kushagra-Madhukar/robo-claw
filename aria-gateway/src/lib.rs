//! # aria-gateway
//!
//! L1 Gateway adapters for ARIA-X signal normalization.
//!
//! Inbound signals from external channels (Telegram, CLI, HTTP, etc.)
//! are normalized into [`AgentRequest`] structs via the [`GatewayAdapter`]
//! trait, stripping channel-specific metadata.

use aria_core::AgentRequest;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from gateway adapters.
#[derive(Debug)]
pub enum GatewayError {
    /// The inbound payload could not be parsed.
    ParseError(String),
    /// A required field is missing from the payload.
    MissingField(String),
    /// I/O or transport error.
    TransportError(String),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GatewayError::ParseError(msg) => write!(f, "parse error: {}", msg),
            GatewayError::MissingField(field) => write!(f, "missing field: {}", field),
            GatewayError::TransportError(msg) => write!(f, "transport error: {}", msg),
        }
    }
}

impl std::error::Error for GatewayError {}

// ---------------------------------------------------------------------------
// GatewayAdapter trait
// ---------------------------------------------------------------------------

/// Async trait for inbound signal adapters.
///
/// Each adapter normalizes a channel-specific payload into an [`AgentRequest`].
#[async_trait::async_trait]
pub trait GatewayAdapter: Send + Sync {
    /// Receive and normalize the next inbound signal.
    async fn receive(&self) -> Result<AgentRequest, GatewayError>;
}

// ---------------------------------------------------------------------------
// Telegram normalizer
// ---------------------------------------------------------------------------

/// Telegram webhook update (simplified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: u64,
    pub message: Option<TelegramMessage>,
}

/// Telegram message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramMessage {
    pub message_id: u64,
    pub from: Option<TelegramUser>,
    pub chat: TelegramChat,
    pub text: Option<String>,
    pub date: u64,
}

/// Telegram user info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramUser {
    pub id: u64,
    pub first_name: String,
}

/// Telegram chat info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

/// Normalizes Telegram webhook JSON into [`AgentRequest`].
pub struct TelegramNormalizer;

impl TelegramNormalizer {
    /// Parse a raw JSON string from a Telegram webhook and normalize
    /// it into an [`AgentRequest`].
    ///
    /// Channel-specific metadata (chat_id, update_id, message_id) is
    /// stripped. The user's text becomes [`AgentRequest::content`].
    pub fn normalize(json: &str) -> Result<AgentRequest, GatewayError> {
        let update: TelegramUpdate =
            serde_json::from_str(json).map_err(|e| GatewayError::ParseError(format!("{}", e)))?;

        let message = update
            .message
            .ok_or_else(|| GatewayError::MissingField("message".into()))?;

        let text = message
            .text
            .clone()
            .ok_or_else(|| GatewayError::MissingField("message.text".into()))?;

        // Extract numeric user_id before consuming the Option
        let user_id_num = message.from.as_ref().map(|u| u.id).unwrap_or(0);
        let user_id_str = message
            .from
            .map(|u| u.id.to_string())
            .unwrap_or_else(|| "0".to_string());
        let mut req_id_bytes = [0u8; 16];
        req_id_bytes[0..8].copy_from_slice(&user_id_num.to_le_bytes());
        req_id_bytes[8..16].copy_from_slice(&message.date.to_le_bytes());

        // Session ID from chat_id
        let mut session_bytes = [0u8; 16];
        session_bytes[0..8].copy_from_slice(&(message.chat.id as u64).to_le_bytes());

        Ok(AgentRequest {
            request_id: req_id_bytes,
            session_id: session_bytes,
            user_id: user_id_str,
            content: text,
            timestamp_us: message.date * 1_000_000, // seconds → microseconds
        })
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const TELEGRAM_PAYLOAD: &str = r#"{
        "update_id": 123456789,
        "message": {
            "message_id": 42,
            "from": {
                "id": 99887766,
                "first_name": "Alice"
            },
            "chat": {
                "id": -1001234567890,
                "type": "supergroup"
            },
            "text": "Hello ARIA!",
            "date": 1709500000
        }
    }"#;

    // =====================================================================
    // Telegram normalization tests
    // =====================================================================

    #[test]
    fn normalize_telegram_to_agent_request() {
        let req = TelegramNormalizer::normalize(TELEGRAM_PAYLOAD).expect("normalize");

        // Content should be the user's text, without channel metadata
        assert_eq!(req.content, "Hello ARIA!");

        // Timestamp should be converted to microseconds
        assert_eq!(req.timestamp_us, 1709500000 * 1_000_000);

        // user_id should be the telegram user id as a string
        assert_eq!(req.user_id, "99887766");

        // session_id should be derived from chat id
        assert_ne!(req.session_id, [0u8; 16], "session_id should not be zero");
    }

    #[test]
    fn normalize_telegram_strips_channel_metadata() {
        let req = TelegramNormalizer::normalize(TELEGRAM_PAYLOAD).expect("normalize");

        // Content should NOT contain any Telegram-specific data
        assert!(!req.content.contains("update_id"));
        assert!(!req.content.contains("message_id"));
        assert!(!req.content.contains("supergroup"));
        assert!(!req.content.contains("chat"));
    }

    #[test]
    fn normalize_missing_message_returns_error() {
        let json = r#"{"update_id": 123}"#;
        let result = TelegramNormalizer::normalize(json);
        assert!(result.is_err());
        match result {
            Err(GatewayError::MissingField(field)) => {
                assert_eq!(field, "message");
            }
            other => panic!("expected MissingField, got: {:?}", other),
        }
    }

    #[test]
    fn normalize_missing_text_returns_error() {
        let json = r#"{
            "update_id": 123,
            "message": {
                "message_id": 1,
                "chat": {"id": 1, "type": "private"},
                "date": 1000
            }
        }"#;
        let result = TelegramNormalizer::normalize(json);
        assert!(result.is_err());
        match result {
            Err(GatewayError::MissingField(field)) => {
                assert_eq!(field, "message.text");
            }
            other => panic!("expected MissingField, got: {:?}", other),
        }
    }

    #[test]
    fn normalize_malformed_json_returns_parse_error() {
        let result = TelegramNormalizer::normalize("not json at all {{{");
        assert!(result.is_err());
        match result {
            Err(GatewayError::ParseError(_)) => {}
            other => panic!("expected ParseError, got: {:?}", other),
        }
    }

    #[test]
    fn normalize_no_from_field_uses_zero_id() {
        let json = r#"{
            "update_id": 123,
            "message": {
                "message_id": 1,
                "chat": {"id": 1, "type": "private"},
                "text": "hello",
                "date": 1000
            }
        }"#;
        let req = TelegramNormalizer::normalize(json).expect("normalize");
        assert_eq!(req.content, "hello");
        // user_id should be "0" when no from field
        assert_eq!(req.user_id, "0");
    }

    // =====================================================================
    // Error display tests
    // =====================================================================

    #[test]
    fn error_display() {
        let e = GatewayError::ParseError("bad json".into());
        assert!(format!("{}", e).contains("parse error"));

        let e = GatewayError::MissingField("text".into());
        assert!(format!("{}", e).contains("missing field"));

        let e = GatewayError::TransportError("timeout".into());
        assert!(format!("{}", e).contains("transport error"));
    }
}
