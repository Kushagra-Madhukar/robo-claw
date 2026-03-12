use super::{LLMBackend, ModelMetadata, ModelProvider, SecretRef};
use crate::{CachedTool, LLMResponse, OrchestratorError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct OpenRouterBackend {
    pub api_key: SecretRef,
    pub model: String,
    pub site_url: String,
    pub site_title: String,
    client: reqwest::Client,
}

impl OpenRouterBackend {
    pub fn new(
        api_key: SecretRef,
        model: impl Into<String>,
        site_url: impl Into<String>,
        site_title: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            model: model.into(),
            site_url: site_url.into(),
            site_title: site_title.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait]
impl LLMBackend for OpenRouterBackend {
    async fn query(
        &self,
        prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve("openrouter.ai")?;

        let body = json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": false
        });

        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("HTTP-Referer", &self.site_url)
            .header("X-Title", &self.site_title)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                OrchestratorError::LLMError(format!("OpenRouter request failed: {}", e))
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter returned {}: {}",
                status, text
            )));
        }

        let res_json: serde_json::Value = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("OpenRouter JSON parse failed: {}", e))
        })?;

        // Handle both text and tool calls (though for now we primarily use text or
        // rely on the orchestrator's repair loop for tool calls in text).
        if let Some(content) = res_json["choices"][0]["message"]["content"].as_str() {
            return Ok(LLMResponse::TextAnswer(content.to_string()));
        }

        Err(OrchestratorError::LLMError(
            "OpenRouter returned no content".into(),
        ))
    }
}

pub struct OpenRouterProvider {
    pub api_key: SecretRef,
    pub site_url: String,
    pub site_title: String,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModel {
    id: String,
    name: String,
    description: Option<String>,
    context_length: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct OpenRouterModelsResponse {
    data: Vec<OpenRouterModel>,
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn id(&self) -> &str {
        "openrouter"
    }

    fn name(&self) -> &str {
        "OpenRouter"
    }

    async fn list_models(&self) -> Result<Vec<ModelMetadata>, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/models";
        let client = reqwest::Client::builder()
            .user_agent("aria-x/1.0")
            .build()
            .unwrap_or_default();

        let resp = client.get(url).send().await.map_err(|e| {
            error!(error = %e, "OpenRouter list models network failed");
            OrchestratorError::LLMError(format!("OpenRouter list models failed: {}", e))
        })?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            error!(status = %status, body = %text, "OpenRouter list models error response");
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter models list returned {}",
                status
            )));
        }

        let json: OpenRouterModelsResponse = resp.json().await.map_err(|e| {
            error!(error = %e, "OpenRouter list models JSON parse failed");
            OrchestratorError::LLMError(format!("OpenRouter list models JSON failed: {}", e))
        })?;

        debug!(
            count = json.data.len(),
            "OpenRouter models listed successfully"
        );

        Ok(json
            .data
            .into_iter()
            .map(|m| ModelMetadata {
                id: m.id,
                name: m.name,
                description: m.description,
                context_length: m.context_length,
            })
            .collect())
    }

    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(OpenRouterBackend::new(
            self.api_key.clone(),
            model_id,
            self.site_url.clone(),
            self.site_title.clone(),
        )))
    }
}
