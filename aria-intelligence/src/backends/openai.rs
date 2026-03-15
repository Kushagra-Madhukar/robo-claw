use super::{
    adapter_for_family, build_openai_compatible_context_body,
    build_openai_compatible_followup_body, collect_sse_like_stream,
    default_model_capability_profile, send_with_response_start_timeout, EgressCredentialBroker,
    LLMBackend, ModelMetadata, ModelProvider, ProviderHealthIdentity, SecretRef,
};
use crate::{CachedTool, ExecutedToolCall, LLMResponse, OrchestratorError, ToolCall};
use aria_core::{
    AdapterFamily, CapabilitySourceKind, CapabilitySupport, ExecutionContextPack,
    ModelCapabilityProbeRecord, ModelCapabilityProfile, ModelRef, ToolCallingMode,
    ToolRuntimePolicy,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct OpenAiBackend {
    pub api_key: SecretRef,
    pub model: String,
    pub base_url: String,
    capability_profile: ModelCapabilityProfile,
    client: reqwest::Client,
    egress_broker: EgressCredentialBroker,
}

impl OpenAiBackend {
    pub fn new(api_key: SecretRef, model: impl Into<String>, base_url: impl Into<String>) -> Self {
        let model = model.into();
        Self {
            api_key,
            capability_profile: default_model_capability_profile(
                "openai",
                &model,
                AdapterFamily::OpenAiCompatible,
                0,
            ),
            model,
            base_url: base_url.into().trim_end_matches('/').to_string(),
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
        base_url: impl Into<String>,
    ) -> Self {
        Self {
            api_key,
            model: profile.model_ref.model_id.clone(),
            capability_profile: profile,
            base_url: base_url.into().trim_end_matches('/').to_string(),
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
}

#[async_trait]
impl LLMBackend for OpenAiBackend {
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
        let api_key = self.api_key.resolve_with_broker(
            "api.openai.com",
            "openai_provider",
            &self.egress_broker,
        )?;
        let url = format!("{}/chat/completions", self.base_url);
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
            "stream": false
        });
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !tool_defs.is_empty()
        {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }

        let resp = send_with_response_start_timeout(
            "OpenAI",
            self.client
                .post(&url)
                .bearer_auth(api_key)
                .json(&body)
                .send(),
        )
        .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI returned {}: {}",
                status, text
            )));
        }

        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("OpenAI JSON parse failed: {}", e)))?;

        if let Some(tool_calls) = res_json["choices"][0]["message"]["tool_calls"].as_array() {
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
                        invocation_id: call["id"].as_str().map(|v| v.to_string()),
                        name,
                        arguments,
                    })
                })
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                return Ok(LLMResponse::ToolCalls(normalized));
            }
        }

        if let Some(content) = res_json["choices"][0]["message"]["content"].as_str() {
            return Ok(LLMResponse::TextAnswer(content.to_string()));
        }

        Err(OrchestratorError::LLMError(
            "OpenAI returned no content".into(),
        ))
    }

    async fn query_context_with_policy(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "api.openai.com",
            "openai_provider",
            &self.egress_broker,
        )?;
        let url = format!("{}/chat/completions", self.base_url);
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
        let body = super::build_openai_compatible_context_body(
            &self.model,
            context,
            if matches!(
                tool_mode,
                ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
            ) {
                tool_defs
            } else {
                Vec::new()
            },
            policy,
        );
        let resp = send_with_response_start_timeout(
            "OpenAI",
            self.client
                .post(&url)
                .bearer_auth(api_key)
                .json(&body)
                .send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("OpenAI JSON parse failed: {}", e)))?;
        if let Some(tool_calls) = res_json["choices"][0]["message"]["tool_calls"].as_array() {
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
                        invocation_id: call["id"].as_str().map(|v| v.to_string()),
                        name,
                        arguments,
                    })
                })
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                return Ok(LLMResponse::ToolCalls(normalized));
            }
        }
        if let Some(content) = res_json["choices"][0]["message"]["content"].as_str() {
            return Ok(LLMResponse::TextAnswer(content.to_string()));
        }
        Err(OrchestratorError::LLMError(
            "OpenAI returned no content".into(),
        ))
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "api.openai.com",
            "openai_provider",
            &self.egress_broker,
        )?;
        let url = format!("{}/chat/completions", self.base_url);
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
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !tool_defs.is_empty()
        {
            super::apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
        }
        let resp = send_with_response_start_timeout(
            "OpenAI streaming",
            self.client
                .post(&url)
                .bearer_auth(api_key)
                .json(&body)
                .send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI streaming returned {}: {}",
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
            "api.openai.com",
            "openai_provider",
            &self.egress_broker,
        )?;
        let url = format!("{}/chat/completions", self.base_url);
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let tool_defs = self.translated_tool_definitions(tools);
        let mut body = self.build_tool_follow_up_body(prompt, &tool_defs, executed_tools);
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
            "OpenAI",
            self.client
                .post(&url)
                .bearer_auth(api_key)
                .json(&body)
                .send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("OpenAI JSON parse failed: {}", e)))?;
        if let Some(tool_calls) = res_json["choices"][0]["message"]["tool_calls"].as_array() {
            let normalized = tool_calls
                .iter()
                .filter_map(|call| {
                    Some(ToolCall {
                        invocation_id: call["id"].as_str().map(|v| v.to_string()),
                        name: call["function"]["name"].as_str()?.to_string(),
                        arguments: if call["function"]["arguments"].is_string() {
                            call["function"]["arguments"].as_str()?.to_string()
                        } else {
                            call["function"]["arguments"].to_string()
                        },
                    })
                })
                .collect::<Vec<_>>();
            if !normalized.is_empty() {
                return Ok(LLMResponse::ToolCalls(normalized));
            }
        }
        Ok(LLMResponse::TextAnswer(
            res_json["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ))
    }

    async fn query_stream_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "api.openai.com",
            "openai_provider",
            &self.egress_broker,
        )?;
        let url = format!("{}/chat/completions", self.base_url);
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let tool_defs = self.translated_tool_definitions(tools);
        let mut body = self.build_tool_follow_up_body(prompt, &tool_defs, executed_tools);
        body["stream"] = json!(true);
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
            "OpenAI streaming",
            self.client
                .post(&url)
                .bearer_auth(api_key)
                .json(&body)
                .send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI streaming returned {}: {}",
                status, text
            )));
        }
        collect_sse_like_stream(resp, adapter).await
    }

    fn inspect_context_payload(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        let tool_defs = self.translated_tool_definitions(tools);
        Some(build_openai_compatible_context_body(
            &self.model,
            context,
            tool_defs,
            policy,
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToolCall, ToolExecutionResult};
    use aria_core::{ToolChoicePolicy, ToolRuntimePolicy};

    fn backend() -> OpenAiBackend {
        OpenAiBackend::new(
            SecretRef::Literal(String::from("test-key")),
            "gpt-4o-mini",
            "https://api.openai.com/v1",
        )
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
                invocation_id: Some(String::from("call_123")),
                name: String::from("write_file"),
                arguments: String::from(r#"{"path":"notes.txt","content":"hello"}"#),
            },
            result: ToolExecutionResult::structured(
                "write succeeded",
                "write_file",
                json!({"ok": true, "bytes": 5}),
            ),
        }
    }

    fn context_pack() -> ExecutionContextPack {
        ExecutionContextPack {
            system_prompt: String::from("system guidance"),
            history_messages: vec![aria_core::PromptContextMessage {
                role: String::from("user"),
                content: String::from("hello"),
                timestamp_us: 1,
            }],
            context_blocks: vec![aria_core::ContextBlock {
                kind: aria_core::ContextBlockKind::Retrieval,
                label: String::from("retrieval"),
                content: String::from("source text"),
                token_estimate: 3,
            }],
            user_request: String::from("save this"),
            channel: aria_core::GatewayChannel::Cli,
            execution_contract: None,
            retrieved_context: None,
        }
    }

    #[test]
    fn openai_follow_up_body_uses_structured_tool_messages() {
        let backend = backend();
        let tool_defs = backend.translated_tool_definitions(&[tool()]);
        let body = backend.build_tool_follow_up_body("save this", &tool_defs, &[executed_tool()]);

        assert_eq!(body["model"], json!("gpt-4o-mini"));
        assert_eq!(body["messages"][0]["role"], json!("user"));
        assert_eq!(body["messages"][1]["role"], json!("assistant"));
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["id"],
            json!("call_123")
        );
        assert_eq!(
            body["messages"][1]["tool_calls"][0]["function"]["name"],
            json!("write_file")
        );
        assert_eq!(body["messages"][2]["role"], json!("tool"));
        assert_eq!(body["messages"][2]["tool_call_id"], json!("call_123"));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["tools"][0]["function"]["name"], json!("write_file"));
    }

    #[test]
    fn openai_tool_policy_can_force_specific_tool_and_disable_parallel() {
        let backend = backend();
        let tool_defs = backend.translated_tool_definitions(&[tool()]);
        let mut body =
            backend.build_tool_follow_up_body("save this", &tool_defs, &[executed_tool()]);
        super::super::apply_openai_compatible_tool_policy(
            &mut body,
            &tool_defs,
            &ToolRuntimePolicy {
                tool_choice: ToolChoicePolicy::Specific(String::from("write_file")),
                allow_parallel_tool_calls: false,
            },
        );

        assert_eq!(body["tool_choice"]["function"]["name"], json!("write_file"));
        assert_eq!(body["parallel_tool_calls"], json!(false));
    }

    #[test]
    fn openai_inspect_context_payload_includes_messages_and_tools() {
        let backend = backend();
        let payload = backend
            .inspect_context_payload(&context_pack(), &[tool()], &ToolRuntimePolicy::default())
            .expect("payload");

        assert_eq!(payload["model"], json!("gpt-4o-mini"));
        assert!(payload["messages"].is_array());
        assert_eq!(payload["tools"][0]["function"]["name"], json!("write_file"));
    }
}

pub struct OpenAiProvider {
    pub api_key: SecretRef,
    pub base_url: String,
    pub egress_broker: Option<EgressCredentialBroker>,
}

#[derive(Debug, Deserialize)]
struct OpenAiModel {
    id: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModel>,
}

#[async_trait]
impl ModelProvider for OpenAiProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn name(&self) -> &str {
        "OpenAI"
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
        let broker = self
            .egress_broker
            .clone()
            .unwrap_or_else(EgressCredentialBroker::new);
        let api_key =
            self.api_key
                .resolve_with_broker("api.openai.com", "openai_provider", &broker)?;
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let resp = reqwest::Client::new()
            .get(&url)
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|e| {
                OrchestratorError::LLMError(format!("OpenAI list models failed: {}", e))
            })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "OpenAI models list returned {}: {}",
                status, text
            )));
        }
        let json: OpenAiModelsResponse = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("OpenAI models JSON failed: {}", e))
        })?;
        Ok(json
            .data
            .into_iter()
            .map(|m| ModelMetadata {
                name: m.id.clone(),
                id: m.id,
                description: None,
                context_length: None,
            })
            .collect())
    }

    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(
            OpenAiBackend::new(self.api_key.clone(), model_id, self.base_url.clone())
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
            OpenAiBackend::with_capability_profile(
                self.api_key.clone(),
                profile.clone(),
                self.base_url.clone(),
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
        let lower = model_id.to_ascii_lowercase();
        let tool_calling = if lower.starts_with("gpt-") || lower.starts_with("o") {
            CapabilitySupport::Supported
        } else {
            CapabilitySupport::Degraded
        };
        let supports_images = if lower.contains("gpt-4o") || lower.contains("gpt-4.1") {
            CapabilitySupport::Supported
        } else {
            CapabilitySupport::Unknown
        };
        Ok(ModelCapabilityProbeRecord {
            probe_id: format!("probe-openai-{}-{}", model_id, observed_at_us),
            model_ref: ModelRef::new("openai", model_id),
            adapter_family: AdapterFamily::OpenAiCompatible,
            tool_calling,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Supported,
            vision: supports_images,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens: None,
            supports_images,
            supports_audio: CapabilitySupport::Unknown,
            schema_acceptance: Some(CapabilitySupport::Supported),
            native_tool_probe: Some(tool_calling),
            modality_probe: Some(supports_images),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("models_api+heuristic")),
            probe_status: Some(String::from("success")),
            probe_error: None,
            raw_summary: Some(format!("openai probe for '{}'", model_id)),
            observed_at_us,
            expires_at_us: Some(observed_at_us + 86_400_000_000),
        })
    }
}
