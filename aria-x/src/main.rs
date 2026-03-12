//! ARIA-X Orchestrator — the final binary that wires all crates together.
//!
//! Reads TOML configuration, initializes all subsystems, and runs the
//! ReAct agent loop with graceful SIGINT shutdown via a CLI gateway.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::{collections::HashMap, time::Duration};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use axum::{http::StatusCode, routing::post, Router};
use serde::{Deserialize, Serialize};

use aria_core::{AgentRequest, GatewayChannel, MessageContent};
use aria_gateway::{GatewayAdapter, GatewayError, TelegramNormalizer};
use aria_intelligence::{
    backends::{self, ollama::OllamaBackend, ProviderRegistry, SecretRef},
    AgentConfigStore, AgentOrchestrator, CachedTool, CronScheduler, DynamicToolCache,
    EmbeddingModel, FastEmbedder, LLMBackend, LLMResponse, LlmBackendPool, OrchestratorError,
    RouteConfig, RouterIndex, ScheduleSpec, ScheduledJobKind, ScheduledPromptJob, SemanticRouter, ToolCall,
    ToolExecutor, ToolManifestStore,
};
use aria_ssmu::{
    vector::{KeywordIndex, VectorStore},
    HybridMemoryEngine, PageIndexTree, PageNode, QueryPlannerConfig,
};
use aria_vault::CredentialVault;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Top-level TOML configuration.
#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub llm: LlmConfig,
    pub policy: PolicyConfig,
    pub gateway: GatewayConfig,
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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LlmConfig {
    pub backend: String,
    pub model: String,
    pub max_tool_rounds: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PolicyConfig {
    pub policy_path: String,
    #[serde(default = "default_whitelist")]
    pub whitelist: Vec<String>,
    #[serde(default = "default_forbid")]
    pub forbid: Vec<String>,
}

fn default_whitelist() -> Vec<String> {
    vec!["/workspace/".into(), "./".into()]
}

fn default_forbid() -> Vec<String> {
    vec!["/etc/".into(), "/usr/".into(), "/var/".into()]
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayConfig {
    pub adapter: String,
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

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            adapter: "cli".into(),
            telegram_token: String::new(),
            telegram_token_file: None,
            telegram_port: 8080,
            telegram_mode: default_telegram_mode(),
            bind_address: default_bind_address(),
        }
    }
}

/// Load .env from CWD and ~/.aria/.env. Does not override existing env vars.
fn load_env() {
    // CWD .env first (project-local overrides)
    let _ = dotenvy::from_path(".env");
    // User config dir (persistent for daemon)
    if let Some(home) = std::env::var_os("HOME") {
        let aria_env = std::path::Path::new(&home).join(".aria").join(".env");
        if aria_env.exists() {
            let _ = dotenvy::from_path(aria_env);
        }
    }
}

/// Resolve Telegram bot token: config value → TELEGRAM_BOT_TOKEN env → telegram_token_file.
pub fn resolve_telegram_token(config: &GatewayConfig) -> Result<String, String> {
    let from_config = config.telegram_token.trim();
    if !from_config.is_empty() {
        return Ok(from_config.to_string());
    }
    if let Ok(t) = std::env::var("TELEGRAM_BOT_TOKEN") {
        let t = t.trim();
        if !t.is_empty() {
            return Ok(t.to_string());
        }
    }
    if let Some(ref path) = config.telegram_token_file {
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
}

impl Default for SsmuConfig {
    fn default() -> Self {
        Self {
            sessions_dir: "./workspace/sessions".to_string(),
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
fn init_tracing(config: &TelemetryConfig) {
    let filter = if let Ok(env) = std::env::var("RUST_LOG") {
        tracing_subscriber::EnvFilter::new(env)
    } else if config.enabled {
        tracing_subscriber::EnvFilter::new(&config.log_level)
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

fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
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
    Ok(config)
}

/// Resolve config path: try as-is, then relative to executable directory.
fn resolve_config_path(path: &str) -> PathBuf {
    let p = PathBuf::from(path);
    if p.exists() {
        return p;
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

/// Reject paths with null bytes (injection risk).
fn has_safe_path_chars(path: &str) -> bool {
    !path.contains('\0')
}

fn validate_config(config: &Config) -> Result<(), String> {
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
    if config.gateway.adapter == "telegram" {
        if resolve_telegram_token(&config.gateway).is_err() {
            return Err(
                "gateway.adapter=telegram requires telegram_token, TELEGRAM_BOT_TOKEN env, or telegram_token_file"
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

// ---------------------------------------------------------------------------
// Gateway & Mock Adapters
// ---------------------------------------------------------------------------

struct CliGateway;

#[async_trait::async_trait]
impl GatewayAdapter for CliGateway {
    async fn receive(&self) -> Result<AgentRequest, GatewayError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        tokio::task::spawn_blocking(move || {
            print!("\n[aria-x] Enter request > ");
            let _ = io::stdout().flush();
            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let _ = tx.send(input.trim().to_string());
            }
        });

        let content = rx.await.unwrap_or_default();
        if content.is_empty() {
            // Keep the loop alive if empty line
            return Err(GatewayError::TransportError("Empty input".into()));
        }

        Ok(AgentRequest {
            request_id: [0u8; 16],
            session_id: [0u8; 16],
            channel: GatewayChannel::Cli,
            user_id: "cli_user".into(),
            content: MessageContent::Text(content),
            timestamp_us: 0,
        })
    }
}

#[derive(Clone)]
struct LocalMockLLM;

#[async_trait::async_trait]
impl LLMBackend for LocalMockLLM {
    async fn query(
        &self,
        prompt: &str,
        _tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        Ok(LLMResponse::TextAnswer(format!(
            "Echo LLM: I processed '{}'",
            prompt
        )))
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
}

struct WasmToolExecutor {
    vault: Arc<aria_vault::CredentialVault>,
    tools_dir: std::path::PathBuf,
    agent_id: String,
    session_id: uuid::Uuid,
}

impl WasmToolExecutor {
    pub fn new(
        vault: Arc<aria_vault::CredentialVault>,
        agent_id: String,
        session_id: uuid::Uuid,
    ) -> Self {
        Self {
            vault,
            tools_dir: std::path::PathBuf::from("./tools"),
            agent_id,
            session_id,
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for WasmToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        let wasm_path = self.tools_dir.join(format!("{}.wasm", call.name));
        let wasm_bytes = std::fs::read(&wasm_path).map_err(|e| {
            OrchestratorError::ToolError(format!(
                "Failed to read Wasm file for tool '{}' at {:?}: {}",
                call.name, wasm_path, e
            ))
        })?;

        let mut secrets = std::collections::HashMap::new();

        // In a real system, we'd map "call.name" or "agent_id" to a list of required keys
        // For this implementation, we attempt to retrieve a known key (e.g., "api_key")
        // and inject it if present.
        if let Ok(key) = self
            .vault
            .retrieve_for_egress(&self.agent_id, "api_key", "api.github.com")
        {
            secrets.insert("GITHUB_TOKEN".to_string(), key);
        }
        if let Ok(key) = self
            .vault
            .retrieve_for_egress(&self.agent_id, "api_key", "api.openai.com")
        {
            secrets.insert("OPENAI_API_KEY".to_string(), key);
        }

        let ws_dir = format!("/tmp/aria_ws_{}", self.session_id);
        let _ = std::fs::create_dir_all(&ws_dir);

        // Attempt to read allowed_hosts from tool manifest
        let manifest_path = self.tools_dir.join(format!("{}.manifest.json", call.name));
        let mut allowed_hosts = None;
        if let Ok(manifest_str) = std::fs::read_to_string(&manifest_path) {
            #[derive(serde::Deserialize)]
            struct ToolManifest {
                allowed_hosts: Option<Vec<String>>,
            }
            if let Ok(parsed) = serde_json::from_str::<ToolManifest>(&manifest_str) {
                allowed_hosts = parsed.allowed_hosts;
            }
        }

        let config = aria_skill_runtime::ExtismConfig {
            max_memory_pages: Some(256),
            wasi_enabled: true, // often needed for HTTP egress
            secrets,
            workspace_dir: Some(std::path::PathBuf::from(&ws_dir)),
            allowed_hosts,
        };
        let _ = std::fs::create_dir_all(&ws_dir);
        let backend = aria_skill_runtime::ExtismBackend::with_config(config);

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
        Ok(result)
    }
}

pub struct NativeToolExecutor {
    pub tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    pub session_id: Option<aria_core::Uuid>,
    pub user_id: Option<String>,
    pub channel: Option<aria_core::GatewayChannel>,
    pub session_memory: Option<aria_ssmu::SessionMemory>,
    pub cedar: Option<Arc<aria_policy::CedarEvaluator>>,
    scheduling_intent: Option<SchedulingIntent>,
    user_timezone: chrono_tz::Tz,
}

#[async_trait::async_trait]
impl ToolExecutor for NativeToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        match call.name.as_str() {
            "read_file" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                std::fs::read_to_string(path).map_err(|e| {
                    OrchestratorError::ToolError(format!("Failed to read {}: {}", path, e))
                })
            }
            "write_file" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(parent) = std::path::Path::new(path).parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::write(path, content).map_err(|e| {
                    OrchestratorError::ToolError(format!("Failed to write {}: {}", path, e))
                })?;
                Ok(format!(
                    "Successfully wrote {} bytes to {}",
                    content.len(),
                    path
                ))
            }
            "run_shell" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let command = args.get("command").and_then(|v| v.as_str()).unwrap_or("");
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .output()
                    .map_err(|e| {
                        OrchestratorError::ToolError(format!("Failed to execute shell: {}", e))
                    })?;
                let mut res = String::new();
                if !output.stdout.is_empty() {
                    res.push_str(&String::from_utf8_lossy(&output.stdout));
                }
                if !output.stderr.is_empty() {
                    if !res.is_empty() {
                        res.push('\n');
                    }
                    res.push_str("STDERR:\n");
                    res.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                if res.is_empty() {
                    res.push_str("Command executed successfully with no output.");
                }
                Ok(res)
            }
            "schedule_message" | "set_reminder" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let task = args
                    .get("task")
                    .or_else(|| args.get("prompt"))
                    .or_else(|| args.get("message"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let delay_raw = args
                    .get("delay")
                    .or_else(|| args.get("schedule"))
                    .unwrap_or(&serde_json::Value::String("1m".to_string()))
                    .clone();
                let delay = if let Some(s) = delay_raw.as_str() {
                    s.to_string()
                } else if let Some(i) = delay_raw.as_u64() {
                    format!("{}s", i)
                } else if let Some(f) = delay_raw.as_f64() {
                    format!("{}s", f as u64)
                } else {
                    "1m".to_string()
                };

                if task.is_empty() {
                    return Err(OrchestratorError::ToolError(
                        "Missing 'task', 'prompt', or 'message'".into(),
                    ));
                }

                let classified_schedule = self
                    .scheduling_intent
                    .as_ref()
                    .and_then(|intent| intent.normalized_schedule.clone());
                let normalized_delay = if delay_raw.is_null()
                    || args.get("delay").is_none() && args.get("schedule").is_none()
                {
                    classified_schedule
                        .clone()
                        .unwrap_or_else(|| {
                            normalize_schedule_input(
                                &delay,
                                chrono::Utc::now().with_timezone(&self.user_timezone),
                            )
                        })
                } else {
                    normalize_schedule_input(
                        &delay,
                        chrono::Utc::now().with_timezone(&self.user_timezone),
                    )
                };
                let (normalized_delay, spec) = if let Some(spec) =
                    aria_intelligence::ScheduleSpec::parse(&normalized_delay)
                {
                    (normalized_delay, spec)
                } else if let Some(fallback_schedule) = classified_schedule
                    .as_ref()
                    .filter(|fallback| *fallback != &normalized_delay)
                    .and_then(|fallback| {
                        aria_intelligence::ScheduleSpec::parse(fallback).map(|spec| (fallback, spec))
                    })
                {
                    tracing::warn!(
                        requested_delay = %delay,
                        normalized_delay = %normalized_delay,
                        fallback_schedule = %fallback_schedule.0,
                        "schedule_message: using classifier normalized_schedule fallback"
                    );
                    (fallback_schedule.0.clone(), fallback_schedule.1)
                } else {
                    return Err(OrchestratorError::ToolError(
                        "Invalid delay format. Examples: '2m', 'daily@19:30', 'weekly:sat@11:00', 'biweekly:sat@11:00', 'at:2026-08-28 19:00'.".into(),
                    ));
                };
                let agent_id = args
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("omni")
                    .to_string();
                let mode = args
                    .get("mode")
                    .or_else(|| args.get("execution_mode"))
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        self.scheduling_intent
                            .as_ref()
                            .map(|intent| intent.mode.as_tool_mode())
                    })
                    .unwrap_or("notify")
                    .trim()
                    .to_ascii_lowercase();
                let deferred_prompt = args
                    .get("deferred_prompt")
                    .or_else(|| args.get("task_prompt"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .or_else(|| {
                        self.scheduling_intent
                            .as_ref()
                            .and_then(|intent| intent.deferred_task.clone())
                    });
                let mut mode = mode;
                if deferred_prompt.is_some() && mode == "notify" {
                    // If a deferred prompt is present but mode is notify, prefer executing deferred work.
                    // This avoids losing deferred actions due to model inconsistency.
                    mode = self
                        .scheduling_intent
                        .as_ref()
                        .map(|intent| intent.mode.as_tool_mode().to_string())
                        .unwrap_or_else(|| "defer".to_string());
                }
                let deferred_task = deferred_prompt.unwrap_or_else(|| task.clone());
                if !matches!(
                    mode.as_str(),
                    "notify" | "defer" | "deferred" | "execute_later" | "both"
                ) {
                    return Err(OrchestratorError::ToolError(
                        "Invalid mode. Use 'notify', 'defer', or 'both'.".into(),
                    ));
                }

                // De-duplicate identical reminders to prevent repeated LLM tool calls
                // from creating multiple equivalent jobs in the same session.
                let (tx_reply, rx_reply) = tokio::sync::oneshot::channel();
                self.tx_cron
                    .send(aria_intelligence::CronCommand::List(tx_reply))
                    .await
                    .map_err(|_| {
                        OrchestratorError::ToolError(
                            "Scheduler is unavailable; cannot verify existing reminders".into(),
                        )
                    })?;
                let jobs = rx_reply.await.map_err(|_| {
                    OrchestratorError::ToolError(
                        "Scheduler is unavailable; failed to inspect existing reminders".into(),
                    )
                })?;
                let mut created = Vec::new();
                let mut existing = Vec::new();

                let wants_notify = matches!(mode.as_str(), "notify" | "both");
                let wants_defer = matches!(mode.as_str(), "defer" | "deferred" | "execute_later" | "both");

                if wants_notify {
                    if let Some(found) = jobs.iter().find(|j| {
                        j.kind == aria_intelligence::ScheduledJobKind::Notify
                            && j.agent_id == agent_id
                            && j.prompt == task
                            && j.schedule_str == normalized_delay
                            && j.session_id == self.session_id
                            && j.user_id == self.user_id
                            && j.channel == self.channel
                    }) {
                        existing.push(format!("notify:{}", found.id));
                    } else {
                        let id = format!("reminder-{}", uuid::Uuid::new_v4());
                        let job = aria_intelligence::ScheduledPromptJob {
                            id: id.clone(),
                            agent_id: agent_id.clone(),
                            prompt: task.clone(),
                            schedule_str: normalized_delay.clone(),
                            kind: aria_intelligence::ScheduledJobKind::Notify,
                            schedule: spec.clone(),
                            session_id: self.session_id,
                            user_id: self.user_id.clone(),
                            channel: self.channel,
                        };
                        self
                            .tx_cron
                            .send(aria_intelligence::CronCommand::Add(job))
                            .await
                            .map_err(|_| {
                                OrchestratorError::ToolError(
                                    "Scheduler is unavailable; reminder was not queued".into(),
                                )
                            })?;
                        created.push(format!("notify:{}", id));
                    }
                }

                if wants_defer {
                    if let Some(found) = jobs.iter().find(|j| {
                        j.kind == aria_intelligence::ScheduledJobKind::Orchestrate
                            && j.agent_id == agent_id
                            && j.prompt == deferred_task
                            && j.schedule_str == normalized_delay
                            && j.session_id == self.session_id
                            && j.user_id == self.user_id
                            && j.channel == self.channel
                    }) {
                        existing.push(format!("defer:{}", found.id));
                    } else {
                        let id = format!("deferred-{}", uuid::Uuid::new_v4());
                        let job = aria_intelligence::ScheduledPromptJob {
                            id: id.clone(),
                            agent_id: agent_id.clone(),
                            prompt: deferred_task.clone(),
                            schedule_str: normalized_delay.clone(),
                            kind: aria_intelligence::ScheduledJobKind::Orchestrate,
                            schedule: spec.clone(),
                            session_id: self.session_id,
                            user_id: self.user_id.clone(),
                            channel: self.channel,
                        };
                        self
                            .tx_cron
                            .send(aria_intelligence::CronCommand::Add(job))
                            .await
                            .map_err(|_| {
                                OrchestratorError::ToolError(
                                    "Scheduler is unavailable; deferred task was not queued".into(),
                                )
                            })?;
                        created.push(format!("defer:{}", id));
                    }
                }

                if created.is_empty() && !existing.is_empty() {
                    return Ok(format!("Already scheduled ({})", existing.join(", ")));
                }

                let mode_text = if mode == "both" {
                    "notify + deferred execution"
                } else if wants_defer {
                    "deferred execution"
                } else {
                    "reminder notification"
                };
                let mut msg = format!(
                    "Scheduled {} for '{}' at '{}' (agent: {}).",
                    mode_text, task, normalized_delay, agent_id
                );
                if !created.is_empty() {
                    msg.push_str(&format!(" created=[{}]", created.join(", ")));
                }
                if !existing.is_empty() {
                    msg.push_str(&format!(" existing=[{}]", existing.join(", ")));
                }
                Ok(msg)
            }
            "search_codebase" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
                if query.is_empty() {
                    return Err(OrchestratorError::ToolError("Missing 'query'".into()));
                }
                let output = std::process::Command::new("grep")
                    .arg("-rIn")
                    .arg("--exclude-dir=.git")
                    .arg("--max-count=20")
                    .arg(query)
                    .arg(".")
                    .output()
                    .map_err(|e| {
                        OrchestratorError::ToolError(format!("Failed to execute grep: {}", e))
                    })?;
                let res = String::from_utf8_lossy(&output.stdout).to_string();
                if res.is_empty() {
                    Ok("No matches found.".to_string())
                } else {
                    Ok(res)
                }
            }
            "run_tests" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
                let mut cmd = std::process::Command::new("cargo");
                cmd.arg("test");
                if !target.is_empty() {
                    cmd.arg(target);
                }
                let output = cmd.output().map_err(|e| {
                    OrchestratorError::ToolError(format!("Failed to execute cargo test: {}", e))
                })?;
                let mut res = String::from_utf8_lossy(&output.stdout).to_string();
                res.push_str(&String::from_utf8_lossy(&output.stderr));
                Ok(res)
            }
            "manage_cron" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

                if action == "list" {
                    let (tx_reply, rx_reply) = tokio::sync::oneshot::channel();
                    self
                        .tx_cron
                        .send(aria_intelligence::CronCommand::List(tx_reply))
                        .await
                        .map_err(|_| {
                            OrchestratorError::ToolError(
                                "Scheduler is unavailable; cannot list jobs".into(),
                            )
                        })?;
                    if let Ok(jobs) = rx_reply.await {
                        let json = serde_json::to_string(&jobs).unwrap_or_default();
                        return Ok(format!("Active crons: {}", json));
                    }
                    return Err(OrchestratorError::ToolError("Failed to list crons".into()));
                }

                let mut id = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if action == "add" && id.is_empty() {
                    id = format!("cron-{}", uuid::Uuid::new_v4());
                }
                if (action == "delete" || action == "update") && id.is_empty() {
                    return Err(OrchestratorError::ToolError("Missing 'id'".into()));
                }

                if action == "delete" {
                    self
                        .tx_cron
                        .send(aria_intelligence::CronCommand::Remove(id.clone()))
                        .await
                        .map_err(|_| {
                            OrchestratorError::ToolError(
                                "Scheduler is unavailable; cannot delete job".into(),
                            )
                        })?;
                    if let Ok(config_path) = std::env::var("ARIA_CONFIG_PATH")
                        .or_else(|_| Ok::<_, ()>("config.toml".into()))
                    {
                        if let Ok(content) = std::fs::read_to_string(&config_path) {
                            if let Ok(mut doc) = content.parse::<toml_edit::DocumentMut>() {
                                if let Some(jobs) = doc
                                    .get_mut("scheduler")
                                    .and_then(|i| i.get_mut("jobs"))
                                    .and_then(|i| i.as_array_of_tables_mut())
                                {
                                    jobs.retain(|table| {
                                        table.get("id").and_then(|v| v.as_str()) != Some(&id)
                                    });
                                    let _ = std::fs::write(&config_path, doc.to_string());
                                }
                            }
                        }
                    }
                    return Ok(format!("Cron {} deleted natively and from config.toml", id));
                }

                if action == "add" || action == "update" {
                    let prompt = args
                        .get("prompt")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let schedule_str = args
                        .get("schedule")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let agent_id = args
                        .get("agent_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("omni")
                        .to_string();

                    if prompt.is_empty() || schedule_str.is_empty() {
                        return Err(OrchestratorError::ToolError(
                            "Missing 'prompt' or 'schedule'".into(),
                        ));
                    }
                    let normalized_schedule =
                        normalize_schedule_input(
                            &schedule_str,
                            chrono::Utc::now().with_timezone(&self.user_timezone),
                        );
                    let spec =
                        aria_intelligence::ScheduleSpec::parse(&normalized_schedule).ok_or_else(|| {
                            OrchestratorError::ToolError(
                                "Invalid schedule spec. Use cron ('30 19 * * *'), duration ('2m'), daily ('daily@19:30'), weekly ('weekly:sat@11:00'), biweekly ('biweekly:sat@11:00'), or one-shot ('at:2026-08-28 19:00')."
                                    .into(),
                            )
                        })?;

                    let job = aria_intelligence::ScheduledPromptJob {
                        id: id.clone(),
                        agent_id: agent_id.clone(),
                        prompt: prompt.clone(),
                        schedule_str: normalized_schedule.clone(),
                        kind: aria_intelligence::ScheduledJobKind::Orchestrate,
                        schedule: spec,
                        session_id: self.session_id,
                        user_id: self.user_id.clone(),
                        channel: self.channel,
                    };
                    self
                        .tx_cron
                        .send(aria_intelligence::CronCommand::Add(job))
                        .await
                        .map_err(|_| {
                            OrchestratorError::ToolError(
                                "Scheduler is unavailable; cannot add or update job".into(),
                            )
                        })?;

                    if let Ok(config_path) = std::env::var("ARIA_CONFIG_PATH")
                        .or_else(|_| Ok::<_, ()>("config.toml".into()))
                    {
                        if let Ok(content) = std::fs::read_to_string(&config_path) {
                            if let Ok(mut doc) = content.parse::<toml_edit::DocumentMut>() {
                                let mut updated = false;
                                if let Some(jobs) = doc
                                    .get_mut("scheduler")
                                    .and_then(|i| i.get_mut("jobs"))
                                    .and_then(|i| i.as_array_of_tables_mut())
                                {
                                    for table in jobs.iter_mut() {
                                        if table.get("id").and_then(|v| v.as_str()) == Some(&id) {
                                            table["agent_id"] = toml_edit::value(agent_id.clone());
                                            table["prompt"] = toml_edit::value(prompt.clone());
                                            table["schedule"] =
                                                toml_edit::value(normalized_schedule.clone());
                                            updated = true;
                                            break;
                                        }
                                    }
                                    if !updated {
                                        let mut new_table = toml_edit::Table::new();
                                        new_table.insert("id", toml_edit::value(id.clone()));
                                        new_table
                                            .insert("agent_id", toml_edit::value(agent_id.clone()));
                                        new_table
                                            .insert("prompt", toml_edit::value(prompt.clone()));
                                        new_table.insert(
                                            "schedule",
                                            toml_edit::value(normalized_schedule.clone()),
                                        );
                                        if let Some(sid) = self.session_id {
                                            new_table.insert(
                                                "session_id",
                                                toml_edit::value(hex::encode(sid)),
                                            );
                                        }
                                        if let Some(uid) = &self.user_id {
                                            new_table
                                                .insert("user_id", toml_edit::value(uid.clone()));
                                        }
                                        if let Some(ch) = self.channel {
                                            new_table.insert(
                                                "channel",
                                                toml_edit::value(format!("{:?}", ch)),
                                            );
                                        }
                                        jobs.push(new_table);
                                    }
                                    let _ = std::fs::write(&config_path, doc.to_string());
                                }
                            }
                        }
                    }
                    return Ok(format!("Cron {} set and pushed to config.toml", id));
                }
                Err(OrchestratorError::ToolError("Invalid action".into()))
            }
            "compact_session" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let threshold =
                    args.get("threshold").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let session_id = self
                    .session_id
                    .ok_or_else(|| OrchestratorError::ToolError("No session context".into()))?;

                if let Some(mem) = &self.session_memory {
                    let summarized = mem
                        .summarize_if_over_threshold(
                            uuid::Uuid::from_bytes(session_id),
                            threshold,
                            chrono::Utc::now().timestamp_micros() as u64,
                            |msgs| {
                                let mut full_text = String::new();
                                for m in msgs {
                                    full_text.push_str(&m.content);
                                    full_text.push('\n');
                                }
                                format!(
                                    "Summary of previous session: {}",
                                    full_text.chars().take(500).collect::<String>()
                                )
                            },
                        )
                        .map_err(|e| {
                            OrchestratorError::ToolError(format!("Compaction failed: {}", e))
                        })?;

                    if summarized {
                        Ok("Session compacted successfully.".to_string())
                    } else {
                        Ok("Session under threshold, no compaction needed.".to_string())
                    }
                } else {
                    Err(OrchestratorError::ToolError(
                        "Session storage unavailable".into(),
                    ))
                }
            }
            "grant_access" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let principal = args
                    .get("principal")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| OrchestratorError::ToolError("Missing principal".into()))?;
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| OrchestratorError::ToolError("Missing action".into()))?;
                let resource = args
                    .get("resource")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| OrchestratorError::ToolError("Missing resource".into()))?;

                let rule = format!("\npermit(principal == Agent::\"{}\", action == Action::\"{}\", resource == Resource::\"{}\");", principal, action, resource);

                if let Ok(config_path) =
                    std::env::var("ARIA_CONFIG_PATH").or_else(|_| Ok::<_, ()>("config.toml".into()))
                {
                    if let Ok(content) = std::fs::read_to_string(&config_path) {
                        if let Ok(doc) = content.parse::<toml_edit::DocumentMut>() {
                            if let Some(policy_path) = doc
                                .get("policy")
                                .and_then(|p| p.get("path"))
                                .and_then(|v| v.as_str())
                            {
                                let mut policy_content = std::fs::read_to_string(policy_path)
                                    .map_err(|e| {
                                        OrchestratorError::ToolError(format!(
                                            "Failed to read policy: {}",
                                            e
                                        ))
                                    })?;
                                policy_content.push_str(&rule);
                                std::fs::write(policy_path, &policy_content).map_err(|e| {
                                    OrchestratorError::ToolError(format!(
                                        "Failed to write policy: {}",
                                        e
                                    ))
                                })?;
                                return Ok(format!("Access granted: {}", rule));
                            }
                        }
                    }
                }
                Err(OrchestratorError::ToolError(
                    "Policy configuration unavailable".into(),
                ))
            }
            "manage_prompts" => {
                let args: serde_json::Value = serde_json::from_str(&call.arguments)
                    .map_err(|e| OrchestratorError::ToolError(format!("Invalid args: {}", e)))?;
                let action = args
                    .get("action")
                    .and_then(|v| v.as_str())
                    .unwrap_or("list");
                let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let template = args.get("template").and_then(|v| v.as_str()).unwrap_or("");

                if let Ok(config_path) =
                    std::env::var("ARIA_CONFIG_PATH").or_else(|_| Ok::<_, ()>("config.toml".into()))
                {
                    if let Ok(content) = std::fs::read_to_string(&config_path) {
                        if let Ok(mut doc) = content.parse::<toml_edit::DocumentMut>() {
                            if action == "list" {
                                let prompts = doc
                                    .get("prompts")
                                    .and_then(|p| p.as_table())
                                    .map(|t| t.to_string())
                                    .unwrap_or_else(|| "No prompts found.".to_string());
                                return Ok(prompts);
                            }
                            if action == "add" {
                                if name.is_empty() || template.is_empty() {
                                    return Err(OrchestratorError::ToolError(
                                        "Missing name or template".into(),
                                    ));
                                }
                                doc["prompts"][name] = toml_edit::value(template);
                                let _ = std::fs::write(&config_path, doc.to_string());
                                return Ok(format!("Prompt '{}' added successfully.", name));
                            }
                            if action == "remove" {
                                if name.is_empty() {
                                    return Err(OrchestratorError::ToolError(
                                        "Missing name".into(),
                                    ));
                                }
                                if let Some(prompts) =
                                    doc.get_mut("prompts").and_then(|v| v.as_table_mut())
                                {
                                    prompts.remove(name);
                                    let _ = std::fs::write(&config_path, doc.to_string());
                                    return Ok(format!("Prompt '{}' removed successfully.", name));
                                }
                            }
                        }
                    }
                }
                Err(OrchestratorError::ToolError("Config unavailable".into()))
            }
            _ => Err(OrchestratorError::ToolError(format!(
                "NativeToolExecutor does not support: {}",
                call.name
            ))),
        }
    }
}

pub struct MultiplexToolExecutor {
    wasm: WasmToolExecutor,
    native: NativeToolExecutor,
}

impl MultiplexToolExecutor {
    fn new(
        vault: Arc<aria_vault::CredentialVault>,
        agent_id: String,
        session_id: aria_core::Uuid,
        user_id: String,
        channel: aria_core::GatewayChannel,
        tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
        session_memory: aria_ssmu::SessionMemory,
        cedar: Arc<aria_policy::CedarEvaluator>,
        scheduling_intent: Option<SchedulingIntent>,
        user_timezone: chrono_tz::Tz,
    ) -> Self {
        Self {
            wasm: WasmToolExecutor::new(vault, agent_id, uuid::Uuid::from_bytes(session_id)),
            native: NativeToolExecutor {
                tx_cron,
                session_id: Some(session_id),
                user_id: Some(user_id),
                channel: Some(channel),
                session_memory: Some(session_memory),
                cedar: Some(cedar),
                scheduling_intent,
                user_timezone,
            },
        }
    }
}

#[async_trait::async_trait]
impl ToolExecutor for MultiplexToolExecutor {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        match call.name.as_str() {
            "read_file" | "write_file" | "run_shell" | "search_codebase" | "run_tests"
            | "manage_cron" | "schedule_message" | "set_reminder" | "compact_session"
            | "grant_access" | "manage_prompts" => self.native.execute(call).await,
            _ => self.wasm.execute(call).await,
        }
    }
}

struct PolicyCheckedExecutor<T: ToolExecutor> {
    inner: T,
    cedar: Arc<aria_policy::CedarEvaluator>,
    principal: String,
    channel: aria_core::GatewayChannel,
    whitelist: Vec<String>,
    forbid: Vec<String>,
}

impl<T: ToolExecutor> PolicyCheckedExecutor<T> {
    fn new(
        inner: T,
        cedar: Arc<aria_policy::CedarEvaluator>,
        principal: String,
        channel: aria_core::GatewayChannel,
        whitelist: Vec<String>,
        forbid: Vec<String>,
    ) -> Self {
        Self {
            inner,
            cedar,
            principal,
            channel,
            whitelist,
            forbid,
        }
    }

    fn to_ast_call(call: &ToolCall) -> String {
        let mut ast_args = Vec::new();
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&call.arguments) {
            if let Some(obj) = value.as_object() {
                for (k, v) in obj {
                    let v_str = if let Some(s) = v.as_str() {
                        s.to_string()
                    } else {
                        v.to_string()
                    };
                    let escaped = v_str.replace('"', "\\\"");
                    ast_args.push(format!(r#"{}="{}""#, k, escaped));
                }
            }
        }
        format!("{}({})", call.name, ast_args.join(", "))
    }
}

#[async_trait::async_trait]
impl<T: ToolExecutor> ToolExecutor for PolicyCheckedExecutor<T> {
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
        let ast_call = Self::to_ast_call(call);
        debug!(
            tool = %call.name,
            principal = %self.principal,
            ast_call = %ast_call,
            "PolicyCheckedExecutor: evaluating"
        );
        let parsed = aria_policy::parse_ast_action(&ast_call)
            .map_err(|e| OrchestratorError::ToolError(format!("policy AST parse failed: {}", e)))?;
        let ctx = aria_policy::EvalContext {
            channel: format!("{:?}", self.channel),
            blast_radius: 1, // default
            prompt_origin: self.principal.clone(),
            whitelist: self.whitelist.clone(),
            forbid: self.forbid.clone(),
        };

        let decision = self
            .cedar
            .evaluate_with_context(&self.principal, &parsed.action, &parsed.resource, &ctx)
            .map_err(|e| {
                OrchestratorError::ToolError(format!("policy evaluation failed: {}", e))
            })?;
        if decision == aria_policy::Decision::Deny {
            debug!(
                tool = %call.name,
                action = %parsed.action,
                resource = %parsed.resource,
                "PolicyCheckedExecutor: DENIED"
            );
            return Err(OrchestratorError::ToolError(format!(
                "tool '{}' denied by policy for resource '{}'",
                parsed.action, parsed.resource
            )));
        }
        debug!(tool = %call.name, "PolicyCheckedExecutor: ALLOWED, delegating to executor");
        self.inner.execute(call).await
    }
}

fn build_dynamic_page_index(agent_store: &AgentConfigStore) -> PageIndexTree {
    let mut tree = PageIndexTree::new(32);
    let mut idx = 100u32;

    if agent_store.is_empty() {
        let _ = tree.insert(PageNode {
            node_id: "agent.developer".into(),
            title: "developer Agent".into(),
            summary: "Developer agent fallback".into(),
            start_index: 0,
            end_index: 1,
            children: vec![],
        });
        return tree;
    }

    for cfg in agent_store.all() {
        let node = PageNode {
            node_id: format!("agent.{}", cfg.id),
            title: format!("{} Agent", cfg.id),
            summary: cfg.description.clone(),
            start_index: idx,
            end_index: idx + 1,
            children: vec![],
        };
        let _ = tree.insert(node);
        idx += 1;
    }
    tree
}

fn local_embed(text: &str, dim: usize) -> Vec<f32> {
    let mut vec = vec![0.0_f32; dim];
    if dim == 0 {
        return vec;
    }
    for token in text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.trim().is_empty())
    {
        let normalized = token.to_ascii_lowercase();
        let mut hash = 0_u64;
        for b in normalized.as_bytes() {
            hash = hash.wrapping_mul(16777619).wrapping_add(u64::from(*b));
        }
        let idx = (hash as usize) % dim;
        vec[idx] += 1.0;
    }
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for v in &mut vec {
            *v /= norm;
        }
    }
    vec
}

#[allow(clippy::too_many_arguments)]
use std::future::Future;
use std::pin::Pin;

pub type AsyncHookFn = Box<
    dyn Fn(
            &AgentRequest,
            Arc<VectorStore>,
            Arc<PageIndexTree>,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

pub struct HookRegistry {
    pub message_pre: Vec<AsyncHookFn>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            message_pre: Vec::new(),
        }
    }

    pub fn register_message_pre(&mut self, hook: AsyncHookFn) {
        self.message_pre.push(hook);
    }

    pub async fn execute_message_pre(
        &self,
        req: &AgentRequest,
        vector_store: &Arc<VectorStore>,
        page_index: &Arc<PageIndexTree>,
    ) -> String {
        let mut contexts = Vec::new();
        for hook in &self.message_pre {
            if let Ok(ctx) = hook(req, vector_store.clone(), page_index.clone()).await {
                if !ctx.is_empty() {
                    contexts.push(ctx);
                }
            }
        }
        contexts.join("\n\n")
    }
}

fn scheduled_session_id(job_id: &str) -> [u8; 16] {
    let mut out = [0u8; 16];
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for b in job_id.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    out[0..8].copy_from_slice(&hash.to_le_bytes());
    out[8..16].copy_from_slice(&(!hash).to_le_bytes());
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchedulingMode {
    Notify,
    Defer,
    Both,
}

impl SchedulingMode {
    fn as_tool_mode(self) -> &'static str {
        match self {
            SchedulingMode::Notify => "notify",
            SchedulingMode::Defer => "defer",
            SchedulingMode::Both => "both",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SchedulingIntent {
    mode: SchedulingMode,
    normalized_schedule: Option<String>,
    deferred_task: Option<String>,
    rationale: &'static str,
}

fn parse_time_of_day_expr(expr: &str) -> Option<(u32, u32)> {
    let mut s = expr.trim().to_ascii_lowercase();
    if s.is_empty() || s.contains('*') || s.contains('/') {
        return None;
    }
    s = s.replace(' ', "");
    let (meridian, base) = if let Some(v) = s.strip_suffix("am") {
        ("am", v)
    } else if let Some(v) = s.strip_suffix("pm") {
        ("pm", v)
    } else {
        ("", s.as_str())
    };

    let (hour_raw, minute_raw) = if let Some((h, m)) = base.split_once(':') {
        (h, m)
    } else {
        (base, "0")
    };
    let mut hour = hour_raw.parse::<u32>().ok()?;
    let minute = minute_raw.parse::<u32>().ok()?;
    if minute > 59 {
        return None;
    }

    if !meridian.is_empty() {
        if hour == 0 || hour > 12 {
            return None;
        }
        hour %= 12;
        if meridian == "pm" {
            hour += 12;
        }
    } else if hour > 23 {
        return None;
    }
    Some((hour, minute))
}

fn normalize_schedule_input(raw: &str, now_local: chrono::DateTime<chrono_tz::Tz>) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "1m".to_string();
    }
    // If an absolute datetime is provided without `at:`, interpret it in request-local timezone.
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return format!("at:{}", dt.to_rfc3339());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M:%S")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(trimmed, "%Y-%m-%d %H:%M"))
    {
        use chrono::TimeZone;
        let tz = now_local.timezone();
        if let Some(dt) = tz
            .from_local_datetime(&ndt)
            .single()
            .or_else(|| tz.from_local_datetime(&ndt).earliest())
            .or_else(|| tz.from_local_datetime(&ndt).latest())
        {
            return format!("at:{}", dt.to_rfc3339());
        }
    }
    if let Some(at_text) = trimmed.strip_prefix("at:") {
        let at_text = at_text.trim();
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(at_text) {
            return format!("at:{}", dt.to_rfc3339());
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(at_text, "%Y-%m-%d %H:%M:%S")
            .or_else(|_| chrono::NaiveDateTime::parse_from_str(at_text, "%Y-%m-%d %H:%M"))
        {
            use chrono::TimeZone;
            let tz = now_local.timezone();
            if let Some(dt) = tz
                .from_local_datetime(&ndt)
                .single()
                .or_else(|| tz.from_local_datetime(&ndt).earliest())
                .or_else(|| tz.from_local_datetime(&ndt).latest())
            {
                return format!("at:{}", dt.to_rfc3339());
            }
        }
    }
    if aria_intelligence::ScheduleSpec::parse(trimmed).is_some() {
        return trimmed.to_string();
    }
    if let Some((hour, minute)) = parse_time_of_day_expr(trimmed) {
        let mut target = now_local.date_naive().and_time(
            chrono::NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or(chrono::NaiveTime::MIN),
        );
        if target <= now_local.naive_local() {
            target += chrono::Duration::days(1);
        }
        use chrono::TimeZone;
        let tz = now_local.timezone();
        if let Some(dt) = tz
            .from_local_datetime(&target)
            .single()
            .or_else(|| tz.from_local_datetime(&target).earliest())
            .or_else(|| tz.from_local_datetime(&target).latest())
        {
            return format!("at:{}", dt.to_rfc3339());
        }
    }
    trimmed.to_string()
}

fn sanitize_text_token(raw: &str) -> String {
    raw.trim_matches(|c: char| {
        c.is_whitespace()
            || matches!(c, ',' | '.' | '!' | '?' | ';' | '"' | '\'' | '(' | ')' | '[' | ']')
    })
    .to_ascii_lowercase()
}

fn normalize_duration_pair(value: &str, unit: Option<&str>) -> Option<String> {
    let token = sanitize_text_token(value);
    if token.is_empty() {
        return None;
    }
    if let Some(num) = token
        .strip_suffix("seconds")
        .or_else(|| token.strip_suffix("second"))
    {
        return Some(format!("{}s", num));
    }
    if let Some(num) = token.strip_suffix("secs").or_else(|| token.strip_suffix("sec")) {
        return Some(format!("{}s", num));
    }
    if let Some(num) = token.strip_suffix('s') {
        if num.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}s", num));
        }
    }
    if let Some(num) = token
        .strip_suffix("minutes")
        .or_else(|| token.strip_suffix("minute"))
    {
        return Some(format!("{}m", num));
    }
    if let Some(num) = token.strip_suffix("mins").or_else(|| token.strip_suffix("min")) {
        return Some(format!("{}m", num));
    }
    if let Some(num) = token.strip_suffix('m') {
        if num.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}m", num));
        }
    }
    if let Some(num) = token.strip_suffix("hours").or_else(|| token.strip_suffix("hour")) {
        return Some(format!("{}h", num));
    }
    if let Some(num) = token.strip_suffix("hrs").or_else(|| token.strip_suffix("hr")) {
        return Some(format!("{}h", num));
    }
    if let Some(num) = token.strip_suffix('h') {
        if num.chars().all(|c| c.is_ascii_digit()) {
            return Some(format!("{}h", num));
        }
    }
    if token.chars().all(|c| c.is_ascii_digit()) {
        let u = unit.map(sanitize_text_token).unwrap_or_default();
        let suffix = match u.as_str() {
            "second" | "seconds" | "sec" | "secs" | "s" => "s",
            "minute" | "minutes" | "min" | "mins" | "m" => "m",
            "hour" | "hours" | "hr" | "hrs" | "h" => "h",
            _ => return None,
        };
        return Some(format!("{}{}", token, suffix));
    }
    None
}

fn extract_schedule_hint(text: &str, now_local: chrono::DateTime<chrono_tz::Tz>) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let words_lower: Vec<String> = words.iter().map(|w| sanitize_text_token(w)).collect();

    for i in 0..words_lower.len() {
        let token = words_lower[i].as_str();
        if token == "in" || token == "after" {
            if let Some(next) = words.get(i + 1) {
                let unit = words.get(i + 2).copied();
                if let Some(duration) = normalize_duration_pair(next, unit) {
                    return Some(duration);
                }
            }
        }
        if let Some(duration) = normalize_duration_pair(words[i], words.get(i + 1).copied()) {
            if text.to_ascii_lowercase().contains(" in ")
                || text.to_ascii_lowercase().contains(" after ")
            {
                return Some(duration);
            }
        }
    }

    let lower = text.to_ascii_lowercase();
    if lower.contains("every day") || lower.contains("daily") || lower.contains("everyday") {
        if let Some(idx) = words_lower.iter().position(|w| w == "at") {
            let candidate = if let Some(next) = words.get(idx + 1) {
                if let Some(next2) = words.get(idx + 2) {
                    let joined = format!("{} {}", next, next2);
                    if parse_time_of_day_expr(&joined).is_some() {
                        joined
                    } else {
                        (*next).to_string()
                    }
                } else {
                    (*next).to_string()
                }
            } else {
                String::new()
            };
            if let Some((hour, minute)) = parse_time_of_day_expr(&candidate) {
                return Some(format!("daily@{:02}:{:02}", hour, minute));
            }
        }
    }

    const WEEKDAYS: [(&str, &str); 7] = [
        ("monday", "mon"),
        ("tuesday", "tue"),
        ("wednesday", "wed"),
        ("thursday", "thu"),
        ("friday", "fri"),
        ("saturday", "sat"),
        ("sunday", "sun"),
    ];
    for (full, short) in WEEKDAYS {
        if lower.contains(full) || lower.contains(short) {
            let biweekly = lower.contains("every two weeks")
                || lower.contains("every 2 weeks")
                || lower.contains("alternate ")
                || lower.contains("every other ");
            if let Some(idx) = words_lower.iter().position(|w| w == "at") {
                let candidate = if let Some(next) = words.get(idx + 1) {
                    if let Some(next2) = words.get(idx + 2) {
                        let joined = format!("{} {}", next, next2);
                        if parse_time_of_day_expr(&joined).is_some() {
                            joined
                        } else {
                            (*next).to_string()
                        }
                    } else {
                        (*next).to_string()
                    }
                } else {
                    String::new()
                };
                if let Some((hour, minute)) = parse_time_of_day_expr(&candidate) {
                    let prefix = if biweekly { "biweekly" } else { "weekly" };
                    return Some(format!("{}:{}@{:02}:{:02}", prefix, short, hour, minute));
                }
            }
        }
    }

    if let Some(idx) = words_lower.iter().position(|w| w == "at") {
        let candidate = if let Some(next) = words.get(idx + 1) {
            if let Some(next2) = words.get(idx + 2) {
                let joined = format!("{} {}", next, next2);
                if parse_time_of_day_expr(&joined).is_some() {
                    joined
                } else {
                    (*next).to_string()
                }
            } else {
                (*next).to_string()
            }
        } else {
            String::new()
        };
        if !candidate.is_empty() {
            let normalized = normalize_schedule_input(&candidate, now_local);
            if normalized != candidate || aria_intelligence::ScheduleSpec::parse(&normalized).is_some()
            {
                return Some(normalized);
            }
        }
    }

    None
}

fn strip_schedule_phrase(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    for needle in [" in ", " after ", " at ", " every day", " daily", " every "] {
        if let Some(idx) = lower.find(needle) {
            return text[..idx].trim().trim_end_matches(|c: char| c == ',' || c == '.').to_string();
        }
    }
    text.trim().trim_end_matches(|c: char| c == ',' || c == '.').to_string()
}

fn infer_deferred_task(text: &str) -> Option<String> {
    let trimmed = strip_schedule_phrase(text);
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("remind me to ") {
        let offset = trimmed.len() - rest.len();
        return Some(trimmed[offset..].trim().to_string());
    }
    if let Some(rest) = lower.strip_prefix("remind me ") {
        let offset = trimmed.len() - rest.len();
        return Some(trimmed[offset..].trim().to_string());
    }
    Some(trimmed)
}

fn classify_scheduling_intent(
    text: &str,
    now_local: chrono::DateTime<chrono_tz::Tz>,
) -> Option<SchedulingIntent> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let schedule_hint = extract_schedule_hint(trimmed, now_local);
    let has_schedule = schedule_hint.is_some();
    let has_reminder_words = lower.contains("remind")
        || lower.contains("reminder")
        || lower.contains("notify me")
        || lower.contains("notification");
    let has_immediate_words = lower.contains(" now ")
        || lower.starts_with("now ")
        || lower.contains(" immediately")
        || lower.contains(" right now");

    if !has_schedule && !has_reminder_words {
        return None;
    }

    let mode = if has_schedule && has_immediate_words {
        SchedulingMode::Both
    } else if has_reminder_words {
        SchedulingMode::Notify
    } else {
        SchedulingMode::Defer
    };
    let rationale = match mode {
        SchedulingMode::Notify => "explicit reminder language",
        SchedulingMode::Defer => "delayed work request without reminder phrasing",
        SchedulingMode::Both => "request contains immediate and delayed cues",
    };

    Some(SchedulingIntent {
        mode,
        normalized_schedule: schedule_hint.map(|s| normalize_schedule_input(&s, now_local)),
        deferred_task: infer_deferred_task(trimmed),
        rationale,
    })
}

fn scheduling_intent_context(intent: &SchedulingIntent) -> String {
    let mut lines = vec![
        "<request_classifier>".to_string(),
        format!("scheduling_intent=true"),
        format!("mode={}", intent.mode.as_tool_mode()),
        format!("rationale={}", intent.rationale),
    ];
    if let Some(schedule) = &intent.normalized_schedule {
        lines.push(format!("normalized_schedule={}", schedule));
    }
    if let Some(task) = &intent.deferred_task {
        lines.push(format!("deferred_task={}", task));
    }
    lines.push(
        "When using schedule_message/set_reminder, respect this classified mode unless the user explicitly asked otherwise.".to_string(),
    );
    lines.push("</request_classifier>".to_string());
    lines.join("\n")
}

fn parse_tz_or_utc(tz_name: &str) -> chrono_tz::Tz {
    tz_name
        .trim()
        .parse::<chrono_tz::Tz>()
        .unwrap_or(chrono_tz::UTC)
}

fn resolve_request_timezone(config: &Config, user_id: &str) -> chrono_tz::Tz {
    if let Some(tz_name) = config.localization.user_timezones.get(user_id) {
        return parse_tz_or_utc(tz_name);
    }
    parse_tz_or_utc(&config.localization.default_timezone)
}

fn resolve_request_timezone_with_overrides(
    config: &Config,
    user_id: &str,
    overrides: Option<&dashmap::DashMap<String, String>>,
) -> chrono_tz::Tz {
    if let Some(map) = overrides {
        if let Some(tz_name) = map.get(user_id) {
            return parse_tz_or_utc(tz_name.value());
        }
    }
    resolve_request_timezone(config, user_id)
}

fn persist_user_timezone_override(
    runtime_config_path: &std::path::Path,
    config: &Config,
    user_id: &str,
    tz_name: Option<&str>,
) -> Result<(), String> {
    let mut root = if runtime_config_path.exists() {
        let content = std::fs::read_to_string(runtime_config_path)
            .map_err(|e| format!("failed to read runtime config: {}", e))?;
        serde_json::from_str::<serde_json::Value>(&content)
            .map_err(|e| format!("failed to parse runtime config: {}", e))?
    } else {
        serde_json::to_value(config).map_err(|e| format!("failed to seed runtime config: {}", e))?
    };

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "runtime config root is not an object".to_string())?;
    let localization = root_obj
        .entry("localization")
        .or_insert_with(|| serde_json::json!({}));
    if !localization.is_object() {
        *localization = serde_json::json!({});
    }
    let localization_obj = localization
        .as_object_mut()
        .ok_or_else(|| "runtime localization is not an object".to_string())?;
    localization_obj
        .entry("default_timezone")
        .or_insert_with(|| serde_json::json!(config.localization.default_timezone.clone()));
    let user_timezones = localization_obj
        .entry("user_timezones")
        .or_insert_with(|| serde_json::json!({}));
    if !user_timezones.is_object() {
        *user_timezones = serde_json::json!({});
    }
    let tz_obj = user_timezones
        .as_object_mut()
        .ok_or_else(|| "runtime localization.user_timezones is not an object".to_string())?;
    match tz_name {
        Some(tz) => {
            tz_obj.insert(user_id.to_string(), serde_json::Value::String(tz.to_string()));
        }
        None => {
            tz_obj.remove(user_id);
        }
    }

    if let Some(parent) = runtime_config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create runtime config dir: {}", e))?;
    }
    let tmp = runtime_config_path.with_extension("runtime.json.tmp");
    let json = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("failed to serialize runtime config: {}", e))?;
    std::fs::write(&tmp, json).map_err(|e| format!("failed to write temp runtime config: {}", e))?;
    std::fs::rename(&tmp, runtime_config_path)
        .map_err(|e| format!("failed to replace runtime config: {}", e))?;
    Ok(())
}

fn looks_like_tool_payload(text: &str) -> bool {
    let start = match text.find('{') {
        Some(i) => i,
        None => return false,
    };
    let candidate = &text[start..];
    let parsed = serde_json::from_str::<serde_json::Value>(candidate).ok();
    let Some(v) = parsed else {
        return false;
    };
    let Some(obj) = v.as_object() else {
        return false;
    };
    let has_tool_key = ["tool", "name", "function", "fn", "action"]
        .iter()
        .any(|k| obj.get(*k).and_then(|x| x.as_str()).is_some());
    let has_args_key = ["args", "arguments", "parameters", "input", "params"]
        .iter()
        .any(|k| obj.contains_key(*k));
    has_tool_key && has_args_key
}

/// Universal response dispatcher that routes messages back to the originating channel.
async fn send_universal_response(req: &AgentRequest, text: &str, config: &Config) {
    match req.channel {
        aria_core::GatewayChannel::Telegram => {
            if let Ok(token) = resolve_telegram_token(&config.gateway) {
                let url = format!("https://api.telegram.org/bot{}/sendMessage", token);
                let chat_id =
                    i64::from_le_bytes(req.session_id[0..8].try_into().unwrap_or([0u8; 8]));

                let client = reqwest::Client::new();
                let body = serde_json::json!({
                    "chat_id": chat_id,
                    "text": text,
                    "parse_mode": "HTML"
                });
                let _ = client.post(&url).json(&body).send().await;
            }
        }
        _ => {
            // For CLI and others, print to stdout.
            println!("\n[aria-x] Agent: {}", text);
            let _ = io::stdout().flush();
        }
    }
}

async fn process_request(
    req: &AgentRequest,
    _router_index: &RouterIndex,
    embedder: &impl EmbeddingModel,
    llm_pool: &Arc<LlmBackendPool>,
    cedar: &Arc<aria_policy::CedarEvaluator>,
    agent_store: &AgentConfigStore,
    tool_registry: &ToolManifestStore,
    session_memory: &aria_ssmu::SessionMemory,
    page_index: &Arc<PageIndexTree>,
    vector_store: &Arc<VectorStore>,
    keyword_index: &Arc<KeywordIndex>,
    firewall: &aria_safety::DfaFirewall,
    vault: &Arc<aria_vault::CredentialVault>,
    tx_cron: &tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    provider_registry: &Arc<tokio::sync::Mutex<ProviderRegistry>>,
    session_tool_caches: &mut HashMap<([u8; 16], String), DynamicToolCache>,
    _hooks: &HookRegistry,
    session_locks: &dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>,
    _embed_semaphore: &Arc<tokio::sync::Semaphore>,
    max_rounds: usize,
    steering_rx: Option<&mut tokio::sync::mpsc::Receiver<aria_intelligence::SteeringCommand>>,
    global_estop: Option<&Arc<std::sync::atomic::AtomicBool>>,
    sessions_dir: &std::path::Path,
    whitelist: Vec<String>,
    forbid: Vec<String>,
    user_timezone: chrono_tz::Tz,
) -> Result<aria_intelligence::OrchestratorResult, OrchestratorError> {
    let _started = std::time::Instant::now();
    let request_text = req.content.as_text().unwrap_or_default().to_string();

    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
    let scheduling_intent =
        classify_scheduling_intent(&request_text, chrono::Utc::now().with_timezone(&user_timezone));
    if let Some(intent) = &scheduling_intent {
        debug!(
            mode = %intent.mode.as_tool_mode(),
            schedule = ?intent.normalized_schedule,
            deferred_task = ?intent.deferred_task,
            rationale = %intent.rationale,
            "Scheduling intent classified"
        );
    }
    debug!(
        session_id = %session_uuid,
        user_id = %req.user_id,
        channel = ?req.channel,
        request_text = %request_text,
        "Request received"
    );

    match firewall.scan_ingress(&request_text) {
        aria_safety::ScanResult::Alert(alerts) => {
            warn!(session_id = %session_uuid, alerts = ?alerts, "DfaFirewall blocked ingress payload");
            return Err(OrchestratorError::SecurityViolation(format!(
                "Blocked bad patterns: {:?}",
                alerts
            )));
        }
        aria_safety::ScanResult::Clean => {}
    }

    let session_uuid_str = session_uuid.to_string();
    let session_mutex = session_locks
        .entry(session_uuid_str.clone())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone();
    let _session_guard = session_mutex.lock().await;

    let (override_agent, _override_model) = session_memory
        .get_overrides(&session_uuid)
        .unwrap_or((None, None));

    let agent = if let Some(a) = override_agent {
        info!(override_agent = %a, "Using session agent override");
        a
    } else {
        "omni".to_string()
    };

    info!(agent = %agent, "Routed to agent");

    let policy_executor = PolicyCheckedExecutor::new(
        MultiplexToolExecutor::new(
            vault.clone(),
            agent.clone(),
            *session_uuid.as_bytes(),
            req.user_id.clone(),
            req.channel,
            tx_cron.clone(),
            session_memory.clone(),
            cedar.clone(),
            scheduling_intent.clone(),
            user_timezone,
        ),
        cedar.clone(),
        agent.clone(),
        req.channel,
        whitelist,
        forbid,
    );
    let mut override_backend: Option<Arc<dyn LLMBackend>> = None;
    if let Some(combined) = _override_model {
        if let Some((pid, mid)) = combined.split_once(':') {
            let reg = provider_registry.lock().await;
            if let Ok(b) = reg.create_backend(pid, mid) {
                override_backend = Some(Arc::from(b));
                info!(provider = %pid, model = %mid, "Using session model override");
            }
        }
    }

    let orchestrator = AgentOrchestrator::new(
        PoolBackedLLM::new(llm_pool.clone(), override_backend),
        policy_executor,
    );
    let (max_rounds, context_cap, session_ceiling, base_tool_names, system_prompt) = agent_store
        .get(&agent)
        .map(|cfg| {
            (
                cfg.max_tool_rounds,
                cfg.context_cap,
                cfg.session_tool_ceiling,
                cfg.base_tool_names.clone(),
                cfg.system_prompt.clone(),
            )
        })
        .unwrap_or((
            max_rounds,
            8,
            15,
            Vec::new(),
            "You are a helpful AI assistant.".to_string(),
        ));

    let cache = session_tool_caches
        .entry((req.session_id, agent.clone()))
        .or_insert_with(|| {
            debug!(
                session_id = %session_uuid,
                context_cap,
                session_ceiling,
                "DynamicToolCache: new session cache"
            );
            DynamicToolCache::new(context_cap, session_ceiling)
        });
    let is_new_cache = cache.total_seen() == 0;
    if is_new_cache {
        debug!(
            agent = %agent,
            base_tools = ?base_tool_names,
            "DynamicToolCache: injecting base tools + search_tool_registry"
        );
        for tool_name in &base_tool_names {
            let tool = if let Some(t) = tool_registry.get_by_name(tool_name) {
                t
            } else {
                CachedTool {
                    name: tool_name.clone(),
                    description: format!("Base tool '{}'", tool_name),
                    parameters_schema: "{}".into(),
                }
            };
            let _ = cache.insert(tool);
        }
        let _ = cache.insert(CachedTool {
            name: "search_tool_registry".into(),
            description: "Search tool registry and inject best tool.".into(),
            parameters_schema: r#"{"query":"string"}"#.into(),
        });
    }

    let mut history_ctx = String::new();
    let mut durable_constraints_ctx = String::new();

    if let Ok(constraints) = session_memory.get_durable_constraints(&session_uuid) {
        if !constraints.is_empty() {
            durable_constraints_ctx = format!(
                "\n<durable_constraints>\n{}\n</durable_constraints>\n",
                constraints.join("\n")
            );
        }
    }

    if let Ok(hist) = session_memory.get_history(&session_uuid) {
        let hist_len = hist.len();

        // Token-aware auto-compaction trigger
        let mut total_tokens = 0;
        for m in &hist {
            // Rough approximation: 1 token ≈ 4 chars or 1 token ≈ 1 word
            total_tokens += m.content.split_whitespace().count();
        }

        let start_idx;
        let token_limit = 2000; // Trigger compaction when history exceeds ~2K tokens

        if total_tokens > token_limit && hist_len > 3 {
            // We need to compact. Determine how many turns to keep (say, the last 2 turns).
            let keep_turns = 2;
            let remove_count = hist_len.saturating_sub(keep_turns);
            start_idx = remove_count;

            // Extract the old turns for summarization
            let old_turns: Vec<String> = hist
                .iter()
                .take(remove_count)
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect();
            let old_ctx = old_turns.join("\n");

            info!(session_id = %session_uuid, tokens = total_tokens, "Triggering background memory compaction & constraint extraction");

            // Background LLM call for compaction & constraint extraction
            let prompt = format!(
                "You are an AI memory manager. Your task is to analyze the following conversation snippet and extract durable constraints and provide a concise summary.\n\
                1. Identify any long-term durable user constraints (e.g., \"always use Python 3\", \"prefers dark mode\", \"production DB is Postgres 16\"). List them as a JSON array under the key 'durable_constraints' (max 8 items).\n\
                2. Provide a concise summary of what was discussed under the key 'summary'.\n\
                Return ONLY valid JSON. Example: {{\"durable_constraints\": [\"constraint 1\"], \"summary\": \"User asked to build a web server\"}}\n\n\
                Conversation:\n{}",
                old_ctx
            );

            // Sync LLM call on the primary pool for compaction
            // Note: Since we are in the request processing path, we await it here before building prompt.
            if let Ok(LLMResponse::TextAnswer(summary_res)) =
                llm_pool.query_with_fallback(&prompt, &[]).await
            {
                if let Some(json_start) = summary_res.find('{') {
                    if let Some(json_end) = summary_res.rfind('}') {
                        let json_str = &summary_res[json_start..=json_end];
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
                            // Extract durable constraints
                            if let Some(constraints) =
                                parsed.get("durable_constraints").and_then(|v| v.as_array())
                            {
                                for c in constraints {
                                    if let Some(c_str) = c.as_str() {
                                        let _ = session_memory.add_durable_constraint(
                                            session_uuid,
                                            c_str.to_string(),
                                        );
                                    }
                                }
                            }
                            // Extract summary
                            if let Some(summary_text) =
                                parsed.get("summary").and_then(|v| v.as_str())
                            {
                                let summary_msg = aria_ssmu::Message {
                                    role: "system".into(),
                                    content: format!(
                                        "[Previous Conversation Summary]: {}",
                                        summary_text
                                    ),
                                    timestamp_us: req.timestamp_us,
                                };
                                let _ = session_memory.replace_old_history(
                                    session_uuid,
                                    remove_count,
                                    summary_msg,
                                );
                            }
                        }
                    }
                }
            }

            // Refresh constraints for context injection if we extracted new ones
            if let Ok(constraints) = session_memory.get_durable_constraints(&session_uuid) {
                if !constraints.is_empty() {
                    durable_constraints_ctx = format!(
                        "\n<durable_constraints>\n{}\n</durable_constraints>\n",
                        constraints.join("\n")
                    );
                }
            }
        } else {
            // Not exceeding limit, keep everything (or apply dynamic windowing)
            // Just drop old messages that exceed a generous fallback window (e.g. 50)
            let max_window = 50;
            start_idx = hist_len.saturating_sub(max_window);
        }

        // Build history text to pass to LLM
        for m in hist.iter().skip(start_idx) {
            history_ctx.push_str(&format!("{}: {}\n", m.role, m.content));
        }

        debug!(
            session_id = %session_uuid,
            total_history_turns = hist_len,
            windowed_turns = hist_len - start_idx,
            tokens = total_tokens,
            "process_request: loaded session history window"
        );
    } else {
        debug!(session_id = %session_uuid, "process_request: no session history (new user)");
    }

    let hybrid = HybridMemoryEngine::new(vector_store, page_index, QueryPlannerConfig::default())
        .with_keyword_index(keyword_index)
        .retrieve_hybrid(&request_text, &embedder.embed(&request_text), 5, 3, 0.005);
    let vector_hits = hybrid.vector_context.len();
    let page_hits = hybrid.page_context.len();
    let vector_context = hybrid.vector_context.join("\n");
    let page_context = hybrid
        .page_context
        .into_iter()
        .map(|n| format!("- {}: {}", n.title, n.summary))
        .collect::<Vec<_>>()
        .join("\n");
    let rag_context = format!(
        "Plan: {:?}\nVector Context:\n{}\n\nPageIndex Context:\n{}",
        hybrid.plan, vector_context, page_context
    );
    debug!(
        plan = ?hybrid.plan,
        vector_hits,
        page_hits,
        rag_context_len = rag_context.len(),
        "process_request: RAG context built"
    );

    let user_msg = aria_ssmu::Message {
        role: "user".into(),
        content: request_text,
        timestamp_us: req.timestamp_us,
    };
    let _ = session_memory.append(session_uuid, user_msg.clone());
    let _ = session_memory.append_audit_event(sessions_dir, &session_uuid, &user_msg);

    let intent_ctx = scheduling_intent
        .as_ref()
        .map(scheduling_intent_context)
        .unwrap_or_default();
    let tz_ctx = format!(
        "\n<request_timezone>\niana={}\nlocal_now={}\n</request_timezone>\n",
        user_timezone.name(),
        chrono::Utc::now()
            .with_timezone(&user_timezone)
            .format("%Y-%m-%d %H:%M:%S %:z")
    );
    let final_system_prompt =
        format!("{}{}{}{}", system_prompt, durable_constraints_ctx, tz_ctx, intent_ctx);

    // Item 3 – Secrets Audit: scan RAG context and system prompt for sensitive
    // patterns (API keys, tokens) before forwarding to the LLM.
    // Pattern matches are redacted so they never reach the model context.
    let rag_context = match firewall.scan_egress(&rag_context) {
        aria_safety::ScanResult::Alert(patterns) => {
            tracing::warn!(matched = ?patterns, "Firewall blocked RAG context egress — sensitive pattern detected");
            "[RAG context redacted by firewall]".to_string()
        }
        aria_safety::ScanResult::Clean => rag_context,
    };
    let final_system_prompt = match firewall.scan_egress(&final_system_prompt) {
        aria_safety::ScanResult::Alert(patterns) => {
            tracing::warn!(matched = ?patterns, "Firewall blocked system prompt egress — sensitive pattern detected");
            system_prompt.to_string()
        }
        aria_safety::ScanResult::Clean => final_system_prompt,
    };

    let mut orchestrator_result = orchestrator
        .run_for_request_with_dynamic_tools(
            &final_system_prompt,
            req,
            &history_ctx,
            &rag_context,
            cache,
            tool_registry,
            embedder,
            max_rounds,
            steering_rx,
            global_estop,
        )
        .await?;

    let response_text = match &orchestrator_result {
        aria_intelligence::OrchestratorResult::Completed(text) => text.clone(),
        aria_intelligence::OrchestratorResult::ToolApprovalRequired { .. } => "".to_string(),
    };
    if matches!(
        orchestrator_result,
        aria_intelligence::OrchestratorResult::Completed(_)
    ) && looks_like_tool_payload(&response_text)
    {
        warn!(
            session_id = %session_uuid,
            preview = %response_text.chars().take(160).collect::<String>(),
            "Suppressing leaked internal tool payload from user-visible response"
        );
        orchestrator_result = aria_intelligence::OrchestratorResult::Completed(
            "I hit an internal tool-execution formatting issue. Please retry your request.".to_string(),
        );
    }

    match firewall.scan_egress(&response_text) {
        aria_safety::ScanResult::Alert(alerts) => {
            warn!(session_id = %session_uuid, alerts = ?alerts, "DfaFirewall blocked egress payload");
            return Err(OrchestratorError::SecurityViolation(format!(
                "Blocked bad patterns in egress: {:?}",
                alerts
            )));
        }
        aria_safety::ScanResult::Clean => {}
    }

    // Only append to history if the LLM completed its turn
    if let aria_intelligence::OrchestratorResult::Completed(ref response_text) = orchestrator_result
    {
        let assistant_msg = aria_ssmu::Message {
            role: "assistant".into(),
            content: response_text.clone(),
            timestamp_us: req.timestamp_us,
        };
        let _ = session_memory.append(session_uuid, assistant_msg.clone());
        let _ = session_memory.append_audit_event(sessions_dir, &session_uuid, &assistant_msg);
    }

    Ok(orchestrator_result)
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use aria_intelligence::{AgentConfig, LocalHashEmbedder};
    use std::sync::Mutex;

    fn base_test_config() -> Config {
        toml::from_str(
            r#"
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
            "#,
        )
        .expect("parse test config")
    }

    #[derive(Clone)]
    struct PromptCaptureLLM {
        last_prompt: Arc<Mutex<Option<String>>>,
        answer: String,
    }

    #[async_trait::async_trait]
    impl LLMBackend for PromptCaptureLLM {
        async fn query(
            &self,
            prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            let mut guard = self.last_prompt.lock().expect("prompt lock poisoned");
            *guard = Some(prompt.to_string());
            Ok(LLMResponse::TextAnswer(self.answer.clone()))
        }
    }

    #[tokio::test]
    async fn process_request_runs_full_per_request_flow() {
        let embedder = LocalHashEmbedder::new(64);
        let mut router = SemanticRouter::new();
        router
            .register_agent_text("developer", "rust project workspace code", &embedder)
            .expect("register agent");
        let router_index = router.build_index(RouteConfig {
            confidence_threshold: 0.1,
            tie_break_gap: 0.01,
        });

        let mut agent_store = AgentConfigStore::new();
        agent_store.insert(AgentConfig {
            id: "developer".into(),
            description: "Rust coding agent".into(),
            system_prompt: "You are a developer.".into(),
            base_tool_names: vec!["read_file".into()],
            context_cap: 8,
            session_tool_ceiling: 15,
            max_tool_rounds: 5,
            fallback_agent: None,
        });

        let mut tool_registry = ToolManifestStore::new();
        tool_registry.register(CachedTool {
            name: "search_tool_registry".into(),
            description: "Search tools".into(),
            parameters_schema: r#"{"query":"string"}"#.into(),
        });

        let captured = Arc::new(Mutex::new(None));
        let llm_pool = LlmBackendPool::new(vec!["primary".into()], Duration::from_millis(100));
        llm_pool.register_backend(
            "primary",
            Box::new(PromptCaptureLLM {
                last_prompt: captured.clone(),
                answer: "done".into(),
            }),
        );
        let llm_pool = Arc::new(llm_pool);

        let cedar = Arc::new(
            aria_policy::CedarEvaluator::from_policy_str("").expect("empty policy should parse"),
        );
        let session_memory = aria_ssmu::SessionMemory::new(100);
        let page_index = build_dynamic_page_index(&agent_store);
        let mut vector_store = VectorStore::new();
        vector_store.index_document(
            "workspace.files",
            "List files and source context",
            super::local_embed("list files source", 64),
            "workspace",
            vec!["files".into(), "source".into()],
            false,
        );
        let mut session_tool_caches = HashMap::new();

        let session_id = uuid::Uuid::new_v4().into_bytes();
        let session_uuid = uuid::Uuid::from_bytes(session_id);
        session_memory
            .append(
                session_uuid,
                aria_ssmu::Message {
                    role: "assistant".into(),
                    content: "earlier context".into(),
                    timestamp_us: 1,
                },
            )
            .expect("append history");

        let req = AgentRequest {
            request_id: uuid::Uuid::new_v4().into_bytes(),
            session_id,
            channel: GatewayChannel::Cli,
            user_id: "u1".into(),
            content: MessageContent::Text("list workspace files".into()),
            timestamp_us: 2,
        };

        let dummy_hooks = HookRegistry::new();
        let kw_index = Arc::new(KeywordIndex::new().expect("keyword index"));
        let res = process_request(
            &req,
            &router_index,
            &embedder,
            &llm_pool,
            &cedar,
            &agent_store,
            &tool_registry,
            &session_memory,
            &Arc::new(page_index),
            &Arc::new(vector_store),
            &kw_index,
            &aria_safety::DfaFirewall::new(vec![]),
            &Arc::new(aria_vault::CredentialVault::new(
                "/tmp/test_vault.json",
                [0; 32],
            )),
            &{
                let (tx, _rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(1);
                tx
            },
            &Arc::new(tokio::sync::Mutex::new(ProviderRegistry::new())), // Pass dummy registry
            &mut session_tool_caches,
            &dummy_hooks,
            &dashmap::DashMap::new(),
            &Arc::new(tokio::sync::Semaphore::new(2)),
            5,
            None, // steering_rx
            Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
            std::path::Path::new("/tmp/test_sessions"),
            vec!["/workspace/".into()],
            vec![],
            chrono_tz::UTC,
        )
        .await
        .expect("process request");

        assert_eq!(
            res,
            aria_intelligence::OrchestratorResult::Completed("done".to_string())
        );

        let prompt = captured
            .lock()
            .expect("prompt lock poisoned")
            .clone()
            .unwrap_or_default();
        assert!(
            prompt.contains("earlier context"),
            "history should be loaded"
        );

        let hist = session_memory
            .get_history(&session_uuid)
            .expect("history should exist");
        assert!(hist.iter().any(|m| m.role == "user"));
        assert!(hist
            .iter()
            .any(|m| m.role == "assistant" && m.content == "done"));
    }

    #[tokio::test]
    async fn process_request_injects_scheduling_classifier_context_into_prompt() {
        let embedder = LocalHashEmbedder::new(64);
        let router = SemanticRouter::new();
        let router_index = router.build_index(RouteConfig {
            confidence_threshold: 0.1,
            tie_break_gap: 0.01,
        });

        let mut agent_store = AgentConfigStore::new();
        agent_store.insert(AgentConfig {
            id: "omni".into(),
            description: "General agent".into(),
            system_prompt: "You are omni.".into(),
            base_tool_names: vec!["schedule_message".into()],
            context_cap: 8,
            session_tool_ceiling: 15,
            max_tool_rounds: 5,
            fallback_agent: None,
        });

        let mut tool_registry = ToolManifestStore::new();
        tool_registry.register(CachedTool {
            name: "schedule_message".into(),
            description: "Schedule reminder behavior".into(),
            parameters_schema: "{}".into(),
        });
        tool_registry.register(CachedTool {
            name: "search_tool_registry".into(),
            description: "Search tools".into(),
            parameters_schema: r#"{"query":"string"}"#.into(),
        });

        let captured = Arc::new(Mutex::new(None));
        let llm_pool = LlmBackendPool::new(vec!["primary".into()], Duration::from_millis(100));
        llm_pool.register_backend(
            "primary",
            Box::new(PromptCaptureLLM {
                last_prompt: captured.clone(),
                answer: "scheduled".into(),
            }),
        );
        let llm_pool = Arc::new(llm_pool);

        let cedar = Arc::new(
            aria_policy::CedarEvaluator::from_policy_str("").expect("empty policy should parse"),
        );
        let session_memory = aria_ssmu::SessionMemory::new(100);
        let page_index = Arc::new(build_dynamic_page_index(&agent_store));
        let vector_store = Arc::new(VectorStore::new());
        let keyword_index = Arc::new(KeywordIndex::new().expect("keyword index"));
        let firewall = aria_safety::DfaFirewall::new(vec![]);
        let vault = Arc::new(aria_vault::CredentialVault::new(
            "/tmp/test_vault_classifier.json",
            [7u8; 32],
        ));
        let (tx_cron, _rx_cron) = tokio::sync::mpsc::channel(4);
        let provider_registry = Arc::new(tokio::sync::Mutex::new(ProviderRegistry::new()));
        let mut session_tool_caches = HashMap::new();
        let hooks = HookRegistry::new();
        let session_locks = dashmap::DashMap::new();
        let embed_semaphore = Arc::new(tokio::sync::Semaphore::new(1));
        let req = AgentRequest {
            request_id: [1; 16],
            session_id: [2; 16],
            channel: GatewayChannel::Telegram,
            user_id: "u1".into(),
            content: MessageContent::Text("Provide me with a random number in 1min".into()),
            timestamp_us: 42,
        };

        let _ = process_request(
            &req,
            &router_index,
            &embedder,
            &llm_pool,
            &cedar,
            &agent_store,
            &tool_registry,
            &session_memory,
            &page_index,
            &vector_store,
            &keyword_index,
            &firewall,
            &vault,
            &tx_cron,
            &provider_registry,
            &mut session_tool_caches,
            &hooks,
            &session_locks,
            &embed_semaphore,
            5,
            None,
            Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
            std::path::Path::new("/tmp/test_sessions_classifier"),
            vec!["/workspace/".into()],
            vec![],
            chrono_tz::UTC,
        )
        .await
        .expect("process request");

        let prompt = captured
            .lock()
            .expect("prompt lock poisoned")
            .clone()
            .unwrap_or_default();
        assert!(prompt.contains("<request_classifier>"));
        assert!(prompt.contains("mode=defer"));
        assert!(prompt.contains("normalized_schedule=1m"));
    }

    #[tokio::test]
    async fn schedule_message_tool_returns_error_when_scheduler_unavailable() {
        let (tx, rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(1);
        drop(rx); // simulate scheduler not running

        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Cli),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Ping me","delay":"1m","agent_id":"developer"}"#.into(),
            })
            .await;

        assert!(result.is_err(), "must fail if scheduler queue is unavailable");
        let err = format!("{}", result.err().expect("error"));
        assert!(
            err.contains("Scheduler is unavailable"),
            "unexpected error: {}",
            err
        );
    }

    #[tokio::test]
    async fn schedule_message_tool_enqueues_job_with_agent_and_context() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let (job_tx, job_rx) = tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });

        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(session_id),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Daily check-in","delay":"daily@19:30","agent_id":"communicator"}"#
                    .into(),
            })
            .await
            .expect("tool should enqueue successfully");

        assert!(result.contains("Scheduled reminder notification"));
        let job = job_rx.await.expect("expected Add job command");
        assert_eq!(job.prompt, "Daily check-in");
        assert_eq!(job.agent_id, "communicator");
        assert_eq!(job.session_id, Some(session_id));
        assert_eq!(job.user_id.as_deref(), Some("u1"));
        assert_eq!(job.channel, Some(GatewayChannel::Telegram));
        assert_eq!(job.kind, aria_intelligence::ScheduledJobKind::Notify);
        assert!(matches!(
            job.schedule,
            aria_intelligence::ScheduleSpec::DailyAt {
                hour: 19,
                minute: 30
            }
        ));
    }

    #[test]
    fn scheduled_session_id_is_stable_per_job_id() {
        let a1 = scheduled_session_id("job-123");
        let a2 = scheduled_session_id("job-123");
        let b = scheduled_session_id("job-999");
        assert_eq!(a1, a2);
        assert_ne!(a1, b);
    }

    #[test]
    fn parse_time_of_day_expr_supports_12h_and_24h() {
        assert_eq!(parse_time_of_day_expr("8:15 PM"), Some((20, 15)));
        assert_eq!(parse_time_of_day_expr("08:15"), Some((8, 15)));
        assert_eq!(parse_time_of_day_expr("8pm"), Some((20, 0)));
        assert_eq!(parse_time_of_day_expr("25:00"), None);
    }

    #[test]
    fn normalize_schedule_input_converts_plain_time_to_one_shot_at() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-28T18:00:00+05:30")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Asia::Kolkata);
        let normalized = normalize_schedule_input("8:15 PM", now);
        assert!(normalized.starts_with("at:"));
        assert!(
            aria_intelligence::ScheduleSpec::parse(&normalized).is_some(),
            "normalized schedule should parse: {}",
            normalized
        );
    }

    #[test]
    fn normalize_schedule_input_respects_request_timezone_offset() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-06T18:00:00+01:00")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Europe::Zurich);
        let normalized = normalize_schedule_input("10:00 PM", now);
        assert_eq!(normalized, "at:2026-03-06T22:00:00+01:00");
    }

    #[test]
    fn normalize_schedule_input_converts_bare_absolute_datetime_to_localized_at() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-07T02:07:18+05:30")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Asia::Kolkata);
        let normalized = normalize_schedule_input("2026-03-07 02:09:00", now);
        assert_eq!(normalized, "at:2026-03-07T02:09:00+05:30");
    }

    #[test]
    fn looks_like_tool_payload_detects_tool_json_only() {
        assert!(looks_like_tool_payload(
            r#"{"tool":"run_shell","args":{"command":"echo hi"}}"#
        ));
        assert!(!looks_like_tool_payload("Random number: 42"));
        assert!(!looks_like_tool_payload(r#"{"message":"hello"}"#));
    }

    #[test]
    fn classify_scheduling_intent_prefers_defer_for_delayed_work() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-06T20:44:29+05:30")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Asia::Kolkata);
        let intent = classify_scheduling_intent("Provide me with a random number in 1min", now)
            .expect("expected scheduling intent");
        assert_eq!(intent.mode, SchedulingMode::Defer);
        assert_eq!(intent.normalized_schedule.as_deref(), Some("1m"));
        assert_eq!(
            intent.deferred_task.as_deref(),
            Some("Provide me with a random number")
        );
    }

    #[test]
    fn classify_scheduling_intent_prefers_notify_for_reminders() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-06T20:44:29+05:30")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Asia::Kolkata);
        let intent = classify_scheduling_intent("Remind me to drink water in 1 minute", now)
            .expect("expected scheduling intent");
        assert_eq!(intent.mode, SchedulingMode::Notify);
        assert_eq!(intent.normalized_schedule.as_deref(), Some("1m"));
    }

    #[test]
    fn classify_scheduling_intent_detects_now_plus_later_as_both() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-03-06T20:44:29+05:30")
            .expect("rfc3339")
            .with_timezone(&chrono_tz::Asia::Kolkata);
        let intent = classify_scheduling_intent(
            "Generate a random number now and remind me again in 1 minute",
            now,
        )
        .expect("expected scheduling intent");
        assert_eq!(intent.mode, SchedulingMode::Both);
        assert_eq!(intent.normalized_schedule.as_deref(), Some("1m"));
    }

    #[test]
    fn validate_config_rejects_invalid_localization_timezone() {
        let mut cfg = base_test_config();
        cfg.localization.default_timezone = "Invalid/Timezone".into();
        let err = validate_config(&cfg).expect_err("invalid timezone should fail config validation");
        assert!(err.contains("localization.default_timezone"));
    }

    #[test]
    fn resolve_request_timezone_uses_user_timezone_override() {
        let mut cfg = base_test_config();
        cfg.localization.default_timezone = "Europe/Zurich".into();
        cfg.localization
            .user_timezones
            .insert("user-1".into(), "Asia/Kolkata".into());

        let user_tz = resolve_request_timezone(&cfg, "user-1");
        let fallback_tz = resolve_request_timezone(&cfg, "unknown");

        assert_eq!(user_tz, chrono_tz::Asia::Kolkata);
        assert_eq!(fallback_tz, chrono_tz::Europe::Zurich);
    }

    #[test]
    fn default_timezone_always_resolves_to_valid_iana_or_utc() {
        let tz_name = default_timezone();
        assert!(
            tz_name.parse::<chrono_tz::Tz>().is_ok(),
            "default timezone should be valid IANA tz: {}",
            tz_name
        );
    }

    #[test]
    fn resolve_request_timezone_with_overrides_prefers_runtime_map() {
        let mut cfg = base_test_config();
        cfg.localization.default_timezone = "Europe/Zurich".into();
        cfg.localization
            .user_timezones
            .insert("user-1".into(), "Asia/Kolkata".into());
        let overrides = dashmap::DashMap::new();
        overrides.insert("user-1".into(), "America/New_York".into());

        let tz = resolve_request_timezone_with_overrides(&cfg, "user-1", Some(&overrides));
        assert_eq!(tz, chrono_tz::America::New_York);
    }

    #[test]
    fn persist_user_timezone_override_writes_and_clears_runtime_entry() {
        let cfg = base_test_config();
        let dir = std::env::temp_dir().join(format!("aria-x-tz-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let runtime_path = dir.join("config.runtime.json");

        persist_user_timezone_override(&runtime_path, &cfg, "u1", Some("Asia/Kolkata"))
            .expect("persist timezone");
        let first = std::fs::read_to_string(&runtime_path).expect("read runtime file");
        let first_json: serde_json::Value = serde_json::from_str(&first).expect("parse runtime file");
        assert_eq!(
            first_json["localization"]["user_timezones"]["u1"]
                .as_str()
                .unwrap_or_default(),
            "Asia/Kolkata"
        );

        persist_user_timezone_override(&runtime_path, &cfg, "u1", None).expect("clear timezone");
        let second = std::fs::read_to_string(&runtime_path).expect("read runtime file");
        let second_json: serde_json::Value =
            serde_json::from_str(&second).expect("parse runtime file");
        assert!(
            second_json["localization"]["user_timezones"]
                .get("u1")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn schedule_message_tool_deduplicates_identical_notify_jobs() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let existing = aria_intelligence::ScheduledPromptJob {
                    id: "reminder-existing".into(),
                    agent_id: "developer".into(),
                    prompt: "Drink water".into(),
                    schedule_str: "1m".into(),
                    kind: aria_intelligence::ScheduledJobKind::Notify,
                    schedule: aria_intelligence::ScheduleSpec::EverySeconds(60),
                    session_id: Some(session_id),
                    user_id: Some("u1".into()),
                    channel: Some(GatewayChannel::Telegram),
                };
                let _ = reply.send(vec![existing]);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(session_id),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Drink water","delay":"1m","agent_id":"developer"}"#.into(),
            })
            .await
            .expect("schedule should be deduplicated");

        assert!(result.contains("Already scheduled"));
    }

    #[tokio::test]
    async fn schedule_message_tool_uses_classifier_defer_mode_when_model_omits_mode() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let (job_tx, job_rx) = tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: Some(SchedulingIntent {
                mode: SchedulingMode::Defer,
                normalized_schedule: Some("1m".into()),
                deferred_task: Some("Provide me with a random number".into()),
                rationale: "delayed work request without reminder phrasing",
            }),
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Random number: 42","agent_id":"omni"}"#.into(),
            })
            .await
            .expect("schedule should inherit classifier mode");

        assert!(result.contains("deferred execution"));
        let job = job_rx.await.expect("expected Add job");
        assert_eq!(job.kind, aria_intelligence::ScheduledJobKind::Orchestrate);
        assert_eq!(job.prompt, "Provide me with a random number");
        assert_eq!(job.schedule_str, "1m");
    }

    #[tokio::test]
    async fn schedule_message_tool_falls_back_to_classifier_schedule_when_delay_is_unparseable() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let (job_tx, job_rx) =
            tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: Some(SchedulingIntent {
                mode: SchedulingMode::Notify,
                normalized_schedule: Some("at:2026-03-07T02:15:00+05:30".into()),
                deferred_task: Some("Send contents of new_ok.js".into()),
                rationale: "classified by request parser",
            }),
            user_timezone: chrono_tz::Asia::Kolkata,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Send contents of new_ok.js","delay":"today at 2:15 AM","mode":"notify"}"#.into(),
            })
            .await
            .expect("schedule should use classifier fallback");

        assert!(result.contains("Scheduled reminder notification"));
        let job = job_rx.await.expect("expected Add job");
        assert_eq!(job.schedule_str, "at:2026-03-07T02:15:00+05:30");
    }

    #[tokio::test]
    async fn schedule_message_tool_mode_defer_enqueues_orchestrate_job() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let (job_tx, job_rx) = tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(session_id),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Generate a random number and send it","delay":"1m","mode":"defer","agent_id":"omni"}"#.into(),
            })
            .await
            .expect("schedule defer should succeed");
        assert!(result.contains("deferred execution"));

        let job = job_rx.await.expect("expected Add job");
        assert_eq!(job.kind, aria_intelligence::ScheduledJobKind::Orchestrate);
        assert_eq!(job.prompt, "Generate a random number and send it");
        assert_eq!(job.agent_id, "omni");
    }

    #[tokio::test]
    async fn schedule_message_notify_with_deferred_prompt_prefers_defer_execution() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(2);
        let (job_tx, job_rx) = tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Send contents of new_ok.js","delay":"2m","mode":"notify","deferred_prompt":"Read the contents of new_ok.js and send them to me.","agent_id":"omni"}"#.into(),
            })
            .await
            .expect("schedule should coerce to defer");

        assert!(result.contains("deferred execution"));
        let job = job_rx.await.expect("expected Add job");
        assert_eq!(job.kind, aria_intelligence::ScheduledJobKind::Orchestrate);
        assert_eq!(
            job.prompt,
            "Read the contents of new_ok.js and send them to me."
        );
    }

    #[tokio::test]
    async fn schedule_message_tool_mode_both_enqueues_notify_and_orchestrate() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(3);
        let (kinds_tx, kinds_rx) =
            tokio::sync::oneshot::channel::<Vec<aria_intelligence::ScheduledJobKind>>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::List(reply)) = rx.recv().await {
                let _ = reply.send(Vec::new());
            }
            let mut kinds = Vec::new();
            for _ in 0..2 {
                if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                    kinds.push(job.kind);
                }
            }
            let _ = kinds_tx.send(kinds);
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };

        let result = exec
            .execute(&ToolCall {
                name: "schedule_message".into(),
                arguments: r#"{"task":"Reminder text","deferred_prompt":"Generate random number and send","delay":"1m","mode":"both","agent_id":"omni"}"#.into(),
            })
            .await
            .expect("schedule both should succeed");
        assert!(result.contains("notify + deferred execution"));

        let kinds = kinds_rx.await.expect("expected two Add jobs");
        assert_eq!(kinds.len(), 2);
        assert!(kinds.contains(&aria_intelligence::ScheduledJobKind::Notify));
        assert!(kinds.contains(&aria_intelligence::ScheduledJobKind::Orchestrate));
    }

    #[tokio::test]
    async fn manage_cron_add_without_id_generates_id() {
        let (tx, mut rx) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(1);
        let (job_tx, job_rx) = tokio::sync::oneshot::channel::<aria_intelligence::ScheduledPromptJob>();
        tokio::spawn(async move {
            if let Some(aria_intelligence::CronCommand::Add(job)) = rx.recv().await {
                let _ = job_tx.send(job);
            }
        });
        let exec = NativeToolExecutor {
            tx_cron: tx,
            session_id: Some(*uuid::Uuid::new_v4().as_bytes()),
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Cli),
            session_memory: None,
            cedar: None,
            scheduling_intent: None,
            user_timezone: chrono_tz::UTC,
        };
        let result = exec
            .execute(&ToolCall {
                name: "manage_cron".into(),
                arguments: r#"{"action":"add","prompt":"run report","schedule":"every:60s","agent_id":"developer"}"#.into(),
            })
            .await
            .expect("manage_cron add should succeed");
        assert!(result.contains("Cron cron-"));

        let job = job_rx.await.expect("add job not received");
        assert!(job.id.starts_with("cron-"));
        assert_eq!(job.kind, aria_intelligence::ScheduledJobKind::Orchestrate);
    }
}

// ---------------------------------------------------------------------------
// Telegram webhook gateway
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct TelegramState {
    config: Arc<Config>,
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
    page_index: Arc<PageIndexTree>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    pub tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    pub hooks: Arc<HookRegistry>,
    session_tool_caches: Arc<Mutex<HashMap<([u8; 16], String), DynamicToolCache>>>,
    session_locks: Arc<dashmap::DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    session_steering_tx: Arc<
        dashmap::DashMap<String, tokio::sync::mpsc::Sender<aria_intelligence::SteeringCommand>>,
    >,
    embed_semaphore: Arc<tokio::sync::Semaphore>,
    global_estop: Arc<std::sync::atomic::AtomicBool>,
    provider_registry: Arc<Mutex<ProviderRegistry>>,
    telegram_token: String,
    client: reqwest::Client,
}

impl TelegramState {
    async fn send_telegram_secure_message(
        &self,
        chat_id: i64,
        text: &str,
        reply_markup: Option<serde_json::Value>,
        parse_mode: Option<&str>,
    ) {
        // Egress Leak Scanning
        let final_text = match self.firewall.scan_egress(text) {
            aria_safety::ScanResult::Clean => text.to_string(),
            aria_safety::ScanResult::Alert(patterns) => {
                warn!(patterns = ?patterns, "Egress leak detected! Redacting message.");
                "[REDACTED: SENSITIVE DATA DETECTED]".to_string()
            }
        };

        let url = format!(
            "https://api.telegram.org/bot{}/sendMessage",
            self.telegram_token
        );
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

        match self.client.post(&url).json(&body).send().await {
            Ok(resp) => {
                let status = resp.status();
                if !status.is_success() {
                    let text = resp.text().await.unwrap_or_default();
                    error!(status = %status, body = %text, "Telegram sendMessage error");
                } else {
                    debug!(chat_id = %chat_id, "Sent secure telegram message");
                }
            }
            Err(e) => error!(error = %e, "Telegram sendMessage request failed"),
        }
    }
}

/// Process one Telegram update (webhook payload or one getUpdates result item) and send reply.
async fn process_one_telegram_update(state: &TelegramState, update_json: &str) {
    debug!(update_len = update_json.len(), "Telegram: received update");
    let (req, chat_id) = match TelegramNormalizer::normalize_with_chat_id(update_json) {
        Ok(r) => r,
        Err(e) => {
            debug!(error = %e, "Skip update (parse error)");
            return;
        }
    };
    let text = req.content.as_text().unwrap_or_default();
    let session_id = req.session_id;
    debug!(
        chat_id,
        session_id = %uuid::Uuid::from_bytes(session_id),
        user_id = %req.user_id,
        text = %text,
        "Telegram: normalized to AgentRequest"
    );
    if text.is_empty() {
        return;
    }

    // Handle control commands: /agents, /agent <id>, /model <name>, /session
    if text.starts_with("/agents")
        || text.starts_with("/agent")
        || text.starts_with("/models")
        || text.starts_with("/model")
        || text.starts_with("/timezone")
        || text.starts_with("/session")
        || text.starts_with("/approve")
        || text.starts_with("/deny")
    {
        let session_uuid = uuid::Uuid::from_bytes(session_id);
        let (current_agent, _current_model) = state
            .session_memory
            .get_overrides(&session_uuid)
            .unwrap_or((None, None));

        let mut parse_mode = None;
        let mut reply_markup = None;
        let reply = if text.starts_with("/agent ") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 2 {
                let agent_name = parts[1].to_string();
                if agent_name == "omni" || agent_name == "clear" || agent_name == "reset" {
                    let _ = state
                        .session_memory
                        .update_overrides(session_uuid, None, None);
                    "✅ Override cleared. Session is now using the default **Omni** agent."
                        .to_string()
                } else if state.agent_store.get(&agent_name).is_some() {
                    let _ = state.session_memory.update_overrides(
                        session_uuid,
                        Some(agent_name.clone()),
                        None,
                    );
                    format!("✅ Session override set to agent: **{}**.", agent_name)
                } else {
                    format!("❌ Agent '{}' not found in configuration.", agent_name)
                }
            } else {
                "Usage: /agent <persona_name> (e.g., `/agent communicator`, or `/agent omni` to reset)".to_string()
            }
        } else if text.starts_with("/agents") {
            // List agents
            let mut lines = Vec::new();
            lines.push("<b>Available agents:</b>".to_string());
            let mut keyboard = Vec::new();
            let escape = |s: &str| -> String {
                s.replace("&", "&amp;")
                    .replace("<", "&lt;")
                    .replace(">", "&gt;")
            };
            for cfg in state.agent_store.all() {
                lines.push(format!(
                    "• <b>{}</b>: {}",
                    escape(&cfg.id),
                    escape(&cfg.description)
                ));
                keyboard.push(vec![serde_json::json!({
                    "text": format!("Switch to {}", cfg.id),
                    "callback_data": format!("/agent {}", cfg.id)
                })]);
            }
            if let Some(ref a) = current_agent {
                lines.push(format!("\n<b>Current agent:</b> {}", escape(a)));
            }
            reply_markup = Some(serde_json::json!({ "inline_keyboard": keyboard }));
            parse_mode = Some("HTML");
            lines.join("\n")
        } else if text.starts_with("/agent") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 2 {
                "Usage: /agent <id>".to_string()
            } else {
                let id = parts[1];
                if state.agent_store.get(id).is_some() {
                    let _ = state.session_memory.update_overrides(
                        session_uuid,
                        Some(id.to_string()),
                        None,
                    );
                    format!("Agent set to '{}'.", id)
                } else {
                    format!("Unknown agent '{}'. Use /agents to list.", id)
                }
            }
        } else if text.starts_with("/install_skill") {
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
        } else if text.starts_with("/models") || text.starts_with("/model") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() == 1 || (parts.len() == 2 && parts[1] == "providers") {
                // Step 1: List Providers
                let mut lines = Vec::new();
                lines.push("🌐 <b>Select LLM Provider:</b>".to_string());
                let mut keyboard = Vec::new();
                let reg = state.provider_registry.lock().await;
                for p in reg.providers() {
                    keyboard.push(vec![serde_json::json!({
                        "text": p.name(),
                        "callback_data": format!("/models p {}", p.id())
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
            } else if parts.len() >= 4 && (parts[1] == "switch" || parts[1] == "i") {
                // Step 3: Switch Model
                let provider_id = parts[2];
                let model_id = text
                    .split_whitespace()
                    .skip(3)
                    .collect::<Vec<_>>()
                    .join(" ");
                let reg = state.provider_registry.lock().await;
                let resp = match reg.create_backend(provider_id, &model_id) {
                    Ok(backend) => {
                        state.llm_pool.register_backend("primary", backend.clone());
                        state.llm_pool.register_backend("fallback", backend);

                        // Per-session persistence
                        let combined_model = format!("{}:{}", provider_id, model_id);
                        let _ = state.session_memory.update_overrides(
                            session_uuid,
                            None,
                            Some(combined_model.clone()),
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
                "Usage: /models [providers|p <id>|i <provider> <model>]".to_string()
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
                    || state.config.localization.user_timezones.contains_key(&req.user_id);
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
        } else if text.starts_with("/session") {
            let _ = state.session_memory.clear_history(&session_uuid);
            "Session history cleared.".to_string()
        } else if text.starts_with("/stop") {
            let session_id_str = session_uuid.to_string();
            if let Some(tx) = state.session_steering_tx.get(&session_id_str) {
                let _ = tx.send(aria_intelligence::SteeringCommand::Abort).await;
                "Signal sent: aborting current operation.".to_string()
            } else {
                "No active operation to stop.".to_string()
            }
        } else if text.starts_with("/approve") || text.starts_with("/deny") {
            let parts: Vec<&str> = text.split_whitespace().collect();
            if parts.len() < 3 {
                "Invalid approval callback.".to_string()
            } else {
                let approving = text.starts_with("/approve");
                let sess_str = parts[1];
                let tool_name = parts[2];
                let approved_tool_name = tool_name.to_string();
                let approval_file = format!("/tmp/aria_approval_{}_{}.json", sess_str, tool_name);

                let mut reply_msg = if approving {
                    format!("✅ Tool '{}' approved. Executing...", tool_name)
                } else {
                    format!("❌ Tool '{}' denied. Resuming...", tool_name)
                };

                if let Ok(content) = std::fs::read_to_string(&approval_file) {
                    if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&content) {
                        let _prompt = payload
                            .get("prompt")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let _args_str = payload
                            .get("args")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        let original_request = payload
                            .get("original_request")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default()
                            .to_string();

                        // We must resume execution.
                        // To do this cleanly, we need to spawn a task that mimics process_request but calls `resume_tool_execution`
                        // For now we just delete the file so it can't be clicked twice
                        let _ = std::fs::remove_file(&approval_file);

                        // We will execute the tool here if approved, or return the deny message
                        let tool_result = if approving {
                            let args_str = payload
                                .get("args")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default()
                                .to_string();

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
                                None,
                                resolve_request_timezone_with_overrides(
                                    &state.config,
                                    &req.user_id,
                                    Some(state.user_timezone_overrides.as_ref()),
                                ),
                            );
                            let call = aria_intelligence::ToolCall {
                                name: approved_tool_name.clone(),
                                arguments: args_str,
                            };

                            match executor.execute(&call).await {
                                Ok(res) => res,
                                Err(e) => format!("Tool execution failed: {}", e),
                            }
                        } else {
                            "Execution denied by user.".to_string()
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
                                let mut caches = state_clone.session_tool_caches.lock().await;
                                let run_result = process_request(
                                    &current_req,
                                    state_clone.router_index.as_ref(),
                                    state_clone.embedder.as_ref(),
                                    &state_clone.llm_pool,
                                    &state_clone.cedar,
                                    state_clone.agent_store.as_ref(),
                                    state_clone.tool_registry.as_ref(),
                                    state_clone.session_memory.as_ref(),
                                    &state_clone.page_index,
                                    &state_clone.vector_store,
                                    &state_clone.keyword_index,
                                    &state_clone.firewall,
                                    &state_clone.vault,
                                    &state_clone.tx_cron,
                                    &state_clone.provider_registry,
                                    &mut *caches,
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
                                drop(caches);

                                match run_result {
                                    Ok(aria_intelligence::OrchestratorResult::Completed(t)) => {
                                        let text = if t.is_empty() {
                                            "(no response)".to_string()
                                        } else {
                                            t
                                        };
                                        state_clone
                                            .send_telegram_secure_message(chat_id, &text, None, None)
                                            .await;
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
                                            None,
                                            resolve_request_timezone_with_overrides(
                                                &state_clone.config,
                                                &current_req.user_id,
                                                Some(state_clone.user_timezone_overrides.as_ref()),
                                            ),
                                        );
                                        let output = match executor.execute(&call).await {
                                            Ok(out) => out,
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
                        reply_msg =
                            "Expired or invalid approval state (JSON parse failed).".to_string();
                    }
                } else {
                    reply_msg = "Expired or invalid approval state (file missing).".to_string();
                }
                reply_msg
            }
        } else {
            "Unknown or malformed command.".to_string()
        };

        state
            .send_telegram_secure_message(chat_id, &reply, reply_markup, parse_mode)
            .await;
        return;
    }

    info!(chat_id = %chat_id, text = %text, "Processing request");

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

    let mut caches = state.session_tool_caches.lock().await;
    let _response = match process_request(
        &req,
        state.router_index.as_ref(),
        state.embedder.as_ref(),
        &state.llm_pool,
        &state.cedar,
        state.agent_store.as_ref(),
        state.tool_registry.as_ref(),
        state.session_memory.as_ref(),
        &state.page_index,
        &state.vector_store,
        &state.keyword_index,
        &state.firewall,
        &state.vault,
        &state.tx_cron,
        &state.provider_registry,
        &mut *caches,
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
                    state
                        .send_telegram_secure_message(chat_id, &text, None, None)
                        .await;
                }
                aria_intelligence::OrchestratorResult::ToolApprovalRequired {
                    call,
                    pending_prompt,
                } => {
                    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
                    // Send an approval keyboard
                    let approve_cb = format!("/approve {} {}", session_uuid, call.name);
                    let deny_cb = format!("/deny {} {}", session_uuid, call.name);

                    let keyboard = vec![vec![
                        serde_json::json!({ "text": "✅ Approve", "callback_data": &approve_cb[..std::cmp::min(approve_cb.len(), 64)] }),
                        serde_json::json!({ "text": "❌ Deny", "callback_data": &deny_cb[..std::cmp::min(deny_cb.len(), 64)] }),
                    ]];

                    // We need a place to safely stash the pending_prompt and tool arguments.
                    // For now, write it to a temporary file keyed by session_id + tool name
                    let approval_file = format!(
                        "/tmp/aria_approval_{}_{}.json",
                        uuid::Uuid::from_bytes(req.session_id),
                        call.name
                    );
                    let payload = serde_json::json!({
                        "prompt": pending_prompt,
                        "args": call.arguments,
                        "original_request": req.content.as_text().unwrap_or_default(),
                    });
                    let _ = std::fs::write(
                        &approval_file,
                        serde_json::to_string(&payload).unwrap_or_default(),
                    );

                    let text = format!(
                        "⚠️ **Approval Required**

The agent is requesting to execute a sensitive command:
`{}`

Arguments:
```json
{}
```",
                        call.name, call.arguments
                    );
                    state
                        .send_telegram_secure_message(
                            chat_id,
                            &text,
                            Some(serde_json::json!({ "inline_keyboard": keyboard })),
                            Some("Markdown"),
                        )
                        .await;
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
}

async fn handle_telegram_webhook(state: TelegramState, body_str: &str) -> (StatusCode, String) {
    debug!(body_len = body_str.len(), "Webhook received");
    process_one_telegram_update(&state, body_str).await;
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
    config: Arc<Config>,
    runtime_config_path: PathBuf,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    page_index: Arc<PageIndexTree>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    session_tool_caches: Arc<Mutex<HashMap<([u8; 16], String), DynamicToolCache>>>,
    provider_registry: Arc<Mutex<ProviderRegistry>>,
    token: String,
) {
    let timezone_overrides = Arc::new(dashmap::DashMap::<String, String>::new());
    for (user_id, tz_name) in &config.localization.user_timezones {
        timezone_overrides.insert(user_id.clone(), tz_name.clone());
    }
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
        page_index,
        vector_store,
        keyword_index,
        firewall,
        vault,
        tx_cron: tx_cron.clone(),
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        provider_registry,
        telegram_token: token.clone(),
        client: reqwest::Client::new(),
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

                let state_clone = state.clone();
                tokio::spawn(async move {
                    process_one_telegram_update(&state_clone, &json_str).await;
                });
            }
        }
    }
    info!("Telegram long polling stopped.");
}

async fn setup_telegram_bot_commands(token: &str) {
    let url = format!("https://api.telegram.org/bot{}/setMyCommands", token);
    let body = serde_json::json!({
        "commands": [
            {"command": "agents", "description": "List available agents"},
            {"command": "agent", "description": "Switch to a specific agent by ID"},
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
    config: Arc<Config>,
    runtime_config_path: PathBuf,
    router_index: RouterIndex,
    embedder: Arc<FastEmbedder>,
    llm_pool: Arc<LlmBackendPool>,
    cedar: Arc<aria_policy::CedarEvaluator>,
    agent_store: AgentConfigStore,
    tool_registry: ToolManifestStore,
    session_memory: aria_ssmu::SessionMemory,
    page_index: Arc<PageIndexTree>,
    vector_store: Arc<VectorStore>,
    keyword_index: Arc<KeywordIndex>,
    session_tool_caches: Arc<Mutex<HashMap<([u8; 16], String), DynamicToolCache>>>,
    firewall: Arc<aria_safety::DfaFirewall>,
    vault: Arc<aria_vault::CredentialVault>,
    tx_cron: tokio::sync::mpsc::Sender<aria_intelligence::CronCommand>,
    registry: Arc<Mutex<ProviderRegistry>>,
) {
    let token = match resolve_telegram_token(&config.gateway) {
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
            page_index,
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
        page_index,
        vector_store,
        keyword_index,
        firewall,
        vault, // Added vault here
        tx_cron: tx_cron.clone(),
        hooks: Arc::new(HookRegistry::new()),
        session_tool_caches,
        provider_registry: registry.clone(),
        telegram_token: token.clone(),
        client: reqwest::Client::new(),
        session_locks: Arc::new(dashmap::DashMap::new()),
        session_steering_tx: Arc::new(dashmap::DashMap::new()),
        embed_semaphore: Arc::new(tokio::sync::Semaphore::new(2)),
        global_estop: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let shared_state = state.clone();
    let app = Router::new()
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
        );
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

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime build failed")
        .block_on(actual_main());
}

async fn actual_main() {
    // Load .env from CWD and ~/.aria/.env before config (does not override existing vars)
    load_env();

    let config_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("ARIA_CONFIG_PATH").ok())
        .unwrap_or_else(|| {
            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            manifest_dir
                .join("config.toml")
                .to_string_lossy()
                .to_string()
        });
    let runtime_config_path = resolve_config_path(&config_path).with_extension("runtime.json");

    println!("[aria-x] Loading config from: {}", config_path);

    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[aria-x] Failed to load config '{}' (cwd: {}): {}",
                config_path,
                std::env::current_dir().unwrap_or_default().display(),
                e
            );
            let _ = std::io::stderr().flush();
            std::process::exit(1);
        }
    };

    if let Err(err) = validate_config(&config) {
        eprintln!("[aria-x] Config validation error: {}", err);
        eprintln!("[aria-x] For Telegram: set TELEGRAM_BOT_TOKEN or add telegram_token to config");
        let _ = std::io::stderr().flush();
        std::process::exit(1);
    }

    // Init tracing (RUST_LOG overrides config)
    init_tracing(&config.telemetry);

    info!(
        node = %config.node.id,
        role = %config.node.role,
        llm = %config.llm.backend,
        model = %config.llm.model,
        "Config loaded"
    );
    if config.simulator.enabled {
        info!(backend = %config.simulator.backend, "Simulator mode enabled");
    }

    // Initialize Cedar policy engine (fail fast — never run without valid policy)
    let policy_content = match std::fs::read_to_string(&config.policy.policy_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "[aria-x] Fatal: failed to read policy file '{}': {}",
                config.policy.policy_path, e
            );
            std::process::exit(1);
        }
    };
    let cedar = match aria_policy::CedarEvaluator::from_policy_str(&policy_content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[aria-x] Fatal: failed to parse Cedar policies: {}", e);
            std::process::exit(1);
        }
    };
    let cedar = Arc::new(cedar);

    // Initialize Semantic Router with MiniLM-L6-v2 embedder (384-dim SBERT)
    let embedder = Arc::new(
        FastEmbedder::new().unwrap_or_else(|e| {
            warn!(error = %e, "FastEmbedder init failed, falling back to LocalHashEmbedder not available in this path");
            panic!("Cannot initialize embedding model: {}", e);
        })
    );
    let mut router = SemanticRouter::new();
    let agent_store = AgentConfigStore::load_from_dir(&config.agents_dir.path).unwrap_or_default();
    let mut tool_registry = ToolManifestStore::new();
    let mut vector_store = VectorStore::new();

    // Index workspace knowledge documents with real semantic embeddings
    vector_store.index_document(
        "workspace.files",
        "File system tools: list files, read source code, navigate project structure.",
        embedder.embed("list files read source navigate project workspace"),
        "workspace",
        vec!["files".into(), "source".into(), "workspace".into()],
        false,
    );
    vector_store.index_document(
        "workspace.rust",
        "Rust development: cargo build, cargo test, compile crates, fix errors.",
        embedder.embed("rust cargo build test compile crates errors"),
        "workspace",
        vec!["rust".into(), "cargo".into(), "build".into()],
        false,
    );
    vector_store.index_document(
        "security.policy",
        "Cedar policy engine: authorization decisions, access control, denied paths.",
        embedder.embed("security authorization cedar policy access control"),
        "policy",
        vec!["security".into(), "authorization".into(), "cedar".into()],
        false,
    );
    if agent_store.is_empty() {
        // Bootstrap fallback agents when no TOML configs found
        let _ = router.register_agent_text(
            "developer",
            "Write code, read files, search codebase, run tests, execute shell commands",
            &*embedder,
        );
        let _ = router.register_agent_text(
            "researcher",
            "Search the web, fetch URLs, summarise documents, query knowledge base",
            &*embedder,
        );
        warn!(
            path = %config.agents_dir.path,
            "No agent configs found; using bootstrap agents"
        );
    } else {
        // Register each loaded agent and index its full description + system prompt
        for cfg in agent_store.all() {
            // Register agent embedding using full description for better routing
            let _ = router.register_agent_text(&cfg.id, &cfg.description, &*embedder);

            // Index the agent as a knowledge document in the vector store
            let agent_doc_text = format!("{} {}", cfg.description, cfg.system_prompt);
            vector_store.index_document(
                format!("agent.{}", cfg.id),
                format!("{}: {}", cfg.id, cfg.description),
                embedder.embed(&agent_doc_text),
                "agent",
                vec![cfg.id.clone()],
                false,
            );

            // Register tools with real descriptions and schemas
            for tool_name in &cfg.base_tool_names {
                let (desc, schema) = match tool_name.as_str() {
                    "read_file" => (
                        "Read the contents of a file at the given path. Returns the file content as text.",
                        r#"{"path": {"type":"string","description":"File path to read"}}"#,
                    ),
                    "write_file" => (
                        "Write content to a file at the given path. Creates the file if it does not exist.",
                        r#"{"path": {"type":"string","description":"File path to write to"}, "content": {"type":"string","description":"Text content to write"}}"#,
                    ),
                    "search_codebase" => (
                        "Search the codebase for a pattern or keyword. Returns matching file paths and snippets.",
                        r#"{"query": {"type":"string","description":"Search pattern or keyword"}}"#,
                    ),
                    "run_tests" => (
                        "Run the test suite and return pass/fail results.",
                        r#"{"target": {"type":"string","description":"Crate or test name to run, or empty for all"}}"#,
                    ),
                    "run_shell" => (
                        "Execute a shell command and return stdout/stderr output.",
                        r#"{"command": {"type":"string","description":"Shell command to run"}}"#,
                    ),
                    "search_web" => (
                        "Search the web for information about a query. Returns a summary of top results.",
                        r#"{"query": {"type":"string","description":"Web search query"}}"#,
                    ),
                    "fetch_url" => (
                        "Fetch the content of a URL and return it as text.",
                        r#"{"url": {"type":"string","description":"URL to fetch"}}"#,
                    ),
                    "summarise_doc" => (
                        "Summarise a long document into concise bullet points.",
                        r#"{"text": {"type":"string","description":"Document text to summarise"}}"#,
                    ),
                    "query_rag" => (
                        "Query the local RAG knowledge base for relevant context about a topic.",
                        r#"{"query": {"type":"string","description":"Topic or question to search for"}}"#,
                    ),
                    "manage_cron" => (
                        "Manage scheduled jobs. Supports add, update, delete, list. Use cron, every:Ns, daily@HH:MM, weekly:day@HH:MM, biweekly:day@HH:MM, or at:YYYY-MM-DD HH:MM. DO NOT use tool/agent prefixes in response tool field.",
                        r#"{"action": {"type":"string","enum":["add","update","delete","list"],"description":"CRUD action to perform"}, "id": {"type":"string","description":"Unique job ID (required for update/delete)"}, "agent_id": {"type":"string","description":"Agent ID to trigger"}, "prompt": {"type":"string","description":"Prompt to send"}, "schedule": {"type":"string","description":"Schedule expression"}}"#,
                    ),
                    "schedule_message" | "set_reminder" => (
                        "Schedule reminder behavior. Modes: notify (default, sends message at due time), defer (run task prompt at due time via agent), both (notify and defer).",
                        r#"{"task": {"type":"string","description":"Reminder text or deferred task prompt"}, "delay": {"type":"string","description":"Delay/schedule like '2m', '8:15 PM', 'daily@19:30', 'weekly:sat@11:00'"}, "mode": {"type":"string","enum":["notify","defer","both"],"description":"Execution mode"}, "deferred_prompt": {"type":"string","description":"Optional task prompt executed at trigger time when mode is defer/both"}, "agent_id": {"type":"string","description":"Agent to execute deferred task with"}}"#,
                    ),
                    _ => ("Execute a tool operation.", "{}"),
                };
                tool_registry.register(CachedTool {
                    name: tool_name.clone(),
                    description: desc.into(),
                    parameters_schema: schema.into(),
                });
                // Index tool with real description text
                vector_store.index_tool_description(
                    tool_name.clone(), // Use clean tool name as ID
                    desc.to_string(),
                    embedder.embed(&format!("{} {}", tool_name, desc)),
                    tool_name,
                    vec![cfg.id.clone()],
                );
            }
        }
        info!(
            count = agent_store.len(),
            path = %config.agents_dir.path,
            "Loaded agent profiles"
        );
    }

    // Register meta tool: search_tool_registry
    let search_desc =
        "Search the tool registry and hot-swap the best matching tool for the current task.";
    tool_registry.register(CachedTool {
        name: "search_tool_registry".into(),
        description: search_desc.into(),
        parameters_schema:
            r#"{"query": {"type":"string","description":"Description of the capability you need"}}"#
                .into(),
    });
    vector_store.index_tool_description(
        "search_tool_registry", // Use clean tool name as ID
        search_desc.to_string(),
        embedder.embed("search tool registry find best tool capability"),
        "search_tool_registry",
        vec!["registry".into(), "meta".into()],
    );
    // NOTE: sensor.bootstrap.imu removed — irrelevant for non-robotics agents.
    // Sensor annotations are only indexed when robotics_ctrl agent is active.
    let route_cfg = RouteConfig {
        confidence_threshold: config.router.confidence_threshold,
        tie_break_gap: config.router.tie_break_gap,
    };
    let router_index = router.build_index(route_cfg);
    let llm_pool = LlmBackendPool::new(
        vec!["primary".into(), "fallback".into()],
        Duration::from_secs(30),
    );
    // Initialize Credential Vault
    let master_key_raw = std::env::var("ARIA_MASTER_KEY").unwrap_or_else(|_| {
        warn!("ARIA_MASTER_KEY not set; using insecure default key for development");
        "01234567890123456789012345678901".to_string()
    });
    let mut master_key = [0u8; 32];
    let key_bytes = master_key_raw.as_bytes();
    for i in 0..32.min(key_bytes.len()) {
        master_key[i] = key_bytes[i];
    }
    let vault = Arc::new(CredentialVault::new(&config.vault.storage_path, master_key));

    // Check for --vault-set command
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--vault-set") {
        if args.len() > pos + 2 {
            let key_name = &args[pos + 1];
            let secret_value = &args[pos + 2];
            let allowed_domains = vec![
                "openrouter.ai".to_string(),
                "openai.com".to_string(),
                "anthropic.com".to_string(),
            ];
            if let Err(e) = vault.store_secret("system", key_name, secret_value, allowed_domains) {
                error!("Failed to store secret in vault: {}", e);
                std::process::exit(1);
            }
            info!("Successfully stored secret '{}' in vault", key_name);
            std::process::exit(0);
        } else {
            error!("Usage: --vault-set <key_name> <secret_value>");
            std::process::exit(1);
        }
    }

    let registry = Arc::new(Mutex::new(ProviderRegistry::new()));
    {
        let mut reg = registry.lock().await;
        reg.register(Arc::new(backends::ollama::OllamaProvider {
            base_url: "http://localhost:11434".into(),
        }));

        // Resolve OpenRouter key: Vault -> Env -> Placeholder
        let openrouter_key = match vault.retrieve_global_secret("openrouter_key", "openrouter_ai") {
            Ok(_) => SecretRef::Vault {
                key_name: "openrouter_key".to_string(),
                vault: (*vault).clone(),
            },
            Err(_) => {
                if let Ok(key) = std::env::var("OPENROUTER_API_KEY") {
                    SecretRef::Literal(key)
                } else {
                    SecretRef::Literal("sk-or-placeholder".to_string())
                }
            }
        };

        reg.register(Arc::new(backends::openrouter::OpenRouterProvider {
            api_key: openrouter_key,
            site_url: "aria-x".into(),
            site_title: "ARIA-X".into(),
        }));
    }

    match config.llm.backend.to_lowercase().as_str() {
        "ollama" => {
            let ollama = OllamaBackend::from_env(config.llm.model.clone());
            llm_pool.register_backend("primary", Box::new(ollama.clone()));
            llm_pool.register_backend("fallback", Box::new(ollama));
            info!(model = %config.llm.model, "LLM: Ollama (OLLAMA_HOST from env)");
        }
        "openrouter" => {
            let reg = registry.lock().await;
            if let Ok(openrouter) = reg.create_backend("openrouter", &config.llm.model) {
                llm_pool.register_backend("primary", openrouter.clone());
                llm_pool.register_backend("fallback", openrouter);
                info!(model = %config.llm.model, "LLM: OpenRouter (REST)");
            } else {
                warn!("Failed to create OpenRouter backend, falling back to mock");
                llm_pool.register_backend("primary", Box::new(LocalMockLLM));
                llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
            }
        }
        _ => {
            llm_pool.register_backend("primary", Box::new(LocalMockLLM));
            llm_pool.register_backend("fallback", Box::new(LocalMockLLM));
            info!("LLM: mock (set backend=ollama for Ollama)");
        }
    }
    let llm_pool = Arc::new(llm_pool);

    // Initialize Session Memory
    let session_memory = aria_ssmu::SessionMemory::new(100);
    if let Ok(report) = session_memory.load_from_dir(&config.ssmu.sessions_dir) {
        info!(
            loaded = report.loaded_sessions,
            skipped = report.skipped_files,
            "Loaded persisted sessions"
        );
        if report.loaded_sessions > 0 {
            let embedder_clone = Arc::clone(&embedder);
            let _ = session_memory
                .index_session_summaries_to(&mut vector_store, move |s| embedder_clone.embed(s));
        }
    }
    // Build dynamic PageIndex: one node per loaded agent + bootstrap system nodes
    let page_index = build_dynamic_page_index(&agent_store);
    let page_index = Arc::new(page_index);
    let vector_store = Arc::new(vector_store);
    let session_tool_caches: Arc<Mutex<HashMap<([u8; 16], String), DynamicToolCache>>> =
        Arc::new(Mutex::new(HashMap::new()));

    // --- HookRegistry Setup for non-Telegram interfaces ---
    let session_locks = Arc::new(dashmap::DashMap::new());
    let embed_semaphore = Arc::new(tokio::sync::Semaphore::new(2));
    let mut hooks = HookRegistry::new();
    hooks.register_message_pre(Box::new(|req, vector_store, page_index| {
        let request_text = req.content.as_text().unwrap_or_default().to_string();
        Box::pin(async move {
            let hybrid =
                HybridMemoryEngine::new(&vector_store, &page_index, QueryPlannerConfig::default())
                    .retrieve(&request_text, &local_embed(&request_text, 64), 3, 3);
            let vector_context = hybrid.vector_context.join("\n");
            let page_context = hybrid
                .page_context
                .into_iter()
                .map(|n| format!("- {}: {}", n.title, n.summary))
                .collect::<Vec<_>>()
                .join("\n");
            let rag_context = format!(
                "Plan: {:?}\nVector Context:\n{}\n\nPageIndex Context:\n{}",
                hybrid.plan, vector_context, page_context
            );
            Ok(rag_context)
        })
    }));
    let hooks = Arc::new(hooks);

    // Build keyword index for BM25 hybrid search (RRF)
    let keyword_index = Arc::new(KeywordIndex::new().expect("Failed to create keyword index"));
    {
        // Batch-index all documents that are already in the vector store
        let mut kw_docs: Vec<(String, String)> = Vec::new();
        kw_docs.push((
            "workspace.files".into(),
            "File system tools: list files, read source code, navigate project structure.".into(),
        ));
        kw_docs.push((
            "workspace.rust".into(),
            "Rust development: cargo build, cargo test, compile crates, fix errors.".into(),
        ));
        kw_docs.push((
            "security.policy".into(),
            "Cedar policy engine: authorization decisions, access control, denied paths.".into(),
        ));
        for cfg in agent_store.all() {
            kw_docs.push((
                format!("agent.{}", cfg.id),
                format!("{} {}", cfg.description, cfg.system_prompt),
            ));
            for tool_name in &cfg.base_tool_names {
                let desc = match tool_name.as_str() {
                    "read_file" => "Read the contents of a file at the given path.",
                    "write_file" => "Write content to a file at the given path.",
                    "search_codebase" => "Search the codebase for a pattern or keyword.",
                    "run_tests" => "Run the test suite and return pass/fail results.",
                    "run_shell" => "Execute a shell command and return stdout/stderr output.",
                    "search_web" => "Search the web for information about a query.",
                    "fetch_url" => "Fetch the content of a URL and return it as text.",
                    "summarise_doc" => "Summarise a long document into concise bullet points.",
                    "query_rag" => "Query the local RAG knowledge base for relevant context.",
                    _ => "Execute a tool operation.",
                };
                kw_docs.push((format!("tool.{}", tool_name), desc.into()));
            }
        }
        kw_docs.push((
            "tool.search_tool_registry".into(),
            "Search the tool registry and hot-swap the best matching tool.".into(),
        ));
        if let Err(e) = keyword_index.add_documents_batch(&kw_docs) {
            warn!(error = %e, "Failed to populate keyword index");
        } else {
            info!(
                count = kw_docs.len(),
                "Keyword index populated for hybrid RAG"
            );
        }
    }

    // Initialize Credential Vault
    let master_key_raw = std::env::var("ARIA_MASTER_KEY").unwrap_or_else(|_| {
        warn!("ARIA_MASTER_KEY not set; using insecure default key for development");
        "01234567890123456789012345678901".to_string()
    });
    let mut master_key = [0u8; 32];
    let key_bytes = master_key_raw.as_bytes();
    for i in 0..32.min(key_bytes.len()) {
        master_key[i] = key_bytes[i];
    }
    let vault = Arc::new(CredentialVault::new(&config.vault.storage_path, master_key));

    // Check for --vault-set command
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--vault-set") {
        if args.len() > pos + 2 {
            let key_name = &args[pos + 1];
            let secret_value = &args[pos + 2];
            let allowed_domains = vec![
                "openrouter.ai".to_string(),
                "openai.com".to_string(),
                "anthropic.com".to_string(),
            ];
            if let Err(e) = vault.store_secret("system", key_name, secret_value, allowed_domains) {
                error!("Failed to store secret in vault: {}", e);
                std::process::exit(1);
            }
            info!("Successfully stored secret '{}' in vault", key_name);
            std::process::exit(0);
        } else {
            error!("Usage: --vault-set <key_name> <secret_value>");
            std::process::exit(1);
        }
    }

    let mut bad_patterns = vec![
        "sk-".to_string(),
        "ghp_".to_string(),
        "AKIA".to_string(),
        "ignore all previous instructions".to_string(),
        "system prompt".to_string(),
    ];
    // Add all secrets from the vault to the leak scanner patterns
    if let Ok(secrets) = vault.decrypt_all() {
        for s in secrets {
            if s.len() > 5 {
                bad_patterns.push(s);
            }
        }
    }
    let firewall = Arc::new(aria_safety::DfaFirewall::new(bad_patterns));

    let shared_config = Arc::new(config);
    let agent_store = Arc::new(agent_store);
    let tool_registry = Arc::new(tool_registry);

    // Initialise Scheduler early so it runs for all gateways
    let (tx_cron, rx_cron) = tokio::sync::mpsc::channel::<aria_intelligence::CronCommand>(64);
    let mut scheduler = CronScheduler::new();
    if shared_config.scheduler.enabled {
        for job in &shared_config.scheduler.jobs {
            if let Some(spec) = ScheduleSpec::parse(&job.schedule) {
                scheduler.add_job(ScheduledPromptJob {
                    id: job.id.clone(),
                    agent_id: job.agent_id.clone(),
                    prompt: job.prompt.clone(),
                    schedule_str: job.schedule.clone(),
                    kind: ScheduledJobKind::Orchestrate,
                    schedule: spec,
                    session_id: None,
                    user_id: None,
                    channel: None,
                });
            }
        }
        info!(
            jobs = shared_config.scheduler.jobs.len(),
            "Scheduler enabled"
        );
    } else {
        info!(
            "Scheduler preloaded jobs disabled; runtime scheduler remains active for dynamic reminders"
        );
    }
    let mut rx = scheduler.start(shared_config.scheduler.tick_seconds, rx_cron);

    // Spawn Background Scheduler Processor
    let sc_config = Arc::clone(&shared_config);
    let sc_router_index = router_index.clone();
    let sc_embedder = Arc::clone(&embedder);
    let sc_llm_pool = Arc::clone(&llm_pool);
    let sc_cedar = Arc::clone(&cedar);
    let sc_agent_store = Arc::clone(&agent_store);
    let sc_tool_registry = Arc::clone(&tool_registry);
    let sc_session_memory = session_memory.clone();
    let sc_page_index = Arc::clone(&page_index);
    let sc_vector_store = Arc::clone(&vector_store);
    let sc_keyword_index = Arc::clone(&keyword_index);
    let sc_firewall = Arc::clone(&firewall);
    let sc_vault = Arc::clone(&vault);
    let sc_tx_cron = tx_cron.clone();
    let sc_registry = Arc::clone(&registry);
    let sc_caches = Arc::clone(&session_tool_caches);
    let sc_hooks = Arc::clone(&hooks);
    let sc_locks = Arc::clone(&session_locks);
    let sc_semaphore = Arc::clone(&embed_semaphore);

    tokio::spawn(async move {
        info!("Background scheduler processor started");
        let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    debug!("Background scheduler heartbeat: alive");
                }
                ev_opt = rx.recv() => {
                    let ev = match ev_opt {
                        Some(e) => e,
                        None => break,
                    };
                    info!(job_id = %ev.job_id, agent_id = %ev.agent_id, prompt = %ev.prompt, "Scheduled prompt fired (background)");

                    let session_id = ev.session_id.unwrap_or_else(|| scheduled_session_id(&ev.job_id));
                    let session_uuid = uuid::Uuid::from_bytes(session_id);
                    let _ = sc_session_memory.update_overrides(
                        session_uuid,
                        Some(ev.agent_id.clone()),
                        None,
                    );

                    let req = aria_core::AgentRequest {
                        request_id: *uuid::Uuid::new_v4().as_bytes(),
                        session_id,
                        channel: ev.channel.unwrap_or(aria_core::GatewayChannel::Unknown),
                        user_id: ev.user_id.unwrap_or_else(|| "system".to_string()),
                        content: aria_core::MessageContent::Text(ev.prompt.clone()),
                        timestamp_us: chrono::Utc::now().timestamp_micros() as u64,
                    };

                    if matches!(ev.kind, ScheduledJobKind::Notify) {
                        send_universal_response(&req, &ev.prompt, &sc_config).await;
                        continue;
                    }

                    let mut caches_locked = sc_caches.lock().await;
                    match process_request(
                        &req,
                        &sc_router_index,
                        &*sc_embedder,
                        &sc_llm_pool,
                        &sc_cedar,
                        &*sc_agent_store,
                        &*sc_tool_registry,
                        &sc_session_memory,
                        &sc_page_index,
                        &sc_vector_store,
                        &sc_keyword_index,
                        &sc_firewall,
                        &sc_vault,
                        &sc_tx_cron,
                        &sc_registry,
                        &mut *caches_locked,
                        &*sc_hooks,
                        &sc_locks,
                        &sc_semaphore,
                        sc_config.llm.max_tool_rounds,
                        None,
                        Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
                        std::path::Path::new(&sc_config.ssmu.sessions_dir),
                        sc_config.policy.whitelist.clone(),
                        sc_config.policy.forbid.clone(),
                        resolve_request_timezone(&sc_config, &req.user_id),
                    )
                    .await
                    {
                        Ok(aria_intelligence::OrchestratorResult::Completed(text)) => {
                            send_universal_response(&req, &text, &sc_config).await;
                        }
                        Ok(_) => {
                            send_universal_response(
                                &req,
                                "Scheduled task requires approval, which is not supported in crons.",
                                &sc_config,
                            )
                            .await;
                        }
                        Err(e) => error!(error = %e, "Background scheduler orchestrator error"),
                    }
                }
            }
        }
    });

    if shared_config.gateway.adapter.to_lowercase() == "telegram" {
        run_telegram_gateway(
            Arc::clone(&shared_config),
            runtime_config_path,
            router_index,
            embedder,
            llm_pool,
            cedar,
            (*agent_store).clone(),
            (*tool_registry).clone(),
            session_memory.clone(),
            page_index,
            vector_store,
            keyword_index,
            session_tool_caches,
            firewall,
            vault,
            tx_cron,
            registry,
        )
        .await;
        return;
    }

    // Wire Adapters — CLI mode
    let gateway = CliGateway;

    info!("All subsystems wired (Gateway → Router → Orchestrator → Exec)");
    info!("Interactive CLI started (press Ctrl+C or send SIGTERM to exit)");

    let shutdown = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            if let Ok(mut sigterm) = signal(SignalKind::terminate()) {
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = sigterm.recv() => {
                        info!("Received SIGTERM — shutting down gracefully");
                    }
                }
            } else {
                tokio::signal::ctrl_c().await.ok();
            }
        }
        #[cfg(not(unix))]
        {
            tokio::signal::ctrl_c().await.ok();
        }
    };
    tokio::pin!(shutdown);
    loop {
        let req = tokio::select! {
            _ = &mut shutdown => {
                break;
            }
            req_res = gateway.receive() => {
                match req_res {
                    Ok(r) => {
                        let request_text = r.content.as_text().unwrap_or_default().to_string();
                        if request_text.eq_ignore_ascii_case("exit") {
                            info!("Exiting...");
                            break;
                        }
                        r
                    },
                    Err(_) => continue,
                }
            }
        };

        let mut caches = session_tool_caches.lock().await;
        match process_request(
            &req,
            &router_index,
            &*embedder,
            &llm_pool,
            &cedar,
            &*agent_store,
            &*tool_registry,
            &session_memory,
            &page_index,
            &vector_store,
            &keyword_index,
            &firewall,
            &vault,
            &tx_cron,
            &registry,
            &mut *caches,
            &*hooks,
            &session_locks,
            &embed_semaphore,
            shared_config.llm.max_tool_rounds,
            None,
            Some(&Arc::new(std::sync::atomic::AtomicBool::new(false))),
            std::path::Path::new(&shared_config.ssmu.sessions_dir),
            shared_config.policy.whitelist.clone(),
            shared_config.policy.forbid.clone(),
            resolve_request_timezone(&shared_config, &req.user_id),
        )
        .await
        {
            Ok(aria_intelligence::OrchestratorResult::Completed(text)) => {
                send_universal_response(&req, &text, &shared_config).await;
            }
            Ok(aria_intelligence::OrchestratorResult::ToolApprovalRequired { call, .. }) => {
                println!(
                    "\n[aria-x] Tool Approval Required: {}({})",
                    call.name, call.arguments
                );
                println!("(Currently unsupported via CLI gateway loop)");
            }
            Err(e) => error!(error = %e, "Orchestrator error"),
        };
    }

    // Cleanup
    if let Ok(saved) = session_memory.save_to_dir(&shared_config.ssmu.sessions_dir) {
        info!(saved = saved, "Persisted sessions");
    }
    drop(session_memory);
    drop(page_index);
    drop(vector_store);
    drop(router);
    info!("Shutdown complete. Goodbye!");
}
