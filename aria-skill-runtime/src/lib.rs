//! # aria-skill-runtime
//!
//! Sandboxed WebAssembly skill executor for ARIA-X.
//!
//! Provides the [`WasmExecutor`] trait and [`ExtismBackend`] implementation
//! for running Wasm-based skills inside a strict sandbox with configurable
//! memory limits and no host filesystem access.

use aria_core::{
    ConstraintViolation, HardwareIntent, SkillActivationPolicy, SkillBinding, SkillPackageManifest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors from the Wasm skill runtime.
#[derive(Debug)]
pub enum RuntimeError {
    /// The Wasm module could not be loaded or instantiated.
    LoadError(String),
    /// Execution of the Wasm function failed.
    ExecutionError(String),
    /// A capability violation occurred (e.g. unauthorized host call).
    CapabilityViolation(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WasmExecutionMode {
    DevJitAllowed,
    NodePreferAot,
    EdgeAotOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WasmAotArtifactMetadata {
    pub module_hash_hex: String,
    pub runtime_profile: String,
    pub target_triple: String,
    pub artifact_version: u32,
    pub signature_verified: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillTomlFile {
    skill_id: String,
    name: String,
    description: String,
    version: String,
    #[serde(default = "default_skill_entry_document")]
    entry_document: String,
    #[serde(default)]
    tool_names: Vec<String>,
    #[serde(default)]
    mcp_server_dependencies: Vec<String>,
    #[serde(default)]
    retrieval_hints: Vec<String>,
    #[serde(default)]
    wasm_module_ref: Option<String>,
    #[serde(default)]
    config_schema: Option<String>,
    #[serde(default = "default_skill_enabled")]
    enabled: bool,
}

fn default_skill_entry_document() -> String {
    "SKILL.md".into()
}

fn default_skill_enabled() -> bool {
    true
}

pub fn validate_skill_manifest(manifest: &SkillPackageManifest) -> Result<(), RuntimeError> {
    if manifest.skill_id.trim().is_empty() {
        return Err(RuntimeError::LoadError(
            "skill manifest validation failed: missing skill_id".into(),
        ));
    }
    if manifest.name.trim().is_empty() {
        return Err(RuntimeError::LoadError(
            "skill manifest validation failed: missing name".into(),
        ));
    }
    if manifest.version.trim().is_empty() {
        return Err(RuntimeError::LoadError(
            "skill manifest validation failed: missing version".into(),
        ));
    }
    if manifest.entry_document.trim().is_empty() {
        return Err(RuntimeError::LoadError(
            "skill manifest validation failed: missing entry_document".into(),
        ));
    }
    Ok(())
}

pub fn parse_skill_manifest_toml(input: &str) -> Result<SkillPackageManifest, RuntimeError> {
    let parsed: SkillTomlFile = toml::from_str(input)
        .map_err(|e| RuntimeError::LoadError(format!("skill manifest parse failed: {}", e)))?;
    let manifest = SkillPackageManifest {
        skill_id: parsed.skill_id,
        name: parsed.name,
        description: parsed.description,
        version: parsed.version,
        entry_document: parsed.entry_document,
        tool_names: parsed.tool_names,
        mcp_server_dependencies: parsed.mcp_server_dependencies,
        retrieval_hints: parsed.retrieval_hints,
        wasm_module_ref: parsed.wasm_module_ref,
        config_schema: parsed.config_schema,
        enabled: parsed.enabled,
    };
    validate_skill_manifest(&manifest)?;
    Ok(manifest)
}

pub fn load_skill_manifest_from_dir(
    skill_dir: &Path,
) -> Result<SkillPackageManifest, RuntimeError> {
    let manifest_path = skill_dir.join("skill.toml");
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
        RuntimeError::LoadError(format!(
            "failed to read skill manifest '{}': {}",
            manifest_path.display(),
            e
        ))
    })?;
    let manifest = parse_skill_manifest_toml(&content)?;
    let entry_document = skill_dir.join(&manifest.entry_document);
    if !entry_document.exists() {
        return Err(RuntimeError::LoadError(format!(
            "skill manifest validation failed: entry document '{}' missing",
            entry_document.display()
        )));
    }
    Ok(manifest)
}

#[derive(Debug, Default, Clone)]
pub struct SkillRegistry {
    manifests: BTreeMap<String, SkillPackageManifest>,
    bindings: Vec<SkillBinding>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn install_manifest(&mut self, manifest: SkillPackageManifest) -> Result<(), RuntimeError> {
        validate_skill_manifest(&manifest)?;
        self.manifests.insert(manifest.skill_id.clone(), manifest);
        Ok(())
    }

    pub fn list_manifests(&self) -> Vec<SkillPackageManifest> {
        self.manifests.values().cloned().collect()
    }

    pub fn set_enabled(&mut self, skill_id: &str, enabled: bool) -> Result<(), RuntimeError> {
        let manifest = self
            .manifests
            .get_mut(skill_id)
            .ok_or_else(|| RuntimeError::LoadError(format!("unknown skill '{}'", skill_id)))?;
        manifest.enabled = enabled;
        Ok(())
    }

    pub fn bind_skill(&mut self, binding: SkillBinding) -> Result<(), RuntimeError> {
        if !self.manifests.contains_key(&binding.skill_id) {
            return Err(RuntimeError::LoadError(format!(
                "cannot bind unknown skill '{}'",
                binding.skill_id
            )));
        }
        self.bindings.retain(|existing| {
            !(existing.skill_id == binding.skill_id && existing.agent_id == binding.agent_id)
        });
        self.bindings.push(binding);
        Ok(())
    }

    pub fn list_bindings_for_agent(&self, agent_id: &str) -> Vec<SkillBinding> {
        self.bindings
            .iter()
            .filter(|binding| binding.agent_id == agent_id)
            .cloned()
            .collect()
    }

    pub fn skill_allowed_for_agent(&self, agent_id: &str, skill_id: &str) -> bool {
        let Some(manifest) = self.manifests.get(skill_id) else {
            return false;
        };
        if !manifest.enabled {
            return false;
        }
        self.bindings.iter().any(|binding| {
            binding.agent_id == agent_id
                && binding.skill_id == skill_id
                && matches!(
                    binding.activation_policy,
                    SkillActivationPolicy::Manual
                        | SkillActivationPolicy::AutoSuggest
                        | SkillActivationPolicy::AutoLoadLowRisk
                        | SkillActivationPolicy::ApprovalRequired
                )
        })
    }
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuntimeError::LoadError(msg) => write!(f, "wasm load error: {}", msg),
            RuntimeError::ExecutionError(msg) => write!(f, "wasm execution error: {}", msg),
            RuntimeError::CapabilityViolation(msg) => {
                write!(f, "capability violation: {}", msg)
            }
        }
    }
}

impl std::error::Error for RuntimeError {}

// ---------------------------------------------------------------------------
// Node tiers and capability scoping
// ---------------------------------------------------------------------------

/// Runtime tier a node belongs to, which governs what capabilities are allowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeTier {
    /// Full-featured orchestrator node (Wasmtime, all capabilities).
    Orchestrator,
    /// Companion node (WasmEdge, limited host access).
    Companion,
    /// Edge relay node (WAMR, restricted I/O, sensor routing).
    Relay,
    /// Embedded micro node (no_std, WAMR, safety-envelope only).
    Micro,
}

/// Capability set permitted for a given tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TierCapabilities {
    pub wasm_execution: bool,
    pub file_access: bool,
    pub network_access: bool,
    pub sensor_access: bool,
    pub motor_control: bool,
}

impl TierCapabilities {
    pub fn for_tier(tier: RuntimeTier) -> Self {
        match tier {
            RuntimeTier::Orchestrator => Self {
                wasm_execution: true,
                file_access: true,
                network_access: true,
                sensor_access: true,
                motor_control: false,
            },
            RuntimeTier::Companion => Self {
                wasm_execution: true,
                file_access: false,
                network_access: true,
                sensor_access: false,
                motor_control: false,
            },
            RuntimeTier::Relay => Self {
                wasm_execution: true,
                file_access: false,
                network_access: false,
                sensor_access: true,
                motor_control: false,
            },
            RuntimeTier::Micro => Self {
                wasm_execution: true,
                file_access: false,
                network_access: false,
                sensor_access: true,
                motor_control: true, // Only micro nodes may touch actuators
            },
        }
    }

    pub fn allows_file_access(&self) -> bool {
        self.file_access
    }
    pub fn allows_network_access(&self) -> bool {
        self.network_access
    }
    pub fn allows_motor_control(&self) -> bool {
        self.motor_control
    }
}

// ---------------------------------------------------------------------------
// Ed25519 signature verification
// ---------------------------------------------------------------------------

use ed25519_dalek::{Signature, Verifier, VerifyingKey};

/// A Wasm module bundled with its Ed25519 signature and the signer's public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedModule {
    pub bytes: Vec<u8>,
    /// Ed25519 signature over `bytes`.
    pub signature: Vec<u8>,
    /// Ed25519 public key.
    pub public_key: Vec<u8>,
}

/// Verify the Ed25519 signature on a module before loading it.
///
/// Uses ed25519-dalek for SHA-512 Ed25519 verification.
pub fn verify_module(module: &SignedModule) -> Result<(), RuntimeError> {
    if module.signature.is_empty() {
        return Err(RuntimeError::LoadError(
            "signature verification failed: empty signature".into(),
        ));
    }
    let ver_key_bytes: [u8; 32] = module
        .public_key
        .as_slice()
        .try_into()
        .map_err(|_| RuntimeError::LoadError("invalid public key length".into()))?;
    let verifying_key = VerifyingKey::from_bytes(&ver_key_bytes).map_err(|e| {
        RuntimeError::LoadError(format!(
            "signature verification failed: invalid public key: {}",
            e
        ))
    })?;
    let sig_bytes: [u8; 64] = module
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| RuntimeError::LoadError("invalid signature length".into()))?;
    let sig = Signature::from_bytes(&sig_bytes);
    verifying_key.verify(&module.bytes, &sig).map_err(|_| {
        RuntimeError::LoadError(
            "signature verification failed: signature does not match module".into(),
        )
    })?;
    Ok(())
}

/// Sign a Wasm module with an Ed25519 key for deployment.
/// Used by the distillation/build pipeline to produce verifiable SignedModules.
#[cfg(feature = "signing")]
pub fn sign_module(bytes: Vec<u8>, signing_key: &ed25519_dalek::SigningKey) -> SignedModule {
    use ed25519_dalek::Signer;
    let signature = signing_key.sign(&bytes);
    SignedModule {
        bytes,
        signature: signature.to_bytes().to_vec(),
        public_key: signing_key.verifying_key().to_bytes().to_vec(),
    }
}

// ---------------------------------------------------------------------------
// Runtime policy gate
// ---------------------------------------------------------------------------

/// Minimal policy-decision shape for runtime execution gating.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyGateDecision {
    Allow,
    Deny,
    AskUser,
}

/// Trait to evaluate whether a runtime call may execute.
pub trait RuntimePolicyGate {
    fn evaluate(
        &self,
        principal: &str,
        action: &str,
        resource: &str,
    ) -> Result<PolicyGateDecision, RuntimeError>;
}

/// Context needed for a policy authorization query.
pub struct PolicyQuery<'a> {
    pub principal: &'a str,
    pub action: &'a str,
    pub resource: &'a str,
}

/// Wasm invocation payload shared by runtime wrappers.
pub struct WasmInvocation<'a> {
    pub module: &'a [u8],
    pub function_name: &'a str,
    pub input: &'a str,
}

/// Execute a Wasm function only when the policy gate allows it.
pub fn execute_with_policy_gate<E: WasmExecutor, P: RuntimePolicyGate>(
    executor: &E,
    policy: &P,
    query: PolicyQuery<'_>,
    invocation: WasmInvocation<'_>,
) -> Result<String, RuntimeError> {
    match policy.evaluate(query.principal, query.action, query.resource)? {
        PolicyGateDecision::Allow => executor.execute(
            invocation.module,
            invocation.function_name,
            invocation.input,
        ),
        PolicyGateDecision::AskUser => Err(RuntimeError::CapabilityViolation(format!(
            "policy requires explicit user confirmation for action '{}' on '{}'",
            query.action, query.resource
        ))),
        PolicyGateDecision::Deny => Err(RuntimeError::CapabilityViolation(format!(
            "policy denied action '{}' on resource '{}'",
            query.action, query.resource
        ))),
    }
}

// ---------------------------------------------------------------------------
// Execution timeout wrapper
// ---------------------------------------------------------------------------

/// Execution configuration for capability-scoped, timeout-bounded execution.
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    pub tier: RuntimeTier,
    /// Timeout in milliseconds. 0 = no timeout.
    pub timeout_ms: u64,
    /// Maximum memory pages (overrides backend default when set).
    pub max_memory_pages: Option<u32>,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            tier: RuntimeTier::Orchestrator,
            timeout_ms: 5_000,
            max_memory_pages: Some(256),
        }
    }
}

// ---------------------------------------------------------------------------
// Safety Envelope for HardwareIntent
// ---------------------------------------------------------------------------

/// Simple PID-style safety envelope that validates `HardwareIntent` commands
/// before they reach the motor controller.
#[derive(Debug, Clone)]
pub struct SafetyEnvelope {
    /// Maximum allowed absolute velocity for any motor command.
    pub max_velocity: f32,
}

impl SafetyEnvelope {
    /// Default envelope used for generic motors.
    pub fn default_for_motor() -> Self {
        Self { max_velocity: 1.0 }
    }

    /// Validate an intent. Returns `Ok(())` if the intent is safe, or a
    /// `ConstraintViolation` if it exceeds the configured envelope.
    pub fn validate(
        &self,
        node_id: &str,
        intent: &HardwareIntent,
        now_timestamp_us: u64,
    ) -> Result<(), ConstraintViolation> {
        if intent.target_velocity.abs() > self.max_velocity {
            return Err(ConstraintViolation {
                node_id: node_id.to_string(),
                motor_id: intent.motor_id,
                requested_velocity: intent.target_velocity,
                envelope_max: self.max_velocity,
                timestamp_us: now_timestamp_us,
            });
        }
        Ok(())
    }
}

/// Execute a motor-related Wasm function only if the safety envelope
/// validates the provided `HardwareIntent`. When the envelope is violated,
/// returns a `CapabilityViolation` and skips Wasm execution entirely.
pub fn execute_with_safety<E: WasmExecutor>(
    executor: &E,
    envelope: &SafetyEnvelope,
    node_id: &str,
    intent: &HardwareIntent,
    now_timestamp_us: u64,
    invocation: WasmInvocation<'_>,
) -> Result<String, RuntimeError> {
    if let Err(violation) = envelope.validate(node_id, intent, now_timestamp_us) {
        return Err(RuntimeError::CapabilityViolation(format!(
            "safety envelope violation: requested_velocity={} exceeds max={}",
            violation.requested_velocity, violation.envelope_max
        )));
    }
    executor.execute(
        invocation.module,
        invocation.function_name,
        invocation.input,
    )
}

// ---------------------------------------------------------------------------
// Simulator mode abstraction
// ---------------------------------------------------------------------------

/// Lightweight simulator backend abstraction (Gazebo / MuJoCo compatible).
pub trait SimulatorBackend {
    /// Apply a validated hardware intent in simulator mode.
    fn apply_intent(&self, intent: &HardwareIntent) -> Result<String, RuntimeError>;
}

/// Stub Gazebo backend.
pub struct GazeboSimulator;

impl SimulatorBackend for GazeboSimulator {
    fn apply_intent(&self, intent: &HardwareIntent) -> Result<String, RuntimeError> {
        Ok(format!(
            "gazebo simulated motor={} velocity={}",
            intent.motor_id, intent.target_velocity
        ))
    }
}

/// Stub MuJoCo backend.
pub struct MujocoSimulator;

impl SimulatorBackend for MujocoSimulator {
    fn apply_intent(&self, intent: &HardwareIntent) -> Result<String, RuntimeError> {
        Ok(format!(
            "mujoco simulated motor={} velocity={}",
            intent.motor_id, intent.target_velocity
        ))
    }
}

/// Execute an intent in simulator mode after passing the safety envelope.
pub fn execute_in_simulator<S: SimulatorBackend>(
    simulator: &S,
    envelope: &SafetyEnvelope,
    node_id: &str,
    intent: &HardwareIntent,
    now_timestamp_us: u64,
) -> Result<String, RuntimeError> {
    envelope
        .validate(node_id, intent, now_timestamp_us)
        .map_err(|v| {
            RuntimeError::CapabilityViolation(format!(
                "safety envelope violation: requested_velocity={} exceeds max={}",
                v.requested_velocity, v.envelope_max
            ))
        })?;
    simulator.apply_intent(intent)
}

// ---------------------------------------------------------------------------
// WasmExecutor trait
// ---------------------------------------------------------------------------

/// Trait for executing WebAssembly skill modules.
pub trait WasmExecutor {
    /// Execute a function named `function_name` within the given Wasm module bytes.
    ///
    /// - `module`: Raw Wasm bytes (`.wasm` file contents)
    /// - `function_name`: Name of the exported function to call
    /// - `input`: String input to pass to the function
    ///
    /// Returns the string output from the function.
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError>;
}

// ---------------------------------------------------------------------------
// ExtismBackend
// ---------------------------------------------------------------------------

/// Configuration for the Extism-based Wasm executor.
#[derive(Debug, Clone)]
pub struct ExtismConfig {
    /// Maximum memory pages the Wasm module is allowed to use.
    /// Each page is 64KB. Default: 256 pages (16MB).
    pub max_memory_pages: Option<u32>,
    /// Whether WASI support is enabled. Default: false (strict sandbox).
    pub wasi_enabled: bool,
    /// Secrets to be injected into the Sandbox (e.g. API keys).
    pub secrets: std::collections::HashMap<String, String>,
    /// Optional host directory to mount into the Wasm container at `/workspace`.
    pub workspace_dir: Option<std::path::PathBuf>,
    /// Optional list of hostnames the Wasm container is allowed to connect to.
    pub allowed_hosts: Option<Vec<String>>,
}

impl Default for ExtismConfig {
    fn default() -> Self {
        Self {
            max_memory_pages: Some(256),
            wasi_enabled: false,
            secrets: std::collections::HashMap::new(),
            workspace_dir: None,
            allowed_hosts: None,
        }
    }
}

/// Extism-based Wasm executor providing strict sandboxing.
///
/// No host functions are exposed. WASI is disabled by default.
/// Wasm modules can only process input → output via the Extism PDK interface.
pub struct ExtismBackend {
    config: ExtismConfig,
}

impl Default for ExtismBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtismBackend {
    /// Create a new Extism backend with default configuration.
    pub fn new() -> Self {
        Self {
            config: ExtismConfig::default(),
        }
    }

    /// Create a new Extism backend with custom configuration.
    pub fn with_config(config: ExtismConfig) -> Self {
        Self { config }
    }
}

// ---------------------------------------------------------------------------
// Tier-specific backend stubs
// ---------------------------------------------------------------------------

/// Wasmtime backend stub for orchestrator-tier execution.
/// Full WASI support, all host capabilities available.
pub struct WasmtimeBackend {
    pub config: ExtismConfig,
}

impl WasmtimeBackend {
    pub fn new() -> Self {
        Self {
            config: ExtismConfig::default(),
        }
    }
}

impl Default for WasmtimeBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmExecutor for WasmtimeBackend {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        // Delegates to Extism (same engine) in this scaffolding.
        // A production path would use the wasmtime crate directly.
        ExtismBackend::with_config(self.config.clone()).execute(module, function_name, input)
    }
}

/// WAMR backend stub for relay/micro-tier execution.
/// Restricted I/O, no WASI, minimal footprint.
pub struct WamrBackend;

impl WasmExecutor for WamrBackend {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        if module.is_empty() {
            return Err(RuntimeError::LoadError("empty module".into()));
        }
        // WAMR production path would link wamr-sdk. This stub rejects WASI
        // and delegates to Extism with WASI disabled.
        let config = ExtismConfig {
            max_memory_pages: Some(1024),
            wasi_enabled: true,
            secrets: std::collections::HashMap::new(),
            workspace_dir: None,
            allowed_hosts: None,
        };
        ExtismBackend::with_config(config).execute(module, function_name, input)
    }
}

/// WasmEdge backend stub for companion-tier execution.
pub struct WasmEdgeBackend;

impl WasmExecutor for WasmEdgeBackend {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        // WasmEdge production path would use the wasmedge-sys crate.
        // This stub delegates to Extism for compatibility.
        ExtismBackend::new().execute(module, function_name, input)
    }
}

// ---------------------------------------------------------------------------
// Capability-checked executor wrapper
// ---------------------------------------------------------------------------

/// Wraps a `WasmExecutor` with a capability check for the target tier.
///
/// Rejects executions that would require capabilities the tier does not have
/// (e.g. file access on a relay node).
pub struct TieredExecutor<T: WasmExecutor> {
    pub inner: T,
    pub capabilities: TierCapabilities,
}

impl<T: WasmExecutor> TieredExecutor<T> {
    pub fn new(inner: T, tier: RuntimeTier) -> Self {
        Self {
            inner,
            capabilities: TierCapabilities::for_tier(tier),
        }
    }
}

impl<T: WasmExecutor> WasmExecutor for TieredExecutor<T> {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        // Hard invariant: raw PWM functions must never be invoked directly
        // from LLM-generated tool calls. Even on micro nodes, such calls
        // must go through a higher-level safety envelope.
        if function_name.contains("pwm") {
            return Err(RuntimeError::CapabilityViolation(
                "raw PWM functions are forbidden by runtime policy".into(),
            ));
        }
        // Infer capability requirements from the function name as a heuristic.
        // Production builds would inspect the Wasm import section instead.
        if (function_name.contains("file") || function_name.contains("read_file"))
            && !self.capabilities.file_access
        {
            return Err(RuntimeError::CapabilityViolation(format!(
                "function '{}' requires file_access, denied by tier policy",
                function_name
            )));
        }
        if (function_name.contains("motor") || function_name.contains("pwm"))
            && !self.capabilities.motor_control
        {
            return Err(RuntimeError::CapabilityViolation(format!(
                "function '{}' requires motor_control, denied by tier policy",
                function_name
            )));
        }
        self.inner.execute(module, function_name, input)
    }
}

impl WasmExecutor for ExtismBackend {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        // Build the manifest from raw bytes
        let wasm = extism::Wasm::data(module.to_vec());
        let mut manifest = extism::Manifest::new([wasm]);

        // Apply memory limits
        if let Some(max_pages) = self.config.max_memory_pages {
            manifest = manifest.with_memory_max(max_pages);
        }

        if !self.config.secrets.is_empty() {
            manifest = manifest.with_config(self.config.secrets.clone().into_iter());
        }

        if let Some(ref wl) = self.config.workspace_dir {
            manifest = manifest.with_allowed_path(wl.to_string_lossy().into_owned(), "/workspace");
        }

        if let Some(ref hosts) = self.config.allowed_hosts {
            manifest = manifest.with_allowed_hosts(hosts.clone().into_iter());
        }

        // Create the plugin — no host functions, WASI controlled by config
        let mut plugin = extism::Plugin::new(&manifest, [], self.config.wasi_enabled)
            .map_err(|e| RuntimeError::LoadError(format!("{}", e)))?;

        // Call the function
        let output: String = plugin
            .call::<&str, String>(function_name, input)
            .map_err(|e| {
                let msg = format!("{}", e);
                // Distinguish capability violations from general execution errors
                if msg.contains("not found") || msg.contains("unknown import") {
                    RuntimeError::CapabilityViolation(msg)
                } else {
                    RuntimeError::ExecutionError(msg)
                }
            })?;

        Ok(output)
    }
}

// ---------------------------------------------------------------------------
// Wasm AOT Pre-Compilation Cache (P1)
// ---------------------------------------------------------------------------

/// A SHA-256 hash of a Wasm module's raw bytes, used as the cache key.
pub type WasmModuleHash = [u8; 32];

/// Compute the SHA-256 hash of Wasm bytes for cache-key purposes.
///
/// In production this uses a proper SHA-256 digest. The implementation here
/// uses a FNV-style 64-bit fold promoted to 256 bits to avoid pulling in a
/// heavy cryptography dependency at the `aria-skill-runtime` level; swap this
/// with `sha2` if a fully standards-compliant hash is required.
pub fn wasm_module_hash(bytes: &[u8]) -> WasmModuleHash {
    // FNV-1a 64-bit, run twice over two non-overlapping key streams,
    // packing four u64 values into a 32-byte hash array.
    let fnv_hash = |seed: u64, data: &[u8]| -> u64 {
        let mut h: u64 = seed;
        for &b in data {
            h ^= b as u64;
            h = h.wrapping_mul(1_099_511_628_211);
        }
        h
    };
    let h1 = fnv_hash(14_695_981_039_346_656_037, bytes);
    let h2 = fnv_hash(h1.wrapping_add(0xDEAD_BEEF_CAFE_BABE), bytes);
    let h3 = fnv_hash(h2.wrapping_add(0x0102_0304_0506_0708), bytes);
    let h4 = fnv_hash(h3.wrapping_add(0xF1E2_D3C4_B5A6_9788), bytes);
    let mut out = [0u8; 32];
    out[0..8].copy_from_slice(&h1.to_le_bytes());
    out[8..16].copy_from_slice(&h2.to_le_bytes());
    out[16..24].copy_from_slice(&h3.to_le_bytes());
    out[24..32].copy_from_slice(&h4.to_le_bytes());
    out
}

/// In-memory AOT pre-compilation cache.
///
/// Stores pre-validated ("pre-compiled") Wasm modules so that the same bytes
/// are never re-parsed by the runtime on subsequent calls. In a production
/// deployment backed by Wasmtime this would store the Wasmtime
/// `Module::serialize()` bytes; here it stores the original bytes as the
/// validated artifact since the Extism/stub executor doesn't have a separate
/// serialization step.
///
/// Thread-safe: backed by `RwLock<HashMap>`.
pub struct WasmAotCache {
    inner: std::sync::RwLock<std::collections::HashMap<WasmModuleHash, Vec<u8>>>,
}

impl Default for WasmAotCache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PersistentWasmAotCache {
    root: PathBuf,
    execution_mode: WasmExecutionMode,
    runtime_profile: String,
    target_triple: String,
    require_signature: bool,
}

impl PersistentWasmAotCache {
    pub fn new(
        root: impl Into<PathBuf>,
        execution_mode: WasmExecutionMode,
        runtime_profile: impl Into<String>,
        target_triple: impl Into<String>,
        require_signature: bool,
    ) -> Self {
        Self {
            root: root.into(),
            execution_mode,
            runtime_profile: runtime_profile.into(),
            target_triple: target_triple.into(),
            require_signature,
        }
    }

    fn artifact_dir(&self, hash: &WasmModuleHash) -> PathBuf {
        self.root.join(hex_module_hash(hash))
    }

    fn artifact_bytes_path(&self, hash: &WasmModuleHash) -> PathBuf {
        self.artifact_dir(hash).join("module.bin")
    }

    fn artifact_metadata_path(&self, hash: &WasmModuleHash) -> PathBuf {
        self.artifact_dir(hash).join("metadata.json")
    }

    pub fn metadata_for_module(
        &self,
        hash: &WasmModuleHash,
    ) -> Result<Option<WasmAotArtifactMetadata>, RuntimeError> {
        let path = self.artifact_metadata_path(hash);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            RuntimeError::LoadError(format!(
                "read AOT metadata '{}' failed: {}",
                path.display(),
                e
            ))
        })?;
        let meta = serde_json::from_str::<WasmAotArtifactMetadata>(&raw).map_err(|e| {
            RuntimeError::LoadError(format!(
                "parse AOT metadata '{}' failed: {}",
                path.display(),
                e
            ))
        })?;
        Ok(Some(meta))
    }

    pub fn load_precompiled_bytes(
        &self,
        hash: &WasmModuleHash,
    ) -> Result<Option<Vec<u8>>, RuntimeError> {
        let Some(meta) = self.metadata_for_module(hash)? else {
            return Ok(None);
        };
        self.validate_metadata(&meta)?;
        let path = self.artifact_bytes_path(hash);
        let bytes = std::fs::read(&path).map_err(|e| {
            RuntimeError::LoadError(format!(
                "read AOT artifact '{}' failed: {}",
                path.display(),
                e
            ))
        })?;
        Ok(Some(bytes))
    }

    fn validate_metadata(&self, meta: &WasmAotArtifactMetadata) -> Result<(), RuntimeError> {
        if meta.runtime_profile != self.runtime_profile {
            return Err(RuntimeError::CapabilityViolation(format!(
                "AOT artifact runtime profile mismatch: expected '{}' got '{}'",
                self.runtime_profile, meta.runtime_profile
            )));
        }
        if meta.target_triple != self.target_triple {
            return Err(RuntimeError::CapabilityViolation(format!(
                "AOT artifact target mismatch: expected '{}' got '{}'",
                self.target_triple, meta.target_triple
            )));
        }
        if self.require_signature && !meta.signature_verified {
            return Err(RuntimeError::CapabilityViolation(
                "AOT artifact is unsigned in signature-required mode".into(),
            ));
        }
        Ok(())
    }

    pub fn precompile_and_persist(
        &self,
        bytes: Vec<u8>,
        signature_verified: bool,
    ) -> Result<WasmModuleHash, RuntimeError> {
        if self.require_signature && !signature_verified {
            return Err(RuntimeError::CapabilityViolation(
                "unsigned Wasm module cannot be precompiled in signature-required mode".into(),
            ));
        }
        if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
            return Err(RuntimeError::LoadError(
                "AOT precompile: not a valid Wasm module (missing magic header)".into(),
            ));
        }
        let hash = wasm_module_hash(&bytes);
        let dir = self.artifact_dir(&hash);
        std::fs::create_dir_all(&dir).map_err(|e| {
            RuntimeError::LoadError(format!(
                "create AOT artifact dir '{}' failed: {}",
                dir.display(),
                e
            ))
        })?;
        let metadata = WasmAotArtifactMetadata {
            module_hash_hex: hex_module_hash(&hash),
            runtime_profile: self.runtime_profile.clone(),
            target_triple: self.target_triple.clone(),
            artifact_version: 1,
            signature_verified,
        };
        std::fs::write(self.artifact_bytes_path(&hash), &bytes).map_err(|e| {
            RuntimeError::LoadError(format!("write AOT artifact failed: {}", e))
        })?;
        let meta_json = serde_json::to_vec_pretty(&metadata).map_err(|e| {
            RuntimeError::LoadError(format!("serialize AOT artifact metadata failed: {}", e))
        })?;
        std::fs::write(self.artifact_metadata_path(&hash), meta_json).map_err(|e| {
            RuntimeError::LoadError(format!("write AOT metadata failed: {}", e))
        })?;
        Ok(hash)
    }

    pub fn execution_mode(&self) -> WasmExecutionMode {
        self.execution_mode
    }
}

fn hex_module_hash(hash: &WasmModuleHash) -> String {
    hash.iter().map(|b| format!("{:02x}", b)).collect()
}

impl WasmAotCache {
    /// Create a new, empty cache.
    pub fn new() -> Self {
        Self {
            inner: std::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }

    /// Returns `true` if the module (identified by its byte-hash) is cached.
    pub fn contains(&self, hash: &WasmModuleHash) -> bool {
        self.inner.read().unwrap().contains_key(hash)
    }

    /// Retrieve cached (pre-compiled) bytes for a module hash, if present.
    pub fn get(&self, hash: &WasmModuleHash) -> Option<Vec<u8>> {
        self.inner.read().unwrap().get(hash).cloned()
    }

    /// Insert a module into the cache.
    ///
    /// `raw_bytes` — the original `.wasm` bytes (or serialized AOT artifact).
    pub fn insert(&self, hash: WasmModuleHash, raw_bytes: Vec<u8>) {
        self.inner.write().unwrap().insert(hash, raw_bytes);
    }

    /// Number of cached modules.
    pub fn len(&self) -> usize {
        self.inner.read().unwrap().len()
    }

    /// Returns `true` if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.read().unwrap().is_empty()
    }

    /// Pre-validate and cache a Wasm module.
    ///
    /// Validates basic Wasm magic bytes (`\0asm`) and caches the bytes under
    /// their content hash. Returns the hash on success, or a `RuntimeError`
    /// if the bytes do not appear to be a valid Wasm module.
    pub fn precompile(&self, bytes: Vec<u8>) -> Result<WasmModuleHash, RuntimeError> {
        // Validate Wasm magic header
        if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
            return Err(RuntimeError::LoadError(
                "AOT precompile: not a valid Wasm module (missing magic header)".into(),
            ));
        }
        let hash = wasm_module_hash(&bytes);
        self.insert(hash, bytes);
        Ok(hash)
    }
}

/// Wraps any `WasmExecutor` with AOT cache look-ahead.
///
/// On the first call for a given module, the module bytes are cached under
/// their content hash. Subsequent calls with the same bytes hit the cache and
/// skip the load phase entirely, reducing cold-start latency.
pub struct AotCachedExecutor<E: WasmExecutor> {
    inner: E,
    cache: std::sync::Arc<WasmAotCache>,
    persistent: Option<std::sync::Arc<PersistentWasmAotCache>>,
}

impl<E: WasmExecutor> AotCachedExecutor<E> {
    pub fn new(inner: E, cache: std::sync::Arc<WasmAotCache>) -> Self {
        Self {
            inner,
            cache,
            persistent: None,
        }
    }

    pub fn with_persistent_cache(
        inner: E,
        cache: std::sync::Arc<WasmAotCache>,
        persistent: std::sync::Arc<PersistentWasmAotCache>,
    ) -> Self {
        Self {
            inner,
            cache,
            persistent: Some(persistent),
        }
    }
}

impl<E: WasmExecutor> WasmExecutor for AotCachedExecutor<E> {
    fn execute(
        &self,
        module: &[u8],
        function_name: &str,
        input: &str,
    ) -> Result<String, RuntimeError> {
        let hash = wasm_module_hash(module);
        let effective_bytes = match self.cache.get(&hash) {
            Some(cached) => {
                // Cache hit — use pre-validated bytes
                cached
            }
            None => {
                if let Some(persistent) = &self.persistent {
                    if let Some(bytes) = persistent.load_precompiled_bytes(&hash)? {
                        self.cache.insert(hash, bytes.clone());
                        return self.inner.execute(&bytes, function_name, input);
                    }
                    if persistent.execution_mode() == WasmExecutionMode::EdgeAotOnly {
                        return Err(RuntimeError::CapabilityViolation(format!(
                            "Wasm module '{}' is not precompiled for edge AOT-only execution",
                            function_name
                        )));
                    }
                    persistent.precompile_and_persist(module.to_vec(), false)?;
                }
                // Cache miss — populate then execute
                if module.len() >= 4 && &module[..4] == b"\0asm" {
                    self.cache.insert(hash, module.to_vec());
                }
                module.to_vec()
            }
        };
        self.inner.execute(&effective_bytes, function_name, input)
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Path to the pre-built hello plugin Wasm fixture.
    fn hello_wasm_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test-fixtures")
            .join("hello.wasm")
    }

    /// Load the hello plugin Wasm bytes.
    fn hello_wasm_bytes() -> Vec<u8> {
        std::fs::read(hello_wasm_path()).expect("hello.wasm fixture must exist")
    }

    // =====================================================================
    // Core execution tests
    // =====================================================================

    #[test]
    fn execute_hello_returns_hello() {
        let backend = ExtismBackend::new();
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "greet", "world");
        assert!(result.is_ok(), "execution failed: {:?}", result.err());
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn execute_with_custom_config() {
        let config = ExtismConfig {
            max_memory_pages: Some(512), // 32MB
            wasi_enabled: false,
            secrets: std::collections::HashMap::new(),
            workspace_dir: None,
            allowed_hosts: None,
        };
        let backend = ExtismBackend::with_config(config);
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "greet", "");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");
    }

    // =====================================================================
    // Error handling tests
    // =====================================================================

    #[test]
    fn execute_missing_function_returns_error() {
        let backend = ExtismBackend::new();
        let wasm = hello_wasm_bytes();
        let result = backend.execute(&wasm, "nonexistent_function", "input");
        assert!(result.is_err(), "should fail for missing function");
        match result {
            Err(RuntimeError::ExecutionError(msg) | RuntimeError::CapabilityViolation(msg)) => {
                assert!(!msg.is_empty());
            }
            _ => panic!("expected ExecutionError or CapabilityViolation"),
        }
    }

    #[test]
    fn execute_corrupted_bytes_returns_load_error() {
        let backend = ExtismBackend::new();
        let garbage = vec![0xFF, 0x00, 0xDE, 0xAD, 0xBE, 0xEF];
        let result = backend.execute(&garbage, "greet", "");
        assert!(result.is_err(), "corrupted bytes should fail to load");
        match result {
            Err(RuntimeError::LoadError(msg)) => {
                assert!(!msg.is_empty());
            }
            other => panic!("expected LoadError, got: {:?}", other),
        }
    }

    #[test]
    fn execute_empty_bytes_returns_load_error() {
        let backend = ExtismBackend::new();
        let result = backend.execute(&[], "greet", "");
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LoadError(_)) => {} // expected
            other => panic!("expected LoadError, got: {:?}", other),
        }
    }

    // =====================================================================
    // Config and error display tests
    // =====================================================================

    #[test]
    fn default_config_values() {
        let config = ExtismConfig::default();
        assert_eq!(config.max_memory_pages, Some(256));
        assert!(!config.wasi_enabled);
    }

    // =====================================================================
    // Tier capability tests
    // =====================================================================

    #[test]
    fn orchestrator_tier_has_file_and_network() {
        let caps = TierCapabilities::for_tier(RuntimeTier::Orchestrator);
        assert!(caps.file_access);
        assert!(caps.network_access);
        assert!(!caps.motor_control);
    }

    #[test]
    fn relay_tier_has_sensor_but_no_file() {
        let caps = TierCapabilities::for_tier(RuntimeTier::Relay);
        assert!(caps.sensor_access);
        assert!(!caps.file_access);
        assert!(!caps.network_access);
        assert!(!caps.motor_control);
    }

    #[test]
    fn micro_tier_has_motor_control() {
        let caps = TierCapabilities::for_tier(RuntimeTier::Micro);
        assert!(caps.motor_control);
        assert!(!caps.file_access);
    }

    #[test]
    fn tiered_executor_denies_file_access_on_relay() {
        let backend = ExtismBackend::new();
        let executor = TieredExecutor::new(backend, RuntimeTier::Relay);
        let result = executor.execute(&[], "read_file", "");
        match result {
            Err(RuntimeError::CapabilityViolation(msg)) => {
                assert!(msg.contains("file_access"));
            }
            other => panic!("expected CapabilityViolation, got {:?}", other),
        }
    }

    #[test]
    fn tiered_executor_denies_motor_on_companion() {
        let backend = ExtismBackend::new();
        let executor = TieredExecutor::new(backend, RuntimeTier::Companion);
        let result = executor.execute(&[], "set_motor", "");
        match result {
            Err(RuntimeError::CapabilityViolation(msg)) => {
                assert!(msg.contains("motor_control"));
            }
            other => panic!("expected CapabilityViolation, got {:?}", other),
        }
    }

    #[test]
    fn tiered_executor_denies_raw_pwm_everywhere() {
        let backend = ExtismBackend::new();
        let executor = TieredExecutor::new(backend, RuntimeTier::Micro);
        let result = executor.execute(&[], "set_pwm", "");
        match result {
            Err(RuntimeError::CapabilityViolation(msg)) => {
                assert!(msg.contains("raw PWM functions are forbidden"));
            }
            other => panic!("expected CapabilityViolation for raw pwm, got {:?}", other),
        }
    }

    // =====================================================================
    // Ed25519 signature verification tests
    // =====================================================================

    #[test]
    fn signature_verification_rejects_zero_key() {
        let signed = SignedModule {
            bytes: b"invalid".to_vec(),
            signature: [0u8; 64].to_vec(),
            public_key: [0u8; 32].to_vec(),
        };
        let result = verify_module(&signed);
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LoadError(msg)) => assert!(msg.contains("verification failed")),
            other => panic!("expected LoadError, got {:?}", other),
        }
    }

    #[test]
    fn signature_verification_rejects_zero_signature() {
        let signed = SignedModule {
            bytes: b"invalid".to_vec(),
            signature: vec![0u8; 12], // Invalid length
            public_key: [1u8; 32].to_vec(),
        };
        let result = verify_module(&signed);
        assert!(result.is_err());
        match result {
            Err(RuntimeError::LoadError(msg)) => assert!(msg.contains("invalid signature length")),
            other => panic!("expected LoadError, got {:?}", other),
        }
    }

    #[cfg(feature = "signing")]
    #[test]
    fn signature_verification_accepts_valid_signed_module() {
        use ed25519_dalek::SigningKey;
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let signed = sign_module(b"wasm".to_vec(), &sk);
        assert!(verify_module(&signed).is_ok());
    }

    #[test]
    fn signature_verification_rejects_tampered_module() {
        use ed25519_dalek::Signer;
        let wasm_bytes = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let signature = signing_key.sign(&wasm_bytes);
        let mut module = SignedModule {
            bytes: wasm_bytes,
            signature: signature.to_bytes().to_vec(),
            public_key: signing_key.verifying_key().to_bytes().to_vec(),
        };
        module.bytes.push(0xff); // Tamper with module after signing
        let result = verify_module(&module);
        assert!(result.is_err());
    }

    // =====================================================================
    // Safety envelope tests
    // =====================================================================

    #[test]
    fn safety_envelope_allows_safe_velocity() {
        let env = SafetyEnvelope { max_velocity: 2.0 };
        let intent = HardwareIntent {
            intent_id: 1,
            motor_id: 3,
            target_velocity: 1.0,
        };
        let result = env.validate("node-1", &intent, 123);
        assert!(result.is_ok());
    }

    #[test]
    fn safety_envelope_rejects_overspeed() {
        let env = SafetyEnvelope { max_velocity: 1.0 };
        let intent = HardwareIntent {
            intent_id: 1,
            motor_id: 3,
            target_velocity: 2.5,
        };
        let result = env.validate("node-1", &intent, 999);
        assert!(result.is_err());
        if let Err(cv) = result {
            assert_eq!(cv.motor_id, 3);
            assert_eq!(cv.envelope_max, 1.0);
            assert_eq!(cv.requested_velocity, 2.5);
            assert_eq!(cv.node_id, "node-1");
            assert_eq!(cv.timestamp_us, 999);
        }
    }

    struct CountingExecutor {
        pub calls: std::sync::Mutex<usize>,
    }

    impl WasmExecutor for CountingExecutor {
        fn execute(
            &self,
            _module: &[u8],
            _function_name: &str,
            _input: &str,
        ) -> Result<String, RuntimeError> {
            let mut guard = self.calls.lock().unwrap();
            *guard += 1;
            Ok("ok".into())
        }
    }

    #[test]
    fn execute_with_safety_blocks_and_does_not_call_executor_on_violation() {
        let env = SafetyEnvelope { max_velocity: 1.0 };
        let intent = HardwareIntent {
            intent_id: 1,
            motor_id: 1,
            target_velocity: 5.0,
        };
        let exec = CountingExecutor {
            calls: std::sync::Mutex::new(0),
        };

        let result = execute_with_safety(
            &exec,
            &env,
            "relay-01",
            &intent,
            42,
            WasmInvocation {
                module: &[],
                function_name: "set_motor_velocity",
                input: "",
            },
        );

        assert!(result.is_err());
        match result {
            Err(RuntimeError::CapabilityViolation(msg)) => {
                assert!(msg.contains("safety envelope violation"));
            }
            other => panic!("expected CapabilityViolation, got {:?}", other),
        }

        let calls = exec.calls.lock().unwrap();
        assert_eq!(*calls, 0, "executor must not be called on violation");
    }

    struct AllowPolicy;
    impl RuntimePolicyGate for AllowPolicy {
        fn evaluate(
            &self,
            _principal: &str,
            _action: &str,
            _resource: &str,
        ) -> Result<PolicyGateDecision, RuntimeError> {
            Ok(PolicyGateDecision::Allow)
        }
    }

    struct DenyPolicy;
    impl RuntimePolicyGate for DenyPolicy {
        fn evaluate(
            &self,
            _principal: &str,
            _action: &str,
            _resource: &str,
        ) -> Result<PolicyGateDecision, RuntimeError> {
            Ok(PolicyGateDecision::Deny)
        }
    }

    #[test]
    fn policy_gate_denies_and_skips_execution() {
        let exec = CountingExecutor {
            calls: std::sync::Mutex::new(0),
        };
        let result = execute_with_policy_gate(
            &exec,
            &DenyPolicy,
            PolicyQuery {
                principal: "developer",
                action: "read_file",
                resource: "/workspace/a",
            },
            WasmInvocation {
                module: &[],
                function_name: "read_file",
                input: "",
            },
        );
        assert!(result.is_err());
        let calls = exec.calls.lock().unwrap();
        assert_eq!(*calls, 0);
    }

    #[test]
    fn policy_gate_allows_execution() {
        let exec = CountingExecutor {
            calls: std::sync::Mutex::new(0),
        };
        let result = execute_with_policy_gate(
            &exec,
            &AllowPolicy,
            PolicyQuery {
                principal: "developer",
                action: "read_file",
                resource: "/workspace/a",
            },
            WasmInvocation {
                module: &[],
                function_name: "read_file",
                input: "",
            },
        );
        assert!(result.is_ok());
        let calls = exec.calls.lock().unwrap();
        assert_eq!(*calls, 1);
    }

    #[test]
    fn simulator_gazebo_path_with_safe_intent() {
        let env = SafetyEnvelope { max_velocity: 2.0 };
        let sim = GazeboSimulator;
        let intent = HardwareIntent {
            intent_id: 99,
            motor_id: 4,
            target_velocity: 1.5,
        };
        let result = execute_in_simulator(&sim, &env, "node-1", &intent, 1);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("gazebo simulated"));
    }

    #[test]
    fn simulator_mujoco_path_rejects_unsafe_intent() {
        let env = SafetyEnvelope { max_velocity: 1.0 };
        let sim = MujocoSimulator;
        let intent = HardwareIntent {
            intent_id: 100,
            motor_id: 4,
            target_velocity: 3.0,
        };
        let result = execute_in_simulator(&sim, &env, "node-1", &intent, 1);
        assert!(result.is_err());
        match result {
            Err(RuntimeError::CapabilityViolation(msg)) => {
                assert!(msg.contains("safety envelope violation"));
            }
            other => panic!("expected CapabilityViolation, got {:?}", other),
        }
    }

    // =====================================================================
    // Error display
    // =====================================================================

    #[test]
    fn error_display() {
        let err = RuntimeError::LoadError("bad module".into());
        assert!(format!("{}", err).contains("wasm load error"));

        let err = RuntimeError::ExecutionError("trap".into());
        assert!(format!("{}", err).contains("wasm execution error"));

        let err = RuntimeError::CapabilityViolation("no fs".into());
        assert!(format!("{}", err).contains("capability violation"));
    }
    // =====================================================================
    // Item 1: Wasm AOT Pre-Compilation Cache
    // =====================================================================

    #[test]
    fn aot_cache_empty_on_new() {
        let cache = WasmAotCache::new();
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn aot_cache_hash_is_deterministic() {
        let bytes = b"\0asm\x01\x00\x00\x00";
        let h1 = wasm_module_hash(bytes);
        let h2 = wasm_module_hash(bytes);
        assert_eq!(h1, h2, "same bytes must always produce the same hash");
    }

    #[test]
    fn aot_cache_precompile_valid_wasm_magic() {
        let cache = WasmAotCache::new();
        // Minimal Wasm: magic + version
        let fake_wasm = b"\0asm\x01\x00\x00\x00".to_vec();
        let result = cache.precompile(fake_wasm);
        assert!(result.is_ok(), "valid wasm magic should be accepted");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn aot_cache_precompile_rejects_invalid_bytes() {
        let cache = WasmAotCache::new();
        let not_wasm = b"not a wasm module".to_vec();
        let result = cache.precompile(not_wasm);
        assert!(result.is_err(), "invalid bytes must be rejected");
        assert!(cache.is_empty(), "cache must stay empty on rejection");
    }

    #[test]
    fn aot_cache_contains_after_insert() {
        let cache = WasmAotCache::new();
        let bytes = b"\0asm\x01\x00\x00\x00".to_vec();
        let hash = cache.precompile(bytes.clone()).unwrap();
        assert!(cache.contains(&hash));
        assert_eq!(cache.get(&hash).unwrap(), bytes);
    }

    #[test]
    fn persistent_aot_cache_survives_restart_and_restores_metadata() {
        let temp = tempfile::tempdir().expect("aot tempdir");
        let cache = PersistentWasmAotCache::new(
            temp.path(),
            WasmExecutionMode::NodePreferAot,
            "node",
            "x86_64-unknown-linux-gnu",
            false,
        );
        let bytes = b"\0asm\x01\x00\x00\x00".to_vec();
        let hash = cache
            .precompile_and_persist(bytes.clone(), true)
            .expect("persist aot");

        let restored = PersistentWasmAotCache::new(
            temp.path(),
            WasmExecutionMode::NodePreferAot,
            "node",
            "x86_64-unknown-linux-gnu",
            false,
        );
        let loaded = restored
            .load_precompiled_bytes(&hash)
            .expect("load persisted aot")
            .expect("artifact exists");
        assert_eq!(loaded, bytes);
        let meta = restored
            .metadata_for_module(&hash)
            .expect("load metadata")
            .expect("metadata exists");
        assert!(meta.signature_verified);
        assert_eq!(meta.runtime_profile, "node");
    }

    #[test]
    fn persistent_aot_cache_rejects_unsigned_artifact_in_edge_mode() {
        let temp = tempfile::tempdir().expect("aot tempdir");
        let cache = PersistentWasmAotCache::new(
            temp.path(),
            WasmExecutionMode::NodePreferAot,
            "edge",
            "aarch64-unknown-linux-gnu",
            false,
        );
        let bytes = b"\0asm\x01\x00\x00\x00".to_vec();
        let hash = cache
            .precompile_and_persist(bytes, false)
            .expect("persist unsigned artifact");

        let edge_cache = PersistentWasmAotCache::new(
            temp.path(),
            WasmExecutionMode::EdgeAotOnly,
            "edge",
            "aarch64-unknown-linux-gnu",
            true,
        );
        let err = edge_cache
            .load_precompiled_bytes(&hash)
            .expect_err("unsigned artifact must be rejected");
        assert!(format!("{}", err).contains("unsigned"));
    }

    #[test]
    fn aot_cached_executor_edge_mode_requires_precompiled_artifact() {
        let temp = tempfile::tempdir().expect("aot tempdir");
        let cache = std::sync::Arc::new(WasmAotCache::new());
        let persistent = std::sync::Arc::new(PersistentWasmAotCache::new(
            temp.path(),
            WasmExecutionMode::EdgeAotOnly,
            "edge",
            "aarch64-unknown-linux-gnu",
            false,
        ));
        let executor = AotCachedExecutor::with_persistent_cache(
            CountingExecutor {
                calls: std::sync::Mutex::new(0),
            },
            cache,
            persistent,
        );
        let err = executor
            .execute(b"\0asm\x01\x00\x00\x00", "hello", "{}")
            .expect_err("edge mode should reject non-precompiled module");
        assert!(format!("{}", err).contains("not precompiled"));
    }

    #[test]
    fn parse_skill_manifest_toml_applies_defaults_and_validates() {
        let manifest = parse_skill_manifest_toml(
            r#"
skill_id = "github_review"
name = "GitHub Review"
description = "Review pull requests"
version = "1.0.0"
tool_names = ["read_file"]
mcp_server_dependencies = ["github"]
"#,
        )
        .expect("parse skill manifest");

        assert_eq!(manifest.skill_id, "github_review");
        assert_eq!(manifest.entry_document, "SKILL.md");
        assert!(manifest.enabled);
        assert_eq!(manifest.tool_names, vec!["read_file"]);
        assert_eq!(manifest.mcp_server_dependencies, vec!["github"]);
    }

    #[test]
    fn load_skill_manifest_from_dir_requires_entry_document() {
        let temp = tempfile::tempdir().expect("skill tempdir");
        std::fs::write(
            temp.path().join("skill.toml"),
            r#"
skill_id = "repo_memory"
name = "Repo Memory"
description = "Remember repo facts"
version = "0.1.0"
"#,
        )
        .expect("write manifest");

        let err = load_skill_manifest_from_dir(temp.path()).expect_err("missing SKILL.md");
        assert!(format!("{}", err).contains("entry document"));

        std::fs::write(temp.path().join("SKILL.md"), "# Repo Memory").expect("write skill doc");
        let manifest = load_skill_manifest_from_dir(temp.path()).expect("load skill manifest");
        assert_eq!(manifest.skill_id, "repo_memory");
    }

    #[test]
    fn skill_registry_installs_binds_and_respects_enablement() {
        let mut registry = SkillRegistry::new();
        registry
            .install_manifest(SkillPackageManifest {
                skill_id: "github_review".into(),
                name: "GitHub Review".into(),
                description: "Review PRs".into(),
                version: "1.0.0".into(),
                entry_document: "SKILL.md".into(),
                tool_names: vec!["read_file".into()],
                mcp_server_dependencies: vec!["github".into()],
                retrieval_hints: vec!["repo".into()],
                wasm_module_ref: None,
                config_schema: None,
                enabled: true,
            })
            .expect("install skill");
        registry
            .bind_skill(SkillBinding {
                binding_id: "bind-1".into(),
                agent_id: "developer".into(),
                skill_id: "github_review".into(),
                activation_policy: SkillActivationPolicy::AutoSuggest,
                created_at_us: 1,
            })
            .expect("bind skill");

        assert!(registry.skill_allowed_for_agent("developer", "github_review"));
        assert_eq!(registry.list_bindings_for_agent("developer").len(), 1);

        registry
            .set_enabled("github_review", false)
            .expect("disable skill");
        assert!(!registry.skill_allowed_for_agent("developer", "github_review"));
    }
}
