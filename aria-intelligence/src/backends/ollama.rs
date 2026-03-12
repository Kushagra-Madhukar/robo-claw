use super::{LLMBackend, ModelMetadata, ModelProvider};
use crate::{CachedTool, LLMResponse, OrchestratorError};
use async_trait::async_trait;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct OllamaBackend {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

impl OllamaBackend {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: model.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn from_env(model: impl Into<String>) -> Self {
        let host = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".into());
        Self::new(host, model)
    }

    pub async fn query_streaming(&self, prompt: &str) -> Result<String, OrchestratorError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": true
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                OrchestratorError::LLMError(format!("Ollama streaming request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Ollama streaming returned {}: {}",
                status, text
            )));
        }

        let bytes = resp.bytes().await.map_err(|e| {
            OrchestratorError::LLMError(format!("Ollama streaming read error: {}", e))
        })?;

        let mut assembled = String::new();
        for line in bytes.split(|&b| b == b'\n') {
            if line.is_empty() {
                continue;
            }
            if let Ok(obj) = serde_json::from_slice::<serde_json::Value>(line) {
                if let Some(token) = obj.get("response").and_then(|v| v.as_str()) {
                    assembled.push_str(token);
                }
                if obj.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                    break;
                }
            }
        }

        for pat in ["<|eot_id|>", "<|end|>", "</s>", "[INST]", "[/INST]"] {
            assembled = assembled.replace(pat, "");
        }

        Ok(assembled.trim().to_string())
    }
}

#[async_trait]
impl LLMBackend for OllamaBackend {
    async fn query(
        &self,
        prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = format!("{}/api/generate", self.base_url);
        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("Ollama request failed: {}", e)))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Ollama returned {}: {}",
                status, text
            )));
        }

        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("Ollama JSON parse failed: {}", e)))?;

        let mut text = res_json["response"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        for pat in ["<|eot_id|>", "<|end|>", "</s>", "[INST]", "[/INST]"] {
            text = text.replace(pat, "");
        }

        Ok(LLMResponse::TextAnswer(text.trim().to_string()))
    }
}

pub struct OllamaProvider {
    pub base_url: String,
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn name(&self) -> &str {
        "Ollama (Local)"
    }

    async fn list_models(&self) -> Result<Vec<ModelMetadata>, OrchestratorError> {
        let url = format!("{}/api/tags", self.base_url.trim_end_matches('/'));
        let resp = reqwest::get(&url).await.map_err(|e| {
            OrchestratorError::LLMError(format!("Ollama list models failed: {}", e))
        })?;

        let json: serde_json::Value = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("Ollama list models JSON failed: {}", e))
        })?;

        let mut models = Vec::new();
        if let Some(tags) = json["models"].as_array() {
            for tag in tags {
                if let Some(name) = tag["name"].as_str() {
                    models.push(ModelMetadata {
                        id: name.to_string(),
                        name: name.to_string(),
                        description: None,
                        context_length: None,
                    });
                }
            }
        }
        Ok(models)
    }

    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(OllamaBackend::new(
            self.base_url.clone(),
            model_id,
        )))
    }
}
