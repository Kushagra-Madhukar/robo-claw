// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

use std::ops::Deref;

use figment::{
    providers::{Env, Serialized},
    Figment,
};

/// Top-level TOML configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub policy: PolicyConfig,
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub features: FeatureFlagsConfig,
    pub mesh: MeshConfig,
    #[serde(default)]
    pub vault: VaultConfig,
    #[serde(default)]
    pub agents_dir: AgentsDirConfig,
    #[serde(default)]
    pub router: RouterConfig,
    #[serde(default)]
    pub ssmu: SsmuConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub simulator: SimulatorConfig,
    /// Per-node identity and role information.
    #[serde(default)]
    pub node: NodeConfig,
    /// Telemetry and observability configuration.
    #[serde(default)]
    pub telemetry: TelemetryConfig,
    /// Control UI configuration.
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub localization: LocalizationConfig,
    #[serde(default)]
    pub learning: LearningConfig,
    #[serde(default)]
    pub cluster: ClusterConfig,
    #[serde(default)]
    pub rollout: RolloutConfig,
    #[serde(default)]
    pub resource_budget: ResourceBudgetConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawRuntimeEnvConfig {
    #[serde(default)]
    config_path: Option<String>,
    #[serde(default)]
    rust_log: Option<String>,
    #[serde(default)]
    telegram_bot_token: Option<String>,
    #[serde(default)]
    openrouter_api_key: Option<String>,
    #[serde(default)]
    openai_api_key: Option<String>,
    #[serde(default)]
    anthropic_api_key: Option<String>,
    #[serde(default)]
    gemini_api_key: Option<String>,
    #[serde(default = "default_ollama_host")]
    ollama_host: String,
    #[serde(default)]
    whisper_cpp_model: Option<String>,
    #[serde(default = "default_whisper_cpp_bin")]
    whisper_cpp_bin: String,
    #[serde(default = "default_ffmpeg_bin")]
    ffmpeg_bin: String,
    #[serde(default)]
    whisper_cpp_language: Option<String>,
    #[serde(default)]
    allow_private_web_targets: Option<String>,
    #[serde(default)]
    browser_chromium_bin: Option<String>,
    #[serde(default)]
    browser_chrome_bin: Option<String>,
    #[serde(default)]
    browser_edge_bin: Option<String>,
    #[serde(default)]
    browser_safari_bin: Option<String>,
    #[serde(default)]
    browser_automation_bin: Option<String>,
    #[serde(default)]
    browser_automation_sha256_allowlist: Option<String>,
    #[serde(default)]
    browser_automation_os_containment: Option<String>,
    #[serde(default)]
    artifact_scan_bin: Option<String>,
    #[serde(default = "default_browser_artifact_max_count")]
    browser_artifact_max_count: usize,
    #[serde(default = "default_browser_artifact_max_bytes")]
    browser_artifact_max_bytes: u64,
    #[serde(default = "default_browser_session_state_max_count")]
    browser_session_state_max_count: usize,
    #[serde(default = "default_browser_session_state_max_bytes")]
    browser_session_state_max_bytes: u64,
    #[serde(default = "default_crawl_job_max_count")]
    crawl_job_max_count: usize,
    #[serde(default = "default_watch_job_max_count")]
    watch_job_max_count: usize,
    #[serde(default = "default_website_memory_max_count")]
    website_memory_max_count: usize,
    #[serde(default = "default_web_domain_min_interval_ms")]
    web_domain_min_interval_ms: u64,
    #[serde(default = "default_web_fetch_retry_attempts")]
    web_fetch_retry_attempts: u32,
    #[serde(default = "default_web_fetch_retry_base_delay_ms")]
    web_fetch_retry_base_delay_ms: u64,
    #[serde(default = "default_web_fetch_retry_max_delay_ms")]
    web_fetch_retry_max_delay_ms: u64,
    #[serde(default = "default_watch_max_jobs_per_agent")]
    watch_max_jobs_per_agent: usize,
    #[serde(default = "default_watch_max_jobs_per_domain")]
    watch_max_jobs_per_domain: usize,
    #[serde(default = "default_download_max_bytes")]
    download_max_bytes: u64,
    #[serde(default = "default_snapshot_max_bytes")]
    snapshot_max_bytes: u64,
    #[serde(default = "default_extract_max_bytes")]
    extract_max_bytes: u64,
    #[serde(default = "default_screenshot_max_bytes")]
    screenshot_max_bytes: u64,
    #[serde(default)]
    allowed_download_mime_prefixes: Option<String>,
    #[serde(default)]
    blocked_download_extensions: Option<String>,
    #[serde(default)]
    master_key: Option<String>,
    #[serde(default = "default_idempotency_cache_max_entries")]
    idempotency_cache_max_entries: u64,
    #[serde(default = "default_idempotency_cache_ttl_secs")]
    idempotency_cache_ttl_secs: u64,
    #[serde(default = "default_dedupe_key_retention_secs")]
    dedupe_key_retention_secs: u64,
    #[serde(default = "default_web_domain_state_max_entries")]
    web_domain_state_max_entries: u64,
    #[serde(default = "default_session_tool_cache_max_entries")]
    session_tool_cache_max_entries: usize,
    #[serde(default = "default_global_request_concurrency_limit")]
    global_request_concurrency_limit: usize,
}

#[derive(Debug, Clone)]
pub struct RuntimeEnvConfig {
    pub config_path: Option<String>,
    pub rust_log: Option<String>,
    pub telegram_bot_token: Option<String>,
    pub openrouter_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub gemini_api_key: Option<String>,
    pub ollama_host: String,
    pub whisper_cpp_model: Option<String>,
    pub whisper_cpp_bin: String,
    pub ffmpeg_bin: String,
    pub whisper_cpp_language: Option<String>,
    pub allow_private_web_targets: bool,
    pub browser_chromium_bin: Option<String>,
    pub browser_chrome_bin: Option<String>,
    pub browser_edge_bin: Option<String>,
    pub browser_safari_bin: Option<String>,
    pub browser_automation_bin: Option<String>,
    pub browser_automation_sha256_allowlist: Vec<String>,
    pub browser_automation_os_containment: bool,
    pub artifact_scan_bin: Option<String>,
    pub browser_artifact_max_count: usize,
    pub browser_artifact_max_bytes: u64,
    pub browser_session_state_max_count: usize,
    pub browser_session_state_max_bytes: u64,
    pub crawl_job_max_count: usize,
    pub watch_job_max_count: usize,
    pub website_memory_max_count: usize,
    pub web_domain_min_interval_ms: u64,
    pub web_fetch_retry_attempts: u32,
    pub web_fetch_retry_base_delay_ms: u64,
    pub web_fetch_retry_max_delay_ms: u64,
    pub watch_max_jobs_per_agent: usize,
    pub watch_max_jobs_per_domain: usize,
    pub download_max_bytes: u64,
    pub snapshot_max_bytes: u64,
    pub extract_max_bytes: u64,
    pub screenshot_max_bytes: u64,
    pub allowed_download_mime_prefixes: Vec<String>,
    pub blocked_download_extensions: Vec<String>,
    pub master_key: Option<String>,
    pub idempotency_cache_max_entries: u64,
    pub idempotency_cache_ttl_secs: u64,
    pub dedupe_key_retention_secs: u64,
    pub web_domain_state_max_entries: u64,
    pub session_tool_cache_max_entries: usize,
    pub global_request_concurrency_limit: usize,
}

#[derive(Debug)]
pub struct ResolvedAppConfig {
    pub path: PathBuf,
    pub file: Config,
    pub runtime: RuntimeEnvConfig,
}

impl Deref for ResolvedAppConfig {
    type Target = Config;

    fn deref(&self) -> &Self::Target {
        &self.file
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub backend: String,
    pub model: String,
    pub max_tool_rounds: usize,
    #[serde(default = "default_first_token_timeout_ms")]
    pub first_token_timeout_ms: u64,
    #[serde(default = "default_provider_circuit_cooldown_ms")]
    pub provider_circuit_breaker_cooldown_ms: u64,
    #[serde(default = "default_provider_circuit_failure_threshold")]
    pub provider_circuit_breaker_failure_threshold: usize,
    #[serde(default)]
    pub capability_overrides: Vec<ModelCapabilityOverrideConfig>,
    #[serde(default)]
    pub repair_fallback_model_allowlist: Vec<String>,
}

fn default_first_token_timeout_ms() -> u64 {
    20_000
}

fn default_provider_circuit_cooldown_ms() -> u64 {
    30_000
}

fn default_provider_circuit_failure_threshold() -> usize {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCapabilityOverrideConfig {
    pub provider_id: String,
    pub model_id: String,
    #[serde(default)]
    pub adapter_family: Option<aria_core::AdapterFamily>,
    #[serde(default)]
    pub tool_calling: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub parallel_tool_calling: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub streaming: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub vision: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub json_mode: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub max_context_tokens: Option<u32>,
    #[serde(default)]
    pub tool_schema_mode: Option<aria_core::ToolSchemaMode>,
    #[serde(default)]
    pub tool_result_mode: Option<aria_core::ToolResultMode>,
    #[serde(default)]
    pub supports_images: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub supports_audio: Option<aria_core::CapabilitySupport>,
    #[serde(default)]
    pub source_detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub policy_path: String,
    #[serde(default = "default_whitelist")]
    pub whitelist: Vec<String>,
    #[serde(default = "default_forbid")]
    pub forbid: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FeatureFlagsConfig {
    #[serde(default)]
    pub multi_channel_gateway: bool,
    #[serde(default)]
    pub append_only_session_log: bool,
    #[serde(default)]
    pub resource_leases_enforced: bool,
    #[serde(default)]
    pub outbox_delivery: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentProfile {
    Edge,
    Node,
    Cluster,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeStoreBackend {
    Sqlite,
    Postgres,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ClusterConfig {
    #[serde(default = "default_deployment_profile")]
    pub profile: DeploymentProfile,
    #[serde(default = "default_runtime_store_backend")]
    pub runtime_store_backend: RuntimeStoreBackend,
    #[serde(default)]
    pub postgres_url: Option<String>,
    #[serde(default = "default_cluster_tenant_id")]
    pub tenant_id: String,
    #[serde(default = "default_cluster_workspace_scope")]
    pub workspace_scope: String,
    #[serde(default = "default_cluster_scheduler_shards")]
    pub scheduler_shards: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceBudgetConfig {
    #[serde(default = "default_budget_parallel_requests")]
    pub max_parallel_requests: usize,
    #[serde(default = "default_budget_wasm_max_memory_pages")]
    pub wasm_max_memory_pages: u32,
    #[serde(default = "default_budget_tool_round_cap")]
    pub max_tool_rounds: usize,
    #[serde(default = "default_budget_retrieval_chars")]
    pub retrieval_context_char_budget: usize,
    #[serde(default = "default_budget_browser_automation")]
    pub browser_automation_enabled: bool,
    #[serde(default = "default_budget_learning_enabled")]
    pub learning_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeResourceBudget {
    pub max_parallel_requests: usize,
    pub wasm_max_memory_pages: u32,
    pub max_tool_rounds: usize,
    pub retrieval_context_char_budget: usize,
    pub browser_automation_enabled: bool,
    pub learning_enabled: bool,
}

fn default_budget_parallel_requests() -> usize {
    8
}

fn default_budget_wasm_max_memory_pages() -> u32 {
    256
}

fn default_budget_tool_round_cap() -> usize {
    8
}

fn default_budget_retrieval_chars() -> usize {
    16_000
}

fn default_budget_browser_automation() -> bool {
    true
}

fn default_budget_learning_enabled() -> bool {
    true
}

impl Default for ResourceBudgetConfig {
    fn default() -> Self {
        Self {
            max_parallel_requests: default_budget_parallel_requests(),
            wasm_max_memory_pages: default_budget_wasm_max_memory_pages(),
            max_tool_rounds: default_budget_tool_round_cap(),
            retrieval_context_char_budget: default_budget_retrieval_chars(),
            browser_automation_enabled: default_budget_browser_automation(),
            learning_enabled: default_budget_learning_enabled(),
        }
    }
}

fn default_deployment_profile() -> DeploymentProfile {
    DeploymentProfile::Node
}

fn default_runtime_store_backend() -> RuntimeStoreBackend {
    RuntimeStoreBackend::Sqlite
}

fn default_cluster_tenant_id() -> String {
    "default".to_string()
}

fn default_cluster_workspace_scope() -> String {
    "default".to_string()
}

fn default_cluster_scheduler_shards() -> u16 {
    1
}

impl Default for ClusterConfig {
    fn default() -> Self {
        Self {
            profile: default_deployment_profile(),
            runtime_store_backend: default_runtime_store_backend(),
            postgres_url: None,
            tenant_id: default_cluster_tenant_id(),
            workspace_scope: default_cluster_workspace_scope(),
            scheduler_shards: default_cluster_scheduler_shards(),
        }
    }
}

impl ClusterConfig {
    pub fn is_cluster(&self) -> bool {
        self.profile == DeploymentProfile::Cluster
    }

    pub fn runtime_uses_postgres(&self) -> bool {
        self.runtime_store_backend == RuntimeStoreBackend::Postgres
            && self.postgres_url.as_ref().is_some_and(|url| !url.trim().is_empty())
    }
}

fn resolve_runtime_resource_budget(config: &Config, runtime: &RuntimeEnvConfig) -> RuntimeResourceBudget {
    let mut budget = RuntimeResourceBudget {
        max_parallel_requests: config
            .resource_budget
            .max_parallel_requests
            .max(1)
            .min(runtime.global_request_concurrency_limit.max(1)),
        wasm_max_memory_pages: config.resource_budget.wasm_max_memory_pages.max(32),
        max_tool_rounds: config.resource_budget.max_tool_rounds.max(1).min(config.llm.max_tool_rounds.max(1)),
        retrieval_context_char_budget: config.resource_budget.retrieval_context_char_budget.max(1024),
        browser_automation_enabled: config.resource_budget.browser_automation_enabled,
        learning_enabled: config.resource_budget.learning_enabled,
    };
    if config.cluster.profile == DeploymentProfile::Edge {
        budget.max_parallel_requests = budget.max_parallel_requests.min(2);
        budget.wasm_max_memory_pages = budget.wasm_max_memory_pages.min(96);
        budget.max_tool_rounds = budget.max_tool_rounds.min(4);
        budget.retrieval_context_char_budget = budget.retrieval_context_char_budget.min(6000);
        budget.browser_automation_enabled = false;
        budget.learning_enabled = false;
    }
    budget
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct RolloutConfig {
    #[serde(default)]
    pub canary_enabled: bool,
    #[serde(default)]
    pub allowed_node_ids: Vec<String>,
    #[serde(default)]
    pub enabled_features: Vec<String>,
}

impl RolloutConfig {
    pub fn allows_node(&self, node_id: &str) -> bool {
        if !self.canary_enabled {
            return true;
        }
        self.allowed_node_ids.iter().any(|entry| entry == node_id)
    }

    pub fn feature_enabled_for_node(&self, node_id: &str, feature_name: &str) -> bool {
        if !self.canary_enabled {
            return true;
        }
        self.allows_node(node_id)
            && self
                .enabled_features
                .iter()
                .any(|entry| entry == feature_name)
    }
}

fn default_whitelist() -> Vec<String> {
    vec!["/workspace/".into(), "./".into()]
}

fn default_forbid() -> Vec<String> {
    vec!["/etc/".into(), "/usr/".into(), "/var/".into()]
}

fn default_ollama_host() -> String {
    "http://localhost:11434".to_string()
}

fn default_whisper_cpp_bin() -> String {
    "whisper-cli".to_string()
}

fn default_ffmpeg_bin() -> String {
    "ffmpeg".to_string()
}

fn default_browser_artifact_max_count() -> usize {
    512
}

fn default_browser_artifact_max_bytes() -> u64 {
    512 * 1024 * 1024
}

fn default_browser_session_state_max_count() -> usize {
    32
}

fn default_browser_session_state_max_bytes() -> u64 {
    128 * 1024 * 1024
}

fn default_crawl_job_max_count() -> usize {
    512
}

fn default_watch_job_max_count() -> usize {
    512
}

fn default_website_memory_max_count() -> usize {
    1024
}

fn default_web_domain_min_interval_ms() -> u64 {
    250
}

fn default_web_fetch_retry_attempts() -> u32 {
    2
}

fn default_web_fetch_retry_base_delay_ms() -> u64 {
    500
}

fn default_web_fetch_retry_max_delay_ms() -> u64 {
    5_000
}

fn default_watch_max_jobs_per_agent() -> usize {
    64
}

fn default_watch_max_jobs_per_domain() -> usize {
    16
}

fn default_download_max_bytes() -> u64 {
    50 * 1024 * 1024
}

fn default_snapshot_max_bytes() -> u64 {
    10 * 1024 * 1024
}

fn default_extract_max_bytes() -> u64 {
    2 * 1024 * 1024
}

fn default_screenshot_max_bytes() -> u64 {
    10 * 1024 * 1024
}

fn default_idempotency_cache_max_entries() -> u64 {
    1024
}

fn default_idempotency_cache_ttl_secs() -> u64 {
    3600
}

fn default_dedupe_key_retention_secs() -> u64 {
    24 * 60 * 60
}

fn default_web_domain_state_max_entries() -> u64 {
    4096
}

fn default_session_tool_cache_max_entries() -> usize {
    256
}

fn default_global_request_concurrency_limit() -> usize {
    32
}

fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("com", "anima", "aria")
}

fn default_project_config_path() -> PathBuf {
    let standard = project_dirs().map(|dirs| dirs.config_dir().join("config.toml"));
    if let Some(path) = standard.filter(|path| path.exists()) {
        path
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config.toml")
    }
}

fn default_runtime_config_path() -> PathBuf {
    default_project_config_path().with_extension("runtime.json")
}

fn default_sessions_dir() -> String {
    project_dirs()
        .map(|dirs| dirs.data_local_dir().join("sessions"))
        .unwrap_or_else(|| PathBuf::from("./workspace/sessions"))
        .to_string_lossy()
        .to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub adapter: String,
    #[serde(default)]
    pub adapters: Vec<String>,
    #[serde(default)]
    pub session_scope_policy: aria_core::SessionScopePolicy,
    /// Telegram bot token. Resolution order: config → TELEGRAM_BOT_TOKEN env → telegram_token_file.
    #[serde(default)]
    pub telegram_token: String,
    /// Path to file containing the bot token (e.g. Docker secret, k8s secret mount).
    #[serde(default)]
    pub telegram_token_file: Option<String>,
    /// Port for Telegram webhook server (default 8080). Only used when telegram_mode = "webhook".
    #[serde(default = "default_telegram_port")]
    pub telegram_port: u16,
    /// "polling" = long-poll getUpdates (no webhook, no ngrok, works anywhere). "webhook" = HTTP server (requires public URL).
    #[serde(default = "default_telegram_mode")]
    pub telegram_mode: String,
    /// Network bind address for the webhook HTTP server.
    /// Set to `"127.0.0.1"` for Tailscale Serve/Funnel overlay deployments
    /// where mTLS ingress is handled externally on the tailnet.
    /// Defaults to `"0.0.0.0"` (all interfaces).
    #[serde(default = "default_bind_address")]
    pub bind_address: String,
    /// Speech-to-text backend for voice/video media.
    /// Supported: "local_whisper", "cloud_http".
    #[serde(default = "default_stt_backend")]
    pub stt_backend: String,
    /// Speech-to-text runtime mode.
    /// Supported: "auto", "local", "cloud", "off".
    #[serde(default = "default_stt_mode")]
    pub stt_mode: String,
    /// If true, fallback to cloud STT when the primary backend fails.
    #[serde(default)]
    pub stt_cloud_fallback: bool,
    /// Cloud STT endpoint accepting JSON payload:
    /// `{ "audio_base64": "...", "mime_type": "...", "ext": "..." }`
    /// and returning `{ "transcript": "...", "confidence": 0.0-1.0, "provider": "..." }`.
    #[serde(default)]
    pub stt_cloud_endpoint: Option<String>,
    /// Environment variable name containing cloud STT API key.
    #[serde(default = "default_stt_cloud_api_key_env")]
    pub stt_cloud_api_key_env: String,
    /// Bind address for websocket gateway server.
    #[serde(default = "default_websocket_bind_address")]
    pub websocket_bind_address: String,
    /// Port for websocket gateway server.
    #[serde(default = "default_websocket_port")]
    pub websocket_port: u16,
    /// Bind address for WhatsApp ingress webhook server.
    #[serde(default = "default_whatsapp_bind_address")]
    pub whatsapp_bind_address: String,
    /// Port for WhatsApp ingress webhook server.
    #[serde(default = "default_whatsapp_port")]
    pub whatsapp_port: u16,
    /// Optional outbound provider endpoint for WhatsApp delivery.
    #[serde(default)]
    pub whatsapp_outbound_url: Option<String>,
    /// Optional bearer token used when calling WhatsApp outbound provider.
    #[serde(default)]
    pub whatsapp_auth_token: Option<String>,
    #[serde(default)]
    pub fanout: Vec<ChannelFanoutRule>,
    #[serde(default = "default_workspace_lock_wait_timeout_ms")]
    pub workspace_lock_wait_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChannelFanoutRule {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub enabled: bool,
}

fn default_telegram_port() -> u16 {
    8080
}

fn default_telegram_mode() -> String {
    "polling".to_string()
}

fn default_bind_address() -> String {
    "0.0.0.0".to_string()
}

fn default_stt_backend() -> String {
    "local_whisper".to_string()
}

fn default_stt_mode() -> String {
    "auto".to_string()
}

fn default_stt_cloud_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

fn default_websocket_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_websocket_port() -> u16 {
    8090
}

fn default_whatsapp_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_whatsapp_port() -> u16 {
    8091
}

fn default_workspace_lock_wait_timeout_ms() -> u64 {
    5_000
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            adapter: "cli".into(),
            adapters: Vec::new(),
            session_scope_policy: aria_core::SessionScopePolicy::Main,
            telegram_token: String::new(),
            telegram_token_file: None,
            telegram_port: 8080,
            telegram_mode: default_telegram_mode(),
            bind_address: default_bind_address(),
            stt_backend: default_stt_backend(),
            stt_mode: default_stt_mode(),
            stt_cloud_fallback: false,
            stt_cloud_endpoint: None,
            stt_cloud_api_key_env: default_stt_cloud_api_key_env(),
            websocket_bind_address: default_websocket_bind_address(),
            websocket_port: default_websocket_port(),
            whatsapp_bind_address: default_whatsapp_bind_address(),
            whatsapp_port: default_whatsapp_port(),
            whatsapp_outbound_url: None,
            whatsapp_auth_token: None,
            fanout: Vec::new(),
            workspace_lock_wait_timeout_ms: default_workspace_lock_wait_timeout_ms(),
        }
    }
}

fn parse_gateway_channel_label(value: &str) -> Option<aria_core::GatewayChannel> {
    match value.trim().to_ascii_lowercase().as_str() {
        "telegram" => Some(aria_core::GatewayChannel::Telegram),
        "whatsapp" => Some(aria_core::GatewayChannel::WhatsApp),
        "cli" => Some(aria_core::GatewayChannel::Cli),
        "websocket" => Some(aria_core::GatewayChannel::WebSocket),
        "discord" => Some(aria_core::GatewayChannel::Discord),
        "slack" => Some(aria_core::GatewayChannel::Slack),
        "imessage" => Some(aria_core::GatewayChannel::IMessage),
        "ros2" => Some(aria_core::GatewayChannel::Ros2),
        "unknown" => Some(aria_core::GatewayChannel::Unknown),
        _ => None,
    }
}

fn configured_gateway_adapters(gateway: &GatewayConfig) -> Vec<String> {
    let mut adapters = if gateway.adapters.is_empty() {
        vec![gateway.adapter.clone()]
    } else {
        gateway.adapters.clone()
    };
    for adapter in &mut adapters {
        *adapter = adapter.trim().to_ascii_lowercase();
    }
    adapters.retain(|adapter| !adapter.is_empty());
    adapters.sort();
    adapters.dedup();
    if adapters.is_empty() {
        vec!["cli".to_string()]
    } else {
        adapters
    }
}

/// Load `.env` from CWD and the standard ARIA config dir. Does not override existing env vars.
fn load_env() {
    let _ = dotenvy::from_path(".env");
    if let Some(project_dirs) = project_dirs() {
        let aria_env = project_dirs.config_dir().join(".env");
        if aria_env.exists() {
            let _ = dotenvy::from_path(aria_env);
        }
    }
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_flag_override(name: &str) -> Option<bool> {
    non_empty_env(name).and_then(|value| parse_flag_value(&value))
}

fn parse_flag_value(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

fn parse_csv_list(raw: Option<String>, default: &[&str]) -> Vec<String> {
    raw.map(|value| {
        value.split(',')
            .map(|entry| entry.trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect::<Vec<_>>()
    })
    .filter(|values| !values.is_empty())
    .unwrap_or_else(|| default.iter().map(|value| (*value).to_string()).collect())
}

fn load_runtime_env_config() -> Result<RuntimeEnvConfig, figment::Error> {
    let raw: RawRuntimeEnvConfig = Figment::new()
        .merge(Serialized::defaults(RawRuntimeEnvConfig {
            config_path: None,
            rust_log: None,
            telegram_bot_token: None,
            openrouter_api_key: None,
            openai_api_key: None,
            anthropic_api_key: None,
            gemini_api_key: None,
            ollama_host: default_ollama_host(),
            whisper_cpp_model: None,
            whisper_cpp_bin: default_whisper_cpp_bin(),
            ffmpeg_bin: default_ffmpeg_bin(),
            whisper_cpp_language: None,
            allow_private_web_targets: None,
            browser_chromium_bin: None,
            browser_chrome_bin: None,
            browser_edge_bin: None,
            browser_safari_bin: None,
            browser_automation_bin: None,
            browser_automation_sha256_allowlist: None,
            browser_automation_os_containment: None,
            artifact_scan_bin: None,
            browser_artifact_max_count: default_browser_artifact_max_count(),
            browser_artifact_max_bytes: default_browser_artifact_max_bytes(),
            browser_session_state_max_count: default_browser_session_state_max_count(),
            browser_session_state_max_bytes: default_browser_session_state_max_bytes(),
            crawl_job_max_count: default_crawl_job_max_count(),
            watch_job_max_count: default_watch_job_max_count(),
            website_memory_max_count: default_website_memory_max_count(),
            web_domain_min_interval_ms: default_web_domain_min_interval_ms(),
            web_fetch_retry_attempts: default_web_fetch_retry_attempts(),
            web_fetch_retry_base_delay_ms: default_web_fetch_retry_base_delay_ms(),
            web_fetch_retry_max_delay_ms: default_web_fetch_retry_max_delay_ms(),
            watch_max_jobs_per_agent: default_watch_max_jobs_per_agent(),
            watch_max_jobs_per_domain: default_watch_max_jobs_per_domain(),
            download_max_bytes: default_download_max_bytes(),
            snapshot_max_bytes: default_snapshot_max_bytes(),
            extract_max_bytes: default_extract_max_bytes(),
            screenshot_max_bytes: default_screenshot_max_bytes(),
            allowed_download_mime_prefixes: None,
            blocked_download_extensions: None,
            master_key: None,
            idempotency_cache_max_entries: default_idempotency_cache_max_entries(),
            idempotency_cache_ttl_secs: default_idempotency_cache_ttl_secs(),
            dedupe_key_retention_secs: default_dedupe_key_retention_secs(),
            web_domain_state_max_entries: default_web_domain_state_max_entries(),
            session_tool_cache_max_entries: default_session_tool_cache_max_entries(),
            global_request_concurrency_limit: default_global_request_concurrency_limit(),
        }))
        .merge(
            Env::prefixed("ARIA_")
                .ignore(&["ALLOW_PRIVATE_WEB_TARGETS", "BROWSER_AUTOMATION_OS_CONTAINMENT"]),
        )
        .extract()?;

    Ok(RuntimeEnvConfig {
        config_path: raw.config_path,
        rust_log: non_empty_env("RUST_LOG").or(raw.rust_log),
        telegram_bot_token: non_empty_env("TELEGRAM_BOT_TOKEN").or(raw.telegram_bot_token),
        openrouter_api_key: non_empty_env("OPENROUTER_API_KEY").or(raw.openrouter_api_key),
        openai_api_key: non_empty_env("OPENAI_API_KEY").or(raw.openai_api_key),
        anthropic_api_key: non_empty_env("ANTHROPIC_API_KEY").or(raw.anthropic_api_key),
        gemini_api_key: non_empty_env("GEMINI_API_KEY").or(raw.gemini_api_key),
        ollama_host: non_empty_env("OLLAMA_HOST").unwrap_or(raw.ollama_host),
        whisper_cpp_model: non_empty_env("WHISPER_CPP_MODEL").or(raw.whisper_cpp_model),
        whisper_cpp_bin: non_empty_env("WHISPER_CPP_BIN").unwrap_or(raw.whisper_cpp_bin),
        ffmpeg_bin: non_empty_env("FFMPEG_BIN").unwrap_or(raw.ffmpeg_bin),
        whisper_cpp_language: non_empty_env("WHISPER_CPP_LANGUAGE").or(raw.whisper_cpp_language),
        allow_private_web_targets: env_flag_override("ARIA_ALLOW_PRIVATE_WEB_TARGETS")
            .or_else(|| raw.allow_private_web_targets.as_deref().and_then(parse_flag_value))
            .unwrap_or(false),
        browser_chromium_bin: raw.browser_chromium_bin,
        browser_chrome_bin: raw.browser_chrome_bin,
        browser_edge_bin: raw.browser_edge_bin,
        browser_safari_bin: raw.browser_safari_bin,
        browser_automation_bin: raw.browser_automation_bin,
        browser_automation_sha256_allowlist: parse_csv_list(
            raw.browser_automation_sha256_allowlist,
            &[],
        ),
        browser_automation_os_containment: env_flag_override(
            "ARIA_BROWSER_AUTOMATION_OS_CONTAINMENT",
        )
        .or_else(|| {
            raw.browser_automation_os_containment
                .as_deref()
                .and_then(parse_flag_value)
        })
        .unwrap_or(false),
        artifact_scan_bin: raw.artifact_scan_bin.and_then(|v| {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() { None } else { Some(trimmed) }
        }),
        browser_artifact_max_count: raw.browser_artifact_max_count,
        browser_artifact_max_bytes: raw.browser_artifact_max_bytes,
        browser_session_state_max_count: raw.browser_session_state_max_count,
        browser_session_state_max_bytes: raw.browser_session_state_max_bytes,
        crawl_job_max_count: raw.crawl_job_max_count,
        watch_job_max_count: raw.watch_job_max_count,
        website_memory_max_count: raw.website_memory_max_count,
        web_domain_min_interval_ms: raw.web_domain_min_interval_ms,
        web_fetch_retry_attempts: raw.web_fetch_retry_attempts,
        web_fetch_retry_base_delay_ms: raw.web_fetch_retry_base_delay_ms,
        web_fetch_retry_max_delay_ms: raw.web_fetch_retry_max_delay_ms,
        watch_max_jobs_per_agent: raw.watch_max_jobs_per_agent,
        watch_max_jobs_per_domain: raw.watch_max_jobs_per_domain,
        download_max_bytes: raw.download_max_bytes,
        snapshot_max_bytes: raw.snapshot_max_bytes,
        extract_max_bytes: raw.extract_max_bytes,
        screenshot_max_bytes: raw.screenshot_max_bytes,
        allowed_download_mime_prefixes: parse_csv_list(
            raw.allowed_download_mime_prefixes,
            &[
                "text/",
                "application/json",
                "application/pdf",
                "application/xml",
                "application/zip",
                "image/",
            ],
        ),
        blocked_download_extensions: parse_csv_list(
            raw.blocked_download_extensions,
            &["exe", "msi", "dmg", "pkg", "app", "bat", "cmd", "ps1", "sh"],
        ),
        master_key: raw.master_key.or_else(|| non_empty_env("ARIA_MASTER_KEY")),
        idempotency_cache_max_entries: raw.idempotency_cache_max_entries,
        idempotency_cache_ttl_secs: raw.idempotency_cache_ttl_secs,
        dedupe_key_retention_secs: raw.dedupe_key_retention_secs,
        web_domain_state_max_entries: raw.web_domain_state_max_entries,
        session_tool_cache_max_entries: raw.session_tool_cache_max_entries,
        global_request_concurrency_limit: raw.global_request_concurrency_limit,
    })
}

/// Resolve Telegram bot token: config value → resolved runtime env → telegram_token_file.
pub fn resolve_telegram_token(config: &ResolvedAppConfig) -> Result<String, String> {
    let from_config = config.gateway.telegram_token.trim();
    if !from_config.is_empty() {
        return Ok(from_config.to_string());
    }
    if let Some(token) = config.runtime.telegram_bot_token.as_deref() {
        return Ok(token.to_string());
    }
    if let Some(ref path) = config.gateway.telegram_token_file {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("telegram_token_file '{}' read failed: {}", path, e))?;
        let t = content.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    Err("Telegram token required: set gateway.telegram_token, TELEGRAM_BOT_TOKEN env, or gateway.telegram_token_file".into())
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MeshConfig {
    pub mode: String,
    pub endpoints: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NodeConfig {
    pub id: String,
    pub role: String,
    pub tier: String,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            id: "orchestrator-1".to_string(),
            role: "orchestrator".to_string(),
            tier: "orchestrator".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VaultConfig {
    pub storage_path: String,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            storage_path: "./vault.json".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AgentsDirConfig {
    pub path: String,
}

impl Default for AgentsDirConfig {
    fn default() -> Self {
        Self {
            path: "./agents".to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RouterConfig {
    pub confidence_threshold: f32,
    pub tie_break_gap: f32,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.60,
            tie_break_gap: 0.05,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SsmuConfig {
    pub sessions_dir: String,
    #[serde(default = "default_operator_skill_signature_max_rows")]
    pub operator_skill_signature_max_rows: u32,
    #[serde(default = "default_operator_shell_exec_audit_max_rows")]
    pub operator_shell_exec_audit_max_rows: u32,
    #[serde(default = "default_operator_scope_denial_max_rows")]
    pub operator_scope_denial_max_rows: u32,
    #[serde(default = "default_operator_request_policy_audit_max_rows")]
    pub operator_request_policy_audit_max_rows: u32,
    #[serde(default = "default_operator_repair_fallback_audit_max_rows")]
    pub operator_repair_fallback_audit_max_rows: u32,
    #[serde(default = "default_operator_streaming_decision_audit_max_rows")]
    pub operator_streaming_decision_audit_max_rows: u32,
    #[serde(default = "default_operator_browser_action_audit_max_rows")]
    pub operator_browser_action_audit_max_rows: u32,
    #[serde(default = "default_operator_browser_challenge_event_max_rows")]
    pub operator_browser_challenge_event_max_rows: u32,
}

fn default_operator_skill_signature_max_rows() -> u32 {
    5_000
}

fn default_operator_shell_exec_audit_max_rows() -> u32 {
    20_000
}

fn default_operator_scope_denial_max_rows() -> u32 {
    20_000
}

fn default_operator_request_policy_audit_max_rows() -> u32 {
    10_000
}

fn default_operator_repair_fallback_audit_max_rows() -> u32 {
    5_000
}

fn default_operator_streaming_decision_audit_max_rows() -> u32 {
    10_000
}

fn default_operator_browser_action_audit_max_rows() -> u32 {
    20_000
}

fn default_operator_browser_challenge_event_max_rows() -> u32 {
    5_000
}

impl Default for SsmuConfig {
    fn default() -> Self {
        Self {
            sessions_dir: default_sessions_dir(),
            operator_skill_signature_max_rows: default_operator_skill_signature_max_rows(),
            operator_shell_exec_audit_max_rows: default_operator_shell_exec_audit_max_rows(),
            operator_scope_denial_max_rows: default_operator_scope_denial_max_rows(),
            operator_request_policy_audit_max_rows: default_operator_request_policy_audit_max_rows(),
            operator_repair_fallback_audit_max_rows: default_operator_repair_fallback_audit_max_rows(),
            operator_streaming_decision_audit_max_rows:
                default_operator_streaming_decision_audit_max_rows(),
            operator_browser_action_audit_max_rows:
                default_operator_browser_action_audit_max_rows(),
            operator_browser_challenge_event_max_rows:
                default_operator_browser_challenge_event_max_rows(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TelemetryConfig {
    /// Whether to enable verbose telemetry / tracing.
    pub enabled: bool,
    /// Log level hint (e.g. "info", "debug").
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

fn default_log_level() -> String {
    "info".to_string()
}

/// Initialize tracing. RUST_LOG overrides config.
/// For full data-flow debug logs (router, RAG, tools, LLM, etc.):
///   RUST_LOG=debug ./aria-x
/// Or scoped: RUST_LOG=aria_x=debug,aria_intelligence=debug,aria_ssmu=debug
fn init_tracing(config: &ResolvedAppConfig) {
    let filter = if let Some(env) = config.runtime.rust_log.as_deref() {
        tracing_subscriber::EnvFilter::new(env)
    } else if config.telemetry.enabled {
        tracing_subscriber::EnvFilter::new(&config.telemetry.log_level)
    } else {
        tracing_subscriber::EnvFilter::new("aria_x=info,warn,error")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .with_thread_ids(false)
        .init();
    info!("Tracing initialized");
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_level: default_log_level(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UiConfig {
    /// Whether to serve the embedded Control UI.
    pub enabled: bool,
    /// Bind address for the HTTP server hosting `/ui/*`.
    #[serde(default = "default_ui_bind")]
    pub bind_addr: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LearningConfig {
    #[serde(default = "default_learning_enabled")]
    pub enabled: bool,
    #[serde(default = "default_learning_sampling_percent")]
    pub sampling_percent: u8,
    #[serde(default = "default_learning_max_trace_rows")]
    pub max_trace_rows: u32,
    #[serde(default = "default_learning_max_reward_rows")]
    pub max_reward_rows: u32,
    #[serde(default = "default_learning_max_derivative_rows")]
    pub max_derivative_rows: u32,
    #[serde(default = "default_learning_redact_sensitive")]
    pub redact_sensitive: bool,
}

fn default_learning_enabled() -> bool {
    true
}

fn default_learning_sampling_percent() -> u8 {
    100
}

fn default_learning_max_trace_rows() -> u32 {
    10_000
}

fn default_learning_max_reward_rows() -> u32 {
    10_000
}

fn default_learning_max_derivative_rows() -> u32 {
    10_000
}

fn default_learning_redact_sensitive() -> bool {
    true
}

impl Default for LearningConfig {
    fn default() -> Self {
        Self {
            enabled: default_learning_enabled(),
            sampling_percent: default_learning_sampling_percent(),
            max_trace_rows: default_learning_max_trace_rows(),
            max_reward_rows: default_learning_max_reward_rows(),
            max_derivative_rows: default_learning_max_derivative_rows(),
            redact_sensitive: default_learning_redact_sensitive(),
        }
    }
}

fn default_ui_bind() -> String {
    "127.0.0.1:8080".to_string()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind_addr: default_ui_bind(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimulatorConfig {
    /// Enable simulator mode (no real hardware).
    pub enabled: bool,
    /// Simulator backend identifier: "gazebo", "mujoco", etc.
    #[serde(default)]
    pub backend: String,
}

impl Default for SimulatorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            backend: String::from("none"),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ScheduledPromptConfig {
    pub id: String,
    pub agent_id: String,
    pub prompt: String,
    pub schedule: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SchedulerConfig {
    pub enabled: bool,
    #[serde(default = "default_scheduler_tick_seconds")]
    pub tick_seconds: u64,
    #[serde(default)]
    pub jobs: Vec<ScheduledPromptConfig>,
}

fn default_scheduler_tick_seconds() -> u64 {
    1
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tick_seconds: 1,
            jobs: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalizationConfig {
    #[serde(default = "default_timezone")]
    pub default_timezone: String,
    #[serde(default)]
    pub user_timezones: HashMap<String, String>,
}

fn default_timezone() -> String {
    detect_default_timezone_name().unwrap_or_else(|| "UTC".to_string())
}

fn detect_default_timezone_name() -> Option<String> {
    // Prefer explicit TZ when present, then OS-level timezone detection.
    if let Ok(tz) = std::env::var("TZ") {
        let trimmed = tz.trim();
        if !trimmed.is_empty() && trimmed.parse::<chrono_tz::Tz>().is_ok() {
            return Some(trimmed.to_string());
        }
    }
    if let Ok(tz) = iana_time_zone::get_timezone() {
        let trimmed = tz.trim();
        if !trimmed.is_empty() && trimmed.parse::<chrono_tz::Tz>().is_ok() {
            return Some(trimmed.to_string());
        }
    }
    None
}

impl Default for LocalizationConfig {
    fn default() -> Self {
        Self {
            default_timezone: default_timezone(),
            user_timezones: HashMap::new(),
        }
    }
}

fn load_config(path: &str) -> Result<ResolvedAppConfig, Box<dyn std::error::Error>> {
    let resolved = resolve_config_path(path);
    let runtime_path = resolved.with_extension("runtime.json");

    let mut config: Config = if runtime_path.exists() {
        let content = std::fs::read_to_string(&runtime_path)?;
        serde_json::from_str(&content)?
    } else {
        let content = std::fs::read_to_string(&resolved)?;
        let cfg: Config = toml::from_str(&content)?;
        // Seed runtime.json next to the config file for future runs.
        if let Some(parent) = runtime_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = runtime_path.with_extension("runtime.json.tmp");
        let json = serde_json::to_string_pretty(&cfg)?;
        let _ = std::fs::write(&tmp, json);
        let _ = std::fs::rename(&tmp, &runtime_path);
        cfg
    };

    // Resolve relative paths (policy_path, agents_dir, sessions_dir) relative to config file's directory.
    // This ensures paths work regardless of CWD when running `cargo run -p aria-x` from repo root.
    let config_dir = resolved
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    if !std::path::Path::new(&config.policy.policy_path).is_absolute() {
        config.policy.policy_path = config_dir
            .join(&config.policy.policy_path)
            .to_string_lossy()
            .into_owned();
    }
    if !config.agents_dir.path.is_empty()
        && !std::path::Path::new(&config.agents_dir.path).is_absolute()
    {
        config.agents_dir.path = config_dir
            .join(&config.agents_dir.path)
            .to_string_lossy()
            .into_owned();
    }
    if !config.ssmu.sessions_dir.is_empty()
        && !std::path::Path::new(&config.ssmu.sessions_dir).is_absolute()
    {
        config.ssmu.sessions_dir = config_dir
            .join(&config.ssmu.sessions_dir)
            .to_string_lossy()
            .into_owned();
    }
    let mut runtime = load_runtime_env_config()?;
    let budget = resolve_runtime_resource_budget(&config, &runtime);
    config.llm.max_tool_rounds = config.llm.max_tool_rounds.min(budget.max_tool_rounds).max(1);
    runtime.global_request_concurrency_limit = runtime
        .global_request_concurrency_limit
        .min(budget.max_parallel_requests)
        .max(1);
    Ok(ResolvedAppConfig {
        path: resolved,
        file: config,
        runtime,
    })
}

/// Resolve config path: try as-is, then relative to executable directory.
fn resolve_config_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.exists() {
        return p;
    }
    if p.is_relative() {
        if let Some(project_dirs) = project_dirs() {
            let alt = project_dirs.config_dir().join(path);
            if alt.exists() {
                return alt;
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let alt = dir.join(path);
            if alt.exists() {
                return alt;
            }
        }
    }
    p
}

struct AppRuntimeState {
    locked_config: bool,
    runtime: RuntimeEnvConfig,
    active_config_path: PathBuf,
    instance_id: String,
    features: FeatureFlagsConfig,
    repair_fallback_allowlist: Vec<String>,
    native_tool_vault: std::sync::Mutex<Option<Arc<aria_vault::CredentialVault>>>,
    idempotency_results: moka::sync::Cache<String, ToolExecutionResult>,
    global_request_permits: Arc<tokio::sync::Semaphore>,
    in_flight_compactions: std::sync::Mutex<HashSet<String>>,
    web_domain_rate_limiters:
        moka::sync::Cache<String, Arc<governor::DefaultDirectRateLimiter>>,
    workspace_locks: Arc<WorkspaceLockManager>,
    resource_budget: RuntimeResourceBudget,
    deployment_profile: DeploymentProfile,
}

static APP_RUNTIME_STATE: OnceLock<Arc<AppRuntimeState>> = OnceLock::new();

fn install_app_runtime(config: Arc<ResolvedAppConfig>) {
    let runtime = config.runtime.clone();
    let resource_budget = resolve_runtime_resource_budget(&config.file, &runtime);
    let instance_id = uuid::Uuid::new_v4().to_string();
    let _ = APP_RUNTIME_STATE.set(Arc::new(AppRuntimeState {
        locked_config: true,
        runtime: runtime.clone(),
        active_config_path: config.path.clone(),
        instance_id,
        features: config.features.clone(),
        repair_fallback_allowlist: config.llm.repair_fallback_model_allowlist.clone(),
        native_tool_vault: std::sync::Mutex::new(None),
        idempotency_results: moka::sync::Cache::builder()
            .max_capacity(runtime.idempotency_cache_max_entries.max(1))
            .time_to_live(Duration::from_secs(runtime.idempotency_cache_ttl_secs.max(1)))
            .build(),
        global_request_permits: Arc::new(tokio::sync::Semaphore::new(
            resource_budget.max_parallel_requests.max(1),
        )),
        in_flight_compactions: std::sync::Mutex::new(HashSet::new()),
        web_domain_rate_limiters: moka::sync::Cache::builder()
            .max_capacity(runtime.web_domain_state_max_entries.max(1))
            .build(),
        workspace_locks: Arc::new(WorkspaceLockManager::new(Duration::from_millis(
            config.gateway.workspace_lock_wait_timeout_ms.max(1),
        ))),
        resource_budget,
        deployment_profile: config.cluster.profile,
    }));
}

fn fallback_runtime_state() -> Arc<AppRuntimeState> {
    let runtime = load_runtime_env_config().unwrap_or_else(|_| RuntimeEnvConfig {
        config_path: None,
        rust_log: None,
        telegram_bot_token: None,
        openrouter_api_key: None,
        openai_api_key: None,
        anthropic_api_key: None,
        gemini_api_key: None,
        ollama_host: default_ollama_host(),
        whisper_cpp_model: None,
        whisper_cpp_bin: default_whisper_cpp_bin(),
        ffmpeg_bin: default_ffmpeg_bin(),
        whisper_cpp_language: None,
        allow_private_web_targets: false,
        browser_chromium_bin: None,
        browser_chrome_bin: None,
        browser_edge_bin: None,
        browser_safari_bin: None,
        browser_automation_bin: None,
        browser_automation_sha256_allowlist: Vec::new(),
        browser_automation_os_containment: false,
        artifact_scan_bin: None,
        browser_artifact_max_count: default_browser_artifact_max_count(),
        browser_artifact_max_bytes: default_browser_artifact_max_bytes(),
        browser_session_state_max_count: default_browser_session_state_max_count(),
        browser_session_state_max_bytes: default_browser_session_state_max_bytes(),
        crawl_job_max_count: default_crawl_job_max_count(),
        watch_job_max_count: default_watch_job_max_count(),
        website_memory_max_count: default_website_memory_max_count(),
        web_domain_min_interval_ms: default_web_domain_min_interval_ms(),
        web_fetch_retry_attempts: default_web_fetch_retry_attempts(),
        web_fetch_retry_base_delay_ms: default_web_fetch_retry_base_delay_ms(),
        web_fetch_retry_max_delay_ms: default_web_fetch_retry_max_delay_ms(),
        watch_max_jobs_per_agent: default_watch_max_jobs_per_agent(),
        watch_max_jobs_per_domain: default_watch_max_jobs_per_domain(),
        download_max_bytes: default_download_max_bytes(),
        snapshot_max_bytes: default_snapshot_max_bytes(),
        extract_max_bytes: default_extract_max_bytes(),
        screenshot_max_bytes: default_screenshot_max_bytes(),
        allowed_download_mime_prefixes: parse_csv_list(None, &[
            "text/",
            "application/json",
            "application/pdf",
            "application/xml",
            "application/zip",
            "image/",
        ]),
        blocked_download_extensions: parse_csv_list(
            None,
            &["exe", "msi", "dmg", "pkg", "app", "bat", "cmd", "ps1", "sh"],
        ),
        master_key: None,
        idempotency_cache_max_entries: default_idempotency_cache_max_entries(),
        idempotency_cache_ttl_secs: default_idempotency_cache_ttl_secs(),
        dedupe_key_retention_secs: default_dedupe_key_retention_secs(),
        web_domain_state_max_entries: default_web_domain_state_max_entries(),
        session_tool_cache_max_entries: default_session_tool_cache_max_entries(),
        global_request_concurrency_limit: default_global_request_concurrency_limit(),
    });
    let resource_budget = RuntimeResourceBudget {
        max_parallel_requests: runtime.global_request_concurrency_limit.max(1),
        wasm_max_memory_pages: default_budget_wasm_max_memory_pages(),
        max_tool_rounds: default_budget_tool_round_cap(),
        retrieval_context_char_budget: default_budget_retrieval_chars(),
        browser_automation_enabled: default_budget_browser_automation(),
        learning_enabled: default_budget_learning_enabled(),
    };
    Arc::new(AppRuntimeState {
        locked_config: false,
        active_config_path: runtime
            .config_path
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(default_project_config_path),
        runtime: runtime.clone(),
        instance_id: uuid::Uuid::new_v4().to_string(),
        features: FeatureFlagsConfig::default(),
        repair_fallback_allowlist: Vec::new(),
        native_tool_vault: std::sync::Mutex::new(None),
        idempotency_results: moka::sync::Cache::builder()
            .max_capacity(runtime.idempotency_cache_max_entries.max(1))
            .time_to_live(Duration::from_secs(runtime.idempotency_cache_ttl_secs.max(1)))
            .build(),
        global_request_permits: Arc::new(tokio::sync::Semaphore::new(
            resource_budget.max_parallel_requests.max(1),
        )),
        in_flight_compactions: std::sync::Mutex::new(HashSet::new()),
        web_domain_rate_limiters: moka::sync::Cache::builder()
            .max_capacity(runtime.web_domain_state_max_entries.max(1))
            .build(),
        workspace_locks: Arc::new(WorkspaceLockManager::new(Duration::from_millis(
            default_workspace_lock_wait_timeout_ms(),
        ))),
        resource_budget,
        deployment_profile: DeploymentProfile::Node,
    })
}

fn app_runtime() -> &'static Arc<AppRuntimeState> {
    APP_RUNTIME_STATE.get_or_init(fallback_runtime_state)
}

fn runtime_instance_id() -> String {
    app_runtime().instance_id.clone()
}

fn runtime_feature_flags() -> FeatureFlagsConfig {
    app_runtime().features.clone()
}

fn workspace_lock_manager() -> Arc<WorkspaceLockManager> {
    Arc::clone(&app_runtime().workspace_locks)
}

fn runtime_env() -> RuntimeEnvConfig {
    if let Some(runtime) = APP_RUNTIME_STATE.get().filter(|state| state.locked_config) {
        #[cfg(test)]
        {
            return runtime_env_with_test_overrides(&runtime.runtime);
        }

        #[cfg(not(test))]
        {
            return runtime.runtime.clone();
        }
    } else {
        load_runtime_env_config().unwrap_or_else(|_| fallback_runtime_state().runtime.clone())
    }
}

fn runtime_resource_budget() -> RuntimeResourceBudget {
    app_runtime().resource_budget.clone()
}

fn runtime_deployment_profile() -> DeploymentProfile {
    app_runtime().deployment_profile
}

#[cfg(test)]
fn runtime_env_with_test_overrides(base: &RuntimeEnvConfig) -> RuntimeEnvConfig {
    let mut runtime = base.clone();

    if let Some(value) = non_empty_env("ARIA_CONFIG_PATH") {
        runtime.config_path = Some(value);
    }
    if let Some(value) = non_empty_env("RUST_LOG") {
        runtime.rust_log = Some(value);
    }
    if let Some(value) = non_empty_env("TELEGRAM_BOT_TOKEN") {
        runtime.telegram_bot_token = Some(value);
    }
    if let Some(value) = non_empty_env("OPENROUTER_API_KEY") {
        runtime.openrouter_api_key = Some(value);
    }
    if let Some(value) = non_empty_env("OPENAI_API_KEY") {
        runtime.openai_api_key = Some(value);
    }
    if let Some(value) = non_empty_env("ANTHROPIC_API_KEY") {
        runtime.anthropic_api_key = Some(value);
    }
    if let Some(value) = non_empty_env("GEMINI_API_KEY") {
        runtime.gemini_api_key = Some(value);
    }
    if let Some(value) = non_empty_env("OLLAMA_HOST") {
        runtime.ollama_host = value;
    }
    if let Some(value) = non_empty_env("WHISPER_CPP_MODEL") {
        runtime.whisper_cpp_model = Some(value);
    }
    if let Some(value) = non_empty_env("WHISPER_CPP_BIN") {
        runtime.whisper_cpp_bin = value;
    }
    if let Some(value) = non_empty_env("FFMPEG_BIN") {
        runtime.ffmpeg_bin = value;
    }
    if let Some(value) = non_empty_env("WHISPER_CPP_LANGUAGE") {
        runtime.whisper_cpp_language = Some(value);
    }
    if let Some(value) = env_flag_override("ARIA_ALLOW_PRIVATE_WEB_TARGETS") {
        runtime.allow_private_web_targets = value;
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_CHROMIUM_BIN") {
        runtime.browser_chromium_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_CHROME_BIN") {
        runtime.browser_chrome_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_EDGE_BIN") {
        runtime.browser_edge_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_SAFARI_BIN") {
        runtime.browser_safari_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_AUTOMATION_BIN") {
        runtime.browser_automation_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_BROWSER_AUTOMATION_SHA256_ALLOWLIST") {
        runtime.browser_automation_sha256_allowlist = parse_csv_list(Some(value), &[]);
    }
    if let Some(value) = env_flag_override("ARIA_BROWSER_AUTOMATION_OS_CONTAINMENT") {
        runtime.browser_automation_os_containment = value;
    }
    if let Some(value) = non_empty_env("ARIA_ARTIFACT_SCAN_BIN") {
        runtime.artifact_scan_bin = Some(value);
    }
    if let Some(value) = non_empty_env("ARIA_MASTER_KEY") {
        runtime.master_key = Some(value);
    }

    runtime
}

fn active_config_path() -> PathBuf {
    if let Some(runtime) = APP_RUNTIME_STATE.get().filter(|state| state.locked_config) {
        runtime.active_config_path.clone()
    } else {
        runtime_env()
            .config_path
            .map(PathBuf::from)
            .unwrap_or_else(default_project_config_path)
    }
}

/// Reject paths with null bytes (injection risk).
fn has_safe_path_chars(path: &str) -> bool {
    !path.contains('\0')
}

fn validate_config(config: &ResolvedAppConfig) -> Result<(), String> {
    if config.mesh.mode != "peer" && config.mesh.mode != "client" {
        return Err(format!(
            "invalid mesh.mode '{}', expected 'peer' or 'client'",
            config.mesh.mode
        ));
    }
    if config.telemetry.enabled && config.telemetry.log_level.is_empty() {
        return Err("telemetry.enabled=true requires non-empty log_level".into());
    }
    if config.ui.enabled && config.ui.bind_addr.is_empty() {
        return Err("ui.enabled=true requires non-empty bind_addr".into());
    }
    if config.simulator.enabled && config.simulator.backend.is_empty() {
        return Err("simulator.enabled=true requires non-empty backend".into());
    }
    for (name, path) in [
        ("policy_path", &config.policy.policy_path),
        ("agents_dir.path", &config.agents_dir.path),
        ("sessions_dir", &config.ssmu.sessions_dir),
    ] {
        if !has_safe_path_chars(path) {
            return Err(format!("{} must not contain null bytes", name));
        }
    }
    if configured_gateway_adapters(&config.gateway)
        .iter()
        .any(|adapter| adapter == "telegram")
    {
        if resolve_telegram_token(config).is_err() {
            return Err(
                "gateway.adapters includes telegram but token is missing (gateway.telegram_token, TELEGRAM_BOT_TOKEN env, or gateway.telegram_token_file)"
                    .into(),
            );
        }
        let stt_backend = config.gateway.stt_backend.trim().to_lowercase();
        if stt_backend != "local_whisper" && stt_backend != "cloud_http" {
            return Err(format!(
                "gateway.stt_backend '{}' is invalid; expected 'local_whisper' or 'cloud_http'",
                config.gateway.stt_backend
            ));
        }
        let stt_mode = config.gateway.stt_mode.trim().to_lowercase();
        if stt_mode != "auto" && stt_mode != "local" && stt_mode != "cloud" && stt_mode != "off" {
            return Err(format!(
                "gateway.stt_mode '{}' is invalid; expected 'auto', 'local', 'cloud', or 'off'",
                config.gateway.stt_mode
            ));
        }
        if stt_mode == "local" {
            let stt_status = crate::stt::inspect_stt_status(config);
            let Some(model_path) = stt_status.whisper_model_path.as_deref() else {
                return Err(
                    "gateway.stt_mode=local requires WHISPER_CPP_MODEL or runtime.whisper_cpp_model"
                        .into(),
                );
            };
            if !stt_status.whisper_model_exists {
                return Err(format!(
                    "gateway.stt_mode=local requires whisper model file to exist: {}",
                    model_path
                ));
            }
            if !stt_status.whisper_bin_available {
                return Err(format!(
                    "gateway.stt_mode=local requires executable whisper_cpp_bin '{}'",
                    config.runtime.whisper_cpp_bin
                ));
            }
            if !stt_status.ffmpeg_available {
                return Err(format!(
                    "gateway.stt_mode=local requires executable ffmpeg_bin '{}'",
                    config.runtime.ffmpeg_bin
                ));
            }
        }
        if stt_mode == "cloud"
            && config
                .gateway
                .stt_cloud_endpoint
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            return Err(
                "gateway.stt_backend=cloud_http requires non-empty gateway.stt_cloud_endpoint"
                    .into(),
            );
        }
        if config.gateway.stt_cloud_fallback
            && config
                .gateway
                .stt_cloud_endpoint
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            return Err(
                "gateway.stt_cloud_fallback=true requires non-empty gateway.stt_cloud_endpoint"
                    .into(),
            );
        }
    }
    if config
        .localization
        .default_timezone
        .trim()
        .parse::<chrono_tz::Tz>()
        .is_err()
    {
        return Err(format!(
            "localization.default_timezone '{}' is invalid; use an IANA timezone like 'Asia/Kolkata' or 'Europe/Zurich'",
            config.localization.default_timezone
        ));
    }
    for (user_id, tz_name) in &config.localization.user_timezones {
        if tz_name.trim().parse::<chrono_tz::Tz>().is_err() {
            return Err(format!(
                "localization.user_timezones['{}'] has invalid timezone '{}'; use an IANA timezone",
                user_id, tz_name
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn parse_test_config(toml_suffix: &str) -> Config {
        let mut base = r#"
            [llm]
            backend = "mock"
            model = "test"
            max_tool_rounds = 5

            [policy]
            policy_path = "./policy.cedar"

            [gateway]
            adapter = "cli"

            [mesh]
            mode = "peer"
            endpoints = []
        "#
        .to_string();
        if !toml_suffix.trim().is_empty() {
            base.push('\n');
            base.push_str(toml_suffix);
        }
        toml::from_str(&base).expect("parse config")
    }

    #[test]
    fn feature_flags_default_to_disabled() {
        let cfg = parse_test_config("");
        assert!(!cfg.features.multi_channel_gateway);
        assert!(!cfg.features.append_only_session_log);
        assert!(!cfg.features.resource_leases_enforced);
        assert!(!cfg.features.outbox_delivery);
    }

    #[test]
    fn feature_flags_parse_when_present() {
        let cfg = parse_test_config(
            r#"
            [features]
            multi_channel_gateway = true
            append_only_session_log = true
            resource_leases_enforced = true
            outbox_delivery = false
            "#,
        );
        assert!(cfg.features.multi_channel_gateway);
        assert!(cfg.features.append_only_session_log);
        assert!(cfg.features.resource_leases_enforced);
        assert!(!cfg.features.outbox_delivery);
    }

    #[test]
    fn configured_gateway_adapters_uses_legacy_single_adapter_when_list_missing() {
        let cfg = parse_test_config("");
        assert_eq!(
            configured_gateway_adapters(&cfg.gateway),
            vec!["cli".to_string()]
        );
    }

    #[test]
    fn configured_gateway_adapters_prefers_list_and_dedupes() {
        let mut cfg = parse_test_config("");
        cfg.gateway.adapters = vec![
            "telegram".to_string(),
            "cli".to_string(),
            "telegram".to_string(),
            "  CLI  ".to_string(),
        ];
        assert_eq!(
            configured_gateway_adapters(&cfg.gateway),
            vec!["cli".to_string(), "telegram".to_string()]
        );
    }

    #[test]
    fn runtime_instance_id_is_present() {
        let id = runtime_instance_id();
        assert!(!id.trim().is_empty());
    }

    #[test]
    fn global_request_admission_limit_defaults_to_positive_value() {
        let runtime = fallback_runtime_state();
        assert!(runtime.runtime.global_request_concurrency_limit > 0);
        assert!(runtime.global_request_permits.available_permits() > 0);
    }

    #[test]
    fn cluster_config_defaults_to_node_safe_sqlite_profile() {
        let cfg = parse_test_config("");
        assert_eq!(cfg.cluster.profile, DeploymentProfile::Node);
        assert_eq!(cfg.cluster.runtime_store_backend, RuntimeStoreBackend::Sqlite);
        assert!(!cfg.cluster.is_cluster());
        assert!(!cfg.cluster.runtime_uses_postgres());
    }

    #[test]
    fn cluster_config_accepts_optional_postgres_without_affecting_node_default() {
        let cfg = parse_test_config(
            r#"
            [cluster]
            profile = "cluster"
            runtime_store_backend = "postgres"
            postgres_url = "postgres://aria:secret@localhost/aria"
            tenant_id = "team-a"
            workspace_scope = "workspace-a"
            scheduler_shards = 4
            "#,
        );
        assert_eq!(cfg.cluster.profile, DeploymentProfile::Cluster);
        assert!(cfg.cluster.is_cluster());
        assert!(cfg.cluster.runtime_uses_postgres());
        assert_eq!(cfg.cluster.scheduler_shards, 4);
    }

    #[test]
    fn edge_profile_clamps_runtime_resource_budget_and_disables_heavy_features() {
        let cfg = parse_test_config(
            r#"
            [cluster]
            profile = "edge"

            [resource_budget]
            max_parallel_requests = 6
            wasm_max_memory_pages = 256
            max_tool_rounds = 7
            retrieval_context_char_budget = 12000
            browser_automation_enabled = true
            learning_enabled = true
            "#,
        );
        let runtime = RuntimeEnvConfig {
            config_path: None,
            rust_log: None,
            telegram_bot_token: None,
            openrouter_api_key: None,
            openai_api_key: None,
            anthropic_api_key: None,
            gemini_api_key: None,
            ollama_host: default_ollama_host(),
            whisper_cpp_model: None,
            whisper_cpp_bin: default_whisper_cpp_bin(),
            ffmpeg_bin: default_ffmpeg_bin(),
            whisper_cpp_language: None,
            allow_private_web_targets: false,
            browser_chromium_bin: None,
            browser_chrome_bin: None,
            browser_edge_bin: None,
            browser_safari_bin: None,
            browser_automation_bin: None,
            browser_automation_sha256_allowlist: Vec::new(),
            browser_automation_os_containment: false,
            artifact_scan_bin: None,
            browser_artifact_max_count: default_browser_artifact_max_count(),
            browser_artifact_max_bytes: default_browser_artifact_max_bytes(),
            browser_session_state_max_count: default_browser_session_state_max_count(),
            browser_session_state_max_bytes: default_browser_session_state_max_bytes(),
            crawl_job_max_count: default_crawl_job_max_count(),
            watch_job_max_count: default_watch_job_max_count(),
            website_memory_max_count: default_website_memory_max_count(),
            web_domain_min_interval_ms: default_web_domain_min_interval_ms(),
            web_fetch_retry_attempts: default_web_fetch_retry_attempts(),
            web_fetch_retry_base_delay_ms: default_web_fetch_retry_base_delay_ms(),
            web_fetch_retry_max_delay_ms: default_web_fetch_retry_max_delay_ms(),
            watch_max_jobs_per_agent: default_watch_max_jobs_per_agent(),
            watch_max_jobs_per_domain: default_watch_max_jobs_per_domain(),
            download_max_bytes: default_download_max_bytes(),
            snapshot_max_bytes: default_snapshot_max_bytes(),
            extract_max_bytes: default_extract_max_bytes(),
            screenshot_max_bytes: default_screenshot_max_bytes(),
            allowed_download_mime_prefixes: Vec::new(),
            blocked_download_extensions: Vec::new(),
            master_key: None,
            idempotency_cache_max_entries: default_idempotency_cache_max_entries(),
            idempotency_cache_ttl_secs: default_idempotency_cache_ttl_secs(),
            dedupe_key_retention_secs: default_dedupe_key_retention_secs(),
            web_domain_state_max_entries: default_web_domain_state_max_entries(),
            session_tool_cache_max_entries: default_session_tool_cache_max_entries(),
            global_request_concurrency_limit: 6,
        };
        let budget = resolve_runtime_resource_budget(&cfg, &runtime);
        assert_eq!(budget.max_parallel_requests, 2);
        assert_eq!(budget.wasm_max_memory_pages, 96);
        assert_eq!(budget.max_tool_rounds, 4);
        assert_eq!(budget.retrieval_context_char_budget, 6000);
        assert!(!budget.browser_automation_enabled);
        assert!(!budget.learning_enabled);
    }

    #[test]
    fn rollout_config_canary_gate_is_node_aware() {
        let cfg = parse_test_config(
            r#"
            [rollout]
            canary_enabled = true
            allowed_node_ids = ["node-a"]
            enabled_features = ["outbox_delivery", "multi_channel_gateway"]
            "#,
        );
        assert!(cfg.rollout.feature_enabled_for_node("node-a", "outbox_delivery"));
        assert!(!cfg.rollout.feature_enabled_for_node("node-b", "outbox_delivery"));
        assert!(!cfg.rollout.feature_enabled_for_node("node-a", "scheduler"));
    }
}
