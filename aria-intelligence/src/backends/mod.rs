use super::{
    append_tool_results_to_prompt, normalize_tool_schema, provider_transport_config,
    reduce_tool_schema_for_compat, tool_is_compatible_with_model, CachedTool, ExecutedToolCall,
    LLMResponse, OrchestratorError, ProviderHealthIdentity,
};
use aria_core::{
    AdapterFamily, CapabilitySourceKind, CapabilitySupport, ContextBlockKind, ExecutionContextPack,
    ModelCapabilityProbeRecord, ModelCapabilityProfile, ModelRef, ProviderCapabilityProfile,
    ToolCallingMode, ToolChoicePolicy, ToolResultMode, ToolRuntimePolicy, ToolSchemaMode,
};
use async_trait::async_trait;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EgressSecretOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressSecretAuditRecord {
    pub scope: String,
    pub key_name: String,
    pub target_domain: String,
    pub outcome: EgressSecretOutcome,
    pub detail: String,
}

#[derive(Clone, Default)]
pub struct EgressCredentialBroker {
    audit_sink: Option<Arc<dyn Fn(EgressSecretAuditRecord) + Send + Sync>>,
}

impl std::fmt::Debug for EgressCredentialBroker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EgressCredentialBroker")
            .field("has_audit_sink", &self.audit_sink.is_some())
            .finish()
    }
}

impl EgressCredentialBroker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_audit_sink<F>(mut self, sink: F) -> Self
    where
        F: Fn(EgressSecretAuditRecord) + Send + Sync + 'static,
    {
        self.audit_sink = Some(Arc::new(sink));
        self
    }

    pub fn resolve_secret_ref(
        &self,
        secret_ref: &SecretRef,
        scope: &str,
        domain: &str,
    ) -> Result<String, OrchestratorError> {
        match secret_ref {
            SecretRef::Literal(value) => {
                self.emit(EgressSecretAuditRecord {
                    scope: scope.to_string(),
                    key_name: "literal".into(),
                    target_domain: domain.to_string(),
                    outcome: EgressSecretOutcome::Allowed,
                    detail: "literal secret resolved for egress".into(),
                });
                Ok(value.clone())
            }
            SecretRef::Vault { key_name, vault } => {
                self.resolve_vault_secret(vault, "system", key_name, domain, scope)
            }
        }
    }

    pub fn resolve_vault_secret(
        &self,
        vault: &aria_vault::CredentialVault,
        agent_id: &str,
        key_name: &str,
        domain: &str,
        scope: &str,
    ) -> Result<String, OrchestratorError> {
        match vault.retrieve_for_egress(agent_id, key_name, domain) {
            Ok(secret) => {
                self.emit(EgressSecretAuditRecord {
                    scope: scope.to_string(),
                    key_name: key_name.to_string(),
                    target_domain: domain.to_string(),
                    outcome: EgressSecretOutcome::Allowed,
                    detail: "vault secret resolved for egress".into(),
                });
                Ok(secret)
            }
            Err(err) => {
                self.emit(EgressSecretAuditRecord {
                    scope: scope.to_string(),
                    key_name: key_name.to_string(),
                    target_domain: domain.to_string(),
                    outcome: EgressSecretOutcome::Denied,
                    detail: format!("{}", err),
                });
                Err(OrchestratorError::LLMError(format!(
                    "Vault resolution failed: {}",
                    err
                )))
            }
        }
    }

    fn emit(&self, record: EgressSecretAuditRecord) {
        if let Some(sink) = &self.audit_sink {
            sink(record);
        }
    }
}

/// Reference to a secret that can be a literal string or a vault lookup.
#[derive(Debug, Clone)]
pub enum SecretRef {
    Literal(String),
    Vault {
        key_name: String,
        vault: aria_vault::CredentialVault,
    },
}

impl SecretRef {
    pub fn resolve(&self, domain: &str) -> Result<String, OrchestratorError> {
        EgressCredentialBroker::new().resolve_secret_ref(self, "llm_provider", domain)
    }

    pub fn resolve_with_broker(
        &self,
        domain: &str,
        scope: &str,
        broker: &EgressCredentialBroker,
    ) -> Result<String, OrchestratorError> {
        broker.resolve_secret_ref(self, scope, domain)
    }
}

#[async_trait]
pub trait LLMBackend: Send + Sync + dyn_clone::DynClone {
    /// Query the LLM with a prompt and available tools.
    async fn query(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError>;

    async fn query_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        _policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query(prompt, tools).await
    }

    async fn query_context_with_policy(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let fallback_prompt = crate::PromptManager::render_execution_context_pack(context);
        self.query_with_policy(&fallback_prompt, tools, policy)
            .await
    }

    fn inspect_context_payload(
        &self,
        _context: &ExecutionContextPack,
        _tools: &[CachedTool],
        _policy: &ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        None
    }

    async fn query_with_tool_results(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
    ) -> Result<LLMResponse, OrchestratorError> {
        let fallback_prompt = append_tool_results_to_prompt(prompt, executed_tools);
        self.query(&fallback_prompt, tools).await
    }

    async fn query_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        _policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_tool_results(prompt, tools, executed_tools)
            .await
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_policy(prompt, tools, policy).await
    }

    async fn query_stream_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        self.query_with_tool_results_and_policy(prompt, tools, executed_tools, policy)
            .await
    }

    async fn query_context_with_tool_results_and_policy(
        &self,
        context: &ExecutionContextPack,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let follow_up_pack = crate::append_tool_results_to_context_pack(context, executed_tools);
        self.query_context_with_policy(&follow_up_pack, tools, policy)
            .await
    }

    fn model_ref(&self) -> Option<ModelRef> {
        None
    }

    fn capability_profile(&self) -> Option<ModelCapabilityProfile> {
        None
    }

    fn provider_health_identity(&self) -> ProviderHealthIdentity {
        let profile = self.capability_profile();
        let provider_family = profile
            .as_ref()
            .map(|profile| profile.model_ref.provider_id.clone())
            .or_else(|| self.model_ref().map(|model_ref| model_ref.provider_id))
            .unwrap_or_else(|| "unknown".to_string());
        ProviderHealthIdentity {
            upstream_identity: provider_family.clone(),
            provider_family,
        }
    }
}

dyn_clone::clone_trait_object!(LLMBackend);

pub(crate) fn backend_overloaded_error(
    provider: &str,
    detail: impl Into<String>,
) -> OrchestratorError {
    OrchestratorError::BackendOverloaded(format!("{}: {}", provider, detail.into()))
}

pub(crate) fn map_send_error(provider: &str, error: reqwest::Error) -> OrchestratorError {
    if error.is_timeout() || error.is_connect() || error.is_request() || error.is_body() {
        backend_overloaded_error(provider, error.to_string())
    } else {
        OrchestratorError::LLMError(format!("{} request failed: {}", provider, error))
    }
}

pub(crate) async fn send_with_response_start_timeout<F>(
    provider: &str,
    send_op: F,
) -> Result<reqwest::Response, OrchestratorError>
where
    F: std::future::Future<Output = Result<reqwest::Response, reqwest::Error>>,
{
    let timeout = provider_transport_config().response_start_timeout;
    match tokio::time::timeout(timeout, send_op).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(map_send_error(provider, error)),
        Err(_) => Err(backend_overloaded_error(
            provider,
            format!(
                "timed out waiting for first response after {} ms",
                timeout.as_millis()
            ),
        )),
    }
}

/// Metadata for a model available from a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMetadata {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub context_length: Option<usize>,
}

/// Trait for listing and creating LLM backends.
#[async_trait]
pub trait ModelProvider: Send + Sync {
    /// Unique identifier for the provider (e.g., "ollama", "openrouter").
    fn id(&self) -> &str;

    /// Human-readable name for the provider.
    fn name(&self) -> &str;

    fn adapter_family(&self) -> AdapterFamily;

    /// List models available from this provider.
    async fn list_models(&self) -> Result<Vec<ModelMetadata>, OrchestratorError>;

    /// Create a backend instance for a specific model.
    fn create_backend(&self, model_id: &str) -> Result<Box<dyn LLMBackend>, OrchestratorError>;

    fn create_backend_with_profile(
        &self,
        profile: &ModelCapabilityProfile,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        self.create_backend(&profile.model_ref.model_id)
    }

    fn provider_capability_profile(&self, observed_at_us: u64) -> ProviderCapabilityProfile {
        ProviderCapabilityProfile {
            provider_id: self.id().to_string(),
            adapter_family: self.adapter_family(),
            supports_model_listing: CapabilitySupport::Supported,
            supports_runtime_probe: CapabilitySupport::Unknown,
            source: CapabilitySourceKind::ProviderCatalog,
            observed_at_us,
        }
    }

    fn default_model_capability_profile(
        &self,
        model_id: &str,
        observed_at_us: u64,
    ) -> ModelCapabilityProfile {
        default_model_capability_profile(self.id(), model_id, self.adapter_family(), observed_at_us)
    }

    async fn probe_model_capabilities(
        &self,
        model_id: &str,
        observed_at_us: u64,
    ) -> Result<ModelCapabilityProbeRecord, OrchestratorError> {
        Ok(ModelCapabilityProbeRecord {
            probe_id: format!("probe-{}-{}", self.id(), model_id),
            model_ref: ModelRef::new(self.id(), model_id),
            adapter_family: self.adapter_family(),
            tool_calling: CapabilitySupport::Unknown,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Unknown,
            vision: CapabilitySupport::Unknown,
            json_mode: CapabilitySupport::Unknown,
            max_context_tokens: None,
            supports_images: CapabilitySupport::Unknown,
            supports_audio: CapabilitySupport::Unknown,
            schema_acceptance: Some(CapabilitySupport::Unknown),
            native_tool_probe: Some(CapabilitySupport::Unknown),
            modality_probe: Some(CapabilitySupport::Unknown),
            source: CapabilitySourceKind::RuntimeProbe,
            probe_method: Some(String::from("unimplemented")),
            probe_status: Some(String::from("unknown")),
            probe_error: None,
            raw_summary: Some(String::from("provider probe not implemented")),
            observed_at_us,
            expires_at_us: None,
        })
    }
}

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai;
pub mod openrouter;

pub(crate) fn render_context_blocks_for_message(context: &ExecutionContextPack) -> Option<String> {
    let mut rendered = String::new();
    for block in &context.context_blocks {
        let label = match block.kind {
            ContextBlockKind::Retrieval => "Retrieved Context",
            ContextBlockKind::ControlDocument => "Control Documents",
            ContextBlockKind::DurableConstraint => "Durable Constraints",
            ContextBlockKind::SubAgentResult => "Sub-Agent Results",
            ContextBlockKind::ToolInstructions => "Tool Instructions",
            ContextBlockKind::PromptAsset => "Prompt Assets",
            ContextBlockKind::ResourceContext => "Resource Context",
            ContextBlockKind::CapabilityIndex => "Capability Index",
            ContextBlockKind::DocumentIndex => "Document Index",
            ContextBlockKind::ContractRequirements => "Execution Contract",
        };
        if !rendered.is_empty() {
            rendered.push_str("\n\n");
        }
        rendered.push_str(&format!("{} [{}]:\n{}", label, block.label, block.content));
    }
    if rendered.is_empty() {
        None
    } else {
        Some(rendered)
    }
}

pub(crate) fn build_openai_compatible_initial_messages(
    context: &ExecutionContextPack,
) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    if !context.system_prompt.trim().is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
            "content": context.system_prompt
        }));
    }
    for message in &context.history_messages {
        let role = match message.role.trim().to_ascii_lowercase().as_str() {
            "assistant" | "system" | "tool" => message.role.to_ascii_lowercase(),
            _ => String::from("user"),
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": message.content
        }));
    }
    if let Some(extra_context) = render_context_blocks_for_message(context) {
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("Context for this request:\n{}", extra_context)
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": context.user_request
    }));
    messages
}

pub(crate) fn build_openai_compatible_context_body(
    model: &str,
    context: &ExecutionContextPack,
    tool_defs: Vec<serde_json::Value>,
    policy: &ToolRuntimePolicy,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": build_openai_compatible_initial_messages(context),
        "stream": false
    });
    if !tool_defs.is_empty() {
        apply_openai_compatible_tool_policy(&mut body, &tool_defs, policy);
    }
    body
}

pub(crate) fn build_anthropic_context_body(
    model: &str,
    context: &ExecutionContextPack,
    tool_defs: Vec<serde_json::Value>,
    policy: &ToolRuntimePolicy,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": build_anthropic_initial_messages(context),
    });
    if !context.system_prompt.trim().is_empty() {
        body["system"] = serde_json::json!(context.system_prompt);
    }
    if !tool_defs.is_empty() {
        apply_anthropic_tool_policy(&mut body, &tool_defs, policy);
    }
    body
}

pub(crate) fn build_gemini_context_body(
    context: &ExecutionContextPack,
    function_declarations: &[serde_json::Value],
    policy: &ToolRuntimePolicy,
) -> serde_json::Value {
    let (system_instruction, contents) = build_gemini_initial_contents(context);
    let mut body = serde_json::json!({ "contents": contents });
    if let Some(system) = system_instruction {
        body["systemInstruction"] = system;
    }
    if !function_declarations.is_empty() {
        apply_gemini_tool_policy(&mut body, function_declarations, policy);
    }
    body
}

pub(crate) fn build_anthropic_initial_messages(
    context: &ExecutionContextPack,
) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    for message in &context.history_messages {
        let role = match message.role.trim().to_ascii_lowercase().as_str() {
            "assistant" => "assistant",
            _ => "user",
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": message.content
        }));
    }
    if let Some(extra_context) = render_context_blocks_for_message(context) {
        messages.push(serde_json::json!({
            "role": "user",
            "content": format!("Context for this request:\n{}", extra_context)
        }));
    }
    messages.push(serde_json::json!({
        "role": "user",
        "content": context.user_request
    }));
    messages
}

#[allow(dead_code)]
pub(crate) fn build_gemini_initial_contents(
    context: &ExecutionContextPack,
) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let system = if context.system_prompt.trim().is_empty() {
        None
    } else {
        Some(serde_json::json!({
            "parts": [{ "text": context.system_prompt }]
        }))
    };
    let mut contents = Vec::new();
    for message in &context.history_messages {
        let role = match message.role.trim().to_ascii_lowercase().as_str() {
            "assistant" => "model",
            _ => "user",
        };
        contents.push(serde_json::json!({
            "role": role,
            "parts": [{ "text": message.content }]
        }));
    }
    if let Some(extra_context) = render_context_blocks_for_message(context) {
        contents.push(serde_json::json!({
            "role": "user",
            "parts": [{ "text": format!("Context for this request:\n{}", extra_context) }]
        }));
    }
    contents.push(serde_json::json!({
        "role": "user",
        "parts": [{ "text": context.user_request }]
    }));
    (system, contents)
}

use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub provider_id: String,
    pub name: String,
    pub adapter_family: AdapterFamily,
    pub supports_model_listing: CapabilitySupport,
    pub supports_runtime_probe: CapabilitySupport,
}

/// Centralized registry of model providers.
pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn ModelProvider>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn ModelProvider>) {
        self.providers.insert(provider.id().to_string(), provider);
    }

    pub fn get_provider(&self, id: &str) -> Option<Arc<dyn ModelProvider>> {
        self.providers.get(id).cloned()
    }

    pub fn providers(&self) -> Vec<Arc<dyn ModelProvider>> {
        self.providers.values().cloned().collect()
    }

    pub fn provider_descriptors(&self, observed_at_us: u64) -> Vec<ProviderDescriptor> {
        let mut descriptors = self
            .providers
            .values()
            .map(|provider| {
                let profile = provider.provider_capability_profile(observed_at_us);
                ProviderDescriptor {
                    provider_id: provider.id().to_string(),
                    name: provider.name().to_string(),
                    adapter_family: provider.adapter_family(),
                    supports_model_listing: profile.supports_model_listing,
                    supports_runtime_probe: profile.supports_runtime_probe,
                }
            })
            .collect::<Vec<_>>();
        descriptors.sort_by(|a, b| a.provider_id.cmp(&b.provider_id));
        descriptors
    }

    pub async fn list_all_models(&self) -> HashMap<String, Vec<ModelMetadata>> {
        let mut all = HashMap::new();
        for (id, provider) in &self.providers {
            if let Ok(models) = provider.list_models().await {
                all.insert(id.clone(), models);
            }
        }
        all
    }

    pub async fn probe_provider_models(
        &self,
        provider_id: &str,
        model_ids: &[String],
        observed_at_us: u64,
    ) -> Result<Vec<ModelCapabilityProbeRecord>, OrchestratorError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            OrchestratorError::LLMError(format!("Provider {} not found", provider_id))
        })?;
        let mut probes = Vec::with_capacity(model_ids.len());
        for model_id in model_ids {
            probes.push(
                provider
                    .probe_model_capabilities(model_id, observed_at_us)
                    .await?,
            );
        }
        Ok(probes)
    }

    pub fn create_backend(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        let provider = self.providers.get(provider_id).ok_or_else(|| {
            OrchestratorError::LLMError(format!("Provider {} not found", provider_id))
        })?;
        provider.create_backend(model_id)
    }

    pub fn create_backend_with_profile(
        &self,
        profile: &ModelCapabilityProfile,
    ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
        let provider = self
            .providers
            .get(&profile.model_ref.provider_id)
            .ok_or_else(|| {
                OrchestratorError::LLMError(format!(
                    "Provider {} not found",
                    profile.model_ref.provider_id
                ))
            })?;
        provider.create_backend_with_profile(profile)
    }
}

pub trait ProviderAdapter: Send + Sync {
    fn family(&self) -> AdapterFamily;

    fn parse_stream_event(&self, chunk: &str) -> Result<Option<ProviderStreamEvent>, String> {
        let _ = chunk;
        Ok(None)
    }

    fn translate_tool_schema(
        &self,
        profile: &ModelCapabilityProfile,
        tool: &CachedTool,
    ) -> Result<serde_json::Value, String> {
        if !tool_is_compatible_with_model(tool, Some(profile)) {
            return Err(format!(
                "tool '{}' is not compatible with model '{}'",
                tool.name,
                profile.model_ref.as_slash_ref()
            ));
        }
        let schema = match profile.tool_schema_mode {
            ToolSchemaMode::StrictJsonSchema => normalize_tool_schema(&tool.parameters_schema)?,
            ToolSchemaMode::ReducedJsonSchema => {
                if tool.requires_strict_schema {
                    return Err(format!(
                        "tool '{}' requires strict schema support",
                        tool.name
                    ));
                }
                reduce_tool_schema_for_compat(&tool.parameters_schema)?
            }
            ToolSchemaMode::Unsupported => {
                return Err(format!(
                    "model '{}' does not support tool schemas",
                    profile.model_ref.as_slash_ref()
                ))
            }
        };
        serde_json::from_str(&schema)
            .map_err(|e| format!("translated tool schema parse failed: {}", e))
    }

    fn translate_tool_definition(
        &self,
        profile: &ModelCapabilityProfile,
        tool: &CachedTool,
    ) -> Result<serde_json::Value, String> {
        let canonical = tool.canonical_spec();
        let parameters = self.translate_tool_schema(profile, tool)?;
        Ok(serde_json::json!({
            "type": "function",
            "function": {
                "name": canonical.name,
                "description": canonical.description_long,
                "parameters": parameters
            }
        }))
    }

    fn tool_calling_mode(&self, profile: &ModelCapabilityProfile) -> ToolCallingMode {
        match profile.tool_calling {
            CapabilitySupport::Supported => ToolCallingMode::NativeTools,
            CapabilitySupport::Degraded => ToolCallingMode::CompatTools,
            CapabilitySupport::Unknown | CapabilitySupport::Unsupported => {
                ToolCallingMode::TextFallbackNoTools
            }
        }
    }

    fn filter_tools(
        &self,
        profile: &ModelCapabilityProfile,
        tools: &[CachedTool],
    ) -> Vec<CachedTool> {
        match self.tool_calling_mode(profile) {
            ToolCallingMode::NativeTools
            | ToolCallingMode::CompatTools
            | ToolCallingMode::TextFallbackWithRepair => tools
                .iter()
                .filter(|tool| tool_is_compatible_with_model(tool, Some(profile)))
                .filter(|tool| self.translate_tool_schema(profile, tool).is_ok())
                .cloned()
                .collect(),
            ToolCallingMode::TextFallbackNoTools => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderStreamEvent {
    TextDelta(String),
    ToolCallDelta {
        invocation_id: Option<String>,
        name: Option<String>,
        arguments_delta: String,
    },
    Done,
}

#[derive(Debug, Default)]
pub struct ProviderStreamAccumulator {
    text: String,
    tool_calls: Vec<crate::ToolCall>,
}

impl ProviderStreamAccumulator {
    pub fn push(&mut self, event: ProviderStreamEvent) {
        match event {
            ProviderStreamEvent::TextDelta(delta) => self.text.push_str(&delta),
            ProviderStreamEvent::ToolCallDelta {
                invocation_id,
                name,
                arguments_delta,
            } => {
                let target_index = invocation_id.as_ref().and_then(|id| {
                    self.tool_calls
                        .iter()
                        .position(|call| call.invocation_id.as_ref() == Some(id))
                });
                if let Some(index) = target_index {
                    if let Some(name) = name {
                        self.tool_calls[index].name = name;
                    }
                    self.tool_calls[index].arguments.push_str(&arguments_delta);
                } else {
                    self.tool_calls.push(crate::ToolCall {
                        invocation_id,
                        name: name.unwrap_or_default(),
                        arguments: arguments_delta,
                    });
                }
            }
            ProviderStreamEvent::Done => {}
        }
    }

    pub fn into_response(self) -> Result<LLMResponse, OrchestratorError> {
        if !self.tool_calls.is_empty() {
            return Ok(LLMResponse::ToolCalls(self.tool_calls));
        }
        Ok(LLMResponse::TextAnswer(self.text.trim().to_string()))
    }
}

pub async fn collect_sse_like_stream(
    response: reqwest::Response,
    adapter: &dyn ProviderAdapter,
) -> Result<LLMResponse, OrchestratorError> {
    let mut accumulator = ProviderStreamAccumulator::default();
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let first_chunk_timeout = provider_transport_config().response_start_timeout;
    let mut first_chunk_pending = true;
    loop {
        let next_chunk = if first_chunk_pending {
            match tokio::time::timeout(first_chunk_timeout, stream.next()).await {
                Ok(next) => next,
                Err(_) => {
                    return Err(backend_overloaded_error(
                        "stream",
                        format!(
                            "timed out waiting for first stream event after {} ms",
                            first_chunk_timeout.as_millis()
                        ),
                    ));
                }
            }
        } else {
            stream.next().await
        };
        let Some(chunk) = next_chunk else {
            break;
        };
        first_chunk_pending = false;
        let chunk =
            chunk.map_err(|e| OrchestratorError::LLMError(format!("stream read failed: {}", e)))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if let Some(event) = adapter
                .parse_stream_event(&line)
                .map_err(OrchestratorError::LLMError)?
            {
                accumulator.push(event);
            }
        }
    }
    let tail = buffer.trim();
    if !tail.is_empty() {
        if let Some(event) = adapter
            .parse_stream_event(tail)
            .map_err(OrchestratorError::LLMError)?
        {
            accumulator.push(event);
        }
    }
    accumulator.into_response()
}

pub fn adapter_for_family(family: AdapterFamily) -> &'static dyn ProviderAdapter {
    match family {
        AdapterFamily::OpenAiCompatible => &OpenAiCompatibleAdapter,
        AdapterFamily::Anthropic => &AnthropicAdapter,
        AdapterFamily::GoogleGemini => &GoogleGeminiAdapter,
        AdapterFamily::OllamaNative => &OllamaNativeAdapter,
        AdapterFamily::TextOnlyCli => &TextOnlyCliAdapter,
    }
}

pub(crate) fn build_openai_compatible_followup_body(
    model: &str,
    prompt: &str,
    tool_defs: Vec<serde_json::Value>,
    executed_tools: &[ExecutedToolCall],
) -> serde_json::Value {
    let assistant_tool_calls = executed_tools
        .iter()
        .map(|entry| {
            let invocation = entry.call.invocation_envelope();
            serde_json::json!({
                "id": invocation.invocation_id.clone().unwrap_or_else(|| format!("call_{}", invocation.tool_name)),
                "type": "function",
                "function": {
                    "name": invocation.tool_name,
                    "arguments": invocation.arguments_json
                }
            })
        })
        .collect::<Vec<_>>();
    let tool_messages = executed_tools
        .iter()
        .map(|entry| {
            let invocation = entry.call.invocation_envelope();
            let result = entry.result_envelope();
            serde_json::json!({
                "role": "tool",
                "tool_call_id": invocation.invocation_id.clone().unwrap_or_else(|| format!("call_{}", invocation.tool_name)),
                "content": result.as_provider_payload().to_string()
            })
        })
        .collect::<Vec<_>>();
    let mut messages = vec![serde_json::json!({"role":"user","content":prompt})];
    messages.push(
        serde_json::json!({"role":"assistant","content":null,"tool_calls":assistant_tool_calls}),
    );
    messages.extend(tool_messages);
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false
    });
    if !tool_defs.is_empty() {
        body["tools"] = serde_json::Value::Array(tool_defs);
        body["tool_choice"] = serde_json::json!("auto");
    }
    body
}

pub(crate) fn apply_openai_compatible_tool_policy(
    body: &mut serde_json::Value,
    tool_defs: &[serde_json::Value],
    policy: &ToolRuntimePolicy,
) {
    if tool_defs.is_empty() || matches!(policy.tool_choice, ToolChoicePolicy::None) {
        body.as_object_mut().map(|value| {
            value.remove("tools");
            value.remove("tool_choice");
            value.remove("parallel_tool_calls");
        });
        return;
    }
    body["tools"] = serde_json::Value::Array(tool_defs.to_vec());
    body["parallel_tool_calls"] = serde_json::Value::Bool(policy.allow_parallel_tool_calls);
    body["tool_choice"] = match &policy.tool_choice {
        ToolChoicePolicy::Auto => serde_json::json!("auto"),
        ToolChoicePolicy::Required => serde_json::json!("required"),
        ToolChoicePolicy::Specific(name) => serde_json::json!({
            "type": "function",
            "function": { "name": name }
        }),
        ToolChoicePolicy::None => serde_json::json!("none"),
    };
}

pub(crate) fn parse_openai_compatible_tool_calls(
    message: &serde_json::Value,
) -> Vec<crate::ToolCall> {
    message
        .get("tool_calls")
        .and_then(|v| v.as_array())
        .map(|tool_calls| {
            tool_calls
                .iter()
                .filter_map(|call| {
                    let function = call.get("function")?;
                    let name = function.get("name")?.as_str()?.to_string();
                    let arguments = if function.get("arguments")?.is_string() {
                        function.get("arguments")?.as_str()?.to_string()
                    } else {
                        function.get("arguments")?.to_string()
                    };
                    Some(crate::ToolCall {
                        invocation_id: call
                            .get("id")
                            .and_then(|v| v.as_str())
                            .map(|v| v.to_string()),
                        name,
                        arguments,
                    })
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(crate) fn extract_openai_compatible_content(message: &serde_json::Value) -> Option<String> {
    if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
        return Some(content.to_string());
    }
    let parts = message.get("content")?.as_array()?;
    let joined = parts
        .iter()
        .filter_map(|part| {
            if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                part.get("text")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            } else {
                part.as_str().map(|s| s.to_string())
            }
        })
        .collect::<Vec<_>>()
        .join("");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

pub(crate) fn apply_anthropic_tool_policy(
    body: &mut serde_json::Value,
    tool_defs: &[serde_json::Value],
    policy: &ToolRuntimePolicy,
) {
    if tool_defs.is_empty() || matches!(policy.tool_choice, ToolChoicePolicy::None) {
        body.as_object_mut().map(|value| {
            value.remove("tools");
            value.remove("tool_choice");
        });
        return;
    }
    body["tools"] = serde_json::Value::Array(tool_defs.to_vec());
    body["tool_choice"] = match &policy.tool_choice {
        ToolChoicePolicy::Auto => serde_json::json!({"type":"auto"}),
        ToolChoicePolicy::Required => serde_json::json!({"type":"any"}),
        ToolChoicePolicy::Specific(name) => serde_json::json!({"type":"tool","name":name}),
        ToolChoicePolicy::None => serde_json::json!({"type":"auto"}),
    };
}

pub(crate) fn apply_gemini_tool_policy(
    body: &mut serde_json::Value,
    function_declarations: &[serde_json::Value],
    policy: &ToolRuntimePolicy,
) {
    if function_declarations.is_empty() || matches!(policy.tool_choice, ToolChoicePolicy::None) {
        body.as_object_mut().map(|value| {
            value.remove("tools");
            value.remove("toolConfig");
        });
        return;
    }
    body["tools"] = serde_json::json!([{ "functionDeclarations": function_declarations }]);
    body["toolConfig"] = match &policy.tool_choice {
        ToolChoicePolicy::Auto => serde_json::json!({
            "functionCallingConfig": { "mode": "AUTO" }
        }),
        ToolChoicePolicy::Required => serde_json::json!({
            "functionCallingConfig": { "mode": "ANY" }
        }),
        ToolChoicePolicy::Specific(name) => serde_json::json!({
            "functionCallingConfig": {
                "mode": "ANY",
                "allowedFunctionNames": [name]
            }
        }),
        ToolChoicePolicy::None => serde_json::json!({
            "functionCallingConfig": { "mode": "NONE" }
        }),
    };
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiCompatibleAdapter;

impl ProviderAdapter for OpenAiCompatibleAdapter {
    fn family(&self) -> AdapterFamily {
        AdapterFamily::OpenAiCompatible
    }

    fn parse_stream_event(&self, chunk: &str) -> Result<Option<ProviderStreamEvent>, String> {
        let payload = chunk.trim();
        if payload.is_empty() {
            return Ok(None);
        }
        let payload = payload
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(payload);
        if payload == "[DONE]" {
            return Ok(Some(ProviderStreamEvent::Done));
        }
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| format!("openai stream parse failed: {}", e))?;
        let delta = &value["choices"][0]["delta"];
        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
            return Ok(Some(ProviderStreamEvent::TextDelta(text.to_string())));
        }
        if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            if let Some(call) = tool_calls.first() {
                return Ok(Some(ProviderStreamEvent::ToolCallDelta {
                    invocation_id: call
                        .get("id")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string()),
                    name: call
                        .get("function")
                        .and_then(|v| v.get("name"))
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string()),
                    arguments_delta: call
                        .get("function")
                        .and_then(|v| v.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                }));
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn family(&self) -> AdapterFamily {
        AdapterFamily::Anthropic
    }

    fn parse_stream_event(&self, chunk: &str) -> Result<Option<ProviderStreamEvent>, String> {
        let payload = chunk.trim();
        if payload.is_empty() {
            return Ok(None);
        }
        let payload = payload
            .strip_prefix("data:")
            .map(str::trim)
            .unwrap_or(payload);
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| format!("anthropic stream parse failed: {}", e))?;
        match value["type"].as_str() {
            Some("content_block_delta") => {
                if let Some(text) = value["delta"]["text"].as_str() {
                    return Ok(Some(ProviderStreamEvent::TextDelta(text.to_string())));
                }
                if let Some(partial) = value["delta"]["partial_json"].as_str() {
                    return Ok(Some(ProviderStreamEvent::ToolCallDelta {
                        invocation_id: value["content_block"]["id"].as_str().map(|v| v.to_string()),
                        name: value["content_block"]["name"]
                            .as_str()
                            .map(|v| v.to_string()),
                        arguments_delta: partial.to_string(),
                    }));
                }
            }
            Some("message_stop") => return Ok(Some(ProviderStreamEvent::Done)),
            _ => {}
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct GoogleGeminiAdapter;

impl ProviderAdapter for GoogleGeminiAdapter {
    fn family(&self) -> AdapterFamily {
        AdapterFamily::GoogleGemini
    }

    fn translate_tool_schema(
        &self,
        profile: &ModelCapabilityProfile,
        tool: &CachedTool,
    ) -> Result<serde_json::Value, String> {
        if !tool_is_compatible_with_model(tool, Some(profile)) {
            return Err(format!(
                "tool '{}' is not compatible with model '{}'",
                tool.name,
                profile.model_ref.as_slash_ref()
            ));
        }
        let schema = reduce_tool_schema_for_compat(&tool.parameters_schema)?;
        serde_json::from_str(&schema)
            .map_err(|e| format!("translated gemini tool schema parse failed: {}", e))
    }

    fn parse_stream_event(&self, chunk: &str) -> Result<Option<ProviderStreamEvent>, String> {
        let payload = chunk.trim();
        if payload.is_empty() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| format!("gemini stream parse failed: {}", e))?;
        if let Some(parts) = value["candidates"][0]["content"]["parts"].as_array() {
            for part in parts {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    return Ok(Some(ProviderStreamEvent::TextDelta(text.to_string())));
                }
                if let Some(function_call) = part.get("functionCall") {
                    return Ok(Some(ProviderStreamEvent::ToolCallDelta {
                        invocation_id:
                            crate::backends::gemini::GeminiBackend::encode_function_call_part_metadata(
                                part,
                            ),
                        name: function_call["name"].as_str().map(|v| v.to_string()),
                        arguments_delta: function_call["args"].to_string(),
                    }));
                }
            }
        }
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OllamaNativeAdapter;

impl ProviderAdapter for OllamaNativeAdapter {
    fn family(&self) -> AdapterFamily {
        AdapterFamily::OllamaNative
    }

    fn parse_stream_event(&self, chunk: &str) -> Result<Option<ProviderStreamEvent>, String> {
        let payload = chunk.trim();
        if payload.is_empty() {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|e| format!("ollama stream parse failed: {}", e))?;
        if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
            return Ok(Some(ProviderStreamEvent::TextDelta(text.to_string())));
        }
        if let Some(tool_calls) = value
            .get("message")
            .and_then(|v| v.get("tool_calls"))
            .and_then(|v| v.as_array())
        {
            if let Some(call) = tool_calls.first() {
                return Ok(Some(ProviderStreamEvent::ToolCallDelta {
                    invocation_id: None,
                    name: call["function"]["name"].as_str().map(|v| v.to_string()),
                    arguments_delta: call["function"]["arguments"].to_string(),
                }));
            }
        }
        if value.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Ok(Some(ProviderStreamEvent::Done));
        }
        Ok(None)
    }

    fn translate_tool_definition(
        &self,
        profile: &ModelCapabilityProfile,
        tool: &CachedTool,
    ) -> Result<serde_json::Value, String> {
        let canonical = tool.canonical_spec();
        let parameters = self.translate_tool_schema(profile, tool)?;
        Ok(serde_json::json!({
            "type": "function",
            "function": {
                "name": canonical.name,
                "description": canonical.description_long,
                "parameters": parameters
            }
        }))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct TextOnlyCliAdapter;

impl ProviderAdapter for TextOnlyCliAdapter {
    fn family(&self) -> AdapterFamily {
        AdapterFamily::TextOnlyCli
    }

    fn tool_calling_mode(&self, _profile: &ModelCapabilityProfile) -> ToolCallingMode {
        ToolCallingMode::TextFallbackNoTools
    }
}

pub fn default_model_capability_profile(
    provider_id: &str,
    model_id: &str,
    adapter_family: AdapterFamily,
    observed_at_us: u64,
) -> ModelCapabilityProfile {
    let (
        tool_calling,
        parallel_tool_calling,
        streaming,
        json_mode,
        tool_schema_mode,
        tool_result_mode,
    ) = match adapter_family {
        AdapterFamily::OpenAiCompatible => (
            CapabilitySupport::Degraded,
            CapabilitySupport::Unknown,
            CapabilitySupport::Supported,
            CapabilitySupport::Supported,
            ToolSchemaMode::StrictJsonSchema,
            ToolResultMode::NativeStructured,
        ),
        AdapterFamily::OllamaNative => (
            CapabilitySupport::Degraded,
            CapabilitySupport::Unknown,
            CapabilitySupport::Supported,
            CapabilitySupport::Degraded,
            ToolSchemaMode::ReducedJsonSchema,
            ToolResultMode::NativeStructured,
        ),
        AdapterFamily::Anthropic => (
            CapabilitySupport::Unknown,
            CapabilitySupport::Unknown,
            CapabilitySupport::Supported,
            CapabilitySupport::Supported,
            ToolSchemaMode::StrictJsonSchema,
            ToolResultMode::NativeStructured,
        ),
        AdapterFamily::GoogleGemini => (
            CapabilitySupport::Unknown,
            CapabilitySupport::Unknown,
            CapabilitySupport::Supported,
            CapabilitySupport::Supported,
            ToolSchemaMode::ReducedJsonSchema,
            ToolResultMode::NativeStructured,
        ),
        AdapterFamily::TextOnlyCli => (
            CapabilitySupport::Unsupported,
            CapabilitySupport::Unsupported,
            CapabilitySupport::Supported,
            CapabilitySupport::Unsupported,
            ToolSchemaMode::Unsupported,
            ToolResultMode::TextBlock,
        ),
    };
    ModelCapabilityProfile {
        model_ref: ModelRef::new(provider_id, model_id),
        adapter_family,
        tool_calling,
        parallel_tool_calling,
        streaming,
        vision: CapabilitySupport::Unknown,
        json_mode,
        max_context_tokens: None,
        tool_schema_mode,
        tool_result_mode,
        supports_images: CapabilitySupport::Unknown,
        supports_audio: CapabilitySupport::Unknown,
        source: CapabilitySourceKind::ProviderCatalog,
        source_detail: Some(String::from("provider default")),
        observed_at_us,
        expires_at_us: None,
    }
}

pub fn resolve_capability_profile(
    local_override: Option<&ModelCapabilityProfile>,
    runtime_probe: Option<&ModelCapabilityProfile>,
    provider_default: &ModelCapabilityProfile,
    now_us: u64,
) -> ModelCapabilityProfile {
    if let Some(profile) = local_override {
        return profile.clone();
    }
    if let Some(profile) = runtime_probe {
        if profile.expires_at_us.map(|v| v >= now_us).unwrap_or(true) {
            return profile.clone();
        }
    }
    provider_default.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    fn make_tool(schema: &str) -> CachedTool {
        CachedTool {
            name: "search_web".into(),
            description: "Search the web".into(),
            parameters_schema: schema.into(),
            embedding: Vec::new(),
            requires_strict_schema: false,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        }
    }

    fn make_profile(
        adapter_family: AdapterFamily,
        schema_mode: ToolSchemaMode,
    ) -> ModelCapabilityProfile {
        ModelCapabilityProfile {
            model_ref: ModelRef::new("test", "model"),
            adapter_family,
            tool_calling: CapabilitySupport::Supported,
            parallel_tool_calling: CapabilitySupport::Unknown,
            streaming: CapabilitySupport::Supported,
            vision: CapabilitySupport::Unsupported,
            json_mode: CapabilitySupport::Supported,
            max_context_tokens: None,
            tool_schema_mode: schema_mode,
            tool_result_mode: ToolResultMode::NativeStructured,
            supports_images: CapabilitySupport::Unsupported,
            supports_audio: CapabilitySupport::Unsupported,
            source: CapabilitySourceKind::ProviderCatalog,
            source_detail: Some("test".into()),
            observed_at_us: 1,
            expires_at_us: None,
        }
    }

    #[test]
    fn openai_compatible_adapter_translates_strict_schema() {
        let adapter = OpenAiCompatibleAdapter;
        let profile = make_profile(
            AdapterFamily::OpenAiCompatible,
            ToolSchemaMode::StrictJsonSchema,
        );
        let translated = adapter
            .translate_tool_definition(
                &profile,
                &make_tool(r#"{"query":{"type":"string"},"limit":{"type":"integer"}}"#),
            )
            .expect("translate");
        assert_eq!(translated["function"]["parameters"]["type"], "object");
        assert_eq!(
            translated["function"]["parameters"]["required"],
            serde_json::json!(["limit", "query"])
        );
        assert_eq!(
            translated["function"]["parameters"]["additionalProperties"],
            serde_json::json!(false)
        );
    }

    #[test]
    fn ollama_adapter_reduces_schema_for_compatibility() {
        let adapter = OllamaNativeAdapter;
        let profile = make_profile(
            AdapterFamily::OllamaNative,
            ToolSchemaMode::ReducedJsonSchema,
        );
        let translated = adapter
            .translate_tool_definition(
                &profile,
                &make_tool(
                    r#"{"type":"object","properties":{"query":{"type":"string","description":"term"}},"required":["query"],"additionalProperties":false}"#,
                ),
            )
            .expect("translate");
        assert!(translated["function"]["parameters"]["additionalProperties"].is_null());
        assert!(
            translated["function"]["parameters"]["properties"]["query"]["description"].is_null()
        );
        assert_eq!(
            translated["function"]["parameters"]["required"],
            serde_json::json!(["query"])
        );
    }

    #[test]
    fn text_only_adapter_rejects_tool_translation_and_filters_everything() {
        let adapter = TextOnlyCliAdapter;
        let profile = make_profile(AdapterFamily::TextOnlyCli, ToolSchemaMode::Unsupported);
        let tool = make_tool(r#"{"query":{"type":"string"}}"#);
        assert!(adapter.translate_tool_definition(&profile, &tool).is_err());
        assert!(adapter.filter_tools(&profile, &[tool]).is_empty());
    }

    #[test]
    fn adapter_rejects_strict_only_tool_on_reduced_schema_model() {
        let adapter = OllamaNativeAdapter;
        let profile = make_profile(
            AdapterFamily::OllamaNative,
            ToolSchemaMode::ReducedJsonSchema,
        );
        let tool = CachedTool {
            name: "complex_tool".into(),
            description: "Needs strict schema".into(),
            parameters_schema:
                r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#
                    .into(),
            embedding: Vec::new(),
            requires_strict_schema: true,
            streaming_safe: false,
            parallel_safe: true,
            modalities: vec![aria_core::ToolModality::Text],
        };
        assert!(adapter.translate_tool_definition(&profile, &tool).is_err());
    }

    #[test]
    fn openai_stream_parser_emits_tool_delta_and_done() {
        let adapter = OpenAiCompatibleAdapter;
        let tool_delta = adapter
            .parse_stream_event(
                r#"data: {"choices":[{"delta":{"tool_calls":[{"id":"call_1","function":{"name":"read_file","arguments":"{\"path\":\"a"}}]}}]}"#,
            )
            .expect("parse")
            .expect("event");
        assert_eq!(
            tool_delta,
            ProviderStreamEvent::ToolCallDelta {
                invocation_id: Some("call_1".into()),
                name: Some("read_file".into()),
                arguments_delta: "{\"path\":\"a".into(),
            }
        );
        assert_eq!(
            adapter.parse_stream_event("data: [DONE]").expect("done"),
            Some(ProviderStreamEvent::Done)
        );
    }

    #[test]
    fn anthropic_stream_parser_emits_partial_json_delta() {
        let adapter = AnthropicAdapter;
        let event = adapter
            .parse_stream_event(
                r#"data: {"type":"content_block_delta","content_block":{"id":"toolu_1","name":"search_web"},"delta":{"partial_json":"{\"query\":\"rust"}} "#,
            )
            .expect("parse")
            .expect("event");
        assert_eq!(
            event,
            ProviderStreamEvent::ToolCallDelta {
                invocation_id: Some("toolu_1".into()),
                name: Some("search_web".into()),
                arguments_delta: "{\"query\":\"rust".into(),
            }
        );
    }

    #[test]
    fn gemini_stream_parser_emits_text_delta() {
        let adapter = GoogleGeminiAdapter;
        let event = adapter
            .parse_stream_event(r#"{"candidates":[{"content":{"parts":[{"text":"hello"}]}}]}"#)
            .expect("parse")
            .expect("event");
        assert_eq!(event, ProviderStreamEvent::TextDelta("hello".into()));
    }

    #[test]
    fn gemini_stream_parser_preserves_function_call_part_metadata() {
        let adapter = GoogleGeminiAdapter;
        let event = adapter
            .parse_stream_event(
                r#"{"candidates":[{"content":{"parts":[{"functionCall":{"name":"search_web","args":{"query":"rust"}},"thought_signature":"sig_123"}]}}]}"#,
            )
            .expect("parse")
            .expect("event");
        let ProviderStreamEvent::ToolCallDelta {
            invocation_id,
            name,
            arguments_delta,
        } = event
        else {
            panic!("expected tool call delta");
        };
        assert_eq!(name.as_deref(), Some("search_web"));
        assert_eq!(arguments_delta, r#"{"query":"rust"}"#);
        let metadata = crate::backends::gemini::GeminiBackend::decode_function_call_part_metadata(
            invocation_id.as_deref().expect("invocation id"),
        )
        .expect("metadata");
        assert_eq!(metadata["thought_signature"], "sig_123");
        assert_eq!(metadata["functionCall"]["name"], "search_web");
    }

    #[test]
    fn ollama_stream_parser_emits_done() {
        let adapter = OllamaNativeAdapter;
        let event = adapter
            .parse_stream_event(r#"{"response":"token","done":true}"#)
            .expect("parse")
            .expect("event");
        assert_eq!(event, ProviderStreamEvent::TextDelta("token".into()));
    }

    #[test]
    fn stream_accumulator_merges_tool_call_deltas() {
        let mut accumulator = ProviderStreamAccumulator::default();
        accumulator.push(ProviderStreamEvent::ToolCallDelta {
            invocation_id: Some("call_1".into()),
            name: Some("read_file".into()),
            arguments_delta: "{\"path\":\"/tmp".into(),
        });
        accumulator.push(ProviderStreamEvent::ToolCallDelta {
            invocation_id: Some("call_1".into()),
            name: None,
            arguments_delta: "/a.txt\"}".into(),
        });
        let response = accumulator.into_response().expect("response");
        assert_eq!(
            response,
            LLMResponse::ToolCalls(vec![crate::ToolCall {
                invocation_id: Some("call_1".into()),
                name: "read_file".into(),
                arguments: "{\"path\":\"/tmp/a.txt\"}".into(),
            }])
        );
    }

    #[derive(Clone)]
    struct MockProvider {
        id: &'static str,
        name: &'static str,
        family: AdapterFamily,
        probe_tool_calling: CapabilitySupport,
        probe_streaming: CapabilitySupport,
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> &str {
            self.name
        }

        fn adapter_family(&self) -> AdapterFamily {
            self.family
        }

        async fn list_models(&self) -> Result<Vec<ModelMetadata>, OrchestratorError> {
            Ok(vec![ModelMetadata {
                id: "model-a".into(),
                name: "Model A".into(),
                description: None,
                context_length: Some(8192),
            }])
        }

        fn create_backend(
            &self,
            _model_id: &str,
        ) -> Result<Box<dyn LLMBackend>, OrchestratorError> {
            Err(OrchestratorError::LLMError("unused".into()))
        }

        fn provider_capability_profile(&self, observed_at_us: u64) -> ProviderCapabilityProfile {
            ProviderCapabilityProfile {
                provider_id: self.id().to_string(),
                adapter_family: self.family,
                supports_model_listing: CapabilitySupport::Supported,
                supports_runtime_probe: CapabilitySupport::Supported,
                source: CapabilitySourceKind::ProviderCatalog,
                observed_at_us,
            }
        }

        async fn probe_model_capabilities(
            &self,
            model_id: &str,
            observed_at_us: u64,
        ) -> Result<ModelCapabilityProbeRecord, OrchestratorError> {
            Ok(ModelCapabilityProbeRecord {
                probe_id: format!("probe-{}-{}", self.id, model_id),
                model_ref: ModelRef::new(self.id, model_id),
                adapter_family: self.family,
                tool_calling: self.probe_tool_calling,
                parallel_tool_calling: CapabilitySupport::Unknown,
                streaming: self.probe_streaming,
                vision: CapabilitySupport::Unknown,
                json_mode: CapabilitySupport::Supported,
                max_context_tokens: Some(8192),
                supports_images: CapabilitySupport::Unknown,
                supports_audio: CapabilitySupport::Unknown,
                schema_acceptance: Some(CapabilitySupport::Supported),
                native_tool_probe: Some(self.probe_tool_calling),
                modality_probe: Some(CapabilitySupport::Unknown),
                source: CapabilitySourceKind::RuntimeProbe,
                probe_method: Some("mock".into()),
                probe_status: Some("success".into()),
                probe_error: None,
                raw_summary: Some("mock probe".into()),
                observed_at_us,
                expires_at_us: Some(observed_at_us + 10),
            })
        }
    }

    struct ProviderConformanceExpectation {
        family: AdapterFamily,
        expected_default_schema_mode: ToolSchemaMode,
        expected_default_tool_mode: ToolCallingMode,
        expected_probe_tool_calling: CapabilitySupport,
    }

    async fn run_provider_conformance_case(expectation: ProviderConformanceExpectation) {
        let mut registry = ProviderRegistry::new();
        let provider_id = format!("mock-{:?}", expectation.family).to_ascii_lowercase();
        registry.register(Arc::new(MockProvider {
            id: Box::leak(provider_id.clone().into_boxed_str()),
            name: "Mock Provider",
            family: expectation.family,
            probe_tool_calling: expectation.expected_probe_tool_calling,
            probe_streaming: CapabilitySupport::Supported,
        }));

        let descriptors = registry.provider_descriptors(42);
        assert_eq!(descriptors.len(), 1);
        assert_eq!(descriptors[0].adapter_family, expectation.family);

        let provider = registry
            .get_provider(&provider_id)
            .expect("provider registered");
        let default_profile = provider.default_model_capability_profile("model-a", 42);
        assert_eq!(
            default_profile.tool_schema_mode,
            expectation.expected_default_schema_mode
        );
        let adapter = adapter_for_family(expectation.family);
        assert_eq!(
            adapter.tool_calling_mode(&default_profile),
            expectation.expected_default_tool_mode
        );

        let probes = registry
            .probe_provider_models(&provider_id, &[String::from("model-a")], 42)
            .await
            .expect("probe");
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].adapter_family, expectation.family);
        assert_eq!(
            probes[0].tool_calling,
            expectation.expected_probe_tool_calling
        );
        assert_eq!(probes[0].probe_method.as_deref(), Some("mock"));
        assert_eq!(probes[0].probe_status.as_deref(), Some("success"));
    }

    #[tokio::test]
    async fn provider_registry_exposes_descriptors_and_probe_helpers() {
        let mut registry = ProviderRegistry::new();
        registry.register(Arc::new(MockProvider {
            id: "mock-openai",
            name: "Mock OpenAI",
            family: AdapterFamily::OpenAiCompatible,
            probe_tool_calling: CapabilitySupport::Supported,
            probe_streaming: CapabilitySupport::Supported,
        }));
        registry.register(Arc::new(MockProvider {
            id: "mock-cli",
            name: "Mock CLI",
            family: AdapterFamily::TextOnlyCli,
            probe_tool_calling: CapabilitySupport::Unsupported,
            probe_streaming: CapabilitySupport::Supported,
        }));

        let descriptors = registry.provider_descriptors(42);
        assert_eq!(descriptors.len(), 2);
        assert_eq!(descriptors[0].provider_id, "mock-cli");
        assert_eq!(
            descriptors[1].adapter_family,
            AdapterFamily::OpenAiCompatible
        );

        let probes = registry
            .probe_provider_models("mock-openai", &[String::from("model-a")], 42)
            .await
            .expect("probe");
        assert_eq!(probes.len(), 1);
        assert_eq!(probes[0].probe_method.as_deref(), Some("mock"));
        assert_eq!(probes[0].model_ref.as_slash_ref(), "mock-openai/model-a");
    }

    #[tokio::test]
    async fn provider_conformance_matrix_openai_family() {
        run_provider_conformance_case(ProviderConformanceExpectation {
            family: AdapterFamily::OpenAiCompatible,
            expected_default_schema_mode: ToolSchemaMode::StrictJsonSchema,
            expected_default_tool_mode: ToolCallingMode::CompatTools,
            expected_probe_tool_calling: CapabilitySupport::Supported,
        })
        .await;
    }

    #[tokio::test]
    async fn provider_conformance_matrix_ollama_family() {
        run_provider_conformance_case(ProviderConformanceExpectation {
            family: AdapterFamily::OllamaNative,
            expected_default_schema_mode: ToolSchemaMode::ReducedJsonSchema,
            expected_default_tool_mode: ToolCallingMode::CompatTools,
            expected_probe_tool_calling: CapabilitySupport::Degraded,
        })
        .await;
    }

    #[tokio::test]
    async fn provider_conformance_matrix_anthropic_family() {
        run_provider_conformance_case(ProviderConformanceExpectation {
            family: AdapterFamily::Anthropic,
            expected_default_schema_mode: ToolSchemaMode::StrictJsonSchema,
            expected_default_tool_mode: ToolCallingMode::TextFallbackNoTools,
            expected_probe_tool_calling: CapabilitySupport::Supported,
        })
        .await;
    }

    #[tokio::test]
    async fn provider_conformance_matrix_gemini_family() {
        run_provider_conformance_case(ProviderConformanceExpectation {
            family: AdapterFamily::GoogleGemini,
            expected_default_schema_mode: ToolSchemaMode::ReducedJsonSchema,
            expected_default_tool_mode: ToolCallingMode::TextFallbackNoTools,
            expected_probe_tool_calling: CapabilitySupport::Supported,
        })
        .await;
    }

    #[tokio::test]
    async fn provider_conformance_matrix_text_only_family() {
        run_provider_conformance_case(ProviderConformanceExpectation {
            family: AdapterFamily::TextOnlyCli,
            expected_default_schema_mode: ToolSchemaMode::Unsupported,
            expected_default_tool_mode: ToolCallingMode::TextFallbackNoTools,
            expected_probe_tool_calling: CapabilitySupport::Unsupported,
        })
        .await;
    }
}
