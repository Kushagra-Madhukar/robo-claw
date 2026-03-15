use super::{
    adapter_for_family, collect_sse_like_stream, default_model_capability_profile,
    send_with_response_start_timeout, LLMBackend, ModelMetadata, ModelProvider,
    ProviderHealthIdentity,
};
use crate::{CachedTool, LLMResponse, OrchestratorError, ToolCall};
use aria_core::{
    AdapterFamily, CapabilitySourceKind, CapabilitySupport, ModelCapabilityProbeRecord,
    ModelCapabilityProfile, ModelRef, ToolCallingMode, ToolRuntimePolicy,
};
use async_trait::async_trait;
use serde_json::json;

#[derive(Debug, Clone)]
pub struct OllamaBackend {
    base_url: String,
    model: String,
    capability_profile: ModelCapabilityProfile,
    client: reqwest::Client,
}

impl OllamaBackend {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        let model = model.into();
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            capability_profile: default_model_capability_profile(
                "ollama",
                &model,
                AdapterFamily::OllamaNative,
                0,
            ),
            model,
            client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn with_capability_profile(
        base_url: impl Into<String>,
        profile: ModelCapabilityProfile,
    ) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            model: profile.model_ref.model_id.clone(),
            capability_profile: profile,
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

        let resp = send_with_response_start_timeout(
            "Ollama streaming",
            self.client.post(&url).json(&body).send(),
        )
        .await?;

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
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_policy(prompt, tools, &ToolRuntimePolicy::default())
            .await
    }

    async fn query_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        _policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);

        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !filtered_tools.is_empty()
        {
            let url = format!("{}/api/chat", self.base_url);
            let body = json!({
                "model": self.model,
                "messages": [{ "role": "user", "content": prompt }],
                "stream": false,
                "tools": filtered_tools
                    .iter()
                    .filter_map(|tool| adapter.translate_tool_definition(&self.capability_profile, tool).ok())
                    .collect::<Vec<_>>()
            });

            let resp = send_with_response_start_timeout(
                "Ollama chat",
                self.client.post(&url).json(&body).send(),
            )
            .await?;

            if resp.status().is_success() {
                let res_json: serde_json::Value = resp.json().await.map_err(|e| {
                    OrchestratorError::LLMError(format!("Ollama chat JSON parse failed: {}", e))
                })?;
                if let Some(tool_calls) = res_json["message"]["tool_calls"].as_array() {
                    let normalized = tool_calls
                        .iter()
                        .filter_map(|call| {
                            let name = call["function"]["name"].as_str()?.to_string();
                            let arguments = if call["function"]["arguments"].is_string() {
                                call["function"]["arguments"].as_str()?.to_string()
                            } else {
                                call["function"]["arguments"].to_string()
                            };
                            Some(ToolCall {
                                invocation_id: None,
                                name,
                                arguments,
                            })
                        })
                        .collect::<Vec<_>>();
                    if !normalized.is_empty() {
                        return Ok(LLMResponse::ToolCalls(normalized));
                    }
                }
                if let Some(content) = res_json["message"]["content"].as_str() {
                    return Ok(LLMResponse::TextAnswer(content.trim().to_string()));
                }
            }
        }

        let url = format!("{}/api/generate", self.base_url);
        let body = json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false
        });

        let resp =
            send_with_response_start_timeout("Ollama", self.client.post(&url).json(&body).send())
                .await?;

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

    async fn query_context_with_policy(
        &self,
        context: &aria_core::ExecutionContextPack,
        tools: &[CachedTool],
        _policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let messages = super::build_openai_compatible_initial_messages(context);

        let url = format!("{}/api/chat", self.base_url);
        let mut body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !filtered_tools.is_empty()
        {
            body["tools"] = serde_json::Value::Array(
                filtered_tools
                    .iter()
                    .filter_map(|tool| {
                        adapter
                            .translate_tool_definition(&self.capability_profile, tool)
                            .ok()
                    })
                    .collect::<Vec<_>>(),
            );
        }
        let resp = send_with_response_start_timeout(
            "Ollama chat",
            self.client.post(&url).json(&body).send(),
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Ollama chat returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("Ollama chat JSON parse failed: {}", e))
        })?;
        if let Some(tool_calls) = res_json["message"]["tool_calls"].as_array() {
            let normalized = tool_calls
                .iter()
                .filter_map(|call| {
                    let name = call["function"]["name"].as_str()?.to_string();
                    let arguments = if call["function"]["arguments"].is_string() {
                        call["function"]["arguments"].as_str()?.to_string()
                    } else {
                        call["function"]["arguments"].to_string()
                    };
                    Some(ToolCall {
                        invocation_id: None,
                        name,
                        arguments,
                    })
                })
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                return Ok(LLMResponse::ToolCalls(normalized));
            }
        }
        if let Some(content) = res_json["message"]["content"].as_str() {
            return Ok(LLMResponse::TextAnswer(content.trim().to_string()));
        }
        Err(OrchestratorError::LLMError(
            "Ollama chat returned no content".into(),
        ))
    }

    fn inspect_context_payload(
        &self,
        context: &aria_core::ExecutionContextPack,
        tools: &[CachedTool],
        _policy: &ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let messages = super::build_openai_compatible_initial_messages(context);
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !filtered_tools.is_empty()
        {
            Some(json!({
                "model": self.model,
                "messages": messages,
                "stream": false,
                "tools": filtered_tools
                    .iter()
                    .filter_map(|tool| adapter.translate_tool_definition(&self.capability_profile, tool).ok())
                    .collect::<Vec<_>>()
            }))
        } else {
            Some(json!({
                "model": self.model,
                "messages": messages,
                "stream": false
            }))
        }
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !filtered_tools.is_empty()
            && filtered_tools.iter().all(|tool| tool.streaming_safe)
        {
            let url = format!("{}/api/chat", self.base_url);
            let body = json!({
                "model": self.model,
                "messages": [{ "role": "user", "content": prompt }],
                "stream": true,
                "tools": filtered_tools
                    .iter()
                    .filter_map(|tool| adapter.translate_tool_definition(&self.capability_profile, tool).ok())
                    .collect::<Vec<_>>()
            });
            let resp = send_with_response_start_timeout(
                "Ollama chat streaming",
                self.client.post(&url).json(&body).send(),
            )
            .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(OrchestratorError::LLMError(format!(
                    "Ollama chat streaming returned {}: {}",
                    status, text
                )));
            }
            return collect_sse_like_stream(resp, adapter).await;
        }
        let _ = policy;
        let text = self.query_streaming(prompt).await?;
        Ok(LLMResponse::TextAnswer(text))
    }

    async fn query_stream_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[crate::ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !filtered_tools.is_empty()
            && filtered_tools.iter().all(|tool| tool.streaming_safe)
        {
            let url = format!("{}/api/chat", self.base_url);
            let model_calls = executed_tools
                .iter()
                .map(|entry| {
                    let invocation = entry.call.invocation_envelope();
                    json!({
                        "role": "assistant",
                        "tool_calls": [{
                            "function": {
                                "name": invocation.tool_name,
                                "arguments": serde_json::from_str::<serde_json::Value>(&invocation.arguments_json)
                                    .unwrap_or_else(|_| json!({}))
                            }
                        }]
                    })
                })
                .collect::<Vec<_>>();
            let tool_messages = executed_tools
                .iter()
                .map(|entry| {
                    let result = entry.result_envelope();
                    json!({
                        "role": "tool",
                        "content": result.as_provider_payload()
                    })
                })
                .collect::<Vec<_>>();
            let mut messages = vec![json!({ "role": "user", "content": prompt })];
            messages.extend(model_calls);
            messages.extend(tool_messages);
            let body = json!({
                "model": self.model,
                "messages": messages,
                "stream": true,
                "tools": filtered_tools
                    .iter()
                    .filter_map(|tool| adapter.translate_tool_definition(&self.capability_profile, tool).ok())
                    .collect::<Vec<_>>()
            });
            let resp = send_with_response_start_timeout(
                "Ollama chat streaming",
                self.client.post(&url).json(&body).send(),
            )
            .await?;
            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(OrchestratorError::LLMError(format!(
                    "Ollama chat streaming returned {}: {}",
                    status, text
                )));
            }
            return collect_sse_like_stream(resp, adapter).await;
        }
        self.query_with_tool_results_and_policy(prompt, tools, executed_tools, policy)
            .await
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
            upstream_identity: self.base_url.clone(),
        }
    }
}

pub struct OllamaProvider {
    pub base_url: String,
}

fn extract_ollama_context_tokens(value: &serde_json::Value) -> Option<u32> {
    let object = value.as_object()?;
    for (key, value) in object {
        if key.to_ascii_lowercase().contains("context")
            || key.to_ascii_lowercase().contains("num_ctx")
        {
            if let Some(raw) = value.as_u64() {
                return u32::try_from(raw).ok();
            }
        }
        if let Some(found) = extract_ollama_context_tokens(value) {
            return Some(found);
        }
    }
    None
}

#[async_trait]
impl ModelProvider for OllamaProvider {
    fn id(&self) -> &str {
        "ollama"
    }

    fn name(&self) -> &str {
        "Ollama (Local)"
    }

    fn adapter_family(&self) -> AdapterFamily {
        AdapterFamily::OllamaNative
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

    fn create_backend_with_profile(
        &self,
        profile: &ModelCapabilityProfile,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(OllamaBackend::with_capability_profile(
            self.base_url.clone(),
            profile.clone(),
        )))
    }

    async fn probe_model_capabilities(
        &self,
        model_id: &str,
        observed_at_us: u64,
    ) -> Result<ModelCapabilityProbeRecord, OrchestratorError> {
        let show_url = format!("{}/api/show", self.base_url.trim_end_matches('/'));
        let lower = model_id.to_ascii_lowercase();
        let tool_calling =
            if lower.contains("qwen") || lower.contains("llama3.1") || lower.contains("llama3.2") {
                CapabilitySupport::Degraded
            } else {
                CapabilitySupport::Unknown
            };
        let mut vision = if lower.contains("vision") || lower.contains("vl") {
            CapabilitySupport::Supported
        } else {
            CapabilitySupport::Unsupported
        };
        let mut max_context_tokens = None;
        let (provenance, probe_status, probe_error) = match reqwest::Client::new()
            .post(&show_url)
            .json(&json!({ "model": model_id }))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                match response.json::<serde_json::Value>().await {
                    Ok(payload) => {
                        max_context_tokens = extract_ollama_context_tokens(&payload);
                        let payload_text = payload.to_string().to_ascii_lowercase();
                        if payload_text.contains("vision") || payload_text.contains("multimodal") {
                            vision = CapabilitySupport::Supported;
                        }
                        (
                            String::from("api:show+heuristic"),
                            String::from("success"),
                            None,
                        )
                    }
                    Err(err) => (
                        String::from("heuristic:fallback-show-parse-failed"),
                        String::from("degraded"),
                        Some(err.to_string()),
                    ),
                }
            }
            Ok(response) => (
                String::from("heuristic:fallback-show-http-error"),
                String::from("degraded"),
                Some(format!("http {}", response.status())),
            ),
            Err(err) => (
                String::from("heuristic:fallback-show-request-failed"),
                String::from("degraded"),
                Some(err.to_string()),
            ),
        };
        Ok(ModelCapabilityProbeRecord {
            probe_id: format!("probe-ollama-{}-{}", model_id, observed_at_us),
            model_ref: ModelRef::new("ollama", model_id),
            adapter_family: AdapterFamily::OllamaNative,
            tool_calling,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Supported,
            vision,
            json_mode: CapabilitySupport::Degraded,
            max_context_tokens,
            supports_images: vision,
            supports_audio: CapabilitySupport::Unknown,
            schema_acceptance: Some(CapabilitySupport::Degraded),
            native_tool_probe: Some(tool_calling),
            modality_probe: Some(vision),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("api_show")),
            probe_status: Some(probe_status),
            probe_error,
            raw_summary: Some(format!(
                "ollama probe for '{}' via {}",
                model_id, provenance
            )),
            observed_at_us,
            expires_at_us: Some(observed_at_us + 86_400_000_000),
        })
    }
}
