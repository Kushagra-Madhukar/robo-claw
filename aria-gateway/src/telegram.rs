use aria_core::{AgentRequest, GatewayChannel, MessageContent};
use serde::{Deserialize, Serialize};

use crate::GatewayError;

/// Telegram webhook update (simplified).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramUpdate {
    pub update_id: u64,
    pub message: Option<TelegramMessage>,
    pub callback_query: Option<TelegramCallbackQuery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramCallbackQuery {
    pub id: String,
    pub from: TelegramUser,
    pub message: Option<TelegramMessage>,
    pub data: Option<String>,
}

/// Telegram message payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramMessage {
    pub message_id: u64,
    pub from: Option<TelegramUser>,
    pub chat: TelegramChat,
    pub text: Option<String>,
    pub caption: Option<String>,
    pub photo: Option<Vec<TelegramPhotoSize>>,
    pub voice: Option<TelegramVoice>,
    pub date: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramPhotoSize {
    pub file_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramVoice {
    pub file_id: String,
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

/// Normalizes Telegram webhook JSON into `AgentRequest`.
pub struct TelegramNormalizer;

impl TelegramNormalizer {
    pub fn normalize(json: &str) -> Result<AgentRequest, GatewayError> {
        Self::normalize_with_chat_id(json).map(|(req, _)| req)
    }

    /// Normalize and return the chat ID for sending replies.
    pub fn normalize_with_chat_id(json: &str) -> Result<(AgentRequest, i64), GatewayError> {
        let update: TelegramUpdate =
            serde_json::from_str(json).map_err(|e| GatewayError::ParseError(format!("{}", e)))?;

        let mut content = None;
        let (message, user) = if let Some(msg) = update.message {
            let user = msg.from.clone();
            if let Some(photos) = &msg.photo {
                if let Some(best) = photos.last() {
                    content = Some(MessageContent::Image {
                        url: best.file_id.clone(),
                        caption: msg.caption.clone(),
                    });
                }
            } else if let Some(voice) = &msg.voice {
                content = Some(MessageContent::Audio {
                    url: voice.file_id.clone(),
                    transcript: None,
                });
            } else if let Some(t) = msg.text.clone() {
                if !t.is_empty() {
                    content = Some(MessageContent::Text(t));
                }
            }
            (msg, user)
        } else if let Some(cb) = update.callback_query {
            let msg = cb.message.unwrap_or_else(|| TelegramMessage {
                message_id: 0,
                from: Some(cb.from.clone()),
                chat: TelegramChat {
                    id: 0,
                    chat_type: "private".into(),
                },
                text: None,
                caption: None,
                photo: None,
                voice: None,
                date: 0,
            });
            let text = cb.data.unwrap_or_default();
            if !text.is_empty() {
                content = Some(MessageContent::Text(text));
            }
            (msg, Some(cb.from))
        } else {
            return Err(GatewayError::MissingField(
                "message or callback_query".into(),
            ));
        };

        let content = content
            .ok_or_else(|| GatewayError::MissingField("text, data, photo, or voice".into()))?;

        let chat_id = message.chat.id;
        let user_id_num = user.as_ref().map(|u| u.id).unwrap_or(0);
        let user_id_str = user
            .map(|u| u.id.to_string())
            .unwrap_or_else(|| "0".to_string());
        let mut req_id_bytes = [0u8; 16];
        req_id_bytes[0..8].copy_from_slice(&user_id_num.to_le_bytes());
        req_id_bytes[8..16].copy_from_slice(&message.date.to_le_bytes());

        let mut session_bytes = [0u8; 16];
        session_bytes[0..8].copy_from_slice(&(chat_id as u64).to_le_bytes());

        let req = AgentRequest {
            request_id: req_id_bytes,
            session_id: session_bytes,
            channel: GatewayChannel::Telegram,
            user_id: user_id_str,
            content,
            timestamp_us: message.date * 1_000_000,
        };
        Ok((req, chat_id))
    }
}
