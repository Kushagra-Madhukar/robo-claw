use super::*;

pub type ProviderId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider_id: ProviderId,
    pub model_id: String,
}

impl ModelRef {
    pub fn new(provider_id: impl Into<String>, model_id: impl Into<String>) -> Self {
        Self {
            provider_id: provider_id.into(),
            model_id: model_id.into(),
        }
    }

    pub fn as_slash_ref(&self) -> String {
        format!("{}/{}", self.provider_id, self.model_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Supported,
    Unsupported,
    Unknown,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSchemaMode {
    StrictJsonSchema,
    ReducedJsonSchema,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolResultMode {
    NativeStructured,
    TextBlock,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolModality {
    Text,
    Image,
    Audio,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterFamily {
    OpenAiCompatible,
    Anthropic,
    GoogleGemini,
    OllamaNative,
    TextOnlyCli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySourceKind {
    LocalOverride,
    RuntimeProbe,
    ProviderCatalog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilityProfile {
    pub provider_id: ProviderId,
    pub adapter_family: AdapterFamily,
    pub supports_model_listing: CapabilitySupport,
    pub supports_runtime_probe: CapabilitySupport,
    pub source: CapabilitySourceKind,
    pub observed_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilityProfile {
    pub model_ref: ModelRef,
    pub adapter_family: AdapterFamily,
    pub tool_calling: CapabilitySupport,
    pub parallel_tool_calling: CapabilitySupport,
    pub streaming: CapabilitySupport,
    pub vision: CapabilitySupport,
    pub json_mode: CapabilitySupport,
    #[serde(default)]
    pub max_context_tokens: Option<u32>,
    pub tool_schema_mode: ToolSchemaMode,
    pub tool_result_mode: ToolResultMode,
    pub supports_images: CapabilitySupport,
    pub supports_audio: CapabilitySupport,
    pub source: CapabilitySourceKind,
    #[serde(default)]
    pub source_detail: Option<String>,
    pub observed_at_us: u64,
    #[serde(default)]
    pub expires_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilityProbeRecord {
    pub probe_id: String,
    pub model_ref: ModelRef,
    pub adapter_family: AdapterFamily,
    pub tool_calling: CapabilitySupport,
    pub parallel_tool_calling: CapabilitySupport,
    pub streaming: CapabilitySupport,
    pub vision: CapabilitySupport,
    pub json_mode: CapabilitySupport,
    #[serde(default)]
    pub max_context_tokens: Option<u32>,
    pub supports_images: CapabilitySupport,
    pub supports_audio: CapabilitySupport,
    #[serde(default)]
    pub schema_acceptance: Option<CapabilitySupport>,
    #[serde(default)]
    pub native_tool_probe: Option<CapabilitySupport>,
    #[serde(default)]
    pub modality_probe: Option<CapabilitySupport>,
    pub source: CapabilitySourceKind,
    #[serde(default)]
    pub probe_method: Option<String>,
    #[serde(default)]
    pub probe_status: Option<String>,
    #[serde(default)]
    pub probe_error: Option<String>,
    #[serde(default)]
    pub raw_summary: Option<String>,
    pub observed_at_us: u64,
    #[serde(default)]
    pub expires_at_us: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolCallingMode {
    NativeTools,
    CompatTools,
    TextFallbackNoTools,
    TextFallbackWithRepair,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoicePolicy {
    Auto,
    None,
    Required,
    Specific(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRuntimePolicy {
    pub tool_choice: ToolChoicePolicy,
    pub allow_parallel_tool_calls: bool,
}

impl Default for ToolRuntimePolicy {
    fn default() -> Self {
        Self {
            tool_choice: ToolChoicePolicy::Auto,
            allow_parallel_tool_calls: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolExecutionKind {
    Native,
    Skill,
    McpImported,
    ProviderBuiltIn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderKind {
    Native,
    WasmSkill,
    Mcp,
    ExternalCompat,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRunnerClass {
    Native,
    Wasm,
    Mcp,
    ExternalCompat,
    Remote,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolOrigin {
    pub provider_kind: ToolProviderKind,
    pub provider_id: String,
    #[serde(default)]
    pub origin_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProviderHealthStatus {
    Ready,
    Degraded,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolProviderReadiness {
    pub provider_kind: ToolProviderKind,
    pub provider_id: String,
    pub status: ToolProviderHealthStatus,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub imported: bool,
    #[serde(default)]
    pub bound: bool,
    #[serde(default)]
    pub auth_ready: bool,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub latency_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolApprovalClass {
    None,
    LowRisk,
    HighRisk,
}

impl Default for ToolApprovalClass {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSideEffectLevel {
    ReadOnly,
    StateChanging,
    Irreversible,
}

impl Default for ToolSideEffectLevel {
    fn default() -> Self {
        Self::ReadOnly
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCompatibilityHints {
    #[serde(default)]
    pub provider_names: Vec<String>,
    #[serde(default)]
    pub requires_strict_schema: bool,
    #[serde(default)]
    pub prefers_reduced_schema: bool,
    #[serde(default)]
    pub supports_parallel_calls: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalToolSchema {
    pub parameters_json_schema: String,
    #[serde(default)]
    pub result_json_schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalToolSpec {
    pub tool_id: String,
    pub name: String,
    pub description_short: String,
    pub description_long: String,
    pub schema: CanonicalToolSchema,
    pub execution_kind: ToolExecutionKind,
    #[serde(default)]
    pub requires_approval: ToolApprovalClass,
    pub side_effect_level: ToolSideEffectLevel,
    #[serde(default)]
    pub streaming_safe: bool,
    #[serde(default)]
    pub parallel_safe: bool,
    #[serde(default)]
    pub modalities: Vec<ToolModality>,
    #[serde(default)]
    pub provider_hints: ProviderCompatibilityHints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCatalogEntry {
    pub tool_id: String,
    pub public_name: String,
    pub description: String,
    pub parameters_json_schema: String,
    pub execution_kind: ToolExecutionKind,
    pub provider_kind: ToolProviderKind,
    pub runner_class: ToolRunnerClass,
    pub origin: ToolOrigin,
    #[serde(default)]
    pub artifact_kind: Option<String>,
    #[serde(default)]
    pub requires_approval: ToolApprovalClass,
    #[serde(default)]
    pub side_effect_level: ToolSideEffectLevel,
    #[serde(default)]
    pub streaming_safe: bool,
    #[serde(default)]
    pub parallel_safe: bool,
    #[serde(default)]
    pub modalities: Vec<ToolModality>,
    #[serde(default)]
    pub capability_requirements: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptAssetEntry {
    pub asset_id: String,
    pub public_name: String,
    pub description: String,
    pub origin: ToolOrigin,
    #[serde(default)]
    pub arguments_json_schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceContextEntry {
    pub resource_id: String,
    pub public_name: String,
    pub description: String,
    pub origin: ToolOrigin,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInvocationEnvelope {
    #[serde(default)]
    pub invocation_id: Option<String>,
    pub tool_name: String,
    pub arguments_json: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolResultEnvelope {
    pub ok: bool,
    pub summary: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub data: Option<serde_json::Value>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default)]
    pub approval_required: bool,
}

impl ToolResultEnvelope {
    pub fn success(
        summary: impl Into<String>,
        kind: impl Into<String>,
        data: serde_json::Value,
    ) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            kind: Some(kind.into()),
            data: Some(data),
            error: None,
            artifacts: Vec::new(),
            retryable: false,
            approval_required: false,
        }
    }

    pub fn text(summary: impl Into<String>) -> Self {
        Self {
            ok: true,
            summary: summary.into(),
            kind: None,
            data: None,
            error: None,
            artifacts: Vec::new(),
            retryable: false,
            approval_required: false,
        }
    }

    pub fn failure(summary: impl Into<String>, error: impl Into<String>, retryable: bool) -> Self {
        Self {
            ok: false,
            summary: summary.into(),
            kind: None,
            data: None,
            error: Some(error.into()),
            artifacts: Vec::new(),
            retryable,
            approval_required: false,
        }
    }

    pub fn as_provider_payload(&self) -> serde_json::Value {
        let mut object = BTreeMap::new();
        object.insert("ok".to_string(), serde_json::Value::Bool(self.ok));
        object.insert(
            "summary".to_string(),
            serde_json::Value::String(self.summary.clone()),
        );
        if let Some(kind) = &self.kind {
            object.insert("kind".to_string(), serde_json::Value::String(kind.clone()));
        }
        if let Some(data) = &self.data {
            object.insert("data".to_string(), data.clone());
        }
        if let Some(error) = &self.error {
            object.insert(
                "error".to_string(),
                serde_json::Value::String(error.clone()),
            );
        }
        if !self.artifacts.is_empty() {
            object.insert(
                "artifacts".to_string(),
                serde_json::Value::Array(
                    self.artifacts
                        .iter()
                        .cloned()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }
        object.insert(
            "retryable".to_string(),
            serde_json::Value::Bool(self.retryable),
        );
        object.insert(
            "approval_required".to_string(),
            serde_json::Value::Bool(self.approval_required),
        );
        serde_json::Value::Object(object.into_iter().collect())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSelectionScore {
    pub tool_name: String,
    pub score: i32,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSelectionDecision {
    pub tool_choice: ToolChoicePolicy,
    pub tool_calling_mode: ToolCallingMode,
    #[serde(default)]
    pub text_fallback_mode: bool,
    #[serde(default)]
    pub relevance_threshold_millis: Option<i32>,
    #[serde(default)]
    pub available_tool_names: Vec<String>,
    #[serde(default)]
    pub selected_tool_names: Vec<String>,
    #[serde(default)]
    pub candidate_scores: Vec<ToolSelectionScore>,
}
