// ---------------------------------------------------------------------------
// Gateway & Mock Adapters
// ---------------------------------------------------------------------------

use crate::outbound::{cli_mark_awaiting_input, cli_print_prompt};

struct CliGateway;

fn cli_terminal_fingerprint() -> String {
    let mut parts = Vec::new();
    for key in ["USER", "USERNAME", "TERM", "SHELL"] {
        if let Ok(value) = std::env::var(key) {
            if !value.trim().is_empty() {
                parts.push(format!("{}={}", key, value));
            }
        }
    }
    #[cfg(unix)]
    {
        if let Ok(link) = std::fs::read_link("/dev/fd/0") {
            parts.push(format!("tty={}", link.display()));
        }
    }
    if parts.is_empty() {
        "cli-terminal:unknown".to_string()
    } else {
        parts.join("|")
    }
}

fn cli_session_id() -> [u8; 16] {
    static CLI_SESSION_ID: OnceLock<[u8; 16]> = OnceLock::new();
    *CLI_SESSION_ID.get_or_init(|| {
        let fingerprint = cli_terminal_fingerprint();
        let mut hasher = Sha256::new();
        hasher.update(b"aria-x-cli-session");
        hasher.update(fingerprint.as_bytes());
        let digest = hasher.finalize();
        let mut id = [0u8; 16];
        id.copy_from_slice(&digest[..16]);
        id
    })
}

fn cli_request_id() -> [u8; 16] {
    *uuid::Uuid::new_v4().as_bytes()
}

#[async_trait::async_trait]
impl GatewayAdapter for CliGateway {
    async fn receive(&self) -> Result<AgentRequest, GatewayError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            cli_mark_awaiting_input(true);
            cli_print_prompt();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                cli_mark_awaiting_input(false);
                let _ = tx.send(input.trim().to_string());
            } else {
                cli_mark_awaiting_input(false);
            }
        });

        let content = rx.await.unwrap_or_default();
        if content.is_empty() {
            // Keep the loop alive if empty line
            return Err(GatewayError::TransportError("Empty input".into()));
        }

        Ok(AgentRequest {
            request_id: cli_request_id(),
            session_id: cli_session_id(),
            channel: GatewayChannel::Cli,
            user_id: "cli_user".into(),
            content: MessageContent::Text(content),
            tool_runtime_policy: None,
            timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
        })
    }
}

#[cfg(test)]
mod gateway_runtime_cli_tests {
    use super::*;

    #[test]
    fn cli_session_id_is_stable_in_process() {
        let a = cli_session_id();
        let b = cli_session_id();
        assert_eq!(a, b);
        assert_ne!(a, [0u8; 16]);
    }

    #[test]
    fn cli_request_id_is_unique() {
        let a = cli_request_id();
        let b = cli_request_id();
        assert_ne!(a, b);
    }
}

#[derive(Clone)]
struct LocalMockLLM;

#[async_trait::async_trait]
impl LLMBackend for LocalMockLLM {
    async fn query(
        &self,
        _prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        Ok(LLMResponse::TextAnswer(
            "Mock response: request received and processed.".into(),
        ))
    }
}

#[derive(Clone)]
struct PoolBackedLLM {
    pool: Arc<LlmBackendPool>,
    override_backend: Option<Arc<dyn LLMBackend>>,
}

impl PoolBackedLLM {
    fn new(pool: Arc<LlmBackendPool>, override_backend: Option<Arc<dyn LLMBackend>>) -> Self {
        Self {
            pool,
            override_backend,
        }
    }
}

#[async_trait::async_trait]
impl LLMBackend for PoolBackedLLM {
    async fn query(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        if let Some(backend) = &self.override_backend {
            backend.query(prompt, tools).await
        } else {
            self.pool.query_with_fallback(prompt, tools).await
        }
    }

    async fn query_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &aria_core::ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        if let Some(backend) = &self.override_backend {
            backend.query_with_policy(prompt, tools, policy).await
        } else {
            self.pool
                .query_with_fallback_and_policy(prompt, tools, policy)
                .await
        }
    }

    async fn query_stream_with_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        policy: &aria_core::ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        if let Some(backend) = &self.override_backend {
            backend.query_stream_with_policy(prompt, tools, policy).await
        } else {
            self.pool
                .query_stream_with_fallback_and_policy(prompt, tools, policy)
                .await
        }
    }

    async fn query_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[aria_intelligence::ExecutedToolCall],
        policy: &aria_core::ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        if let Some(backend) = &self.override_backend {
            backend
                .query_with_tool_results_and_policy(prompt, tools, executed_tools, policy)
                .await
        } else {
            self.pool
                .query_with_tool_results_with_fallback_and_policy(
                    prompt,
                    tools,
                    executed_tools,
                    policy,
                )
                .await
        }
    }

    async fn query_stream_with_tool_results_and_policy(
        &self,
        prompt: &str,
        tools: &[CachedTool],
        executed_tools: &[aria_intelligence::ExecutedToolCall],
        policy: &aria_core::ToolRuntimePolicy,
    ) -> Result<LLMResponse, OrchestratorError> {
        if let Some(backend) = &self.override_backend {
            backend
                .query_stream_with_tool_results_and_policy(
                    prompt,
                    tools,
                    executed_tools,
                    policy,
                )
                .await
        } else {
            self.pool
                .query_stream_with_tool_results_with_fallback_and_policy(
                    prompt,
                    tools,
                    executed_tools,
                    policy,
                )
                .await
        }
    }

    fn inspect_context_payload(
        &self,
        context: &aria_core::ExecutionContextPack,
        tools: &[CachedTool],
        policy: &aria_core::ToolRuntimePolicy,
    ) -> Option<serde_json::Value> {
        if let Some(backend) = &self.override_backend {
            backend.inspect_context_payload(context, tools, policy)
        } else {
            self.pool
                .primary_backend_clone()
                .and_then(|backend| backend.inspect_context_payload(context, tools, policy))
        }
    }
}

async fn resolve_model_capability_profile(
    provider_registry: &Arc<tokio::sync::Mutex<ProviderRegistry>>,
    sessions_dir: &std::path::Path,
    llm_config: Option<&LlmConfig>,
    provider_id: &str,
    model_id: &str,
    now_us: u64,
) -> Option<aria_core::ModelCapabilityProfile> {
    let reg = provider_registry.lock().await;
    let provider = reg.get_provider(provider_id)?;
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let provider_profile = provider.provider_capability_profile(now_us);
    let _ = store.upsert_provider_capability(&provider_profile, now_us);
    let key = format!("{}/{}", provider_id, model_id);
    let default_profile = provider.default_model_capability_profile(model_id, now_us);
    let local_override = llm_config.and_then(|config| {
        config.capability_overrides.iter().find_map(|entry| {
            if entry.provider_id == provider_id && entry.model_id == model_id {
                Some(aria_core::ModelCapabilityProfile {
                    model_ref: aria_core::ModelRef::new(provider_id, model_id),
                    adapter_family: entry.adapter_family.unwrap_or(default_profile.adapter_family),
                    tool_calling: entry.tool_calling.unwrap_or(default_profile.tool_calling),
                    parallel_tool_calling: entry
                        .parallel_tool_calling
                        .unwrap_or(default_profile.parallel_tool_calling),
                    streaming: entry.streaming.unwrap_or(default_profile.streaming),
                    vision: entry.vision.unwrap_or(default_profile.vision),
                    json_mode: entry.json_mode.unwrap_or(default_profile.json_mode),
                    max_context_tokens: entry
                        .max_context_tokens
                        .or(default_profile.max_context_tokens),
                    tool_schema_mode: entry
                        .tool_schema_mode
                        .unwrap_or(default_profile.tool_schema_mode),
                    tool_result_mode: entry
                        .tool_result_mode
                        .unwrap_or(default_profile.tool_result_mode),
                    supports_images: entry
                        .supports_images
                        .unwrap_or(default_profile.supports_images),
                    supports_audio: entry
                        .supports_audio
                        .unwrap_or(default_profile.supports_audio),
                    source: aria_core::CapabilitySourceKind::LocalOverride,
                    source_detail: entry
                        .source_detail
                        .clone()
                        .or_else(|| Some(String::from("llm.capability_overrides"))),
                    observed_at_us: now_us,
                    expires_at_us: None,
                })
            } else {
                None
            }
        })
    });
    let runtime_probe = match store.read_model_capability(&key) {
        Ok(cached) if cached.expires_at_us.map(|v| v >= now_us).unwrap_or(true) => Some(cached),
        _ => match provider.probe_model_capabilities(model_id, now_us).await {
            Ok(probe) => {
                let _ = store.append_model_capability_probe(&probe);
                let profiled = aria_core::ModelCapabilityProfile {
                    model_ref: probe.model_ref.clone(),
                    adapter_family: probe.adapter_family,
                    tool_calling: probe.tool_calling,
                    parallel_tool_calling: probe.parallel_tool_calling,
                    streaming: probe.streaming,
                    vision: probe.vision,
                    json_mode: probe.json_mode,
                    max_context_tokens: probe.max_context_tokens.or(default_profile.max_context_tokens),
                    tool_schema_mode: default_profile.tool_schema_mode,
                    tool_result_mode: default_profile.tool_result_mode,
                    supports_images: probe.supports_images,
                    supports_audio: probe.supports_audio,
                    source: aria_core::CapabilitySourceKind::RuntimeProbe,
                    source_detail: probe.raw_summary.clone(),
                    observed_at_us: probe.observed_at_us,
                    expires_at_us: probe.expires_at_us,
                };
                let _ = store.upsert_model_capability(&profiled, now_us);
                Some(profiled)
            }
            Err(_) => {
                let _ = store.upsert_model_capability(&default_profile, now_us);
                Some(default_profile.clone())
            }
        },
    };
    let effective = resolve_capability_profile(
        local_override.as_ref(),
        runtime_probe.as_ref(),
        &default_profile,
        now_us,
    );
    let _ = store.upsert_model_capability(&effective, now_us);
    Some(effective)
}

struct WasmToolExecutor {
    vault: Arc<aria_vault::CredentialVault>,
    tools_dir: std::path::PathBuf,
    agent_id: String,
    session_id: uuid::Uuid,
    capability_profile: Option<AgentCapabilityProfile>,
    sessions_dir: PathBuf,
}

impl WasmToolExecutor {
    pub fn new(
        vault: Arc<aria_vault::CredentialVault>,
        agent_id: String,
        session_id: uuid::Uuid,
        capability_profile: Option<AgentCapabilityProfile>,
        sessions_dir: PathBuf,
    ) -> Self {
        Self {
            vault,
            tools_dir: std::path::PathBuf::from("./tools"),
            agent_id,
            session_id,
            capability_profile,
            sessions_dir,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<ToolExecutionResult, OrchestratorError> {
        let wasm_path = self.tools_dir.join(format!("{}.wasm", call.name));
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to read Wasm file for tool '{}' at {:?}: {}",
                call.name, wasm_path, e
            ))
        })?;

        let mut secrets = std::collections::HashMap::new();
        let network_allowed = capability_allows_external_network(self.capability_profile.as_ref());
        let vault_allowed = capability_allows_vault_egress(self.capability_profile.as_ref());
        let agent_id = self.agent_id.clone();
        let sessions_dir = self.sessions_dir.clone();
        let session_id = *self.session_id.as_bytes();
        let scope_name = format!("wasm_tool:{}", call.name);
        let broker = aria_intelligence::EgressCredentialBroker::new().with_audit_sink(
            move |record| {
                let outcome = match record.outcome {
                    aria_intelligence::EgressSecretOutcome::Allowed => {
                        aria_core::SecretUsageOutcome::Allowed
                    }
                    aria_intelligence::EgressSecretOutcome::Denied => {
                        aria_core::SecretUsageOutcome::Denied
                    }
                };
                let _ = RuntimeStore::for_sessions_dir(&sessions_dir).append_secret_usage_audit(
                    &aria_core::SecretUsageAuditRecord {
                        audit_id: uuid::Uuid::new_v4().to_string(),
                        agent_id: agent_id.clone(),
                        session_id: Some(session_id),
                        tool_name: scope_name.clone(),
                        key_name: record.key_name,
                        target_domain: record.target_domain,
                        outcome,
                        detail: record.detail,
                        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
                    },
                );
            },
        );

        if vault_allowed {
            // In a real system, we'd map "call.name" or "agent_id" to a list of required keys.
            if let Ok(key) = broker.resolve_vault_secret(
                &self.vault,
                &self.agent_id,
                "api_key",
                "api.github.com",
                &format!("wasm_tool:{}", call.name),
            ) {
                secrets.insert("GITHUB_TOKEN".to_string(), key);
            }
            if let Ok(key) = broker.resolve_vault_secret(
                &self.vault,
                &self.agent_id,
                "api_key",
                "api.openai.com",
                &format!("wasm_tool:{}", call.name),
            ) {
                secrets.insert("OPENAI_API_KEY".to_string(), key);
            }
        }

        let ws_dir = format!("/tmp/aria_ws_{}", self.session_id);
        let _ = std::fs::create_dir_all(&ws_dir);

        // Attempt to read allowed_hosts from tool manifest
        let manifest_path = self.tools_dir.join(format!("{}.manifest.json", call.name));
        let mut allowed_hosts = None;
        if network_allowed {
            if let Ok(manifest_str) = std::fs::read_to_string(&manifest_path) {
                #[derive(serde::Deserialize)]
                struct ToolManifest {
                    allowed_hosts: Option<Vec<String>>,
                }
                if let Ok(parsed) = serde_json::from_str::<ToolManifest>(&manifest_str) {
                    allowed_hosts = parsed.allowed_hosts;
                }
            }
        }

        let config = aria_skill_runtime::ExtismConfig {
            max_memory_pages: Some(runtime_resource_budget().wasm_max_memory_pages),
            wasi_enabled: true, // often needed for HTTP egress
            secrets,
            workspace_dir: Some(std::path::PathBuf::from(&ws_dir)),
            allowed_hosts,
        };
        let _ = std::fs::create_dir_all(&ws_dir);
        let backend = aria_skill_runtime::ExtismBackend::with_config(config);
        let persistent_cache = std::sync::Arc::new(aria_skill_runtime::PersistentWasmAotCache::new(
            self.sessions_dir.join("wasm_aot"),
            if runtime_deployment_profile() == DeploymentProfile::Edge {
                aria_skill_runtime::WasmExecutionMode::EdgeAotOnly
            } else {
                aria_skill_runtime::WasmExecutionMode::NodePreferAot
            },
            match runtime_deployment_profile() {
                DeploymentProfile::Edge => "edge",
                DeploymentProfile::Node => "node",
                DeploymentProfile::Cluster => "cluster",
            },
            std::env::consts::ARCH,
            runtime_deployment_profile() == DeploymentProfile::Edge,
        ));
        let backend = aria_skill_runtime::AotCachedExecutor::with_persistent_cache(
            backend,
            std::sync::Arc::new(aria_skill_runtime::WasmAotCache::new()),
            persistent_cache,
        );

        use aria_skill_runtime::WasmExecutor;
        let result = backend
            .execute(&wasm_bytes, &call.name, &call.arguments)
            .map_err(|e| {
                OrchestratorError::ToolError(format!(
                    "Wasm execution failed for '{}': {}",
                    call.name, e
                ))
            })?;

        debug!(
            tool = %call.name,
            arguments = %call.arguments,
            result_len = result.len(),
            "WasmToolExecutor: executed"
        );
        Ok(tool_text_result(result))
    }
}

pub struct NativeToolExecutor {
    pub tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    pub invoking_agent_id: Option<String>,
    pub session_id: Option<aria_core::Uuid>,
    pub user_id: Option<String>,
    pub channel: Option<aria_core::GatewayChannel>,
    pub session_memory: Option<aria_ssmu::SessionMemory>,
    pub cedar: Option<Arc<aria_policy::CedarEvaluator>>,
    pub sessions_dir: Option<PathBuf>,
    scheduling_intent: Option<SchedulingIntent>,
    user_timezone: chrono_tz::Tz,
}

// ---------------------------------------------------------------------------
// Telegram webhook gateway
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct WebSocketState {
    config: Arc<ResolvedAppConfig>,
    router_index: Arc<RouterIndex>,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: Arc<AgentConfigStore>,
    tool_registry: Arc<ToolManifestStore>,
    session_memory: Arc<aria_ssmu::SessionMemory>,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    hooks: Arc<HookRegistry>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    session_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    embed_semaphore: Arc<tokio::sync::Semaphore>,
    provider_registry: Arc<Mutex<ProviderRegistry>>,
}

#[derive(Clone)]
struct TelegramState {
    config: Arc<ResolvedAppConfig>,
    runtime_config_path: Arc<PathBuf>,
    runtime_config_lock: Arc<tokio::sync::Mutex<()>>,
    user_timezone_overrides: Arc<dashmap::DashMap<String, String>>,
    router_index: Arc<RouterIndex>,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: Arc<AgentConfigStore>,
    tool_registry: Arc<ToolManifestStore>,
    session_memory: Arc<aria_ssmu::SessionMemory>,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    pub tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    pub hooks: Arc<HookRegistry>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    session_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    session_steering_tx: Arc<
        dashmap::DashMap<String, tokio::sync::mpsc::Sender<aria_intelligence::SteeringCommand>>,
    >,
    embed_semaphore: Arc<tokio::sync::Semaphore>,
    global_estop: Arc<std::sync::atomic::AtomicBool>,
    provider_registry: Arc<Mutex<ProviderRegistry>>,
    telegram_token: String,
    client: reqwest::Client,
    stt_backend: Arc<dyn SpeechToTextBackend>,
    dedupe_window: Arc<tokio::sync::Mutex<DedupeWindow>>,
    dedupe_last_prune_us: Arc<std::sync::atomic::AtomicU64>,
    metrics: Arc<ChannelMetrics>,
}

impl TelegramState {
    async fn is_duplicate_event(
        &self,
        provider_update_id: Option<u64>,
        provider_message_id: Option<u64>,
    ) -> bool {
        let dedupe_key = provider_update_id
            .map(|v| format!("u:{}", v))
            .or_else(|| provider_message_id.map(|v| format!("m:{}", v)));
        let Some(key) = dedupe_key else {
            return false;
        };
        let first_seen_us = chrono::Utc::now().timestamp_micros() as u64;
        let inserted =
            {
                let mut guard = self.dedupe_window.lock().await;
                guard.insert(key.clone())
            } && RuntimeStore::for_sessions_dir(Path::new(&self.config.ssmu.sessions_dir))
                .record_dedupe_key_if_new(&key, first_seen_us)
                .unwrap_or(true);
        if !inserted {
            self.metrics.deduped.fetch_add(1, Ordering::Relaxed);
        }
        self.maybe_prune_dedupe_keys(first_seen_us);
        !inserted
    }

    fn maybe_prune_dedupe_keys(&self, now_us: u64) {
        const PRUNE_INTERVAL_US: u64 = 5 * 60 * 1_000_000;
        const MAX_PRUNED_ROWS_PER_TICK: usize = 1_000;
        let retention_us = self
            .config
            .runtime
            .dedupe_key_retention_secs
            .max(60)
            .saturating_mul(1_000_000);
        let previous = self
            .dedupe_last_prune_us
            .load(std::sync::atomic::Ordering::Relaxed);
        if now_us.saturating_sub(previous) < PRUNE_INTERVAL_US {
            return;
        }
        if self
            .dedupe_last_prune_us
            .compare_exchange(
                previous,
                now_us,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::Relaxed,
            )
            .is_err()
        {
            return;
        }
        let threshold_us = now_us.saturating_sub(retention_us);
        let store = RuntimeStore::for_sessions_dir(Path::new(&self.config.ssmu.sessions_dir));
        if let Ok(pruned) = store.prune_dedupe_keys_older_than(threshold_us, MAX_PRUNED_ROWS_PER_TICK)
        {
            if pruned > 0 {
                debug!(
                    channel = "telegram",
                    pruned = pruned,
                    threshold_us = threshold_us,
                    "Pruned stale dedupe keys"
                );
            }
        }
    }

    fn log_queue_metrics(&self, stage: &'static str) {
        debug!(
            channel = "telegram",
            stage = stage,
            queue_depth = self.metrics.queue_depth.load(Ordering::Relaxed),
            enqueued = self.metrics.enqueued.load(Ordering::Relaxed),
            dropped = self.metrics.dropped.load(Ordering::Relaxed),
            processed = self.metrics.processed.load(Ordering::Relaxed),
            deduped = self.metrics.deduped.load(Ordering::Relaxed),
            "Telegram queue metrics"
        );
    }

    fn sanitize_egress_text(&self, text: &str) -> String {
        match self.firewall.scan_egress(text) {
            aria_safety::ScanResult::Clean => text.to_string(),
            aria_safety::ScanResult::Alert(patterns) => {
                warn!(patterns = ?patterns, "Egress leak detected; redacting outbound text.");
                "[REDACTED: SENSITIVE DATA DETECTED]".to_string()
            }
        }
    }

    async fn post_telegram_json(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<(), String> {
        let url = format!(
            "https://api.telegram.org/bot{}/{}",
            self.telegram_token, method
        );
        let started = std::time::Instant::now();
        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("request failed: {}", e))?;
        let latency_ms = started.elapsed().as_millis() as u64;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("status={} body={}", status, text));
        }
        debug!(
            channel = "telegram",
            stage = "egress_send",
            method = %method,
            latency_ms = latency_ms,
            "Telegram outbound API call succeeded"
        );
        Ok(())
    }

    async fn send_telegram_secure_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<serde_json::Value>,
        parse_mode: Option<&str>,
    ) {
        let final_text = self.sanitize_egress_text(text);
        let mut body = serde_json::json!({
            "chat_id": chat_id,
            "text": final_text
        });
        if let Some(pm) = parse_mode {
            body.as_object_mut()
                .unwrap()
                .insert("parse_mode".to_string(), serde_json::json!(pm));
        }
        if let Some(markup) = reply_markup {
            body.as_object_mut()
                .unwrap()
                .insert("reply_markup".to_string(), markup);
        }
        if let Err(err) = self.post_telegram_json("sendMessage", body).await {
            error!(chat_id = %chat_id, error = %err, "Telegram sendMessage error");
        }
    }

    async fn send_telegram_content_with_fallback(
        &self,
        chat_id: i64,
        content: &MessageContent,
        fallback_text: &str,
    ) {
        let fallback = fallback_text.trim();
        let result = match content {
            MessageContent::Text(text) => {
                self.send_telegram_secure_message(chat_id, text, None, None)
                    .await;
                return;
            }
            MessageContent::Image { url, caption } => {
                let safe_caption = caption.as_deref().map(|c| self.sanitize_egress_text(c));
                self.post_telegram_json(
                    "sendPhoto",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "photo": url,
                        "caption": safe_caption,
                    }),
                )
                .await
            }
            MessageContent::Audio { url, transcript } => {
                let safe_caption = transcript.as_deref().map(|c| self.sanitize_egress_text(c));
                self.post_telegram_json(
                    "sendVoice",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "voice": url,
                        "caption": safe_caption,
                    }),
                )
                .await
            }
            MessageContent::Video {
                url,
                caption,
                transcript,
            } => {
                let caption_text = caption.as_deref().or(transcript.as_deref());
                let safe_caption = caption_text.map(|c| self.sanitize_egress_text(c));
                self.post_telegram_json(
                    "sendVideo",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "video": url,
                        "caption": safe_caption,
                    }),
                )
                .await
            }
            MessageContent::Document {
                url,
                caption,
                mime_type,
            } => {
                let safe_caption = caption.as_deref().map(|c| self.sanitize_egress_text(c));
                self.post_telegram_json(
                    "sendDocument",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "document": url,
                        "caption": safe_caption,
                        "disable_content_type_detection": mime_type.is_none(),
                    }),
                )
                .await
            }
            MessageContent::Location { lat, lng } => {
                self.post_telegram_json(
                    "sendLocation",
                    serde_json::json!({
                        "chat_id": chat_id,
                        "latitude": lat,
                        "longitude": lng,
                    }),
                )
                .await
            }
        };

        if let Err(err) = result {
            warn!(
                channel = "telegram",
                stage = "egress_media",
                error = %err,
                "Media egress failed; falling back to text"
            );
            crate::channel_health::record_channel_health_event(
                GatewayChannel::Telegram,
                crate::channel_health::ChannelHealthEventKind::Retry,
            );
            let fallback_message = if fallback.is_empty() {
                "Unable to send media response."
            } else {
                fallback
            };
            self.send_telegram_secure_message(chat_id, fallback_message, None, None)
                .await;
        }
    }

    async fn send_telegram_agent_reply(&self, chat_id: i64, response_text: &str) {
        if let Some(content) = parse_media_response(response_text) {
            self.send_telegram_content_with_fallback(chat_id, &content, response_text)
                .await;
        } else {
            self.send_telegram_secure_message(chat_id, response_text, None, None)
                .await;
        }
    }

    async fn resolve_telegram_file_url(&self, file_id: &str) -> Option<String> {
        if file_id.trim().is_empty()
            || file_id.starts_with("http://")
            || file_id.starts_with("https://")
        {
            return Some(file_id.to_string());
        }
        let url = format!(
            "https://api.telegram.org/bot{}/getFile",
            self.telegram_token
        );
        let body = serde_json::json!({ "file_id": file_id });
        let resp = self.client.post(&url).json(&body).send().await.ok()?;
        let json: serde_json::Value = resp.json().await.ok()?;
        let file_path = json
            .get("result")
            .and_then(|v| v.get("file_path"))
            .and_then(|v| v.as_str())?;
        Some(format!(
            "https://api.telegram.org/file/bot{}/{}",
            self.telegram_token, file_path
        ))
    }

    async fn download_telegram_file(&self, file_url: &str) -> Option<Vec<u8>> {
        let resp = self.client.get(file_url).send().await.ok()?;
        let bytes = resp.bytes().await.ok()?;
        Some(bytes.to_vec())
    }

    async fn enrich_telegram_media_request(&self, req: &mut AgentRequest) {
        match &mut req.content {
            MessageContent::Image { url, .. } => {
                if let Some(file_url) = self.resolve_telegram_file_url(url).await {
                    *url = file_url;
                }
            }
            MessageContent::Audio { url, transcript } => {
                if let Some(file_url) = self.resolve_telegram_file_url(url).await {
                    *url = file_url.clone();
                    if transcript.is_none() {
                        if let Some(bytes) = self.download_telegram_file(&file_url).await {
                            let started = std::time::Instant::now();
                            if let Some(stt) = self
                                .stt_backend
                                .transcribe(&bytes, "ogg", Some("audio/ogg"))
                                .await
                            {
                                *transcript = Some(stt.transcript.clone());
                                info!(
                                    channel = "telegram",
                                    media_type = "audio",
                                    stage = "stt",
                                    latency_ms = started.elapsed().as_millis() as u64,
                                    provider = %stt.provider,
                                    confidence = ?stt.confidence,
                                    "Audio media transcribed"
                                );
                            } else if let Some(hint) = self.stt_backend.availability_hint() {
                                *transcript = Some(hint);
                            }
                        }
                    }
                }
            }
            MessageContent::Video {
                url, transcript, ..
            } => {
                if let Some(file_url) = self.resolve_telegram_file_url(url).await {
                    *url = file_url.clone();
                    if transcript.is_none() {
                        if let Some(bytes) = self.download_telegram_file(&file_url).await {
                            let started = std::time::Instant::now();
                            if let Some(stt) = self
                                .stt_backend
                                .transcribe(&bytes, "mp4", Some("video/mp4"))
                                .await
                            {
                                *transcript = Some(stt.transcript.clone());
                                info!(
                                    channel = "telegram",
                                    media_type = "video",
                                    stage = "stt",
                                    latency_ms = started.elapsed().as_millis() as u64,
                                    provider = %stt.provider,
                                    confidence = ?stt.confidence,
                                    "Video audio transcribed"
                                );
                            } else if let Some(hint) = self.stt_backend.availability_hint() {
                                *transcript = Some(hint);
                            }
                        }
                    }
                }
            }
            MessageContent::Document { url, .. } => {
                if let Some(file_url) = self.resolve_telegram_file_url(url).await {
                    *url = file_url;
                }
            }
            MessageContent::Text(_) | MessageContent::Location { .. } => {}
        }
    }
}

/// Process one Telegram update (webhook payload or one getUpdates result item) and send reply.
async fn process_one_telegram_update(state: &TelegramState, update_json: &str) {
    let provider_meta: serde_json::Value =
        serde_json::from_str(update_json).unwrap_or(serde_json::Value::Null);
    let provider_update_id = provider_meta.get("update_id").and_then(|v| v.as_u64());
    let provider_message_id = provider_meta
        .get("message")
        .and_then(|v| v.get("message_id"))
        .and_then(|v| v.as_u64())
        .or_else(|| {
            provider_meta
                .get("callback_query")
                .and_then(|v| v.get("message"))
                .and_then(|v| v.get("message_id"))
                .and_then(|v| v.as_u64())
        });
    debug!(
        channel = "telegram",
        stage = "ingress_received",
        update_len = update_json.len(),
        provider_update_id = ?provider_update_id,
        provider_msg_id = ?provider_message_id,
        "Telegram update received"
    );
    if state
        .is_duplicate_event(provider_update_id, provider_message_id)
        .await
    {
        debug!(
            channel = "telegram",
            stage = "dedupe_drop",
            provider_update_id = ?provider_update_id,
            provider_msg_id = ?provider_message_id,
            "Dropping duplicate Telegram update"
        );
        return;
    }
    let (mut req, chat_id) = match TelegramNormalizer::normalize_with_chat_id(update_json) {
        Ok(r) => r,
        Err(e) => {
            debug!(error = %e, "Skip update (parse error)");
            return;
        }
    };
    apply_session_scope_policy(&mut req, &state.config);
    state.enrich_telegram_media_request(&mut req).await;
    let text = req.content.as_text().unwrap_or_default().to_string();
    let request_text = request_text_from_content(&req.content);
    let session_id = req.session_id;
    debug!(
        chat_id,
        session_id = %uuid::Uuid::from_bytes(session_id),
        user_id = %req.user_id,
        request_text = %request_text,
        "Telegram: normalized to AgentRequest"
    );

    // Handle shared control commands through the common router first.
    if !text.is_empty() {
        if let Some(output) = handle_shared_control_command(
            &req,
            &state.config,
            state.agent_store.as_ref(),
            state.session_memory.as_ref(),
        ) {
            state
                .send_telegram_secure_message(
                    chat_id,
                    &output.text,
                    output.reply_markup,
                    output.parse_mode,
                )
                .await;
            return;
        }
    }

    if !text.is_empty() {
        if let Some(output) = handle_runtime_control_command(
            &req,
            &state.config,
            state.session_memory.as_ref(),
            Some(state.session_steering_tx.as_ref()),
        )
        .await
        {
            state
                .send_telegram_secure_message(
                    chat_id,
                    &output.text,
                    output.reply_markup,
                    output.parse_mode,
                )
                .await;
            return;
        }
    }

    if !text.is_empty()
        && (aria_core::parse_control_intent(&text, req.channel).is_some()
            || text.starts_with("/install_skill"))
    {
        let session_uuid = uuid::Uuid::from_bytes(session_id);
        let (_current_agent, _current_model) = get_effective_session_overrides(
            state.session_memory.as_ref(),
            session_id,
            req.channel,
            &req.user_id,
        )
        .unwrap_or((None, None));

        let mut parse_mode = None;
        let mut reply_markup = None;
        let reply = if text.starts_with("/install_skill") {
            let json_part = text
                .strip_prefix("/install_skill")
                .unwrap_or_default()
                .trim();
            if json_part.is_empty() {
                "Usage: /install_skill <SignedModule JSON>".to_string()
            } else {
                match serde_json::from_str::<aria_skill_runtime::SignedModule>(json_part) {
                    Ok(signed) => {
                        if let Err(e) = aria_skill_runtime::verify_module(&signed) {
                            format!("Verification failed: {}", e)
                        } else {
                            let hash = aria_skill_runtime::wasm_module_hash(&signed.bytes);
                            let hex_hash = hex::encode(&hash[..8]);
                            let target = format!("./tools/{}.wasm", hex_hash);
                            if let Err(e) = std::fs::write(&target, &signed.bytes) {
                                format!("Failed to save tool: {}", e)
                            } else {
                                format!("Skill installed successfully as '{}'.", target)
                            }
                        }
                    }
                    Err(e) => format!("Invalid SignedModule JSON: {}", e),
                }
            }
        } else if text.starts_with("/runs") {
            let store = RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
            match store.list_agent_runs_for_session(session_uuid) {
                Ok(runs) => {
                    if runs.is_empty() {
                        "No sub-agent runs found for this session.".to_string()
                    } else {
                        let mut lines = vec!["Sub-agent runs for this session:".to_string()];
                        for run in runs {
                            lines.push(format!(
                                "• {} [{}] agent={} created_at={}",
                                run.run_id,
                                serde_json::to_string(&run.status)
                                    .unwrap_or_else(|_| "\"unknown\"".into())
                                    .replace('"', ""),
                                run.agent_id,
                                run.created_at_us
                            ));
                        }
                        lines.join("\n")
                    }
                }
                Err(e) => format!("Failed to list runs: {}", e),
            }
        } else if text.starts_with("/run ") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 2 {
                "Usage: /run <run_id>".to_string()
            } else {
                let store =
                    RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                match store.read_agent_run(parts[1]) {
                    Ok(run) => format!(
                        "Run {}\nstatus={}\nagent={}\nrequested_by={}\ncreated_at={}\nstarted_at={:?}\nfinished_at={:?}\nresult={}",
                        run.run_id,
                        serde_json::to_string(&run.status)
                            .unwrap_or_else(|_| "\"unknown\"".into())
                            .replace('"', ""),
                        run.agent_id,
                        run.requested_by_agent.unwrap_or_else(|| "user".into()),
                        run.created_at_us,
                        run.started_at_us,
                        run.finished_at_us,
                        run.result
                            .and_then(|r| r.response_summary.or(r.error))
                            .unwrap_or_else(|| "none".into())
                    ),
                    Err(e) => format!("Run lookup failed: {}", e),
                }
            }
        } else if text.starts_with("/run_events ") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 2 {
                "Usage: /run_events <run_id>".to_string()
            } else {
                let store =
                    RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                match store.list_agent_run_events(parts[1]) {
                    Ok(events) => {
                        if events.is_empty() {
                            format!("No events for run '{}'.", parts[1])
                        } else {
                            let mut lines = vec![format!("Events for run {}:", parts[1])];
                            for event in events {
                                lines.push(format!(
                                    "• {} [{}] {}",
                                    event.event_id,
                                    serde_json::to_string(&event.kind)
                                        .unwrap_or_else(|_| "\"unknown\"".into())
                                        .replace('"', ""),
                                    event.summary
                                ));
                            }
                            lines.join("\n")
                        }
                    }
                    Err(e) => format!("Failed to list run events: {}", e),
                }
            }
        } else if text.starts_with("/run_cancel ") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 2 {
                "Usage: /run_cancel <run_id>".to_string()
            } else {
                let store =
                    RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                let now_us = chrono::Utc::now().timestamp_micros() as u64;
                match store.cancel_agent_run(parts[1], "cancelled by user command", now_us) {
                    Ok(Some(run)) => format!("Run '{}' is now {:?}.", run.run_id, run.status),
                    Ok(None) => format!("Run '{}' not found.", parts[1]),
                    Err(e) => format!("Failed to cancel run: {}", e),
                }
            }
        } else if text.starts_with("/run_retry ") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 2 {
                "Usage: /run_retry <run_id>".to_string()
            } else {
                let store =
                    RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                match store.read_agent_run(parts[1]) {
                    Ok(original) => {
                        let now_us = chrono::Utc::now().timestamp_micros() as u64;
                        let new_run_id = format!("run-{}", uuid::Uuid::new_v4());
                        let retried = AgentRunRecord {
                            run_id: new_run_id.clone(),
                            parent_run_id: original
                                .parent_run_id
                                .clone()
                                .or_else(|| Some(original.run_id.clone())),
                            session_id: original.session_id,
                            user_id: original.user_id.clone(),
                            requested_by_agent: original.requested_by_agent.clone(),
                            agent_id: original.agent_id.clone(),
                            status: AgentRunStatus::Queued,
                            request_text: original.request_text.clone(),
                            inbox_on_completion: original.inbox_on_completion,
                            max_runtime_seconds: original.max_runtime_seconds,
                            created_at_us: now_us,
                            started_at_us: None,
                            finished_at_us: None,
                            result: None,
                        };
                        if let Err(e) = store.upsert_agent_run(&retried, now_us) {
                            format!("Failed to queue retry run: {}", e)
                        } else if let Err(e) = store.append_agent_run_event(&AgentRunEvent {
                            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
                            run_id: retried.run_id.clone(),
                            kind: AgentRunEventKind::Queued,
                            summary: format!("Run retried from '{}'", original.run_id),
                            created_at_us: now_us,
                        }) {
                            format!("Retry run queued but event write failed: {}", e)
                        } else {
                            format!(
                                "Retry queued: new run '{}' created from '{}'.",
                                retried.run_id, original.run_id
                            )
                        }
                    }
                    Err(e) => format!("Retry lookup failed: {}", e),
                }
            }
        } else if text.starts_with("/models") || text.starts_with("/model") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 1 || (parts.len() == 2 && parts[1] == "providers") {
                // Step 1: List Providers
                let mut lines = Vec::new();
                lines.push("🌐 <b>Select LLM Provider:</b>".to_string());
                let mut keyboard = Vec::new();
                let reg = state.provider_registry.lock().await;
                let store =
                    RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                for descriptor in reg.provider_descriptors(chrono::Utc::now().timestamp_micros() as u64) {
                    let capability = store.read_provider_capability(&descriptor.provider_id).ok();
                    let capability_note = capability
                        .map(|profile| {
                            format!(
                                " [{} | probe={:?}]",
                                serde_json::to_string(&profile.adapter_family)
                                    .unwrap_or_else(|_| "\"unknown\"".into())
                                    .replace('"', ""),
                                profile.supports_runtime_probe
                            )
                        })
                        .unwrap_or_else(|| {
                            format!(
                                " [{} | probe={:?}]",
                                serde_json::to_string(&descriptor.adapter_family)
                                    .unwrap_or_else(|_| "\"unknown\"".into())
                                    .replace('"', ""),
                                descriptor.supports_runtime_probe
                            )
                        });
                    lines.push(format!("- {}{}", descriptor.name, capability_note));
                    keyboard.push(vec![serde_json::json!({
                        "text": descriptor.name,
                        "callback_data": format!("/models p {}", descriptor.provider_id)
                    })]);
                }
                reply_markup = Some(serde_json::json!({ "inline_keyboard": keyboard }));
                parse_mode = Some("HTML");
                lines.join("\n")
            } else if parts.len() >= 3 && (parts[1] == "provider" || parts[1] == "p") {
                // Step 2: List Models for selected provider with pagination
                let provider_id = parts[2];
                let offset: usize = parts.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);
                let page_size = 20;

                let reg = state.provider_registry.lock().await;
                if let Some(p) = reg.get_provider(provider_id) {
                    let mut lines = Vec::new();
                    let models = p.list_models().await.unwrap_or_default();
                    let store =
                        RuntimeStore::for_sessions_dir(Path::new(&state.config.ssmu.sessions_dir));
                    let cached_capabilities = store
                        .list_model_capabilities_for_provider(provider_id)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|profile| (profile.model_ref.model_id.clone(), profile))
                        .collect::<std::collections::HashMap<_, _>>();
                    let total = models.len();
                    let end = (offset + page_size).min(total);

                    lines.push(format!(
                        "🤖 <b>Models for {} ({}-{} of {}):</b>",
                        p.name(),
                        offset + 1,
                        end,
                        total
                    ));

                    let mut keyboard = Vec::new();
                    for m in models.iter().skip(offset).take(page_size) {
                        if let Some(profile) = cached_capabilities.get(&m.id) {
                            lines.push(format!(
                                "- {} [tools={:?}, stream={:?}, vision={:?}]",
                                m.id, profile.tool_calling, profile.streaming, profile.vision
                            ));
                        } else {
                            lines.push(format!("- {} [unscanned]", m.id));
                        }
                        keyboard.push(vec![serde_json::json!({
                            "text": m.name,
                            "callback_data": format!("/models i {} {}", provider_id, m.id)
                        })]);
                    }

                    // Navigation row
                    let mut nav_row = Vec::new();
                    if offset >= page_size {
                        nav_row.push(serde_json::json!({
                            "text": "⬅️ Prev",
                            "callback_data": format!("/models p {} {}", provider_id, offset - page_size)
                        }));
                    }
                    if end < total {
                        nav_row.push(serde_json::json!({
                            "text": "Next ➡️",
                            "callback_data": format!("/models p {} {}", provider_id, end)
                        }));
                    }
                    if !nav_row.is_empty() {
                        keyboard.push(nav_row);
                    }

                    // Back to providers
                    keyboard.push(vec![serde_json::json!({
                        "text": "🌐 Back to Providers",
                        "callback_data": "/models providers"
                    })]);

                    reply_markup = Some(serde_json::json!({ "inline_keyboard": keyboard }));
                    parse_mode = Some("HTML");
                    lines.join("\n")
                } else {
                    format!("Unknown provider '{}'.", provider_id)
                }
            } else if parts.len() >= 3 && parts[1] == "scan" {
                let provider_id = parts[2];
                let reg = state.provider_registry.lock().await;
                if let Some(provider) = reg.get_provider(provider_id) {
                    let models = provider.list_models().await.unwrap_or_default();
                    let model_ids = models
                        .iter()
                        .take(10)
                        .map(|model| model.id.clone())
                        .collect::<Vec<_>>();
                    let now_us = chrono::Utc::now().timestamp_micros() as u64;
                    let probe_batch = reg
                        .probe_provider_models(provider_id, &model_ids, now_us)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|probe| (probe.model_ref.model_id.clone(), probe))
                        .collect::<std::collections::HashMap<_, _>>();
                    drop(reg);
                    let mut lines = vec![format!("Capability scan for provider '{}':", provider_id)];
                    for model in models.iter().take(10) {
                        if let Some(profile) = resolve_model_capability_profile(
                            &state.provider_registry,
                            Path::new(&state.config.ssmu.sessions_dir),
                            Some(&state.config.llm),
                            provider_id,
                            &model.id,
                            now_us,
                        )
                        .await
                        {
                            let probe_note = probe_batch
                                .get(&model.id)
                                .map(|probe| {
                                    format!(
                                        " method={} status={}",
                                        probe.probe_method
                                            .as_deref()
                                            .unwrap_or("unknown"),
                                        probe.probe_status
                                            .as_deref()
                                            .unwrap_or("unknown")
                                    )
                                })
                                .unwrap_or_default();
                            lines.push(format!(
                                "- {}: tools={:?}, streaming={:?}, vision={:?}, source={:?}{}",
                                model.id,
                                profile.tool_calling,
                                profile.streaming,
                                profile.vision,
                                profile.source,
                                probe_note
                            ));
                        }
                    }
                    lines.join("\n")
                } else {
                    format!("Unknown provider '{}'.", provider_id)
                }
            } else if parts.len() >= 4 && (parts[1] == "switch" || parts[1] == "i") {
                // Step 3: Switch Model
                let provider_id = parts[2];
                let model_id = text
                    .split_whitespace()
                    .skip(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                let now_us = chrono::Utc::now().timestamp_micros() as u64;
                let profile = resolve_model_capability_profile(
                    &state.provider_registry,
                    Path::new(&state.config.ssmu.sessions_dir),
                    Some(&state.config.llm),
                    provider_id,
                    &model_id,
                    now_us,
                )
                .await;
                let reg = state.provider_registry.lock().await;
                let resp = match profile
                    .as_ref()
                    .map(|profile| reg.create_backend_with_profile(profile))
                    .unwrap_or_else(|| reg.create_backend(provider_id, &model_id))
                {
                    Ok(backend) => {
                        state.llm_pool.register_backend("primary", backend.clone());
                        state.llm_pool.register_backend("fallback", backend);

                        // Per-session persistence
                        let combined_model = format!("{}:{}", provider_id, model_id);
                        let _ = persist_session_overrides(
                            state.session_memory.as_ref(),
                            req.session_id,
                            req.channel,
                            &req.user_id,
                            None,
                            Some(combined_model.clone()),
                        );
                        record_learning_reward(
                            &state.config.learning,
                            Path::new(&state.config.ssmu.sessions_dir),
                            req.request_id,
                            req.session_id,
                            RewardKind::OverrideApplied,
                            Some(format!("model override set to {}", combined_model)),
                            req.timestamp_us,
                        );

                        format!(
                            "✅ Switched to <b>{}</b> via <b>{}</b>.",
                            model_id, provider_id
                        )
                    }
                    Err(e) => format!("❌ Failed to switch: {}", e),
                };
                parse_mode = Some("HTML");
                resp
            } else {
                "Usage: /models [providers|p <id>|scan <provider>|i <provider> <model>]".to_string()
            }
        } else if text.starts_with("/timezone") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 1 {
                let current_tz = resolve_request_timezone_with_overrides(
                    &state.config,
                    &req.user_id,
                    Some(state.user_timezone_overrides.as_ref()),
                );
                let default_tz = state.config.localization.default_timezone.clone();
                let is_override = state.user_timezone_overrides.contains_key(&req.user_id)
                    || state
                        .config
                        .localization
                        .user_timezones
                        .contains_key(&req.user_id);
                format!(
                    "Current timezone: {}{}\nDefault timezone: {}\n\nUsage:\n/timezone <IANA_TZ> (example: /timezone Asia/Kolkata)\n/timezone clear",
                    current_tz.name(),
                    if is_override { " (user override)" } else { "" },
                    default_tz
                )
            } else {
                let raw_arg = parts[1..].join(" ").trim().to_string();
                let normalized_arg = raw_arg.replace(' ', "_");
                if normalized_arg.eq_ignore_ascii_case("clear")
                    || normalized_arg.eq_ignore_ascii_case("reset")
                    || normalized_arg.eq_ignore_ascii_case("default")
                {
                    state.user_timezone_overrides.remove(&req.user_id);
                    let _guard = state.runtime_config_lock.lock().await;
                    match persist_user_timezone_override(
                        state.runtime_config_path.as_path(),
                        &state.config,
                        &req.user_id,
                        None,
                    ) {
                        Ok(_) => format!(
                            "Timezone override cleared. Using default timezone: {}",
                            state.config.localization.default_timezone
                        ),
                        Err(e) => format!(
                            "Timezone override cleared in memory, but failed to persist: {}",
                            e
                        ),
                    }
                } else {
                    match normalized_arg.parse::<chrono_tz::Tz>() {
                        Ok(tz) => {
                            let tz_name = tz.name().to_string();
                            state
                                .user_timezone_overrides
                                .insert(req.user_id.clone(), tz_name.clone());
                            let _guard = state.runtime_config_lock.lock().await;
                            match persist_user_timezone_override(
                                state.runtime_config_path.as_path(),
                                &state.config,
                                &req.user_id,
                                Some(&tz_name),
                            ) {
                                Ok(_) => {
                                    record_learning_reward(
                                        &state.config.learning,
                                        Path::new(&state.config.ssmu.sessions_dir),
                                        req.request_id,
                                        req.session_id,
                                        RewardKind::OverrideApplied,
                                        Some(format!("timezone override set to {}", tz_name)),
                                        req.timestamp_us,
                                    );
                                    format!("Timezone set to {} for this user.", tz_name)
                                }
                                Err(e) => format!(
                                    "Timezone set to {} in memory, but failed to persist: {}",
                                    tz_name, e
                                ),
                            }
                        }
                        Err(_) => "Invalid timezone. Use IANA names like `Asia/Kolkata`, `Europe/Zurich`, `America/New_York`.".to_string(),
                    }
                }
            }
        } else if text.starts_with("/stop") {
            let session_id_str = session_uuid.to_string();
            if let Some(tx) = state.session_steering_tx.get(&session_id_str) {
                let _ = tx.send(aria_intelligence::SteeringCommand::Abort).await;
                "Signal sent: aborting current operation.".to_string()
            } else {
                "No active operation to stop.".to_string()
            }
        } else if matches!(
            aria_core::parse_control_intent(&text, req.channel),
            Some(aria_core::ControlIntent::ResolveApproval { .. })
        ) {
            let parsed = aria_core::parse_control_intent(&text, req.channel);
            if let Some(aria_core::ControlIntent::ResolveApproval {
                decision,
                target: Some(target),
                tool_hint: _,
            }) = parsed
            {
                let approving = matches!(decision, aria_core::ApprovalResolutionDecision::Approve);
                let sessions_dir = Path::new(&state.config.ssmu.sessions_dir);
                let approval_id = match resolve_approval_selector(
                    sessions_dir,
                    req.session_id,
                    &req.user_id,
                    &target,
                ) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = state.send_telegram_agent_reply(chat_id, &err).await;
                        return;
                    }
                };
                let tool_name = "approval".to_string();
                let approved_tool_name = tool_name.clone();

                let mut reply_msg = if approving {
                    format!("✅ Tool '{}' approved. Executing...", tool_name)
                } else {
                    format!("❌ Tool '{}' denied. Resuming...", tool_name)
                };
                record_learning_reward(
                    &state.config.learning,
                    Path::new(&state.config.ssmu.sessions_dir),
                    req.request_id,
                    req.session_id,
                    if approving {
                        RewardKind::Accepted
                    } else {
                        RewardKind::Rejected
                    },
                    Some(format!("approval callback for tool {}", tool_name)),
                    req.timestamp_us,
                );

                if let Ok(record) = resolve_approval_record(sessions_dir, &approval_id, decision) {
                    let original_request = record.original_request.clone();
                    if record.tool_name == AGENT_ELEVATION_TOOL_NAME {
                            let requested_agent =
                                serde_json::from_str::<serde_json::Value>(&record.arguments_json)
                                    .ok()
                                    .and_then(|value| {
                                        value
                                            .get("agent_id")
                                            .and_then(|v| v.as_str())
                                            .map(str::to_string)
                                    })
                                    .unwrap_or_else(|| record.agent_id.clone());

                            if approving {
                                let now_us = chrono::Utc::now().timestamp_micros() as u64;
                                let grant = aria_core::ElevationGrant {
                                    session_id,
                                    user_id: req.user_id.clone(),
                                    agent_id: requested_agent.clone(),
                                    granted_at_us: now_us,
                                    expires_at_us: Some(now_us + 3_600_000_000),
                                };
                                let _ = write_elevation_grant(sessions_dir, &grant);
                                let _ = persist_session_overrides(
                                    state.session_memory.as_ref(),
                                    req.session_id,
                                    req.channel,
                                    &req.user_id,
                                    Some(requested_agent.clone()),
                                    None,
                                );

                                let mut resume_req = req.clone();
                                resume_req.content =
                                    aria_core::MessageContent::Text(original_request);
                                let state_clone = state.clone();
                                tokio::spawn(async move {
                                    match process_request(
                                        &resume_req,
                                        &state_clone.config.learning,
                                        state_clone.router_index.as_ref(),
                                        state_clone.embedder.as_ref(),
                                        &state_clone.llm_pool,
                                        &state_clone.cedar,
                                        state_clone.agent_store.as_ref(),
                                        state_clone.tool_registry.as_ref(),
                                        state_clone.session_memory.as_ref(),
                                        &state_clone.capability_index,
                                        &state_clone.vector_store,
                                        &state_clone.keyword_index,
                                        &state_clone.firewall,
                                        &state_clone.vault,
                                        &state_clone.tx_cron,
                                        &state_clone.provider_registry,
                                        state_clone.session_tool_caches.as_ref(),
                                        state_clone.hooks.as_ref(),
                                        &state_clone.session_locks,
                                        &state_clone.embed_semaphore,
                                        state_clone.config.llm.max_tool_rounds,
                                        None,
                                        Some(&state_clone.global_estop),
                                        std::path::Path::new(&state_clone.config.ssmu.sessions_dir),
                                        state_clone.config.policy.whitelist.clone(),
                                        state_clone.config.policy.forbid.clone(),
                                        resolve_request_timezone_with_overrides(
                                            &state_clone.config,
                                            &resume_req.user_id,
                                            Some(state_clone.user_timezone_overrides.as_ref()),
                                        ),
                                    )
                                    .await
                                    {
                                        Ok(aria_intelligence::OrchestratorResult::Completed(text)) => {
                                            state_clone.send_telegram_agent_reply(chat_id, &text).await;
                                        }
                                        Ok(aria_intelligence::OrchestratorResult::AgentElevationRequired { message, .. }) => {
                                            state_clone.send_telegram_agent_reply(chat_id, &message).await;
                                        }
                                        Ok(_) => {
                                            state_clone.send_telegram_agent_reply(chat_id, "Approved elevation, but a further approval is required.").await;
                                        }
                                        Err(e) => {
                                            state_clone.send_telegram_agent_reply(chat_id, &format!("Post-elevation request failed: {}", e)).await;
                                        }
                                    }
                                });
                                return;
                            }

                            let _ = state
                                .send_telegram_agent_reply(
                                    chat_id,
                                    &format!(
                                        "❌ Elevation denied for agent '{}'.",
                                        requested_agent
                                    ),
                                )
                                .await;
                            return;
                        }

                        // We will execute the tool here if approved, or return the deny message
                        let tool_result = if approving {
                            let args_str = record.arguments_json.clone();

                            // Instantiate the native multiplex executor
                            // (We could pass agent_id but the native tools don't map to dynamic vaults yet)
                            let executor = MultiplexToolExecutor::new(
                                state.vault.clone(),
                                "developer".to_string(),
                                *session_uuid.as_bytes(),
                                req.user_id.clone(),
                                req.channel,
                                state.tx_cron.clone(),
                                state.session_memory.as_ref().clone(),
                                state.cedar.clone(),
                                Path::new(&state.config.ssmu.sessions_dir).to_path_buf(),
                                None,
                                None,
                                resolve_request_timezone_with_overrides(
                                    &state.config,
                                    &req.user_id,
                                    Some(state.user_timezone_overrides.as_ref()),
                                ),
                            );
                            let call = aria_intelligence::ToolCall { invocation_id: None,
                                name: approved_tool_name.clone(),
                                arguments: args_str,
                            };

                            match executor.execute(&call).await {
                                Ok(res) => res.render_for_prompt().to_string(),
                                Err(e) => format!("Tool execution failed: {}", e),
                            }
                        } else {
                            ToolExecutionResult::text("Execution denied by user.")
                                .render_for_prompt()
                                .to_string()
                        };

                        // Since we just need to get the LLM to reply, we can craft a fresh AgentRequest
                        // containing the tool result block, acting as if the user sent it, but flag it
                        let mut resume_req = req.clone();
                        let mut resume_payload = String::new();
                        if !original_request.trim().is_empty() {
                            resume_payload.push_str(&original_request);
                            resume_payload.push_str("\n\n");
                        }
                        resume_payload.push_str(&format!(
                            "<TOOL_RESUME_BLOCK>\nTool '{}' completed with output:\n{}</TOOL_RESUME_BLOCK>",
                            approved_tool_name, tool_result
                        ));
                        resume_req.content = aria_core::MessageContent::Text(resume_payload);

                        // Spawn a new processor task with the resume request.
                        // If the model asks for the same sensitive tool again immediately after resume,
                        // auto-execute it and continue, instead of surfacing an internal loop artifact.
                        let state_clone = state.clone();
                        let approved_tool_name = approved_tool_name.clone();
                        let original_request_for_resume = original_request.clone();
                        tokio::spawn(async move {
                                let mut current_req = resume_req;
                                let mut auto_resume_hops = 0usize;
                                const MAX_AUTO_RESUME_HOPS: usize = 3;

                                loop {
                                let run_result = process_request(
                                    &current_req,
                                    &state_clone.config.learning,
                                    state_clone.router_index.as_ref(),
                                    state_clone.embedder.as_ref(),
                                    &state_clone.llm_pool,
                                    &state_clone.cedar,
                                    state_clone.agent_store.as_ref(),
                                    state_clone.tool_registry.as_ref(),
                                    state_clone.session_memory.as_ref(),
                                    &state_clone.capability_index,
                                    &state_clone.vector_store,
                                    &state_clone.keyword_index,
                                    &state_clone.firewall,
                                    &state_clone.vault,
                                    &state_clone.tx_cron,
                                    &state_clone.provider_registry,
                                    state_clone.session_tool_caches.as_ref(),
                                    state_clone.hooks.as_ref(),
                                    &state_clone.session_locks,
                                    &state_clone.embed_semaphore,
                                    state_clone.config.llm.max_tool_rounds,
                                    None,
                                    Some(&state_clone.global_estop),
                                    std::path::Path::new(&state_clone.config.ssmu.sessions_dir),
                                    state_clone.config.policy.whitelist.clone(),
                                    state_clone.config.policy.forbid.clone(),
                                    resolve_request_timezone_with_overrides(
                                        &state_clone.config,
                                        &current_req.user_id,
                                        Some(state_clone.user_timezone_overrides.as_ref()),
                                    ),
                                )
                                .await;

                                match run_result {
                                    Ok(aria_intelligence::OrchestratorResult::Completed(t)) => {
                                        let text = if t.is_empty() {
                                            "(no response)".to_string()
                                        } else {
                                            t
                                        };
                                        state_clone.send_telegram_agent_reply(chat_id, &text).await;
                                        break;
                                    }
                                    Ok(aria_intelligence::OrchestratorResult::AgentElevationRequired {
                                        message,
                                        ..
                                    }) => {
                                        state_clone.send_telegram_agent_reply(chat_id, &message).await;
                                        break;
                                    }
                                    Ok(aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                                        call,
                                        ..
                                    }) if call.name == approved_tool_name
                                        && auto_resume_hops < MAX_AUTO_RESUME_HOPS =>
                                    {
                                        auto_resume_hops += 1;
                                        debug!(
                                            hop = auto_resume_hops,
                                            tool = %call.name,
                                            "Auto-resuming repeated approval-required tool in /approve flow"
                                        );
                                        let executor = MultiplexToolExecutor::new(
                                            state_clone.vault.clone(),
                                            "developer".to_string(),
                                            current_req.session_id,
                                            current_req.user_id.clone(),
                                            current_req.channel,
                                            state_clone.tx_cron.clone(),
                                            state_clone.session_memory.as_ref().clone(),
                                            state_clone.cedar.clone(),
                                            Path::new(&state_clone.config.ssmu.sessions_dir).to_path_buf(),
                                            None,
                                            None,
                                            resolve_request_timezone_with_overrides(
                                                &state_clone.config,
                                                &current_req.user_id,
                                                Some(state_clone.user_timezone_overrides.as_ref()),
                                            ),
                                        );
                                        let output = match executor.execute(&call).await {
                                            Ok(out) => out.render_for_prompt().to_string(),
                                            Err(e) => format!("Tool execution failed: {}", e),
                                        };
                                        let mut resumed = String::new();
                                        if !original_request_for_resume.trim().is_empty() {
                                            resumed.push_str(&original_request_for_resume);
                                            resumed.push_str("\n\n");
                                        }
                                        resumed.push_str(&format!(
                                                "<TOOL_RESUME_BLOCK>\nTool '{}' completed with output:\n{}</TOOL_RESUME_BLOCK>",
                                                call.name, output
                                            ));
                                        current_req.content = aria_core::MessageContent::Text(resumed);
                                        continue;
                                    }
                                    Ok(aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                                        call,
                                        ..
                                    }) => {
                                        // Legitimate next approval (different tool or hop ceiling reached):
                                        // surface it cleanly instead of a generic loop error.
                                        state_clone
                                            .send_telegram_secure_message(
                                                chat_id,
                                                &format!(
                                                    "Another approval is required for tool `{}`. Please run the command again after approving that step.",
                                                    call.name
                                                ),
                                                None,
                                                None,
                                            )
                                            .await;
                                        break;
                                    }
                                    Err(e) => {
                                        error!(error = %e, "Orchestrator error in spawned task");
                                        let _ = state_clone
                                            .send_telegram_secure_message(
                                                chat_id,
                                                &format!(
                                                    "❌ <b>Orchestrator Error:</b>\n<pre>{}</pre>",
                                                    e
                                                ),
                                                None,
                                                Some("HTML"),
                                            )
                                            .await;
                                        break;
                                    }
                                }
                            }
                        });
                } else {
                    reply_msg = "Expired or invalid approval state (record missing).".to_string();
                }
                reply_msg
            } else {
                "Invalid approval callback.".to_string()
            }
        } else {
            "Unknown or malformed command.".to_string()
        };

        state
            .send_telegram_secure_message(chat_id, &reply, reply_markup, parse_mode)
            .await;
        return;
    }

    info!(chat_id = %chat_id, request_text = %request_text, "Processing request");

    let typing_url = format!(
        "https://api.telegram.org/bot{}/sendChatAction",
        state.telegram_token
    );
    let typing_body = serde_json::json!({ "chat_id": chat_id, "action": "typing" });
    let client_clone = state.client.clone();
    tokio::spawn(async move {
        let _ = client_clone
            .post(&typing_url)
            .json(&typing_body)
            .send()
            .await;
    });

    let (steering_tx, mut steering_rx) = tokio::sync::mpsc::channel(4);
    state
        .session_steering_tx
        .insert(uuid::Uuid::from_bytes(session_id).to_string(), steering_tx);

    let _response = match process_request(
        &req,
        &state.config.learning,
        state.router_index.as_ref(),
        state.embedder.as_ref(),
        &state.llm_pool,
        &state.cedar,
        state.agent_store.as_ref(),
        state.tool_registry.as_ref(),
        state.session_memory.as_ref(),
        &state.capability_index,
        &state.vector_store,
        &state.keyword_index,
        &state.firewall,
        &state.vault,
        &state.tx_cron,
        &state.provider_registry,
        state.session_tool_caches.as_ref(),
        state.hooks.as_ref(),
        &state.session_locks,
        &state.embed_semaphore,
        10, // max_rounds
        Some(&mut steering_rx),
        Some(&state.global_estop),
        std::path::Path::new(&state.config.ssmu.sessions_dir),
        state.config.policy.whitelist.clone(),
        state.config.policy.forbid.clone(),
        resolve_request_timezone_with_overrides(
            &state.config,
            &req.user_id,
            Some(state.user_timezone_overrides.as_ref()),
        ),
    )
    .await
    {
        Ok(res) => {
            match res {
                aria_intelligence::OrchestratorResult::Completed(text) => {
                    let text = if text.is_empty() {
                        "(no response)".to_string()
                    } else {
                        text
                    };
                    state.send_telegram_agent_reply(chat_id, &text).await;
                }
                aria_intelligence::OrchestratorResult::AgentElevationRequired {
                    agent_id,
                    message,
                } => {
                    let result = aria_intelligence::OrchestratorResult::AgentElevationRequired {
                        agent_id,
                        message,
                    };
                    match persist_pending_approval_for_result(
                        Path::new(&state.config.ssmu.sessions_dir),
                        &req,
                        &result,
                    ) {
                        Ok((approval_record, _)) => {
                            let handle = ensure_approval_handle(
                                Path::new(&state.config.ssmu.sessions_dir),
                                &approval_record,
                            )
                            .ok();
                            let rendered = render_approval_prompt_for_channel(
                                &approval_record,
                                handle.as_deref(),
                            );
                            state
                                .send_telegram_secure_message(
                                    chat_id,
                                    &rendered.text,
                                    rendered.reply_markup,
                                    rendered.parse_mode,
                                )
                                .await;
                        }
                        Err(err) => {
                            state.send_telegram_agent_reply(chat_id, &err).await;
                        }
                    }
                }
                aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                    call,
                    pending_prompt,
                } => {
                    let result = aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                        call,
                        pending_prompt,
                    };
                    match persist_pending_approval_for_result(
                        Path::new(&state.config.ssmu.sessions_dir),
                        &req,
                        &result,
                    ) {
                        Ok((approval_record, _)) => {
                            let handle = ensure_approval_handle(
                                Path::new(&state.config.ssmu.sessions_dir),
                                &approval_record,
                            )
                            .ok();
                            let rendered = render_approval_prompt_for_channel(
                                &approval_record,
                                handle.as_deref(),
                            );
                            state
                                .send_telegram_secure_message(
                                    chat_id,
                                    &rendered.text,
                                    rendered.reply_markup,
                                    rendered.parse_mode,
                                )
                                .await;
                        }
                        Err(err) => {
                            state.send_telegram_agent_reply(chat_id, &err).await;
                        }
                    }
                }
            }
        }
        Err(e) => {
            error!(error = %e, "Orchestrator error");
            let _ = state
                .send_telegram_secure_message(
                    chat_id,
                    &format!("❌ <b>Orchestrator Error:</b>\n<pre>{}</pre>", e),
                    None,
                    Some("HTML"),
                )
                .await;
        }
    };
    state.metrics.processed.fetch_add(1, Ordering::Relaxed);
}

async fn handle_telegram_webhook(state: TelegramState, body_str: &str) -> (StatusCode, String) {
    debug!(body_len = body_str.len(), "Webhook received");
    crate::channel_health::record_channel_health_event(
        GatewayChannel::Telegram,
        crate::channel_health::ChannelHealthEventKind::IngressEnqueued,
    );
    process_one_telegram_update(&state, body_str).await;
    crate::channel_health::record_channel_health_event(
        GatewayChannel::Telegram,
        crate::channel_health::ChannelHealthEventKind::IngressDequeued,
    );
    (StatusCode::OK, "OK".into())
}

async fn handle_compaction_state_inspect(
    State(state): State<TelegramState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_compaction_state_json(Path::new(&state.config.ssmu.sessions_dir), &session_id).map(Json)
}

async fn handle_control_documents_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ControlDocumentQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_control_documents_json(
        Path::new(&state.config.ssmu.sessions_dir),
        &query.workspace_root,
    )
    .map(Json)
}

async fn handle_provider_health_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_provider_health_json(&state.llm_pool).map(Json)
}

async fn handle_workspace_locks_inspect() -> Result<Json<serde_json::Value>, (StatusCode, String)>
{
    inspect_workspace_locks_json().map(Json)
}

async fn handle_retrieval_traces_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<RetrievalTraceQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_retrieval_traces_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_agent_runs_inspect(
    State(state): State<TelegramState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_agent_runs_json(Path::new(&state.config.ssmu.sessions_dir), &session_id).map(Json)
}

async fn handle_session_overview_inspect(
    State(state): State<TelegramState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_session_overview_json(Path::new(&state.config.ssmu.sessions_dir), &session_id).map(Json)
}

async fn handle_agent_run_events_inspect(
    State(state): State<TelegramState>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_agent_run_events_json(Path::new(&state.config.ssmu.sessions_dir), &run_id).map(Json)
}

async fn handle_agent_presence_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_agent_presence_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_agent_mailbox_inspect(
    State(state): State<TelegramState>,
    AxumPath(run_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_agent_mailbox_json(Path::new(&state.config.ssmu.sessions_dir), &run_id).map(Json)
}

async fn handle_skill_packages_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_skill_packages_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_skill_bindings_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<AgentQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_skill_bindings_json(Path::new(&state.config.ssmu.sessions_dir), &query.agent_id)
        .map(Json)
}

async fn handle_skill_activations_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<AgentQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_skill_activations_json(Path::new(&state.config.ssmu.sessions_dir), &query.agent_id)
        .map(Json)
}

async fn handle_skill_signatures_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<SkillSignatureQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_skill_signatures_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.skill_id.as_deref(),
    )
    .map(Json)
}

async fn handle_mcp_servers_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_mcp_servers_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_mcp_imports_inspect(
    State(state): State<TelegramState>,
    AxumPath(server_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_mcp_imports_json(Path::new(&state.config.ssmu.sessions_dir), &server_id).map(Json)
}

async fn handle_mcp_bindings_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<AgentQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_mcp_bindings_json(Path::new(&state.config.ssmu.sessions_dir), &query.agent_id).map(Json)
}

async fn handle_mcp_cache_inspect(
    State(state): State<TelegramState>,
    AxumPath(server_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_mcp_cache_json(Path::new(&state.config.ssmu.sessions_dir), &server_id).map(Json)
}

async fn handle_mcp_boundary_inspect() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_mcp_boundary_json().map(Json)
}

async fn handle_learning_metrics_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_learning_metrics_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_learning_derivatives_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<FingerprintQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_learning_derivatives_json(
        Path::new(&state.config.ssmu.sessions_dir),
        &query.fingerprint,
    )
    .map(Json)
}

async fn handle_learning_traces_inspect(
    State(state): State<TelegramState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_learning_traces_json(Path::new(&state.config.ssmu.sessions_dir), &session_id).map(Json)
}

async fn handle_scope_denials_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ScopeDenialQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_scope_denials_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.agent_id.as_deref(),
        query.session_id.as_deref(),
    )
    .map(Json)
}

async fn handle_shell_exec_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ShellExecAuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_shell_exec_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_request_policy_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<RequestPolicyAuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_request_policy_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_secret_usage_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<SecretUsageAuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_secret_usage_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_approvals_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ApprovalInspectQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_approvals_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.user_id.as_deref(),
        query.status.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_profiles_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_profiles_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_domain_access_decisions_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<DomainDecisionQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_domain_access_decisions_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.domain.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_crawl_jobs_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_crawl_jobs_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_website_memory_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<WebsiteMemoryQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_website_memory_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.domain.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_profile_bindings_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<BrowserProfileBindingQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_profile_bindings_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_sessions_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<BrowserSessionQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_sessions_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_artifacts_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<BrowserArtifactQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_artifacts_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.browser_session_id.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_action_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<BrowserSessionQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_action_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_browser_challenge_events_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<BrowserSessionQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_challenge_events_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_watch_jobs_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<WatchJobQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_watch_jobs_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_web_storage_policy_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_web_storage_policy_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_browser_bridge_inspect() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_bridge_json().map(Json)
}

async fn handle_browser_runtime_health_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_browser_runtime_health_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_repair_fallback_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<RepairFallbackAuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_repair_fallback_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_streaming_decision_audits_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<StreamingDecisionAuditQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_streaming_decision_audits_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.session_id.as_deref(),
        query.agent_id.as_deref(),
    )
    .map(Json)
}

async fn handle_streaming_activity_inspect(
    State(state): State<TelegramState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<StreamingActivityQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_streaming_activity_json(
        Path::new(&state.config.ssmu.sessions_dir),
        &session_id,
        query.request_id.as_deref(),
    )
    .map(Json)
}

async fn handle_streaming_metrics_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<StreamingMetricQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_streaming_metrics_json(
        Path::new(&state.config.ssmu.sessions_dir),
        query.provider_id.as_deref(),
        query.model_ref.as_deref(),
    )
    .map(Json)
}

async fn handle_provider_capabilities_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_provider_capabilities_json(Path::new(&state.config.ssmu.sessions_dir)).map(Json)
}

async fn handle_registered_providers_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_registered_providers_json(&state.provider_registry)
        .await
        .map(Json)
}

async fn handle_model_capabilities_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ModelCapabilityQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_model_capabilities_json(
        Path::new(&state.config.ssmu.sessions_dir),
        &query.provider_id,
        query.model_id.as_deref(),
    )
    .map(Json)
}

async fn handle_model_probes_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ModelCapabilityQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let model_id = query.model_id.ok_or((
        StatusCode::BAD_REQUEST,
        "model_id query parameter is required".to_string(),
    ))?;
    inspect_model_capability_probes_json(
        Path::new(&state.config.ssmu.sessions_dir),
        &query.provider_id,
        &model_id,
    )
    .map(Json)
}

async fn handle_model_capability_decision_inspect(
    State(state): State<TelegramState>,
    Query(query): Query<ModelCapabilityQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let model_id = query.model_id.ok_or((
        StatusCode::BAD_REQUEST,
        "model_id query parameter is required".to_string(),
    ))?;
    inspect_model_capability_decision_json(
        &state.config,
        Path::new(&state.config.ssmu.sessions_dir),
        &query.provider_id,
        &model_id,
    )
    .map(Json)
}

async fn handle_channel_health_inspect() -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_channel_health_json().map(Json)
}

async fn handle_channel_transport_inspect(
    State(state): State<TelegramState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    inspect_channel_transports_json(&state.config).map(Json)
}

async fn handle_websocket_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<WebSocketState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| async move {
        use futures_util::{SinkExt, StreamExt};
        let (mut sink, mut stream) = socket.split();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let mut last_recipient_id: Option<String> = None;
        let sender_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sink.send(AxumWsMessage::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });

        while let Some(msg) = stream.next().await {
            let Ok(msg) = msg else {
                break;
            };
            match msg {
                AxumWsMessage::Text(text) => {
                    let raw = text.to_string();
                    let mut req = match WebSocketNormalizer::normalize(&raw) {
                        Ok(req) => req,
                        Err(err) => {
                            let _ = tx.send(format!("Invalid websocket payload: {}", err));
                            continue;
                        }
                    };
                    apply_session_scope_policy(&mut req, &state.config);
                    let recipient_id = req.user_id.clone();
                    crate::outbound::register_websocket_recipient(recipient_id.clone(), tx.clone());
                    last_recipient_id = Some(recipient_id);
                    crate::channel_health::record_channel_health_event(
                        GatewayChannel::WebSocket,
                        crate::channel_health::ChannelHealthEventKind::IngressEnqueued,
                    );
                    process_cli_ingress_request(
                        &req,
                        &state.config,
                        &state.router_index,
                        state.embedder.as_ref(),
                        &state.llm_pool,
                        &state.cedar,
                        state.agent_store.as_ref(),
                        state.tool_registry.as_ref(),
                        state.session_memory.as_ref(),
                        &state.capability_index,
                        &state.vector_store,
                        &state.keyword_index,
                        &state.firewall,
                        &state.vault,
                        &state.tx_cron,
                        &state.provider_registry,
                        state.session_tool_caches.as_ref(),
                        state.hooks.as_ref(),
                        &state.session_locks,
                        &state.embed_semaphore,
                    )
                    .await;
                    crate::channel_health::record_channel_health_event(
                        GatewayChannel::WebSocket,
                        crate::channel_health::ChannelHealthEventKind::IngressDequeued,
                    );
                }
                AxumWsMessage::Close(_) => break,
                AxumWsMessage::Ping(_) | AxumWsMessage::Pong(_) | AxumWsMessage::Binary(_) => {}
            }
        }

        sender_task.abort();
        if let Some(recipient_id) = last_recipient_id.as_deref() {
            crate::outbound::unregister_websocket_recipient(recipient_id);
        }
    })
}

async fn handle_whatsapp_webhook(
    State(state): State<WebSocketState>,
    body: String,
) -> (StatusCode, String) {
    crate::channel_health::record_channel_health_event(
        GatewayChannel::WhatsApp,
        crate::channel_health::ChannelHealthEventKind::IngressEnqueued,
    );
    let mut req = match WhatsAppNormalizer::normalize(&body) {
        Ok(req) => req,
        Err(err) => return (StatusCode::BAD_REQUEST, format!("invalid whatsapp payload: {}", err)),
    };
    apply_session_scope_policy(&mut req, &state.config);
    process_cli_ingress_request(
        &req,
        &state.config,
        &state.router_index,
        state.embedder.as_ref(),
        &state.llm_pool,
        &state.cedar,
        state.agent_store.as_ref(),
        state.tool_registry.as_ref(),
        state.session_memory.as_ref(),
        &state.capability_index,
        &state.vector_store,
        &state.keyword_index,
        &state.firewall,
        &state.vault,
        &state.tx_cron,
        &state.provider_registry,
        state.session_tool_caches.as_ref(),
        state.hooks.as_ref(),
        &state.session_locks,
        &state.embed_semaphore,
    )
    .await;
    crate::channel_health::record_channel_health_event(
        GatewayChannel::WhatsApp,
        crate::channel_health::ChannelHealthEventKind::IngressDequeued,
    );
    (StatusCode::OK, "OK".into())
}

/// Check Telegram webhook status and log diagnostics.
async fn check_telegram_webhook(
    token: &str,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url = format!("https://api.telegram.org/bot{}/getWebhookInfo", token);
    let resp: serde_json::Value = reqwest::Client::new()
        .get(&url)
        .send()
        .await?
        .json()
        .await?;
    let ok = resp.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    if !ok {
        warn!("getWebhookInfo failed: {:?}", resp.get("description"));
        return Ok(());
    }
    let result = resp
        .get("result")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let webhook_url = result.get("url").and_then(|v| v.as_str()).unwrap_or("");
    let pending = result
        .get("pending_update_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let last_error = result.get("last_error_message").and_then(|v| v.as_str());
    if webhook_url.is_empty() {
        warn!(
            port = port,
            "Telegram webhook NOT SET — Telegram will NOT deliver messages. \
             Run: curl \"https://api.telegram.org/bot<TOKEN>/setWebhook?url=<YOUR_PUBLIC_HTTPS_URL>/webhook\" \
             (For local dev: ngrok http {})",
            port
        );
    } else {
        info!(url = %webhook_url, "Webhook configured");
        if pending > 0 {
            warn!(
                pending = pending,
                "Telegram has pending updates (delivery may be failing)"
            );
        }
        if let Some(err) = last_error {
            warn!(last_error = %err, "Telegram reported webhook delivery error");
        }
    }
    Ok(())
}

/// Long-polling Telegram gateway: no webhook, no public URL. Works locally and in production.
async fn run_telegram_polling(
    config: Arc<ResolvedAppConfig>,
    runtime_config_path: PathBuf,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    provider_registry: Arc<Mutex<ProviderRegistry>>,
    token: String,
) {
    let timezone_overrides = Arc::new(dashmap::DashMap::<String, String>::new());
    for (user_id, tz_name) in &config.localization.user_timezones {
        timezone_overrides.insert(user_id.clone(), tz_name.clone());
    }
    let telegram_client = reqwest::Client::new();
    let stt_backend = build_stt_backend(&config, telegram_client.clone());
    let state = TelegramState {
        config: config.clone(),
        runtime_config_path: Arc::new(runtime_config_path),
        runtime_config_lock: Arc::new(tokio::sync::Mutex::new(())),
        user_timezone_overrides: timezone_overrides,
        router_index: Arc::new(router_index),
        embedder: embedder.clone(),
        llm_pool,
        cedar,
        agent_store: Arc::new(agent_store),
        tool_registry: Arc::new(tool_registry),
        session_memory: Arc::new(session_memory),
        capability_index,
        vector_store,
        keyword_index,
        firewall,
        vault,
        tx_cron: tx_cron.clone(),
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        provider_registry,
        telegram_token: token.clone(),
        client: telegram_client,
        stt_backend,
        dedupe_window: Arc::new(tokio::sync::Mutex::new(DedupeWindow::default())),
        dedupe_last_prune_us: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        metrics: Arc::new(ChannelMetrics::default()),
        session_locks: Arc::new(dashmap::DashMap::new()),
        session_steering_tx: Arc::new(dashmap::DashMap::new()),
        embed_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
        global_estop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let base = format!("https://api.telegram.org/bot{}", token);
    let delete_url = format!("{}/deleteWebhook", base);
    if state.client.get(&delete_url).send().await.is_ok() {
        info!("Deleted webhook (using long polling)");
    }
    let mut offset: i64 = 0;
    const POLL_TIMEOUT: u64 = 30;
    info!("Telegram long polling started (getUpdates). Send a message to your bot.");
    let (update_tx, update_rx) =
        tokio::sync::mpsc::channel::<String>(TELEGRAM_UPDATE_QUEUE_CAPACITY);
    let shared_update_rx = Arc::new(tokio::sync::Mutex::new(update_rx));
    for worker_id in 0..TELEGRAM_WORKER_COUNT {
        let state_clone = state.clone();
        let rx_clone = Arc::clone(&shared_update_rx);
        tokio::spawn(async move {
            loop {
                let msg = {
                    let mut guard = rx_clone.lock().await;
                    guard.recv().await
                };
                let Some(update_json) = msg else {
                    break;
                };
                state_clone
                    .metrics
                    .queue_depth
                    .fetch_sub(1, Ordering::Relaxed);
                crate::channel_health::record_channel_health_event(
                    GatewayChannel::Telegram,
                    crate::channel_health::ChannelHealthEventKind::IngressDequeued,
                );
                process_one_telegram_update(&state_clone, &update_json).await;
            }
            debug!(
                channel = "telegram",
                worker = worker_id,
                "Polling worker exited"
            );
        });
    }

    // Build a shared shutdown signal that fires on Ctrl+C or SIGTERM.
    let shutdown = std::sync::Arc::new(tokio::sync::Notify::new());
    let shutdown_tx = std::sync::Arc::clone(&shutdown);
    tokio::spawn(async move {
        let ctrl_c = tokio::signal::ctrl_c();
        #[cfg(unix)]
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        #[cfg(unix)]
        tokio::select! {
            _ = ctrl_c => { info!("Polling: received Ctrl+C — shutting down"); }
            _ = sigterm.recv() => { info!("Polling: received SIGTERM — shutting down"); }
        }
        #[cfg(not(unix))]
        ctrl_c.await.ok();
        shutdown_tx.notify_waiters();
    });

    loop {
        // Check shutdown before making the next long-poll request
        let url = format!(
            "{}/getUpdates?offset={}&timeout={}",
            base, offset, POLL_TIMEOUT
        );

        // Race the poll request against the shutdown signal (short 3s timeout for responsiveness)
        let fetch = state
            .client
            .get(&url)
            .timeout(Duration::from_secs(POLL_TIMEOUT + 5))
            .send();

        let resp = tokio::select! {
            _ = shutdown.notified() => {
                info!("Polling loop: shutdown during request, exiting");
                break;
            }
            result = fetch => {
                match result {
                    Ok(r) => r,
                    Err(e) => {
                        warn!(error = %e, "getUpdates failed, retrying in 5s");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    }
                }
            }
        };

        let body: serde_json::Value = match resp.json().await {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "getUpdates body parse failed");
                continue;
            }
        };
        let ok = body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        if !ok {
            warn!(?body, "getUpdates returned ok: false");
            continue;
        }
        let empty: Vec<serde_json::Value> = vec![];
        let results = body
            .get("result")
            .and_then(|v| v.as_array())
            .unwrap_or(&empty);
        for update in results {
            if let Some(id) = update.get("update_id").and_then(|v| v.as_i64()) {
                offset = id + 1;
            }
            if let Ok(json_str) = serde_json::to_string(update) {
                // Answer callback queries so the inline button stops loading
                if let Some(cb_id) = update
                    .get("callback_query")
                    .and_then(|cb| cb.get("id"))
                    .and_then(|v| v.as_str())
                {
                    let answer_url = format!("{}/answerCallbackQuery", base);
                    let body = serde_json::json!({ "callback_query_id": cb_id });
                    let client_clone = state.client.clone();
                    tokio::spawn(async move {
                        let _ = client_clone.post(&answer_url).json(&body).send().await;
                    });
                }
                match update_tx.try_send(json_str) {
                    Ok(()) => {
                        state.metrics.enqueued.fetch_add(1, Ordering::Relaxed);
                        state.metrics.queue_depth.fetch_add(1, Ordering::Relaxed);
                        crate::channel_health::record_channel_health_event(
                            GatewayChannel::Telegram,
                            crate::channel_health::ChannelHealthEventKind::IngressEnqueued,
                        );
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Full(_)) => {
                        state.metrics.dropped.fetch_add(1, Ordering::Relaxed);
                        crate::channel_health::record_channel_health_event(
                            GatewayChannel::Telegram,
                            crate::channel_health::ChannelHealthEventKind::IngressDropped,
                        );
                        warn!(
                            channel = "telegram",
                            stage = "queue_backpressure",
                            "Polling update dropped because queue is full"
                        );
                    }
                    Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {
                        crate::channel_health::record_channel_health_event(
                            GatewayChannel::Telegram,
                            crate::channel_health::ChannelHealthEventKind::IngressDropped,
                        );
                        warn!("Polling worker queue closed; dropping update");
                    }
                }
            }
        }
        state.log_queue_metrics("polling_batch");
    }
    drop(update_tx);
    info!("Telegram long polling stopped.");
}

async fn setup_telegram_bot_commands(token: &str) {
    let url = format!("https://api.telegram.org/bot{}/setMyCommands", token);
    let body = serde_json::json!({
        "commands": [
            {"command": "agents", "description": "List available agents"},
            {"command": "agent", "description": "Switch to a specific agent by ID"},
            {"command": "runs", "description": "List sub-agent runs in this session"},
            {"command": "run", "description": "Inspect a specific run by ID"},
            {"command": "models", "description": "Manage LLM providers and models"},
            {"command": "timezone", "description": "Set or view your local timezone"},
            {"command": "session", "description": "Start a new session context"}
        ]
    });
    let client = reqwest::Client::new();
    if let Err(e) = client.post(&url).json(&body).send().await {
        warn!(error = %e, "Failed to set Telegram bot commands");
    } else {
        info!("Telegram bot commands configured for auto-suggest");
    }
}

async fn run_telegram_gateway(
    config: Arc<ResolvedAppConfig>,
    runtime_config_path: PathBuf,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    registry: Arc<Mutex<ProviderRegistry>>,
) {
    let token = match resolve_telegram_token(&config) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[aria-x] Fatal: {}", e);
            std::process::exit(1);
        }
    };
    setup_telegram_bot_commands(&token).await;
    let mode = config.gateway.telegram_mode.to_lowercase();
    if mode == "polling" || mode == "long_polling" {
        info!("Telegram mode: polling (no webhook, no ngrok)");
        run_telegram_polling(
            config,
            runtime_config_path,
            router_index,
            embedder,
            llm_pool,
            cedar,
            agent_store,
            tool_registry,
            session_memory,
            capability_index,
            vector_store,
            keyword_index,
            firewall,
            vault,
            tx_cron.clone(),
            session_tool_caches,
            registry,
            token,
        )
        .await;
        return;
    }
    let port = config.gateway.telegram_port;
    let timezone_overrides = Arc::new(dashmap::DashMap::<String, String>::new());
    for (user_id, tz_name) in &config.localization.user_timezones {
        timezone_overrides.insert(user_id.clone(), tz_name.clone());
    }
    let telegram_client = reqwest::Client::new();
    let stt_backend = build_stt_backend(&config, telegram_client.clone());
    let state = TelegramState {
        config,
        runtime_config_path: Arc::new(runtime_config_path),
        runtime_config_lock: Arc::new(tokio::sync::Mutex::new(())),
        user_timezone_overrides: timezone_overrides,
        router_index: Arc::new(router_index),
        embedder: embedder.clone(),
        llm_pool,
        cedar,
        agent_store: Arc::new(agent_store),
        tool_registry: Arc::new(tool_registry),
        session_memory: Arc::new(session_memory),
        capability_index,
        vector_store,
        keyword_index,
        firewall,
        vault, // Added vault here
        tx_cron: tx_cron.clone(),
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        provider_registry: registry.clone(),
        telegram_token: token.clone(),
        client: telegram_client,
        stt_backend,
        dedupe_window: Arc::new(tokio::sync::Mutex::new(DedupeWindow::default())),
        dedupe_last_prune_us: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        metrics: Arc::new(ChannelMetrics::default()),
        session_locks: Arc::new(dashmap::DashMap::new()),
        session_steering_tx: Arc::new(dashmap::DashMap::new()),
        embed_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
        global_estop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let shared_state = state.clone();
    let app = Router::new()
        .route(
            "/inspect/compaction/:session_id",
            get(handle_compaction_state_inspect),
        )
        .route(
            "/inspect/control-docs",
            get(handle_control_documents_inspect),
        )
        .route(
            "/inspect/provider-health",
            get(handle_provider_health_inspect),
        )
        .route(
            "/inspect/workspace-locks",
            get(handle_workspace_locks_inspect),
        )
        .route(
            "/inspect/retrieval-traces",
            get(handle_retrieval_traces_inspect),
        )
        .route(
            "/inspect/session-overview/:session_id",
            get(handle_session_overview_inspect),
        )
        .route("/inspect/agent-presence", get(handle_agent_presence_inspect))
        .route("/inspect/runs/:session_id", get(handle_agent_runs_inspect))
        .route(
            "/inspect/run-events/:run_id",
            get(handle_agent_run_events_inspect),
        )
        .route(
            "/inspect/mailbox/:run_id",
            get(handle_agent_mailbox_inspect),
        )
        .route("/inspect/skills", get(handle_skill_packages_inspect))
        .route(
            "/inspect/skill-bindings",
            get(handle_skill_bindings_inspect),
        )
        .route(
            "/inspect/skill-activations",
            get(handle_skill_activations_inspect),
        )
        .route(
            "/inspect/skill-signatures",
            get(handle_skill_signatures_inspect),
        )
        .route("/inspect/mcp/servers", get(handle_mcp_servers_inspect))
        .route(
            "/inspect/mcp/imports/:server_id",
            get(handle_mcp_imports_inspect),
        )
        .route("/inspect/mcp/bindings", get(handle_mcp_bindings_inspect))
        .route(
            "/inspect/mcp/cache/:server_id",
            get(handle_mcp_cache_inspect),
        )
        .route("/inspect/mcp/boundary", get(handle_mcp_boundary_inspect))
        .route(
            "/inspect/learning/metrics",
            get(handle_learning_metrics_inspect),
        )
        .route(
            "/inspect/learning/derivatives",
            get(handle_learning_derivatives_inspect),
        )
        .route(
            "/inspect/learning/traces/:session_id",
            get(handle_learning_traces_inspect),
        )
        .route("/inspect/scope-denials", get(handle_scope_denials_inspect))
        .route(
            "/inspect/secret-usage-audits",
            get(handle_secret_usage_audits_inspect),
        )
        .route(
            "/inspect/shell-exec-audits",
            get(handle_shell_exec_audits_inspect),
        )
        .route(
            "/inspect/request-policy-audits",
            get(handle_request_policy_audits_inspect),
        )
        .route("/inspect/approvals", get(handle_approvals_inspect))
        .route(
            "/inspect/browser-profiles",
            get(handle_browser_profiles_inspect),
        )
        .route(
            "/inspect/domain-access-decisions",
            get(handle_domain_access_decisions_inspect),
        )
        .route("/inspect/crawl-jobs", get(handle_crawl_jobs_inspect))
        .route("/inspect/website-memory", get(handle_website_memory_inspect))
        .route(
            "/inspect/browser-profile-bindings",
            get(handle_browser_profile_bindings_inspect),
        )
        .route("/inspect/browser-sessions", get(handle_browser_sessions_inspect))
        .route("/inspect/browser-artifacts", get(handle_browser_artifacts_inspect))
        .route(
            "/inspect/browser-action-audits",
            get(handle_browser_action_audits_inspect),
        )
        .route(
            "/inspect/browser-challenge-events",
            get(handle_browser_challenge_events_inspect),
        )
        .route("/inspect/watch-jobs", get(handle_watch_jobs_inspect))
        .route("/inspect/browser-bridge", get(handle_browser_bridge_inspect))
        .route(
            "/inspect/browser-runtime-health",
            get(handle_browser_runtime_health_inspect),
        )
        .route(
            "/inspect/web-storage-policy",
            get(handle_web_storage_policy_inspect),
        )
        .route(
            "/inspect/repair-fallback-audits",
            get(handle_repair_fallback_audits_inspect),
        )
        .route(
            "/inspect/streaming-decision-audits",
            get(handle_streaming_decision_audits_inspect),
        )
        .route(
            "/inspect/streaming-activity/:session_id",
            get(handle_streaming_activity_inspect),
        )
        .route(
            "/inspect/streaming-metrics",
            get(handle_streaming_metrics_inspect),
        )
        .route(
            "/inspect/provider-capabilities",
            get(handle_provider_capabilities_inspect),
        )
        .route(
            "/inspect/registered-providers",
            get(handle_registered_providers_inspect),
        )
        .route(
            "/inspect/model-capabilities",
            get(handle_model_capabilities_inspect),
        )
        .route(
            "/inspect/model-probes",
            get(handle_model_probes_inspect),
        )
        .route(
            "/inspect/model-capability-decision",
            get(handle_model_capability_decision_inspect),
        )
        .route("/inspect/channel-health", get(handle_channel_health_inspect))
        .route(
            "/inspect/channel-transports",
            get(handle_channel_transport_inspect),
        )
        .route(
            "/webhook",
            post({
                let state = shared_state.clone();
                move |req: axum::extract::Request| {
                    let state = state.clone();
                    async move {
                        // Item 5: Multi-tenant webhook auth.
                        // Telegram sends X-Telegram-Bot-Api-Secret-Token header when a
                        // webhook secret is configured. Reject any request that doesn't match.
                        let provided_secret = req
                            .headers()
                            .get("X-Telegram-Bot-Api-Secret-Token")
                            .and_then(|v| v.to_str().ok())
                            .map(str::to_string);

                        if let Some(secret) = &provided_secret {
                            if secret != &state.telegram_token {
                                crate::channel_health::record_channel_health_event(
                                    GatewayChannel::Telegram,
                                    crate::channel_health::ChannelHealthEventKind::AuthFailure,
                                );
                                tracing::warn!(
                                    "Webhook rejected: invalid X-Telegram-Bot-Api-Secret-Token"
                                );
                                return (StatusCode::FORBIDDEN, "Forbidden".to_string());
                            }
                        }
                        // If no header is present we fall through (Telegram doesn't set the
                        // header when webhook secrets are not configured; tolerate both modes).

                        use http_body_util::BodyExt;
                        let bytes = match req.into_body().collect().await {
                            Ok(c) => c.to_bytes(),
                            Err(e) => {
                                error!(error = %e, "Webhook body error");
                                return (StatusCode::BAD_REQUEST, "Invalid body".to_string());
                            }
                        };
                        let body_str = String::from_utf8_lossy(&bytes);
                        handle_telegram_webhook(state, &body_str).await
                    }
                }
            }),
        )
        .route(
            "/estop",
            post({
                let state = shared_state.clone();
                move || {
                    let state = state.clone();
                    async move {
                        state
                            .global_estop
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        info!("GLOBAL ESTOP TRIGGERED VIA HTTP TARGET /estop");
                        (
                            StatusCode::OK,
                            axum::body::Body::from("Emergency stop engaged"),
                        )
                    }
                }
            }),
        )
        .with_state(shared_state.clone());
    let bind_addr_str = state.config.gateway.bind_address.clone();
    info!(
        bind_addr = %bind_addr_str,
        port = port,
        "Telegram gateway listening (set bind_address=\"127.0.0.1\" for Tailscale Serve/Funnel mode)"
    );
    // Diagnose webhook status at startup
    if let Err(e) = check_telegram_webhook(&token, port).await {
        warn!(error = %e, "Could not check webhook status");
    }
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", bind_addr_str, port))
        .await
        .expect("bind telegram port");
    info!("Press Ctrl+C to stop");
    axum::serve(listener, app)
        .with_graceful_shutdown(telegram_shutdown_signal())
        .await
        .expect("serve telegram");
    info!("Telegram gateway stopped");
}

async fn run_websocket_gateway(
    config: Arc<ResolvedAppConfig>,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    registry: Arc<Mutex<ProviderRegistry>>,
    session_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    embed_semaphore: Arc<tokio::sync::Semaphore>,
) {
    let shared_state = WebSocketState {
        config: Arc::clone(&config),
        router_index: Arc::new(router_index),
        embedder,
        llm_pool,
        cedar,
        agent_store: Arc::new(agent_store),
        tool_registry: Arc::new(tool_registry),
        session_memory: Arc::new(session_memory),
        capability_index,
        vector_store,
        keyword_index,
        firewall,
        vault,
        tx_cron,
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        session_locks,
        embed_semaphore,
        provider_registry: registry,
    };

    let app = Router::new()
        .route("/ws", get(handle_websocket_upgrade))
        .with_state(shared_state);
    let bind = format!(
        "{}:{}",
        config.gateway.websocket_bind_address, config.gateway.websocket_port
    );
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind websocket gateway");
    info!(bind = %bind, "WebSocket gateway started");
    axum::serve(listener, app)
        .with_graceful_shutdown(telegram_shutdown_signal())
        .await
        .expect("serve websocket gateway");
}

async fn run_whatsapp_gateway(
    config: Arc<ResolvedAppConfig>,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    capability_index: Arc<aria_ssmu::CapabilityIndex>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    session_tool_caches: Arc<SessionToolCacheStore>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    registry: Arc<Mutex<ProviderRegistry>>,
    session_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    embed_semaphore: Arc<tokio::sync::Semaphore>,
) {
    let shared_state = WebSocketState {
        config: Arc::clone(&config),
        router_index: Arc::new(router_index),
        embedder,
        llm_pool,
        cedar,
        agent_store: Arc::new(agent_store),
        tool_registry: Arc::new(tool_registry),
        session_memory: Arc::new(session_memory),
        capability_index,
        vector_store,
        keyword_index,
        firewall,
        vault,
        tx_cron,
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        session_locks,
        embed_semaphore,
        provider_registry: registry,
    };
    let app = Router::new()
        .route("/webhook", post(handle_whatsapp_webhook))
        .with_state(shared_state);
    let bind = format!(
        "{}:{}",
        config.gateway.whatsapp_bind_address, config.gateway.whatsapp_port
    );
    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .expect("bind whatsapp gateway");
    info!(bind = %bind, "WhatsApp gateway started");
    axum::serve(listener, app)
        .with_graceful_shutdown(telegram_shutdown_signal())
        .await
        .expect("serve whatsapp gateway");
}

async fn telegram_shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C — shutting down gracefully");
        }
        _ = terminate => {
            info!("Received SIGTERM — shutting down gracefully");
        }
    }
}
