use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
#[cfg(feature = "speech-runtime")]
use tracing::{info, warn};

use crate::ResolvedAppConfig;

#[derive(Clone, Debug)]
pub struct TranscriptionRecord {
    pub transcript: String,
    pub provider: String,
    pub confidence: Option<f32>,
}

#[async_trait]
pub trait SpeechToTextBackend: Send + Sync {
    async fn transcribe(
        &self,
        bytes: &[u8],
        ext: &str,
        mime_hint: Option<&str>,
    ) -> Option<TranscriptionRecord>;

    fn availability_hint(&self) -> Option<String> {
        None
    }
}

#[derive(Clone)]
struct DisabledSttBackend {
    reason: Option<String>,
}

#[async_trait]
impl SpeechToTextBackend for DisabledSttBackend {
    async fn transcribe(
        &self,
        _bytes: &[u8],
        _ext: &str,
        _mime_hint: Option<&str>,
    ) -> Option<TranscriptionRecord> {
        None
    }

    fn availability_hint(&self) -> Option<String> {
        self.reason.clone()
    }
}

#[cfg(feature = "speech-runtime")]
#[derive(Clone)]
struct LocalWhisperSttBackend {
    whisper_model: Option<String>,
    whisper_bin: String,
    ffmpeg_bin: String,
    language: Option<String>,
}

#[cfg(feature = "speech-runtime")]
#[async_trait]
impl SpeechToTextBackend for LocalWhisperSttBackend {
    async fn transcribe(
        &self,
        bytes: &[u8],
        ext: &str,
        _mime_hint: Option<&str>,
    ) -> Option<TranscriptionRecord> {
        let whisper_model = self.whisper_model.clone()?;
        let whisper_bin = self.whisper_bin.clone();
        let ffmpeg_bin = self.ffmpeg_bin.clone();
        let language = self.language.clone();
        let base = std::env::temp_dir().join(format!("aria-whisper-{}", uuid::Uuid::new_v4()));
        let input_path = base.with_extension(ext.trim_start_matches('.'));
        let wav_path = base.with_extension("wav");
        let output_base = base.clone();

        let input_bytes = bytes.to_vec();
        if tokio::task::spawn_blocking({
            let input_path = input_path.clone();
            move || std::fs::write(&input_path, input_bytes)
        })
        .await
        .ok()?
        .is_err()
        {
            return None;
        }

        let ffmpeg_status = tokio::process::Command::new(&ffmpeg_bin)
            .arg("-y")
            .arg("-i")
            .arg(&input_path)
            .arg("-ac")
            .arg("1")
            .arg("-ar")
            .arg("16000")
            .arg("-f")
            .arg("wav")
            .arg(&wav_path)
            .status()
            .await
            .ok()?;

        if !ffmpeg_status.success() {
            let _ = tokio::task::spawn_blocking({
                let input_path = input_path.clone();
                move || std::fs::remove_file(&input_path)
            })
            .await;
            warn!("Local STT ffmpeg preprocessing failed");
            return None;
        }

        let mut whisper_command = tokio::process::Command::new(&whisper_bin);
        whisper_command
            .arg("-m")
            .arg(&whisper_model)
            .arg("-f")
            .arg(&wav_path)
            .arg("-of")
            .arg(&output_base)
            .arg("-otxt")
            .arg("-nt");
        if let Some(language) = language.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            whisper_command.arg("-l").arg(language);
        }
        let status = whisper_command.status().await.ok()?;

        if !status.success() {
            let _ = tokio::task::spawn_blocking({
                let input_path = input_path.clone();
                let wav_path = wav_path.clone();
                move || {
                    let _ = std::fs::remove_file(&input_path);
                    let _ = std::fs::remove_file(&wav_path);
                }
            })
            .await;
            return None;
        }

        let txt_path = output_base.with_extension("txt");
        let transcript = tokio::task::spawn_blocking({
            let txt_path = txt_path.clone();
            move || std::fs::read_to_string(&txt_path)
        })
        .await
        .ok()?
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| transcript_looks_plausible(s))?;

        let _ = tokio::task::spawn_blocking({
            let input_path = input_path.clone();
            let wav_path = wav_path.clone();
            let txt_path = txt_path.clone();
            move || {
                let _ = std::fs::remove_file(&input_path);
                let _ = std::fs::remove_file(&wav_path);
                let _ = std::fs::remove_file(&txt_path);
            }
        })
        .await;
        info!(
            provider = "local_whisper_cpp",
            "Accepted local STT transcript"
        );
        Some(TranscriptionRecord {
            transcript,
            provider: "local_whisper_cpp".to_string(),
            confidence: None,
        })
    }
}

#[cfg(feature = "speech-runtime")]
#[derive(Clone)]
struct CloudHttpSttBackend {
    endpoint: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

#[cfg(feature = "speech-runtime")]
#[async_trait]
impl SpeechToTextBackend for CloudHttpSttBackend {
    async fn transcribe(
        &self,
        bytes: &[u8],
        ext: &str,
        mime_hint: Option<&str>,
    ) -> Option<TranscriptionRecord> {
        use base64::Engine;
        let audio_base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let mime_type = mime_hint.map(str::to_string).unwrap_or_else(|| match ext {
            "ogg" => "audio/ogg".to_string(),
            "mp4" => "video/mp4".to_string(),
            _ => "application/octet-stream".to_string(),
        });

        let payload = serde_json::json!({
            "audio_base64": audio_base64,
            "mime_type": mime_type,
            "ext": ext,
        });

        let mut request = self.client.post(&self.endpoint).json(&payload);
        if let Some(api_key) = self.api_key.as_deref() {
            request = request.bearer_auth(api_key.to_string());
        }

        let response = request.send().await.ok()?;
        if !response.status().is_success() {
            warn!(
                endpoint = %self.endpoint,
                status = %response.status(),
                "Cloud STT endpoint returned non-success status"
            );
            return None;
        }
        let json: serde_json::Value = response.json().await.ok()?;
        let transcript = json
            .get("transcript")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())?
            .to_string();
        let confidence = json
            .get("confidence")
            .and_then(|v| v.as_f64())
            .map(|v| v as f32);
        let provider = json
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "cloud_http".to_string());
        Some(TranscriptionRecord {
            transcript,
            provider,
            confidence,
        })
    }
}

#[cfg(feature = "speech-runtime")]
#[derive(Clone)]
struct CompositeSttBackend {
    primary: Arc<dyn SpeechToTextBackend>,
    fallback: Option<Arc<dyn SpeechToTextBackend>>,
}

#[cfg(feature = "speech-runtime")]
#[async_trait]
impl SpeechToTextBackend for CompositeSttBackend {
    async fn transcribe(
        &self,
        bytes: &[u8],
        ext: &str,
        mime_hint: Option<&str>,
    ) -> Option<TranscriptionRecord> {
        if let Some(result) = self.primary.transcribe(bytes, ext, mime_hint).await {
            return Some(result);
        }
        let Some(fallback) = &self.fallback else {
            return None;
        };
        fallback.transcribe(bytes, ext, mime_hint).await
    }

    fn availability_hint(&self) -> Option<String> {
        self.primary.availability_hint().or_else(|| {
            self.fallback
                .as_ref()
                .and_then(|backend| backend.availability_hint())
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SttMode {
    Auto,
    Local,
    Cloud,
    Off,
}

#[derive(Clone, Debug)]
pub(crate) struct SttStatus {
    pub configured_mode: &'static str,
    pub effective_mode: &'static str,
    pub whisper_model_path: Option<String>,
    pub whisper_model_exists: bool,
    pub whisper_bin: String,
    pub whisper_bin_available: bool,
    pub ffmpeg_bin: String,
    pub ffmpeg_available: bool,
    pub cloud_endpoint_configured: bool,
    pub cloud_fallback_enabled: bool,
    pub language_hint: Option<String>,
    pub reason: String,
}

fn parse_stt_mode(value: &str) -> SttMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "local" => SttMode::Local,
        "cloud" => SttMode::Cloud,
        "off" => SttMode::Off,
        _ => SttMode::Auto,
    }
}

fn command_available(command: &str) -> bool {
    let command = command.trim();
    if command.is_empty() {
        return false;
    }
    let path = PathBuf::from(command);
    if path.is_absolute() || path.components().count() > 1 {
        return path.is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|entry| entry.join(command).is_file())
}

pub(crate) fn inspect_stt_status(config: &ResolvedAppConfig) -> SttStatus {
    let configured_mode = parse_stt_mode(&config.gateway.stt_mode);
    let whisper_model_path = config
        .runtime
        .whisper_cpp_model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let whisper_model_exists = whisper_model_path
        .as_deref()
        .map(PathBuf::from)
        .is_some_and(|path| path.is_file());
    let whisper_bin_available = command_available(&config.runtime.whisper_cpp_bin);
    let ffmpeg_available = command_available(&config.runtime.ffmpeg_bin);
    let cloud_endpoint_configured = config
        .gateway
        .stt_cloud_endpoint
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty());
    let local_ready = whisper_model_exists && whisper_bin_available && ffmpeg_available;
    let (effective_mode, reason) = match configured_mode {
        SttMode::Off => ("off", "speech transcription disabled by config".to_string()),
        SttMode::Local => {
            if local_ready {
                ("local", "local whisper runtime is ready".to_string())
            } else {
                (
                    "off",
                    "local whisper mode selected but runtime requirements are missing".to_string(),
                )
            }
        }
        SttMode::Cloud => {
            if cloud_endpoint_configured {
                ("cloud", "cloud STT endpoint is configured".to_string())
            } else {
                (
                    "off",
                    "cloud mode selected but no cloud endpoint is configured".to_string(),
                )
            }
        }
        SttMode::Auto => {
            if local_ready {
                (
                    "local",
                    "auto mode selected local whisper runtime".to_string(),
                )
            } else if cloud_endpoint_configured {
                ("cloud", "auto mode selected cloud STT fallback".to_string())
            } else {
                (
                    "off",
                    "auto mode found no usable local or cloud STT backend".to_string(),
                )
            }
        }
    };

    SttStatus {
        configured_mode: match configured_mode {
            SttMode::Auto => "auto",
            SttMode::Local => "local",
            SttMode::Cloud => "cloud",
            SttMode::Off => "off",
        },
        effective_mode,
        whisper_model_path,
        whisper_model_exists,
        whisper_bin: config.runtime.whisper_cpp_bin.clone(),
        whisper_bin_available,
        ffmpeg_bin: config.runtime.ffmpeg_bin.clone(),
        ffmpeg_available,
        cloud_endpoint_configured,
        cloud_fallback_enabled: config.gateway.stt_cloud_fallback,
        language_hint: config.runtime.whisper_cpp_language.clone(),
        reason,
    }
}

pub fn build_stt_backend(
    config: &ResolvedAppConfig,
    client: reqwest::Client,
) -> Arc<dyn SpeechToTextBackend> {
    #[cfg(not(feature = "speech-runtime"))]
    {
        let _ = config;
        let _ = client;
        return Arc::new(DisabledSttBackend {
            reason: Some("Voice transcription is unavailable in this build.".to_string()),
        });
    }

    #[cfg(feature = "speech-runtime")]
    {
        let status = inspect_stt_status(config);
        let cloud_backend = config
            .gateway
            .stt_cloud_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|endpoint| {
                Arc::new(CloudHttpSttBackend {
                    endpoint: endpoint.to_string(),
                    api_key: crate::non_empty_env(&config.gateway.stt_cloud_api_key_env),
                    client: client.clone(),
                }) as Arc<dyn SpeechToTextBackend>
            });

        let local_backend = Arc::new(LocalWhisperSttBackend {
            whisper_model: config.runtime.whisper_cpp_model.clone(),
            whisper_bin: config.runtime.whisper_cpp_bin.clone(),
            ffmpeg_bin: config.runtime.ffmpeg_bin.clone(),
            language: config.runtime.whisper_cpp_language.clone(),
        }) as Arc<dyn SpeechToTextBackend>;

        match status.effective_mode {
            "local" => {
                let fallback = if config.gateway.stt_cloud_fallback {
                    cloud_backend
                } else {
                    None
                };
                Arc::new(CompositeSttBackend {
                    primary: local_backend,
                    fallback,
                })
            }
            "cloud" => {
                let primary = cloud_backend.unwrap_or_else(|| {
                    Arc::new(DisabledSttBackend {
                        reason: Some(
                            "Voice transcription is unavailable on this runtime. Ask the operator to run `aria-x doctor stt`."
                                .to_string(),
                        ),
                    }) as Arc<dyn SpeechToTextBackend>
                });
                Arc::new(CompositeSttBackend {
                    primary,
                    fallback: None,
                })
            }
            _ => Arc::new(DisabledSttBackend {
                reason: Some(
                    "Voice transcription is unavailable on this runtime. Ask the operator to run `aria-x doctor stt`."
                        .to_string(),
                ),
            }),
        }
    }
}

#[cfg(feature = "speech-runtime")]
fn transcript_looks_plausible(transcript: &str) -> bool {
    let trimmed = transcript.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let alpha_numeric = trimmed.chars().filter(|ch| ch.is_alphanumeric()).count();
    if alpha_numeric < 3 {
        return false;
    }
    let distinct_non_ws = trimmed
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    if distinct_non_ws <= 1 {
        return false;
    }
    let repeated_single_token = trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .all(|pair| pair[0].eq_ignore_ascii_case(pair[1]));
    if repeated_single_token && trimmed.split_whitespace().count() > 2 {
        return false;
    }
    true
}

#[cfg(all(test, feature = "speech-runtime"))]
mod tests {
    use super::transcript_looks_plausible;

    #[test]
    fn transcript_plausibility_rejects_junk_output() {
        assert!(!transcript_looks_plausible(""));
        assert!(!transcript_looks_plausible("a"));
        assert!(!transcript_looks_plausible("..."));
        assert!(!transcript_looks_plausible("ha ha ha ha"));
    }

    #[test]
    fn transcript_plausibility_accepts_normal_speech_text() {
        assert!(transcript_looks_plausible("hello there"));
        assert!(transcript_looks_plausible("please remind me tomorrow at 5"));
    }
}
