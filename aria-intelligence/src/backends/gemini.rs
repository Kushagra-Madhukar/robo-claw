use super::{
    adapter_for_family, build_gemini_context_body, collect_sse_like_stream,
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
pub struct GeminiBackend {
    pub api_key: SecretRef,
    pub model: String,
    pub base_url: String,
    capability_profile: ModelCapabilityProfile,
    client: reqwest::Client,
    egress_broker: EgressCredentialBroker,
}

impl GeminiBackend {
    pub(crate) const FUNCTION_CALL_PART_METADATA_PREFIX: &'static str =
        "gemini_function_call_part:";

    pub fn new(api_key: SecretRef, model: impl Into<String>, base_url: impl Into<String>) -> Self {
        let model = model.into();
        Self {
            api_key,
            capability_profile: default_model_capability_profile(
                "gemini",
                &model,
                AdapterFamily::GoogleGemini,
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

    fn api_model_path(&self) -> String {
        if self.model.starts_with("models/") {
            self.model.clone()
        } else {
            format!("models/{}", self.model)
        }
    }

    fn translated_function_declarations(&self, tools: &[CachedTool]) -> Vec<Value> {
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        adapter
            .filter_tools(&self.capability_profile, tools)
            .iter()
            .filter_map(|tool| {
                let translated = adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()?;
                Some(json!({
                    "name": translated["function"]["name"],
                    "description": translated["function"]["description"],
                    "parameters": translated["function"]["parameters"]
                }))
            })
            .collect::<Vec<_>>()
    }

    fn build_tool_follow_up_body(
        &self,
        prompt: &str,
        function_declarations: &[Value],
        executed_tools: &[ExecutedToolCall],
    ) -> Value {
        let model_parts = executed_tools
            .iter()
            .map(|entry| {
                let invocation = entry.call.invocation_envelope();
                let mut part = invocation
                    .invocation_id
                    .as_deref()
                    .and_then(Self::decode_function_call_part_metadata)
                    .unwrap_or_else(|| json!({}));
                if !part.is_object() {
                    part = json!({});
                }
                if !part.get("functionCall").is_some_and(Value::is_object) {
                    part["functionCall"] = json!({});
                }
                part["functionCall"]["name"] = Value::String(invocation.tool_name.clone());
                part["functionCall"]["args"] =
                    serde_json::from_str::<Value>(&invocation.arguments_json)
                        .unwrap_or_else(|_| json!({}));
                part
            })
            .collect::<Vec<_>>();
        let tool_parts = executed_tools
            .iter()
            .map(|entry| {
                let invocation = entry.call.invocation_envelope();
                let result = entry.result_envelope();
                json!({
                    "functionResponse": {
                        "name": invocation.tool_name,
                        "response": {
                            "name": invocation.tool_name,
                            "content": result.as_provider_payload()
                        }
                    }
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "contents": [
                { "role": "user", "parts": [{ "text": prompt }] },
                { "role": "model", "parts": model_parts },
                { "role": "user", "parts": tool_parts }
            ]
        });
        if !function_declarations.is_empty() {
            body["tools"] = json!([{ "functionDeclarations": function_declarations }]);
            body["toolConfig"] = json!({ "functionCallingConfig": { "mode": "AUTO" } });
        }
        body
    }

    pub(crate) fn encode_function_call_part_metadata(function_call_part: &Value) -> Option<String> {
        if !function_call_part.is_object() {
            return None;
        }
        serde_json::to_string(function_call_part)
            .ok()
            .map(|raw| format!("{}{}", Self::FUNCTION_CALL_PART_METADATA_PREFIX, raw))
    }

    pub(crate) fn decode_function_call_part_metadata(encoded: &str) -> Option<Value> {
        let raw = encoded.strip_prefix(Self::FUNCTION_CALL_PART_METADATA_PREFIX)?;
        serde_json::from_str(raw).ok()
    }
}

#[async_trait]
impl LLMBackend for GeminiBackend {
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
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &self.egress_broker,
        )?;
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url,
            self.api_model_path(),
            api_key
        );
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let function_declarations = filtered_tools
            .iter()
            .filter_map(|tool| {
                let translated = adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()?;
                Some(json!({
                    "name": translated["function"]["name"],
                    "description": translated["function"]["description"],
                    "parameters": translated["function"]["parameters"]
                }))
            })
            .collect::<Vec<_>>();

        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !function_declarations.is_empty()
        {
            super::apply_gemini_tool_policy(&mut body, &function_declarations, policy);
        }

        let resp =
            send_with_response_start_timeout("Gemini", self.client.post(&url).json(&body).send())
                .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("Gemini JSON parse failed: {}", e)))?;
        if let Some(parts) = res_json["candidates"][0]["content"]["parts"].as_array() {
            let mut tool_calls = Vec::new();
            let mut text_parts = Vec::new();
            for part in parts {
                if let Some(function_call) = part.get("functionCall") {
                    if let Some(name) = function_call["name"].as_str() {
                        tool_calls.push(ToolCall {
                            invocation_id: Self::encode_function_call_part_metadata(part),
                            name: name.to_string(),
                            arguments: function_call["args"].to_string(),
                        });
                    }
                } else if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            if !tool_calls.is_empty() {
                return Ok(LLMResponse::ToolCalls(tool_calls));
            }
            if !text_parts.is_empty() {
                return Ok(LLMResponse::TextAnswer(text_parts.join("\n")));
            }
        }
        Err(OrchestratorError::LLMError(
            "Gemini returned no content".into(),
        ))
    }

    async fn query_context_with_policy(
        &self,
        context: &aria_core::ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &self.egress_broker,
        )?;
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url,
            self.api_model_path(),
            api_key
        );
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let function_declarations = self.translated_function_declarations(&filtered_tools);
        let body = super::build_gemini_context_body(
            context,
            if matches!(
                tool_mode,
                ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
            ) {
                &function_declarations
            } else {
                &[]
            },
            policy,
        );

        let resp =
            send_with_response_start_timeout("Gemini", self.client.post(&url).json(&body).send())
                .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("Gemini JSON parse failed: {}", e)))?;
        if let Some(parts) = res_json["candidates"][0]["content"]["parts"].as_array() {
            let mut tool_calls = Vec::new();
            let mut text_parts = Vec::new();
            for part in parts {
                if let Some(function_call) = part.get("functionCall") {
                    if let Some(name) = function_call["name"].as_str() {
                        tool_calls.push(ToolCall {
                            invocation_id: Self::encode_function_call_part_metadata(part),
                            name: name.to_string(),
                            arguments: function_call["args"].to_string(),
                        });
                    }
                } else if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            if !tool_calls.is_empty() {
                return Ok(LLMResponse::ToolCalls(tool_calls));
            }
            if !text_parts.is_empty() {
                return Ok(LLMResponse::TextAnswer(text_parts.join("\n")));
            }
        }
        Err(OrchestratorError::LLMError(
            "Gemini returned no content".into(),
        ))
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let api_key = self.api_key.resolve_with_broker(
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &self.egress_broker,
        )?;
        let url = format!(
            "{}/{}:streamGenerateContent?key={}",
            self.base_url,
            self.api_model_path(),
            api_key
        );
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        let filtered_tools = adapter.filter_tools(&self.capability_profile, tools);
        let tool_mode = adapter.tool_calling_mode(&self.capability_profile);
        let function_declarations = filtered_tools
            .iter()
            .filter_map(|tool| {
                let translated = adapter
                    .translate_tool_definition(&self.capability_profile, tool)
                    .ok()?;
                Some(json!({
                    "name": translated["function"]["name"],
                    "description": translated["function"]["description"],
                    "parameters": translated["function"]["parameters"]
                }))
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "contents": [{
                "role": "user",
                "parts": [{ "text": prompt }]
            }]
        });
        if matches!(
            tool_mode,
            ToolCallingMode::NativeTools | ToolCallingMode::CompatTools
        ) && !function_declarations.is_empty()
        {
            super::apply_gemini_tool_policy(&mut body, &function_declarations, policy);
        }
        let resp = send_with_response_start_timeout(
            "Gemini streaming",
            self.client.post(&url).json(&body).send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini streaming returned {}: {}",
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
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &self.egress_broker,
        )?;
        let url = format!(
            "{}/{}:generateContent?key={}",
            self.base_url,
            self.api_model_path(),
            api_key
        );
        let function_declarations = self.translated_function_declarations(tools);
        let mut body =
            self.build_tool_follow_up_body(prompt, &function_declarations, executed_tools);
        super::apply_gemini_tool_policy(&mut body, &function_declarations, policy);
        let resp =
            send_with_response_start_timeout("Gemini", self.client.post(&url).json(&body).send())
                .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini returned {}: {}",
                status, text
            )));
        }
        let res_json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| OrchestratorError::LLMError(format!("Gemini JSON parse failed: {}", e)))?;
        if let Some(parts) = res_json["candidates"][0]["content"]["parts"].as_array() {
            let mut tool_calls = Vec::new();
            let mut text_parts = Vec::new();
            for part in parts {
                if let Some(function_call) = part.get("functionCall") {
                    if let Some(name) = function_call["name"].as_str() {
                        tool_calls.push(ToolCall {
                            invocation_id: Self::encode_function_call_part_metadata(part),
                            name: name.to_string(),
                            arguments: function_call["args"].to_string(),
                        });
                    }
                } else if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            if !tool_calls.is_empty() {
                return Ok(LLMResponse::ToolCalls(tool_calls));
            }
            if !text_parts.is_empty() {
                return Ok(LLMResponse::TextAnswer(text_parts.join("\n")));
            }
        }
        Err(OrchestratorError::LLMError(
            "Gemini returned no content".into(),
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
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &self.egress_broker,
        )?;
        let url = format!(
            "{}/{}:streamGenerateContent?key={}",
            self.base_url,
            self.api_model_path(),
            api_key
        );
        let function_declarations = self.translated_function_declarations(tools);
        let mut body =
            self.build_tool_follow_up_body(prompt, &function_declarations, executed_tools);
        super::apply_gemini_tool_policy(&mut body, &function_declarations, policy);
        let resp = send_with_response_start_timeout(
            "Gemini streaming",
            self.client.post(&url).json(&body).send(),
        )
        .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini streaming returned {}: {}",
                status, text
            )));
        }
        let adapter = adapter_for_family(self.capability_profile.adapter_family);
        collect_sse_like_stream(resp, adapter).await
    }

    fn inspect_context_payload(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        let function_declarations = self.translated_function_declarations(tools);
        Some(build_gemini_context_body(
            context,
            &function_declarations,
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
    use aria_core::{
        CapabilitySourceKind, ToolChoicePolicy, ToolResultMode, ToolRuntimePolicy, ToolSchemaMode,
    };

    fn backend() -> GeminiBackend {
        GeminiBackend::with_capability_profile(
            SecretRef::Literal(String::from("test-key")),
            ModelCapabilityProfile {
                model_ref: ModelRef::new("gemini", "gemini-2.5-pro"),
                adapter_family: AdapterFamily::GoogleGemini,
                tool_calling: CapabilitySupport::Supported,
                parallel_tool_calling: CapabilitySupport::Unknown,
                streaming: CapabilitySupport::Supported,
                vision: CapabilitySupport::Supported,
                json_mode: CapabilitySupport::Supported,
                max_context_tokens: Some(1_000_000),
                tool_schema_mode: ToolSchemaMode::StrictJsonSchema,
                tool_result_mode: ToolResultMode::NativeStructured,
                supports_images: CapabilitySupport::Supported,
                supports_audio: CapabilitySupport::Unsupported,
                source: CapabilitySourceKind::LocalOverride,
                source_detail: Some(String::from("test")),
                observed_at_us: 1,
                expires_at_us: None,
            },
            "https://generativelanguage.googleapis.com/v1beta",
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
                invocation_id: None,
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
    fn gemini_follow_up_body_uses_function_call_and_response_parts() {
        let backend = backend();
        let function_declarations = backend.translated_function_declarations(&[tool()]);
        let body = backend.build_tool_follow_up_body(
            "save this",
            &function_declarations,
            &[executed_tool()],
        );

        assert_eq!(body["contents"][0]["role"], json!("user"));
        assert_eq!(body["contents"][1]["role"], json!("model"));
        assert_eq!(
            body["contents"][1]["parts"][0]["functionCall"]["name"],
            json!("write_file")
        );
        assert_eq!(body["contents"][2]["role"], json!("user"));
        assert_eq!(
            body["contents"][2]["parts"][0]["functionResponse"]["name"],
            json!("write_file")
        );
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["mode"],
            json!("AUTO")
        );
        assert_eq!(
            body["tools"][0]["functionDeclarations"][0]["name"],
            json!("write_file")
        );
    }

    #[test]
    fn gemini_follow_up_body_preserves_provider_function_call_part_metadata() {
        let backend = backend();
        let function_declarations = backend.translated_function_declarations(&[tool()]);
        let executed = ExecutedToolCall {
            call: ToolCall {
                invocation_id: GeminiBackend::encode_function_call_part_metadata(&json!({
                    "functionCall": {
                        "name": "write_file",
                        "args": { "path": "stale.txt" }
                    },
                    "thought_signature": "sig_123"
                })),
                name: String::from("write_file"),
                arguments: String::from(r#"{"path":"notes.txt","content":"hello"}"#),
            },
            result: ToolExecutionResult::structured(
                "write succeeded",
                "write_file",
                json!({"ok": true, "bytes": 5}),
            ),
        };
        let body =
            backend.build_tool_follow_up_body("save this", &function_declarations, &[executed]);

        assert_eq!(
            body["contents"][1]["parts"][0]["thought_signature"],
            json!("sig_123")
        );
        assert_eq!(
            body["contents"][1]["parts"][0]["functionCall"]["name"],
            json!("write_file")
        );
        assert_eq!(
            body["contents"][1]["parts"][0]["functionCall"]["args"]["path"],
            json!("notes.txt")
        );
    }

    #[test]
    fn gemini_function_declarations_strip_additional_properties() {
        let backend = backend();
        let function_declarations = backend.translated_function_declarations(&[tool()]);
        let parameters = &function_declarations[0]["parameters"];

        assert!(parameters["additionalProperties"].is_null());
        assert_eq!(parameters["type"], json!("object"));
        assert_eq!(parameters["required"], json!(["content", "path"]));
    }

    #[test]
    fn gemini_tool_policy_can_force_single_function() {
        let backend = backend();
        let function_declarations = backend.translated_function_declarations(&[tool()]);
        let mut body = backend.build_tool_follow_up_body(
            "save this",
            &function_declarations,
            &[executed_tool()],
        );
        super::super::apply_gemini_tool_policy(
            &mut body,
            &function_declarations,
            &ToolRuntimePolicy {
                tool_choice: ToolChoicePolicy::Specific(String::from("write_file")),
                allow_parallel_tool_calls: true,
            },
        );

        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["mode"],
            json!("ANY")
        );
        assert_eq!(
            body["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            json!("write_file")
        );
    }

    #[test]
    fn gemini_inspect_context_payload_includes_contents_and_tools() {
        let backend = backend();
        let payload = backend
            .inspect_context_payload(&context_pack(), &[tool()], &ToolRuntimePolicy::default())
            .expect("payload");

        assert!(payload["contents"].is_array());
        assert_eq!(
            payload["tools"][0]["functionDeclarations"][0]["name"],
            json!("write_file")
        );
        assert_eq!(
            payload["systemInstruction"]["parts"][0]["text"],
            json!("system guidance")
        );
    }
}

pub struct GeminiProvider {
    pub api_key: SecretRef,
    pub base_url: String,
    pub egress_broker: Option<EgressCredentialBroker>,
}

#[derive(Debug, Deserialize)]
struct GeminiModel {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
    description: Option<String>,
    #[serde(rename = "inputTokenLimit")]
    input_token_limit: Option<u32>,
    #[serde(rename = "supportedGenerationMethods")]
    supported_generation_methods: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct GeminiModelsResponse {
    models: Vec<GeminiModel>,
}

#[async_trait]
impl ModelProvider for GeminiProvider {
    fn id(&self) -> &str {
        "gemini"
    }

    fn name(&self) -> &str {
        "Google Gemini"
    }

    fn adapter_family(&self) -> AdapterFamily {
        AdapterFamily::GoogleGemini
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
        let api_key = self.api_key.resolve_with_broker(
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &broker,
        )?;
        let url = format!(
            "{}/models?key={}",
            self.base_url.trim_end_matches('/'),
            api_key
        );
        let resp = reqwest::Client::new().get(&url).send().await.map_err(|e| {
            OrchestratorError::LLMError(format!("Gemini list models failed: {}", e))
        })?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(OrchestratorError::LLMError(format!(
                "Gemini models list returned {}: {}",
                status, text
            )));
        }
        let json: GeminiModelsResponse = resp.json().await.map_err(|e| {
            OrchestratorError::LLMError(format!("Gemini models JSON failed: {}", e))
        })?;
        Ok(json
            .models
            .into_iter()
            .map(|m| ModelMetadata {
                id: m.name.trim_start_matches("models/").to_string(),
                name: m.display_name.unwrap_or_else(|| m.name.clone()),
                description: m.description,
                context_length: m.input_token_limit.map(|v| v as usize),
            })
            .collect())
    }

    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        Ok(Box::new(
            GeminiBackend::new(self.api_key.clone(), model_id, self.base_url.clone())
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
            GeminiBackend::with_capability_profile(
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
        let broker = self
            .egress_broker
            .clone()
            .unwrap_or_else(EgressCredentialBroker::new);
        let api_key = self.api_key.resolve_with_broker(
            "generativelanguage.googleapis.com",
            "gemini_provider",
            &broker,
        )?;
        let model_path = if model_id.starts_with("models/") {
            model_id.to_string()
        } else {
            format!("models/{}", model_id)
        };
        let url = format!(
            "{}/{}?key={}",
            self.base_url.trim_end_matches('/'),
            model_path,
            api_key
        );
        let mut max_context_tokens = None;
        let mut probe_status = String::from("success");
        let mut probe_error = None;
        let mut raw_summary = format!("gemini probe for '{}'", model_id);
        let mut supports_images = CapabilitySupport::Unknown;
        let tool_calling = CapabilitySupport::Supported;
        let response = reqwest::Client::new().get(&url).send().await;
        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Ok(model) = resp.json::<GeminiModel>().await {
                    max_context_tokens = model.input_token_limit;
                    let descriptor = format!(
                        "{} {}",
                        model.name.to_ascii_lowercase(),
                        model.description.unwrap_or_default().to_ascii_lowercase()
                    );
                    if descriptor.contains("vision") || descriptor.contains("image") {
                        supports_images = CapabilitySupport::Supported;
                    }
                    raw_summary = format!(
                        "gemini probe for '{}' with methods {:?}",
                        model_id,
                        model.supported_generation_methods.unwrap_or_default()
                    );
                }
            } else {
                probe_status = String::from("degraded");
                probe_error = Some(format!("http {}", resp.status()));
            }
        } else if let Err(err) = response {
            probe_status = String::from("degraded");
            probe_error = Some(err.to_string());
        }
        Ok(ModelCapabilityProbeRecord {
            probe_id: format!("probe-gemini-{}-{}", model_id, observed_at_us),
            model_ref: ModelRef::new("gemini", model_id),
            adapter_family: AdapterFamily::GoogleGemini,
            tool_calling,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Supported,
            vision: supports_images,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens,
            supports_images,
            supports_audio: CapabilitySupport::Unknown,
            schema_acceptance: Some(CapabilitySupport::Supported),
            native_tool_probe: Some(tool_calling),
            modality_probe: Some(supports_images),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("models_api")),
            probe_status: Some(probe_status),
            probe_error,
            raw_summary: Some(raw_summary),
            observed_at_us,
            expires_at_us: Some(observed_at_us + 86_400_000_000),
        })
    }
}
