use super::{
    adapter_for_family, build_openai_compatible_followup_body, collect_sse_like_stream,
    default_model_capability_profile, extract_openai_compatible_content,
    parse_openai_compatible_tool_calls, send_with_response_start_timeout, EgressCredentialBroker,
    LLMBackend, ModelMetadata, ModelProvider, ProviderHealthIdentity, SecretRef,
};
use crate::{CachedTool, ExecutedToolCall, LLMResponse, OrchestratorError};
use aria_core::{
    AdapterFamily, CapabilitySourceKind, CapabilitySupport, ExecutionContextPack,
    ModelCapabilityProbeRecord, ModelCapabilityProfile, ModelRef, ToolCallingMode,
    ToolRuntimePolicy,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, error};

#[derive(Debug, Clone)]
pub struct OpenRouterBackend {
    pub api_key: SecretRef,
    pub model: String,
    pub site_url: String,
    pub site_title: String,
    capability_profile: ModelCapabilityProfile,
    client: reqwest::Client,
    egress_broker: EgressCredentialBroker,
}

impl OpenRouterBackend {
    const DEFAULT_MAX_TOKENS: u64 = 1024;
    const DEFAULT_SEND_ATTEMPTS: u32 = 3;

    pub fn new(
        api_key: SecretRef,
        model: impl Into<String>,
        site_url: impl Into<String>,
        site_title: impl Into<String>,
    ) -> Self {
        let model = model.into();
        Self {
            api_key,
            capability_profile: default_model_capability_profile(
                "openrouter",
                &model,
                AdapterFamily::OpenAiCompatible,
                0,
            ),
            model,
            site_url: site_url.into(),
            site_title: site_title.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
            egress_broker: EgressCredentialBroker::new(),
        }
    }

    pub fn with_capability_profile(
        api_key: SecretRef,
        profile: ModelCapabilityProfile,
        site_url: impl Into<String>,
        site_title: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            model: profile.model_ref.model_id.clone(),
            capability_profile: profile,
            site_url: site_url.into(),
            site_title: site_title.into(),
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
            egress_broker: EgressCredentialBroker::new(),
        }
    }

    pub fn with_egress_broker(mut self, broker: EgressCredentialBroker) -> Self {
        self.egress_broker = broker;
        self
    }

    fn translated_tool_definitions(&self, tools: &[CachedTool]) -> Vec<Value> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        adapter
            .filter_tools(&self.capability_profile, tools)
            .iter()
            .filter_map(|tool| {
                adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()
            })
            .collect::<Vec<_>>()
    }

    fn build_tool_follow_up_body(
        &self,
        prompt: &str,
        tool_defs: &[Value],
        executed_tools: &[ExecutedToolCall],
    ) -> Value {
        build_openai_compatible_followup_body(
            &self.model,
            prompt,
            tool_defs.to_vec(),
            executed_tools,
        )
    }

    fn completion_cap(&self) -> u64 {
        std::env::var("ARIA_OPENROUTER_MAX_TOKENS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(Self::DEFAULT_MAX_TOKENS)
    }

    fn apply_completion_cap(&self, body: &mut Value) {
        body["max_tokens"] = json!(self.completion_cap());
    }

    fn send_attempts(&self) -> u32 {
        std::env::var("ARIA_OPENROUTER_SEND_ATTEMPTS")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(Self::DEFAULT_SEND_ATTEMPTS)
    }

    fn should_retry_transport_error(error: &OrchestratorError) -> bool {
        matches!(error, OrchestratorError::BackendOverloaded(_))
    }

    async fn send_completion_request(
        &self,
        url: &str,
        api_key: &str,
        body: &Value,
    ) -> Result<reqwest::Response, OrchestratorError> {
        let attempts = self.send_attempts();
        let mut last_error: Option<String> = None;
        for attempt in 1..=attempts {
            match send_with_response_start_timeout(
                "OpenRouter",
                self.client
                    .post(url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .header("HTTP-Referer", &self.site_url)
                    .header("X-Title", &self.site_title)
                    .json(body)
                    .send(),
            )
            .await
            {
                Ok(response) => return Ok(response),
                Err(err) => {
                    let retryable = Self::should_retry_transport_error(&err);
                    if retryable && attempt < attempts {
                        let backoff_ms = 200u64.saturating_mul(attempt as u64);
                        debug!(
                            attempt,
                            attempts,
                            backoff_ms,
                            error = %err,
                            "Retrying OpenRouter transport request after transient failure"
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        last_error = Some(err.to_string());
                        continue;
                    }
                    return Err(err);
                }
            }
        }
        let message = last_error
            .map(|err| err.to_string())
            .unwrap_or_else(|| "unknown transport error".to_string());
        Err(OrchestratorError::BackendOverloaded(format!(
            "OpenRouter request failed after {} attempt(s): {}",
            attempts, message
        )))
    }

    fn should_retry_without_tool_choice(status: reqwest::StatusCode, body_text: &str) -> bool {
        status == reqwest::StatusCode::NOT_FOUND
            && body_text.contains("provided 'tool_choice' value")
    }

    async fn resend_without_tool_choice(
        &self,
        url: &str,
        mut body: Value,
    ) -> Result<reqwest::Response, OrchestratorError> {
        body.as_object_mut().map(|value| {
            value.remove("tool_choice");
        });
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        self.send_completion_request(url, &api_key, &body).await
    }

    fn parse_response_json(
        &self,
        res_json: &Value,
        tools: &[CachedTool],
    ) -> Result<Option<LLMResponse>, OrchestratorError> {
        let message = &res_json["choices"][0]["message"];
        let tool_calls = parse_openai_compatible_tool_calls(message);
        if !tool_calls.is_empty() {
            return Ok(Some(LLMResponse::ToolCalls(tool_calls)));
        }
        if let Some(content) = extract_openai_compatible_content(message) {
            let repair_allowed = matches!(
                adapter_for_family(self.capability_profile.adapter_family)
                    .tool_calling_mode(&self.capability_profile),
                ToolCallingMode::TextFallbackWithRepair
            );
            if repair_allowed {
                if let Some(repaired) = crate::runtime::repair_tool_call_json(&content, tools) {
                    return Ok(Some(LLMResponse::ToolCalls(vec![repaired])));
                }
            }
            return Ok(Some(LLMResponse::TextAnswer(content)));
        }
        Ok(None)
    }

    async fn retry_without_native_tools(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<Option<LLMResponse>, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": false
        });
        self.apply_completion_cap(&mut body);
        let resp = self
            .send_completion_request(url, &api_key, &body)
            .await
            .map_err(|e| {
                OrchestratorError::LLMError(format!("OpenRouter compatibility retry failed: {}", e))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter compatibility retry returned {}: {}",
                status, text
            )));
        }
        let res_json: Value = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!(
                "OpenRouter compatibility retry JSON parse failed: {}",
                e
            ))
        })?;
        self.parse_response_json(&res_json, tools)
    }

    async fn retry_context_without_native_tools(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<Option<LLMResponse>, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let mut body =
            super::build_openai_compatible_context_body(&self.model, context, Vec::new(), policy);
        self.apply_completion_cap(&mut body);
        let resp = self
            .send_completion_request(url, &api_key, &body)
            .await
            .map_err(|e| {
                OrchestratorError::LLMError(format!(
                    "OpenRouter context compatibility retry failed: {}",
                    e
                ))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter context compatibility retry returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("OpenRouter JSON parse failed: {}", e))
        })?;
        self.parse_response_json(&res_json, tools)
    }
}

#[async_trait]
impl LLMBackend for OpenRouterBackend {
    async fn query(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_policy(prompt, tools, &ToolRuntimePolicy::default())
            .await
    }

    async fn query_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);

        let tool_defs = filtered_tools
            .iter()
            .filter_map(|tool| {
                adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "user", "content": prompt }
            ],
            "stream": false
        });
        self.apply_completion_cap(&mut body);
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !tool_defs.is_empty()
        {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }

        let resp = self.send_completion_request(url, &api_key, &body).await?;

        let mut resp = resp;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !tool_defs.is_empty() && Self::should_retry_without_tool_choice(status, &text) {
                resp = self.resend_without_tool_choice(url, body.clone()).await?;
            } else {
                return Err(OrchestratorError::LLMError(format!(
                    "OpenRouter returned {}: {}",
                    status, text
                )));
            }
        }

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

        if let Some(response) = self.parse_response_json(&res_json, tools)? {
            return Ok(response);
        }

        if !tool_defs.is_empty() {
            if let Some(response) = self.retry_without_native_tools(prompt, tools).await? {
                return Ok(response);
            }
        }

        Err(OrchestratorError::LLMError(format!(
            "OpenRouter returned no content: {}",
            res_json["choices"][0]["message"]
        )))
    }

    async fn query_context_with_policy(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let tool_defs = filtered_tools
            .iter()
            .filter_map(|tool| {
                adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()
            })
            .collect::<Vec<_>>();
        let mut body = super::build_openai_compatible_context_body(
            &self.model,
            context,
            if matches!(
                tool_mode,
                ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
            ) {
                tool_defs.clone()
            } else {
                Vec::new()
            },
            policy,
        );
        self.apply_completion_cap(&mut body);
        let resp = self.send_completion_request(url, &api_key, &body).await?;
        let mut resp = resp;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !tool_defs.is_empty() && Self::should_retry_without_tool_choice(status, &text) {
                resp = self.resend_without_tool_choice(url, body.clone()).await?;
            } else {
                return Err(OrchestratorError::LLMError(format!(
                    "OpenRouter returned {}: {}",
                    status, text
                )));
            }
        }
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
        if let Some(response) = self.parse_response_json(&res_json, tools)? {
            return Ok(response);
        }
        if !tool_defs.is_empty() {
            if let Some(response) = self
                .retry_context_without_native_tools(context, tools, policy)
                .await?
            {
                return Ok(response);
            }
        }
        Err(OrchestratorError::LLMError(format!(
            "OpenRouter returned no content: {}",
            res_json["choices"][0]["message"]
        )))
    }

    fn inspect_context_payload(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_defs = filtered_tools
            .iter()
            .filter_map(|tool| {
                adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()
            })
            .collect::<Vec<_>>();
        let mut body = super::build_openai_compatible_context_body(
            &self.model,
            context,
            if matches!(
                adapter.tool_calling_mode(&self.capability_profile),
                ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
            ) {
                tool_defs
            } else {
                Vec::new()
            },
            policy,
        );
        self.apply_completion_cap(&mut body);
        Some(body)
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let tool_defs = filtered_tools
            .iter()
            .filter_map(|tool| {
                adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt }],
            "stream": true
        });
        self.apply_completion_cap(&mut body);
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !tool_defs.is_empty()
        {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }
        let resp = send_with_response_start_timeout(
            "OpenRouter streaming",
            self.client
                .post(url)
                .bearer_auth(api_key)
                .header("HTTP-Referer", &self.site_url)
                .header("X-Title", &self.site_title)
                .json(&body)
                .send(),
        )
        .await?;
        let mut resp = resp;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !tool_defs.is_empty() && Self::should_retry_without_tool_choice(status, &text) {
                resp = self.resend_without_tool_choice(url, body.clone()).await?;
            } else {
                return Err(OrchestratorError::LLMError(format!(
                    "OpenRouter streaming returned {}: {}",
                    status, text
                )));
            }
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter streaming returned {}: {}",
                status, text
            )));
        }
        collect_sse_like_stream(resp, adapter).await
    }

    async fn query_with_tool_results(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_tool_results_and_policy(
            prompt,
            tools,
            executed_tools,
            &ToolRuntimePolicy::default(),
        )
        .await
    }

    async fn query_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let tool_defs = self.translated_tool_definitions(tools);
        let mut body = self.build_tool_follow_up_body(prompt, &tool_defs, executed_tools);
        self.apply_completion_cap(&mut body);
        if !matches!(
            adapter.tool_calling_mode(&self.capability_profile),
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) {
            body.as_object_mut().map(|value| {
                value.remove("tools");
                value.remove("tool_choice");
                value.remove("parallel_tool_calls");
            });
        } else {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }
        let resp = self.send_completion_request(url, &api_key, &body).await?;
        let mut resp = resp;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !tool_defs.is_empty() && Self::should_retry_without_tool_choice(status, &text) {
                resp = self.resend_without_tool_choice(url, body.clone()).await?;
            } else {
                return Err(OrchestratorError::LLMError(format!(
                    "OpenRouter returned {}: {}",
                    status, text
                )));
            }
        }
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
        if let Some(response) = self.parse_response_json(&res_json, tools)? {
            return Ok(response);
        }
        if !tool_defs.is_empty() {
            if let Some(response) = self.retry_without_native_tools(prompt, tools).await? {
                return Ok(response);
            }
        }
        Err(OrchestratorError::LLMError(format!(
            "OpenRouter returned no follow-up content: {}",
            res_json["choices"][0]["message"]
        )))
    }

    async fn query_stream_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let url = "https://openrouter.ai/api/v1/chat/completions";
        let api_key = self.api_key.resolve_with_broker(
            "openrouter.ai",
            "openrouter_provider",
            &self.egress_broker,
        )?;
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let tool_defs = self.translated_tool_definitions(tools);
        let mut body = self.build_tool_follow_up_body(prompt, &tool_defs, executed_tools);
        body["stream"] = json!(true);
        self.apply_completion_cap(&mut body);
        if !matches!(
            adapter.tool_calling_mode(&self.capability_profile),
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) {
            body.as_object_mut().map(|value| {
                value.remove("tools");
                value.remove("tool_choice");
                value.remove("parallel_tool_calls");
            });
        } else {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }
        let resp = send_with_response_start_timeout(
            "OpenRouter streaming",
            self.client
                .post(url)
                .bearer_auth(api_key)
                .header("HTTP-Referer", &self.site_url)
                .header("X-Title", &self.site_title)
                .json(&body)
                .send(),
        )
        .await?;
        let mut resp = resp;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            if !tool_defs.is_empty() && Self::should_retry_without_tool_choice(status, &text) {
                resp = self.resend_without_tool_choice(url, body.clone()).await?;
            } else {
                return Err(OrchestratorError::LLMError(format!(
                    "OpenRouter streaming returned {}: {}",
                    status, text
                )));
            }
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenRouter streaming returned {}: {}",
                status, text
            )));
        }
        collect_sse_like_stream(resp, adapter).await
    }

    fn model_ref(&self) -> Option<ModelRef> {
        Some(self.capability_profile.model_ref.clone())
    }

    fn capability_profile(&self) -> Option<ModelCapabilityProfile> {
        Some(self.capability_profile.clone())
    }

    fn provider_health_identity(&self) -> ProviderHealthIdentity {
        ProviderHealthIdentity {
            provider_family: self.capability_profile.model_ref.provider_id.clone(),
            upstream_identity: "https://openrouter.ai/api/v1".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolCall, ToolExecutionResult};
    use aria_core::{ToolChoicePolicy, ToolRuntimePolicy};

    fn backend() -> OpenRouterBackend {
        OpenRouterBackend::new(
            SecretRef::Literal(String::from("test-key")),
            "openai/gpt-4o-mini",
            "https://example.com",
            "ARIA",
        )
    }

    #[test]
    fn openrouter_send_attempts_uses_default_and_env_override() {
        let backend = backend();
        std::env::remove_var("ARIA_OPENROUTER_SEND_ATTEMPTS");
        assert_eq!(
            backend.send_attempts(),
            OpenRouterBackend::DEFAULT_SEND_ATTEMPTS
        );

        std::env::set_var("ARIA_OPENROUTER_SEND_ATTEMPTS", "5");
        assert_eq!(backend.send_attempts(), 5);
        std::env::remove_var("ARIA_OPENROUTER_SEND_ATTEMPTS");
    }

    fn tool() -> CachedTool {
        CachedTool {
            name: String::from("write_file"),
            description: String::from("Write a file"),
            parameters_schema: String::from(
                r#"{"path":{"type":"string"},"content":{"type":"string"}}"#,
            ),
            embedding: Vec::new(),
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        }
    }

    fn executed_tool() -> ExecutedToolCall {
        ExecutedToolCall {
            call: ToolCall {
                invocation_id: Some(String::from("call_router_1")),
                name: String::from("write_file"),
                arguments: String::from(r#"{"path":"notes.txt","content":"hello"}"#),
            },
            result: ToolExecutionResult::structured(
                "write succeeded",
                "write_file",
                json!({"ok": true}),
            ),
        }
    }

    #[test]
    fn openrouter_follow_up_body_uses_openai_compatible_shape() {
        let backend = backend();
        let tool_defs = backend.translated_tool_definitions(&[tool()]);
        let body = backend.build_tool_follow_up_body("save this", &tool_defs, &[executed_tool()]);

        assert_eq!(
            body["messages"][1]["tool_calls"][0]["id"],
            json!("call_router_1")
        );
        assert_eq!(body["messages"][2]["role"], json!("tool"));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["tools"][0]["type"], json!("function"));
    }

    #[test]
    fn openrouter_tool_policy_can_disable_tools() {
        let backend = backend();
        let tool_defs = backend.translated_tool_definitions(&[tool()]);
        let mut body =
            backend.build_tool_follow_up_body("save this", &tool_defs, &[executed_tool()]);
        super::super::apply_openai_compatible_tool_policy(
            &mut body,
            &tool_defs,
            &ToolRuntimePolicy {
                tool_choice: ToolChoicePolicy::None,
                allow_parallel_tool_calls: true,
            },
        );

        assert!(body.get("tools").is_none());
        assert!(body.get("tool_choice").is_none());
    }

    #[test]
    fn openrouter_parse_response_json_repairs_text_tool_calls() {
        let backend = backend();
        let payload = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "{\"tool\":\"write_file\",\"args\":{\"path\":\"notes.txt\",\"content\":\"hello\"}}"
                }
            }]
        });

        let parsed = backend
            .parse_response_json(&payload, &[tool()])
            .expect("response parse succeeds")
            .expect("response should exist");

        match parsed {
            LLMResponse::ToolCalls(calls) => {
                assert_eq!(calls.len(), 1);
                assert_eq!(calls[0].name, "write_file");
            }
            other => panic!("expected tool call, got {:?}", other),
        }
    }

    #[test]
    fn openrouter_apply_completion_cap_sets_default_max_tokens() {
        let backend = backend();
        let mut body = json!({});
        backend.apply_completion_cap(&mut body);
        assert_eq!(
            body["max_tokens"],
            json!(OpenRouterBackend::DEFAULT_MAX_TOKENS)
        );
    }

    #[test]
    fn openrouter_retries_without_tool_choice_for_provider_routing_404() {
        assert!(OpenRouterBackend::should_retry_without_tool_choice(
            reqwest::StatusCode::NOT_FOUND,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
        assert!(!OpenRouterBackend::should_retry_without_tool_choice(
            reqwest::StatusCode::BAD_REQUEST,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
    }
}

pub struct OpenRouterProvider {
    pub api_key: SecretRef,
    pub site_url: String,
    pub site_title: String,
    pub egress_broker: Option<EgressCredentialBroker>,
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

fn classify_openrouter_model(
    model_id: &str,
    description: Option<&str>,
) -> (
    CapabilitySupport,
    CapabilitySupport,
    CapabilitySupport,
    CapabilitySupport,
    String,
) {
    let combined = format!(
        "{} {}",
        model_id.to_ascii_lowercase(),
        description.unwrap_or_default().to_ascii_lowercase()
    );
    let supports_images = if combined.contains("vision")
        || combined.contains("multimodal")
        || combined.contains("image")
        || combined.contains("gpt-4o")
        || combined.contains("claude-3")
        || combined.contains("gemini")
    {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unknown
    };
    let supports_audio = if combined.contains("audio") || combined.contains("speech") {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unknown
    };
    let tool_calling = if combined.contains("gpt-4")
        || combined.contains("gpt-4o")
        || combined.contains("claude")
        || combined.contains("gemini")
    {
        CapabilitySupport::Supported
    } else if combined.contains("gpt") || combined.contains("qwen") {
        CapabilitySupport::Degraded
    } else {
        CapabilitySupport::Unknown
    };
    let vision = if matches!(supports_images, CapabilitySupport::Supported) {
        CapabilitySupport::Supported
    } else {
        CapabilitySupport::Unknown
    };
    (
        tool_calling,
        vision,
        supports_images,
        supports_audio,
        String::from("catalog+heuristic"),
    )
}

#[async_trait]
impl ModelProvider for OpenRouterProvider {
    fn id(&self) -> &str {
        "openrouter"
    }

    fn name(&self) -> &str {
        "OpenRouter"
    }

    fn adapter_family(&self) -> AdapterFamily {
        AdapterFamily::OpenAiCompatible
    }

    fn provider_capability_profile(
        &self,
        observed_at_us: u64,
    ) -> aria_core::ProviderCapabilityProfile {
        aria_core::ProviderCapabilityProfile {
            provider_id: self.id().to_string(),
            adapter_family: self.adapter_family(),
            supports_model_listing: CapabilitySupport::Supported,
            supports_runtime_probe: CapabilitySupport::Supported,
            source: CapabilitySourceKind::ProviderCatalog,
            observed_at_us,
        }
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
        Ok(Box::new(
            OpenRouterBackend::new(
                self.api_key.clone(),
                model_id,
                self.site_url.clone(),
                self.site_title.clone(),
            )
            .with_egress_broker(
                self.egress_broker
                    .clone()
                    .unwrap_or_else(EgressCredentialBroker::new),
            ),
        ))
    }

    fn create_backend_with_profile(
        &self,
        profile: &ModelCapabilityProfile,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(
            OpenRouterBackend::with_capability_profile(
                self.api_key.clone(),
                profile.clone(),
                self.site_url.clone(),
                self.site_title.clone(),
            )
            .with_egress_broker(
                self.egress_broker
                    .clone()
                    .unwrap_or_else(EgressCredentialBroker::new),
            ),
        ))
    }

    async fn probe_model_capabilities(
        &self,
        model_id: &str,
        observed_at_us: u64,
    ) -> Result<ModelCapabilityProbeRecord, OrchestratorError> {
        let mut max_context_tokens = None;
        let (tool_calling, vision, supports_images, supports_audio, provenance) =
            match self.list_models().await {
                Ok(models) => {
                    if let Some(model) = models
                        .into_iter()
                        .find(|candidate| candidate.id == model_id)
                    {
                        max_context_tokens = model.context_length.map(|value| value as u32);
                        classify_openrouter_model(&model.id, model.description.as_deref())
                    } else {
                        let (tool_calling, vision, supports_images, supports_audio, _) =
                            classify_openrouter_model(model_id, None);
                        (
                            tool_calling,
                            vision,
                            supports_images,
                            supports_audio,
                            String::from("heuristic:fallback-no-catalog-match"),
                        )
                    }
                }
                Err(_) => {
                    let (tool_calling, vision, supports_images, supports_audio, _) =
                        classify_openrouter_model(model_id, None);
                    (
                        tool_calling,
                        vision,
                        supports_images,
                        supports_audio,
                        String::from("heuristic:fallback-list-models-failed"),
                    )
                }
            };
        Ok(ModelCapabilityProbeRecord {
            probe_id: format!("probe-openrouter-{}-{}", model_id, observed_at_us),
            model_ref: ModelRef::new("openrouter", model_id),
            adapter_family: AdapterFamily::OpenAiCompatible,
            tool_calling,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Supported,
            vision,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens,
            supports_images,
            supports_audio,
            schema_acceptance: Some(CapabilitySupport::Supported),
            native_tool_probe: Some(tool_calling),
            modality_probe: Some(vision),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("catalog_lookup")),
            probe_status: Some(String::from("success")),
            probe_error: None,
            raw_summary: Some(format!(
                "openrouter probe for '{}' via {}",
                model_id, provenance
            )),
            observed_at_us,
            expires_at_us: Some(observed_at_us + 86_400_000_000),
        })
    }
}
