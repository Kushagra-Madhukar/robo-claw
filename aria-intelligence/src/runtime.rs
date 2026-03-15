use super::*;

// ---------------------------------------------------------------------------
// Orchestrator types
// ---------------------------------------------------------------------------

/// A tool call requested by the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider-normalized invocation identifier when available.
    pub invocation_id: Option<String>,
    /// Name of the tool to invoke.
    pub name: String,
    /// JSON-encoded arguments for the tool.
    pub arguments: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutedToolCall {
    pub call: ToolCall,
    pub result: ToolExecutionResult,
}

impl ToolCall {
    pub fn invocation_envelope(&self) -> aria_core::ToolInvocationEnvelope {
        aria_core::ToolInvocationEnvelope {
            invocation_id: self.invocation_id.clone(),
            tool_name: self.name.clone(),
            arguments_json: self.arguments.clone(),
        }
    }
}

impl ExecutedToolCall {
    pub fn result_envelope(&self) -> aria_core::ToolResultEnvelope {
        self.result.to_envelope()
    }
}

/// Response from the LLM backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LLMResponse {
    /// A final text answer — the ReAct loop should terminate.
    TextAnswer(String),
    /// One or more tool calls to execute before re-querying.
    ToolCalls(Vec<ToolCall>),
}

pub fn tool_calling_mode_for_model(profile: Option<&ModelCapabilityProfile>) -> ToolCallingMode {
    tool_calling_mode_for_model_with_repair(profile, false)
}

pub fn tool_calling_mode_for_model_with_repair(
    profile: Option<&ModelCapabilityProfile>,
    allow_repair_fallback: bool,
) -> ToolCallingMode {
    match profile.map(|p| p.tool_calling) {
        Some(aria_core::CapabilitySupport::Supported) => ToolCallingMode::NativeTools,
        Some(aria_core::CapabilitySupport::Degraded) => ToolCallingMode::CompatTools,
        Some(aria_core::CapabilitySupport::Unknown)
        | Some(aria_core::CapabilitySupport::Unsupported) => {
            if allow_repair_fallback {
                ToolCallingMode::TextFallbackWithRepair
            } else {
                ToolCallingMode::TextFallbackNoTools
            }
        }
        None => ToolCallingMode::CompatTools,
    }
}

pub fn filter_tools_for_model_capability(
    tools: &[CachedTool],
    profile: Option<&ModelCapabilityProfile>,
) -> Vec<CachedTool> {
    filter_tools_for_model_capability_with_repair(tools, profile, false)
}

pub fn filter_tools_for_model_capability_with_repair(
    tools: &[CachedTool],
    profile: Option<&ModelCapabilityProfile>,
    allow_repair_fallback: bool,
) -> Vec<CachedTool> {
    match tool_calling_mode_for_model_with_repair(profile, allow_repair_fallback) {
        ToolCallingMode::NativeTools | ToolCallingMode::CompatTools => tools
            .iter()
            .filter(|tool| tool_is_compatible_with_model(tool, profile))
            .cloned()
            .collect(),
        ToolCallingMode::TextFallbackWithRepair => tools
            .iter()
            .filter(|tool| tool_is_text_repair_compatible(tool, profile))
            .cloned()
            .collect(),
        ToolCallingMode::TextFallbackNoTools => Vec::new(),
    }
}

fn tool_is_text_repair_compatible(
    tool: &CachedTool,
    profile: Option<&ModelCapabilityProfile>,
) -> bool {
    let Some(profile) = profile else {
        return true;
    };
    if tool.modalities.contains(&aria_core::ToolModality::Image)
        && !matches!(
            profile.supports_images,
            aria_core::CapabilitySupport::Supported
        )
    {
        return false;
    }
    if tool.modalities.contains(&aria_core::ToolModality::Audio)
        && !matches!(
            profile.supports_audio,
            aria_core::CapabilitySupport::Supported
        )
    {
        return false;
    }
    true
}

pub fn tool_mode_limitation_message(profile: Option<&ModelCapabilityProfile>) -> Option<String> {
    let profile = profile?;
    match tool_calling_mode_for_model(Some(profile)) {
        ToolCallingMode::TextFallbackNoTools => Some(format!(
            "The active model '{}' on provider '{}' is currently operating in text-only mode and cannot reliably use tools.",
            profile.model_ref.model_id, profile.model_ref.provider_id
        )),
        ToolCallingMode::TextFallbackWithRepair => Some(format!(
            "The active model '{}' on provider '{}' is in transitional repair mode; tool use is restricted.",
            profile.model_ref.model_id, profile.model_ref.provider_id
        )),
        ToolCallingMode::NativeTools | ToolCallingMode::CompatTools => None,
    }
}

/// Errors from the orchestrator.
#[derive(Debug)]
pub enum OrchestratorError {
    /// The LLM backend returned an error.
    LLMError(String),
    /// A tool execution failed.
    ToolError(String),
    /// The maximum number of tool rounds was exceeded.
    MaxRoundsExceeded { limit: usize },
    /// The backend is overloaded or timed out.
    BackendOverloaded(String),
    /// A workspace-level coordinator rejected or timed out a conflicting run.
    ResourceBusy(String),
    /// A security violation was intercepted by the firewall.
    SecurityViolation(String),
    /// The human operator triggered an abort via steering.
    UserAborted,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::LLMError(msg) => write!(f, "LLM error: {}", msg),
            OrchestratorError::ToolError(msg) => write!(f, "tool error: {}", msg),
            OrchestratorError::MaxRoundsExceeded { limit } => {
                write!(f, "max rounds ({}) exceeded", limit)
            }
            OrchestratorError::BackendOverloaded(msg) => write!(f, "backend overloaded: {}", msg),
            OrchestratorError::ResourceBusy(msg) => write!(f, "resource busy: {}", msg),
            OrchestratorError::SecurityViolation(msg) => {
                write!(f, "security violation: {}", msg)
            }
            OrchestratorError::UserAborted => write!(f, "aborted by user steering"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

// LLM backends are now in the `backends` module.

pub const APPROVAL_REQUIRED_PREFIX: &str = "APPROVAL_REQUIRED::";

pub fn approval_required_error(tool_name: &str) -> OrchestratorError {
    OrchestratorError::ToolError(format!("{}{}", APPROVAL_REQUIRED_PREFIX, tool_name))
}

pub(crate) fn approval_required_tool_name(message: &str) -> Option<&str> {
    message.strip_prefix(APPROVAL_REQUIRED_PREFIX)
}

/// Identifier for registered LLM backends.
pub type LlmBackendId = String;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderHealthIdentity {
    pub provider_family: String,
    pub upstream_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCircuitState {
    pub provider_family: String,
    pub upstream_identity: String,
    pub circuit_open: bool,
    pub consecutive_failures: usize,
    pub impacted_backends: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderTransportConfig {
    pub response_start_timeout: Duration,
}

impl Default for ProviderTransportConfig {
    fn default() -> Self {
        Self {
            response_start_timeout: Duration::from_secs(20),
        }
    }
}

static PROVIDER_TRANSPORT_CONFIG: std::sync::OnceLock<ProviderTransportConfig> =
    std::sync::OnceLock::new();

pub fn install_provider_transport_config(config: ProviderTransportConfig) {
    let _ = PROVIDER_TRANSPORT_CONFIG.set(config);
}

pub fn provider_transport_config() -> ProviderTransportConfig {
    *PROVIDER_TRANSPORT_CONFIG.get_or_init(ProviderTransportConfig::default)
}

pub struct LlmBackendPool {
    backends: Mutex<HashMap<LlmBackendId, Box<dyn LLMBackend>>>,
    fallback_order: Vec<LlmBackendId>,
    cooldown_for: Duration,
    cooldown_until: Mutex<HashMap<LlmBackendId, Instant>>,
    consecutive_failures: Mutex<HashMap<LlmBackendId, usize>>,
    provider_circuit_cooldown_for: Duration,
    provider_circuit_failure_threshold: usize,
    provider_cooldown_until: Mutex<HashMap<String, Instant>>,
    provider_failures: Mutex<HashMap<String, usize>>,
    provider_last_error: Mutex<HashMap<String, String>>,
}

impl LlmBackendPool {
    pub fn new(fallback_order: Vec<LlmBackendId>, cooldown_for: Duration) -> Self {
        Self {
            backends: Mutex::new(HashMap::new()),
            fallback_order,
            cooldown_for,
            cooldown_until: Mutex::new(HashMap::new()),
            consecutive_failures: Mutex::new(HashMap::new()),
            provider_circuit_cooldown_for: cooldown_for,
            provider_circuit_failure_threshold: 3,
            provider_cooldown_until: Mutex::new(HashMap::new()),
            provider_failures: Mutex::new(HashMap::new()),
            provider_last_error: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_provider_circuit_breaker(
        mut self,
        cooldown_for: Duration,
        failure_threshold: usize,
    ) -> Self {
        self.provider_circuit_cooldown_for = cooldown_for;
        self.provider_circuit_failure_threshold = failure_threshold.max(1);
        self
    }

    pub fn register_backend(&self, id: impl Into<LlmBackendId>, backend: Box<dyn LLMBackend>) {
        let mut guard = self.backends.lock().expect("poisoned pool mutex");
        guard.insert(id.into(), backend);
    }

    pub fn primary_capability_profile(&self) -> Option<ModelCapabilityProfile> {
        for backend_id in &self.fallback_order {
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            if let Some(backend) = backend {
                if let Some(profile) = backend.capability_profile() {
                    return Some(profile);
                }
            }
        }
        None
    }

    pub fn primary_backend_clone(&self) -> Option<Box<dyn LLMBackend>> {
        for backend_id in &self.fallback_order {
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            if backend.is_some() {
                return backend;
            }
        }
        None
    }

    pub(crate) fn is_cooling_down(&self, id: &str) -> bool {
        let guard = self.cooldown_until.lock().expect("poisoned cooldown mutex");
        guard
            .get(id)
            .map(|until| *until > Instant::now())
            .unwrap_or(false)
    }

    fn mark_cooldown(&self, id: &str) {
        let mut guard = self.cooldown_until.lock().expect("poisoned cooldown mutex");
        guard.insert(id.to_string(), Instant::now() + self.cooldown_for);
    }

    fn provider_key_for_backend(&self, backend: &dyn LLMBackend) -> String {
        let identity = backend.provider_health_identity();
        format!(
            "{}::{}",
            identity.provider_family, identity.upstream_identity
        )
    }

    fn is_provider_circuit_open(&self, provider_key: &str) -> bool {
        let guard = self
            .provider_cooldown_until
            .lock()
            .expect("poisoned provider cooldown mutex");
        guard
            .get(provider_key)
            .map(|until| *until > Instant::now())
            .unwrap_or(false)
    }

    fn record_provider_failure(
        &self,
        provider_key: &str,
        error: &OrchestratorError,
    ) -> Option<Instant> {
        let failures = {
            let mut guard = self
                .provider_failures
                .lock()
                .expect("poisoned provider failures mutex");
            *guard
                .entry(provider_key.to_string())
                .and_modify(|count| *count += 1)
                .or_insert(1)
        };
        {
            let mut guard = self
                .provider_last_error
                .lock()
                .expect("poisoned provider last-error mutex");
            guard.insert(provider_key.to_string(), error.to_string());
        }
        if failures >= self.provider_circuit_failure_threshold {
            let until = Instant::now() + self.provider_circuit_cooldown_for;
            let mut guard = self
                .provider_cooldown_until
                .lock()
                .expect("poisoned provider cooldown mutex");
            guard.insert(provider_key.to_string(), until);
            Some(until)
        } else {
            None
        }
    }

    fn reset_provider_failures(&self, provider_key: &str) {
        {
            let mut guard = self
                .provider_failures
                .lock()
                .expect("poisoned provider failures mutex");
            guard.insert(provider_key.to_string(), 0);
        }
        {
            let mut guard = self
                .provider_last_error
                .lock()
                .expect("poisoned provider last-error mutex");
            guard.remove(provider_key);
        }
        {
            let mut guard = self
                .provider_cooldown_until
                .lock()
                .expect("poisoned provider cooldown mutex");
            guard.remove(provider_key);
        }
    }

    fn should_trip_provider_circuit(error: &OrchestratorError) -> bool {
        match error {
            OrchestratorError::BackendOverloaded(_) => true,
            OrchestratorError::LLMError(message) => {
                let message = message.to_ascii_lowercase();
                [
                    "timeout",
                    "timed out",
                    "connection reset",
                    "connection refused",
                    "dns",
                    "temporarily unavailable",
                    "transport",
                    "connect error",
                    "network",
                    "first token",
                    "overloaded",
                ]
                .iter()
                .any(|needle| message.contains(needle))
            }
            OrchestratorError::ToolError(_)
            | OrchestratorError::MaxRoundsExceeded { .. }
            | OrchestratorError::SecurityViolation(_)
            | OrchestratorError::UserAborted
            | OrchestratorError::ResourceBusy(_) => false,
        }
    }

    pub fn provider_circuit_state(&self) -> Vec<ProviderCircuitState> {
        let backends = self.backends.lock().expect("poisoned pool mutex");
        let provider_failures = self
            .provider_failures
            .lock()
            .expect("poisoned provider failures mutex");
        let provider_cooldowns = self
            .provider_cooldown_until
            .lock()
            .expect("poisoned provider cooldown mutex");
        let provider_last_error = self
            .provider_last_error
            .lock()
            .expect("poisoned provider last-error mutex");
        let mut by_provider: HashMap<String, ProviderCircuitState> = HashMap::new();
        for backend_id in &self.fallback_order {
            let Some(backend) = backends.get(backend_id) else {
                continue;
            };
            let identity = backend.provider_health_identity();
            let provider_key = self.provider_key_for_backend(&**backend);
            let entry =
                by_provider
                    .entry(provider_key.clone())
                    .or_insert_with(|| ProviderCircuitState {
                        provider_family: identity.provider_family.clone(),
                        upstream_identity: identity.upstream_identity.clone(),
                        circuit_open: provider_cooldowns
                            .get(&provider_key)
                            .map(|until| *until > Instant::now())
                            .unwrap_or(false),
                        consecutive_failures: provider_failures
                            .get(&provider_key)
                            .copied()
                            .unwrap_or(0),
                        impacted_backends: Vec::new(),
                        last_error: provider_last_error.get(&provider_key).cloned(),
                    });
            entry.impacted_backends.push(backend_id.clone());
        }
        let mut states = by_provider
            .into_values()
            .filter(|state| state.consecutive_failures > 0 || state.circuit_open)
            .collect::<Vec<_>>();
        states.sort_by(|a, b| {
            a.provider_family
                .cmp(&b.provider_family)
                .then(a.upstream_identity.cmp(&b.upstream_identity))
        });
        states
    }

    fn increment_failure(&self, id: &str) -> usize {
        let mut guard = self
            .consecutive_failures
            .lock()
            .expect("poisoned failures mutex");
        *guard
            .entry(id.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1)
    }

    fn reset_failures(&self, id: &str) {
        let mut guard = self
            .consecutive_failures
            .lock()
            .expect("poisoned failures mutex");
        guard.insert(id.to_string(), 0);
    }

    /// Query configured backends in fallback order, skipping cooling-down entries.
    pub async fn query_with_fallback(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        info!(
            prompt_len = prompt.len(),
            prompt = %prompt,
            "LLM: raw prompt sent to backend"
        );
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            let Some(backend) = backend else {
                continue;
            };
            let provider_key = self.provider_key_for_backend(backend.as_ref());
            if self.is_provider_circuit_open(&provider_key) {
                info!(
                    backend_id = %backend_id,
                    provider_key = %provider_key,
                    "LLM: skipping backend because provider circuit is open"
                );
                continue;
            }
            match backend.query(prompt, tools).await {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    self.reset_provider_failures(&provider_key);
                    match &resp {
                        LLMResponse::TextAnswer(t) => info!(
                            response_len = t.len(),
                            response = %t,
                            "LLM: raw response"
                        ),
                        LLMResponse::ToolCalls(calls) => info!(
                            tool_count = calls.len(),
                            tool_names = ?calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
                            "LLM: tool calls returned"
                        ),
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    if Self::should_trip_provider_circuit(&err) {
                        if let Some(until) = self.record_provider_failure(&provider_key, &err) {
                            info!(
                                backend_id = %backend_id,
                                provider_key = %provider_key,
                                cooldown_ms = self.provider_circuit_cooldown_for.as_millis() as u64,
                                "LLM: opened provider circuit after retryable failure"
                            );
                            let _ = until;
                        }
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }

    pub async fn query_with_fallback_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            let Some(backend) = backend else {
                continue;
            };
            let provider_key = self.provider_key_for_backend(backend.as_ref());
            if self.is_provider_circuit_open(&provider_key) {
                info!(
                    backend_id = %backend_id,
                    provider_key = %provider_key,
                    "LLM: skipping backend because provider circuit is open"
                );
                continue;
            }
            match backend.query_with_policy(prompt, tools, policy).await {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    self.reset_provider_failures(&provider_key);
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    if Self::should_trip_provider_circuit(&err) {
                        let _ = self.record_provider_failure(&provider_key, &err);
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }

    pub async fn query_stream_with_fallback_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            let Some(backend) = backend else {
                continue;
            };
            let provider_key = self.provider_key_for_backend(backend.as_ref());
            if self.is_provider_circuit_open(&provider_key) {
                info!(
                    backend_id = %backend_id,
                    provider_key = %provider_key,
                    "LLM: skipping backend because provider circuit is open"
                );
                continue;
            }
            match backend
                .query_stream_with_policy(prompt, tools, policy)
                .await
            {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    self.reset_provider_failures(&provider_key);
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    if Self::should_trip_provider_circuit(&err) {
                        let _ = self.record_provider_failure(&provider_key, &err);
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }

    pub async fn query_with_tool_results_with_fallback_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            let Some(backend) = backend else {
                continue;
            };
            let provider_key = self.provider_key_for_backend(backend.as_ref());
            if self.is_provider_circuit_open(&provider_key) {
                info!(
                    backend_id = %backend_id,
                    provider_key = %provider_key,
                    "LLM: skipping backend because provider circuit is open"
                );
                continue;
            }
            match backend
                .query_with_tool_results_and_policy(prompt, tools, executed_tools, policy)
                .await
            {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    self.reset_provider_failures(&provider_key);
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    if Self::should_trip_provider_circuit(&err) {
                        let _ = self.record_provider_failure(&provider_key, &err);
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }

    pub async fn query_stream_with_tool_results_with_fallback_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[ExecutedToolCall],
        policy: &ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard
                    .get(backend_id)
                    .map(|backend| dyn_clone::clone_box(&**backend))
            };
            let Some(backend) = backend else {
                continue;
            };
            let provider_key = self.provider_key_for_backend(backend.as_ref());
            if self.is_provider_circuit_open(&provider_key) {
                info!(
                    backend_id = %backend_id,
                    provider_key = %provider_key,
                    "LLM: skipping backend because provider circuit is open"
                );
                continue;
            }
            match backend
                .query_stream_with_tool_results_and_policy(prompt, tools, executed_tools, policy)
                .await
            {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    self.reset_provider_failures(&provider_key);
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    if Self::should_trip_provider_circuit(&err) {
                        let _ = self.record_provider_failure(&provider_key, &err);
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Tool-call JSON drift recovery
// ---------------------------------------------------------------------------

/// Balance incomplete JSON braces.
/// Small models sometimes truncate the closing brace of tool call JSON.
pub(crate) fn balance_json(s: &str) -> String {
    let open = s.chars().filter(|&c| c == '{').count();
    let close = s.chars().filter(|&c| c == '}').count();
    if open > close {
        let mut out = s.to_string();
        for _ in 0..(open - close) {
            out.push('}');
        }
        out
    } else {
        s.to_string()
    }
}

fn extract_json_candidate(text: &str) -> Option<String> {
    let trimmed = text.trim();
    let fenced = if let Some(rest) = trimmed.strip_prefix("```json") {
        Some(rest)
    } else if let Some(rest) = trimmed.strip_prefix("```") {
        Some(rest)
    } else {
        None
    };

    let unfenced = if let Some(rest) = fenced {
        let body = rest.trim_start();
        let end = body.rfind("```").unwrap_or(body.len());
        body[..end].trim()
    } else {
        trimmed
    };

    let start = unfenced.find('{')?;
    let mut candidate = unfenced[start..].trim();
    if let Some(idx) = candidate.rfind("```") {
        candidate = candidate[..idx].trim();
    }
    if let Some(idx) = candidate.rfind('}') {
        candidate = candidate[..=idx].trim();
    }
    Some(candidate.to_string())
}

/// Attempt to extract a `ToolCall` from potentially drifted LLM JSON output.
///
/// Accepts any of the common key aliases that small models emit:
/// - Tool name: `"tool"`, `"name"`, `"function"`, `"fn"`, `"action"`
/// - Arguments: `"args"`, `"arguments"`, `"parameters"`, `"input"`, `"params"`
///
/// Returns `None` when: no JSON object is found, more than one candidate exists
/// but none of the names match the active tool set, or the tool list is empty.
pub fn repair_tool_call_json(text: &str, tools: &[CachedTool]) -> Option<ToolCall> {
    if tools.is_empty() {
        return None;
    }
    let raw_candidate_owned = extract_json_candidate(text)?;
    let mut raw_candidate = raw_candidate_owned.as_str();

    // Strip trailing markdown blocks that can confuse standard parsing
    // but be CAREFUL not to strip inner braces if the JSON is malformed (missing closing brace).
    if let Some(idx) = raw_candidate.rfind("```") {
        raw_candidate = raw_candidate[..idx].trim();
    }
    if let Some(idx) = raw_candidate.rfind('}') {
        raw_candidate = raw_candidate[..=idx].trim();
    }
    // Only strip trailing '}' if we are reasonably sure it's the outer one.
    // Heuristic: if there's significant text after it that doesn't look like JSON fluff, keep it.
    if let Some(idx) = raw_candidate.rfind('}') {
        let after = &raw_candidate[idx + 1..].trim();
        if after.is_empty() || after.starts_with(',') || after.starts_with(']') {
            // likely the real end
            // But wait, if we are missing the closing brace, this might still be an inner one.
            // For now, let's only strip if it's the VERY last non-whitespace char.
            if raw_candidate.trim_end().ends_with('}') {
                raw_candidate = raw_candidate.trim_end();
            }
        }
    }

    let balanced = balance_json(raw_candidate);
    let v_opt = serde_json::from_str::<serde_json::Value>(&balanced).ok();

    if let Some(v) = v_opt {
        // Try each alias for the tool name key.
        const NAME_KEYS: &[&str] = &["tool", "name", "function", "fn", "action"];
        let tool_name: Option<String> = NAME_KEYS.iter().find_map(|k| {
            v.get(*k).and_then(|n| n.as_str()).map(|s| {
                // Architectural Fix: Strip "tool." or "agent." prefixes common in LLM hallucinations
                s.strip_prefix("tool.")
                    .or_else(|| s.strip_prefix("agent."))
                    .unwrap_or(s)
                    .to_string()
            })
        });

        // Try each alias for the arguments key.
        const ARGS_KEYS: &[&str] = &["args", "arguments", "parameters", "input", "params"];
        let args = ARGS_KEYS
            .iter()
            .find_map(|k| v.get(*k).cloned())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(name) = tool_name {
            if tools.iter().any(|t| t.name == name)
                || matches!(
                    name.as_str(),
                    "schedule_message" | "set_reminder" | "manage_cron" | "search_tool_registry"
                )
            {
                return Some(ToolCall {
                    invocation_id: None,
                    name,
                    arguments: args.to_string(),
                });
            }
        }
    }

    // --- Aggressive Fallback Heuristics for Severe JSON Drift ---
    // Often, small LLMs hallucinate executable code syntax inside the JSON string
    // (e.g. `);`) which breaks the standard serde_json parsing entirely.
    if raw_candidate.contains("write_file") && tools.iter().any(|t| t.name == "write_file") {
        let path_start = raw_candidate
            .find(r#""path":"#)
            .or_else(|| raw_candidate.find(r#""path": "#));
        let content_start = raw_candidate
            .find(r#""content":"#)
            .or_else(|| raw_candidate.find(r#""content": "#));

        if let (Some(ps), Some(cs)) = (path_start, content_start) {
            let after_path = &raw_candidate[ps + 6..];
            if let Some(path_q1) = after_path.find('"') {
                if let Some(path_q2) = after_path[path_q1 + 1..].find('"') {
                    let path = &after_path[path_q1 + 1..path_q1 + 1 + path_q2];

                    let after_content = &raw_candidate[cs + 9..];
                    if let Some(content_q1) = after_content.find('"') {
                        let mut content = &after_content[content_q1 + 1..];
                        if let Some(last_quote) = content.rfind('"') {
                            // Only strip if there's trailing junk after the last quote
                            // otherwise we might be stripping the only quote we have.
                            if last_quote < content.len() - 1 {
                                content = &content[..last_quote];
                            }
                        }

                        let escaped_content = content.replace('\n', "\\n").replace('"', "\\\"");
                        let args = format!(
                            r#"{{"path": "{}", "content": "{}"}}"#,
                            path, escaped_content
                        );

                        return Some(ToolCall {
                            invocation_id: None,
                            name: "write_file".to_string(),
                            arguments: args,
                        });
                    }
                }
            }
        }
    }

    // Generic fallback for tools other than write_file (if they are simple key-value)
    None
}

pub(crate) fn extract_tool_name_candidate(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let raw_candidate = balance_json(text[start..].trim());
    let v = serde_json::from_str::<serde_json::Value>(&raw_candidate).ok()?;
    const NAME_KEYS: &[&str] = &["tool", "name", "function", "fn", "action"];
    NAME_KEYS.iter().find_map(|k| {
        v.get(*k).and_then(|n| n.as_str()).map(|s| {
            s.strip_prefix("tool.")
                .or_else(|| s.strip_prefix("agent."))
                .unwrap_or(s)
                .to_string()
        })
    })
}
