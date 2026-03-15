use aria_core::{channel_capability_profile, GatewayChannel, MessageContent, OutboundEnvelope};
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc;
use tracing::warn;

use crate::{resolve_telegram_token, ResolvedAppConfig};

static CLI_STDOUT_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
static CLI_AWAITING_INPUT: AtomicBool = AtomicBool::new(false);
static WEBSOCKET_OUTBOUND_SENDERS: OnceLock<DashMap<String, mpsc::UnboundedSender<String>>> =
    OnceLock::new();

fn cli_stdout_lock() -> &'static Mutex<()> {
    CLI_STDOUT_LOCK.get_or_init(|| Mutex::new(()))
}

fn websocket_sender_registry() -> &'static DashMap<String, mpsc::UnboundedSender<String>> {
    WEBSOCKET_OUTBOUND_SENDERS.get_or_init(DashMap::new)
}

pub(crate) fn register_websocket_recipient(
    recipient_id: String,
    tx: mpsc::UnboundedSender<String>,
) {
    websocket_sender_registry().insert(recipient_id, tx);
}

pub(crate) fn unregister_websocket_recipient(recipient_id: &str) {
    websocket_sender_registry().remove(recipient_id);
}

pub(crate) fn cli_mark_awaiting_input(waiting: bool) {
    CLI_AWAITING_INPUT.store(waiting, Ordering::SeqCst);
}

pub(crate) fn cli_print_prompt() {
    let _guard = cli_stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
    print!("\n[aria-x] Enter request > ");
    let _ = io::stdout().flush();
}

fn cli_print_response_line(text: &str) {
    let _guard = cli_stdout_lock().lock().unwrap_or_else(|e| e.into_inner());
    print!("\r\x1b[2K");
    println!("[aria-x] Agent: {}", text);
    if CLI_AWAITING_INPUT.load(Ordering::SeqCst) {
        print!("\n[aria-x] Enter request > ");
        let _ = io::stdout().flush();
    }
}

pub fn parse_media_response(text: &str) -> Option<MessageContent> {
    let start = text.find('{')?;
    let payload = text.get(start..)?.trim();
    let json: serde_json::Value = serde_json::from_str(payload).ok()?;
    let kind = json
        .get("type")
        .and_then(|v| v.as_str())?
        .to_ascii_lowercase();
    match kind.as_str() {
        "text" => Some(MessageContent::Text(
            json.get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        )),
        "image" => Some(MessageContent::Image {
            url: json.get("url").and_then(|v| v.as_str())?.to_string(),
            caption: json
                .get("caption")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }),
        "audio" | "voice" => Some(MessageContent::Audio {
            url: json.get("url").and_then(|v| v.as_str())?.to_string(),
            transcript: json
                .get("transcript")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }),
        "video" => Some(MessageContent::Video {
            url: json.get("url").and_then(|v| v.as_str())?.to_string(),
            caption: json
                .get("caption")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            transcript: json
                .get("transcript")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }),
        "document" => Some(MessageContent::Document {
            url: json.get("url").and_then(|v| v.as_str())?.to_string(),
            caption: json
                .get("caption")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            mime_type: json
                .get("mime_type")
                .and_then(|v| v.as_str())
                .map(str::to_string),
        }),
        "location" => Some(MessageContent::Location {
            lat: json.get("lat").and_then(|v| v.as_f64())?,
            lng: json.get("lng").and_then(|v| v.as_f64())?,
        }),
        _ => None,
    }
}

pub fn envelope_from_text_response(
    session_id: [u8; 16],
    channel: GatewayChannel,
    recipient_id: String,
    text: &str,
) -> OutboundEnvelope {
    let profile = channel_capability_profile(channel);
    let content = if profile.supports_rich_media {
        parse_media_response(text).unwrap_or_else(|| MessageContent::Text(text.to_string()))
    } else {
        MessageContent::Text(text.to_string())
    };
    OutboundEnvelope {
        envelope_id: *uuid::Uuid::new_v4().as_bytes(),
        session_id,
        channel,
        recipient_id,
        provider_message_id: None,
        content,
        attachments: Vec::new(),
        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
    }
}

pub fn envelope_from_text_response_with_correlation(
    session_id: [u8; 16],
    channel: GatewayChannel,
    recipient_id: String,
    text: &str,
    correlation_id: Option<String>,
) -> OutboundEnvelope {
    let mut envelope = envelope_from_text_response(session_id, channel, recipient_id, text);
    envelope.provider_message_id = correlation_id;
    envelope
}

pub fn deterministic_outbound_envelope_id(
    request_id: [u8; 16],
    channel: GatewayChannel,
    recipient_id: &str,
    text: &str,
) -> [u8; 16] {
    let mut hasher = Sha256::new();
    hasher.update(b"aria-x-outbound-envelope");
    hasher.update(request_id);
    hasher.update(format!("{:?}", channel).as_bytes());
    hasher.update(recipient_id.as_bytes());
    hasher.update(text.as_bytes());
    let digest = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&digest[..16]);
    id
}

async fn send_telegram_envelope(
    envelope: &OutboundEnvelope,
    config: &ResolvedAppConfig,
) -> Result<(), String> {
    let token = resolve_telegram_token(config)?;
    let base_url = format!("https://api.telegram.org/bot{}", token);
    let chat_id = envelope.recipient_id.parse::<i64>().map_err(|e| {
        format!(
            "invalid telegram recipient_id '{}': {}",
            envelope.recipient_id, e
        )
    })?;
    let client = reqwest::Client::new();
    let escaped_text = match &envelope.content {
        MessageContent::Text(response_text) => escape_telegram_html(response_text),
        _ => String::new(),
    };

    let (method, body) = match &envelope.content {
        MessageContent::Text(_) => (
            "sendMessage",
            serde_json::json!({
                "chat_id": chat_id,
                "text": escaped_text,
                "parse_mode": "HTML"
            }),
        ),
        MessageContent::Image { url, caption } => (
            "sendPhoto",
            serde_json::json!({
                "chat_id": chat_id,
                "photo": url,
                "caption": caption,
            }),
        ),
        MessageContent::Audio { url, transcript } => (
            "sendVoice",
            serde_json::json!({
                "chat_id": chat_id,
                "voice": url,
                "caption": transcript,
            }),
        ),
        MessageContent::Video {
            url,
            caption,
            transcript,
        } => (
            "sendVideo",
            serde_json::json!({
                "chat_id": chat_id,
                "video": url,
                "caption": caption.clone().or(transcript.clone()),
            }),
        ),
        MessageContent::Document { url, caption, .. } => (
            "sendDocument",
            serde_json::json!({
                "chat_id": chat_id,
                "document": url,
                "caption": caption,
            }),
        ),
        MessageContent::Location { lat, lng } => (
            "sendLocation",
            serde_json::json!({
                "chat_id": chat_id,
                "latitude": lat,
                "longitude": lng,
            }),
        ),
    };

    let url = format!("{}/{}", base_url, method);
    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => Ok(()),
        Ok(resp) => {
            let status = resp.status();
            let fallback_url = format!("{}/sendMessage", base_url);
            let fallback_text = match &envelope.content {
                MessageContent::Text(_) => escaped_text.clone(),
                _ => "Unable to send media response.".to_string(),
            };
            let fallback_body = serde_json::json!({
                "chat_id": chat_id,
                "text": fallback_text,
                "parse_mode": "HTML"
            });
            let _ = client.post(&fallback_url).json(&fallback_body).send().await;
            Err(format!(
                "telegram method '{}' failed with status {}",
                method, status
            ))
        }
        Err(err) => {
            warn!(
                channel = "telegram",
                error = %err,
                "Telegram outbound request failed"
            );
            let fallback_url = format!("{}/sendMessage", base_url);
            let fallback_text = match &envelope.content {
                MessageContent::Text(_) => escaped_text.clone(),
                _ => "Unable to send media response.".to_string(),
            };
            let fallback_body = serde_json::json!({
                "chat_id": chat_id,
                "text": fallback_text,
                "parse_mode": "HTML"
            });
            let _ = client.post(&fallback_url).json(&fallback_body).send().await;
            Err(format!("telegram request error: {}", err))
        }
    }
}

fn escape_telegram_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn outbound_content_to_websocket_payload(content: &MessageContent) -> String {
    match content {
        MessageContent::Text(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| {
            serde_json::json!({
                "type": "text",
                "text": "Unable to serialize outbound payload"
            })
            .to_string()
        }),
    }
}

async fn send_websocket_envelope(envelope: &OutboundEnvelope) -> Result<(), String> {
    let Some(sender) = websocket_sender_registry().get(&envelope.recipient_id) else {
        return Err(format!(
            "websocket recipient '{}' is not connected",
            envelope.recipient_id
        ));
    };
    let payload = outbound_content_to_websocket_payload(&envelope.content);
    sender.send(payload).map_err(|_| {
        format!(
            "websocket recipient '{}' send failed",
            envelope.recipient_id
        )
    })
}

async fn send_whatsapp_envelope(
    envelope: &OutboundEnvelope,
    config: &ResolvedAppConfig,
) -> Result<(), String> {
    let Some(url) = config.gateway.whatsapp_outbound_url.as_ref() else {
        return Err("whatsapp outbound url is not configured".into());
    };
    let payload = outbound_content_to_websocket_payload(&envelope.content);
    let mut req = reqwest::Client::new().post(url).json(&serde_json::json!({
        "recipient_id": envelope.recipient_id,
        "session_id": uuid::Uuid::from_bytes(envelope.session_id).to_string(),
        "provider_message_id": envelope.provider_message_id,
        "content": payload,
        "timestamp_us": envelope.timestamp_us,
    }));
    if let Some(token) = config.gateway.whatsapp_auth_token.as_ref() {
        req = req.bearer_auth(token);
    }
    let response = req
        .send()
        .await
        .map_err(|err| format!("whatsapp outbound request failed: {}", err))?;
    if !response.status().is_success() {
        return Err(format!(
            "whatsapp outbound request failed with status {}",
            response.status()
        ));
    }
    Ok(())
}

pub async fn dispatch_outbound(
    envelope: &OutboundEnvelope,
    config: &ResolvedAppConfig,
) -> Result<(), String> {
    let attempts = if config.features.outbox_delivery {
        3
    } else {
        1
    };
    dispatch_outbound_with_retry(envelope, config, attempts).await
}

fn is_non_retryable_outbound_error(error: &str) -> bool {
    let e = error.to_ascii_lowercase();
    e.contains("not configured")
        || e.contains("unknown outbound transport")
        || e.contains("invalid telegram recipient_id")
}

pub async fn dispatch_outbound_with_retry(
    envelope: &OutboundEnvelope,
    config: &ResolvedAppConfig,
    max_attempts: u8,
) -> Result<(), String> {
    let attempts = max_attempts.max(1);
    let mut last_error = String::new();
    for attempt in 1..=attempts {
        let result = dispatch_outbound_once(envelope, config).await;
        match result {
            Ok(()) => return Ok(()),
            Err(err) => {
                last_error = err;
                if attempt == attempts || is_non_retryable_outbound_error(&last_error) {
                    break;
                }
                crate::channel_health::record_channel_health_event(
                    envelope.channel,
                    crate::channel_health::ChannelHealthEventKind::Retry,
                );
                let delay_ms = 150u64.saturating_mul(2u64.saturating_pow((attempt - 1) as u32));
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }
    Err(last_error)
}

async fn dispatch_outbound_once(
    envelope: &OutboundEnvelope,
    config: &ResolvedAppConfig,
) -> Result<(), String> {
    match envelope.channel {
        GatewayChannel::Telegram => send_telegram_envelope(envelope, config).await,
        GatewayChannel::Cli => {
            if let MessageContent::Text(t) = &envelope.content {
                cli_print_response_line(t);
            } else {
                cli_print_response_line("Agent sent non-text response on CLI channel");
            }
            Ok(())
        }
        GatewayChannel::WebSocket => send_websocket_envelope(envelope).await,
        GatewayChannel::WhatsApp => send_whatsapp_envelope(envelope, config).await,
        GatewayChannel::Discord => Err("discord outbound transport is not configured".into()),
        GatewayChannel::Slack => Err("slack outbound transport is not configured".into()),
        GatewayChannel::IMessage => Err("imessage outbound transport is not configured".into()),
        GatewayChannel::Ros2 => Err("ros2 outbound transport is not configured".into()),
        GatewayChannel::Unknown => Err("unknown outbound transport channel".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_outbound_returns_error_for_unconfigured_non_cli_channels() {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let config = crate::load_config(config_path.to_string_lossy().as_ref())
            .expect("load example config");
        let envelope = OutboundEnvelope {
            envelope_id: *uuid::Uuid::new_v4().as_bytes(),
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            channel: GatewayChannel::WebSocket,
            recipient_id: "user-1".into(),
            provider_message_id: None,
            content: MessageContent::Text("hello".into()),
            attachments: Vec::new(),
            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
        };
        let err = dispatch_outbound(&envelope, &config)
            .await
            .expect_err("websocket should fail without active recipient");
        assert!(err.contains("is not connected"));
    }

    #[tokio::test]
    async fn dispatch_outbound_delivers_to_registered_websocket_recipient() {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let config =
            crate::load_config(config_path.to_string_lossy().as_ref()).expect("load config");
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        register_websocket_recipient("ws-user".into(), tx);
        let envelope = OutboundEnvelope {
            envelope_id: *uuid::Uuid::new_v4().as_bytes(),
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            channel: GatewayChannel::WebSocket,
            recipient_id: "ws-user".into(),
            provider_message_id: None,
            content: MessageContent::Text("hello websocket".into()),
            attachments: Vec::new(),
            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
        };
        dispatch_outbound(&envelope, &config)
            .await
            .expect("websocket dispatch should succeed");
        let msg = rx.recv().await.expect("message should be forwarded");
        assert_eq!(msg, "hello websocket");
        unregister_websocket_recipient("ws-user");
    }

    #[tokio::test]
    async fn dispatch_outbound_whatsapp_requires_outbound_url_config() {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let config =
            crate::load_config(config_path.to_string_lossy().as_ref()).expect("load config");
        let envelope = OutboundEnvelope {
            envelope_id: *uuid::Uuid::new_v4().as_bytes(),
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            channel: GatewayChannel::WhatsApp,
            recipient_id: "wa-user".into(),
            provider_message_id: None,
            content: MessageContent::Text("hello whatsapp".into()),
            attachments: Vec::new(),
            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
        };
        let err = dispatch_outbound(&envelope, &config)
            .await
            .expect_err("whatsapp without provider endpoint should fail");
        assert!(err.contains("whatsapp outbound url is not configured"));
    }

    #[tokio::test]
    async fn dispatch_outbound_with_retry_stops_on_non_retryable_error() {
        let config_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.toml");
        let config =
            crate::load_config(config_path.to_string_lossy().as_ref()).expect("load config");
        let envelope = OutboundEnvelope {
            envelope_id: *uuid::Uuid::new_v4().as_bytes(),
            session_id: *uuid::Uuid::new_v4().as_bytes(),
            channel: GatewayChannel::Unknown,
            recipient_id: "user-1".into(),
            provider_message_id: None,
            content: MessageContent::Text("hello".into()),
            attachments: Vec::new(),
            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
        };
        let err = dispatch_outbound_with_retry(&envelope, &config, 5)
            .await
            .expect_err("unknown channel should fail immediately");
        assert!(err.contains("unknown outbound transport channel"));
    }

    #[test]
    fn deterministic_outbound_envelope_id_is_stable_for_same_input() {
        let request_id = *uuid::Uuid::new_v4().as_bytes();
        let a =
            deterministic_outbound_envelope_id(request_id, GatewayChannel::Cli, "user-1", "hello");
        let b =
            deterministic_outbound_envelope_id(request_id, GatewayChannel::Cli, "user-1", "hello");
        let c = deterministic_outbound_envelope_id(
            request_id,
            GatewayChannel::Cli,
            "user-1",
            "hello changed",
        );
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn escape_telegram_html_sanitizes_angle_brackets() {
        let rendered = escape_telegram_html("agent_override=<default> & ready");
        assert_eq!(rendered, "agent_override=&lt;default&gt; &amp; ready");
    }
}
