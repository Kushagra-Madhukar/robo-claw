use super::*;

/// A universally-unique identifier stored as 16 raw bytes.
/// This representation is `no_std`-safe — no heap allocation required
/// for the ID itself.
pub type Uuid = [u8; 16];

/// Inbound user request normalized across all gateway channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Unique identifier for this request.
    pub request_id: Uuid,
    /// Session this request belongs to.
    pub session_id: Uuid,
    /// Gateway channel that produced this request.
    pub channel: GatewayChannel,
    /// Identifier of the requesting user.
    pub user_id: String,
    /// Normalized request payload content.
    pub content: MessageContent,
    /// Optional per-request runtime policy controlling tool behavior.
    #[serde(default)]
    pub tool_runtime_policy: Option<ToolRuntimePolicy>,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Outbound agent response sent back through the gateway.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentResponse {
    /// The request this response corresponds to.
    pub request_id: Uuid,
    /// Generated response content.
    pub content: MessageContent,
    /// Ordered records of tools that contributed to this response.
    pub skill_trace: Vec<SkillExecutionRecord>,
    /// End-to-end response latency in milliseconds.
    pub latency_ms: u32,
}

/// High-level category for runtime agent trust and usage patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentClass {
    Generalist,
    Specialist,
    Restricted,
    Notifier,
    RoboticsPlanner,
}

/// Coarse side-effect ceiling for an agent capability profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SideEffectLevel {
    ReadOnly,
    ExternalFetch,
    StatefulWrite,
    Privileged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustProfile {
    TrustedLocal,
    TrustedWorkspace,
    UntrustedWeb,
    UntrustedSocial,
    RoboticsControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemScope {
    pub root_path: String,
    #[serde(default)]
    pub allow_read: bool,
    #[serde(default)]
    pub allow_write: bool,
    #[serde(default)]
    pub allow_execute: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalScope {
    SessionMemory,
    Workspace,
    PolicyRuntime,
    External,
    Social,
    McpResource,
    ControlDocument,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationScope {
    #[serde(default)]
    pub can_spawn_children: bool,
    #[serde(default)]
    pub allowed_agents: Vec<String>,
    #[serde(default)]
    pub max_fanout: u16,
    #[serde(default)]
    pub max_runtime_seconds: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionScope {
    DiscoverOnly,
    ReadOnly,
    InteractiveNonAuth,
    InteractiveAuth,
    Download,
    SubmitWrite,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserSessionScope {
    NoSessionAccess,
    EphemeralOnly,
    ManagedProfileOnly,
    AttachedProfileAllowed,
    ExtensionBoundAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrawlScope {
    SinglePage,
    SameOrigin,
    AllowlistedDomains,
    ScheduledWatchAllowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebApprovalPolicy {
    PromptOnUnknownDomain,
    RequireApprovalAlways,
    AllowAllowlistedOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserTransportKind {
    ManagedBrowser,
    AttachedBrowser,
    ExtensionBrowser,
    RemoteBrowser,
}

/// Runtime-enforced capability profile for an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentCapabilityProfile {
    pub agent_id: String,
    pub class: AgentClass,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub skill_allowlist: Vec<String>,
    #[serde(default)]
    pub mcp_server_allowlist: Vec<String>,
    #[serde(default)]
    pub mcp_tool_allowlist: Vec<String>,
    #[serde(default)]
    pub mcp_prompt_allowlist: Vec<String>,
    #[serde(default)]
    pub mcp_resource_allowlist: Vec<String>,
    #[serde(default)]
    pub filesystem_scopes: Vec<FilesystemScope>,
    #[serde(default)]
    pub retrieval_scopes: Vec<RetrievalScope>,
    #[serde(default)]
    pub delegation_scope: Option<DelegationScope>,
    #[serde(default)]
    pub web_domain_allowlist: Vec<String>,
    #[serde(default)]
    pub web_domain_blocklist: Vec<String>,
    #[serde(default)]
    pub browser_profile_allowlist: Vec<String>,
    #[serde(default)]
    pub browser_action_scope: Option<BrowserActionScope>,
    #[serde(default)]
    pub computer_profile_allowlist: Vec<String>,
    #[serde(default)]
    pub computer_action_scope: Option<ComputerActionScope>,
    #[serde(default)]
    pub browser_session_scope: Option<BrowserSessionScope>,
    #[serde(default)]
    pub crawl_scope: Option<CrawlScope>,
    #[serde(default)]
    pub web_approval_policy: Option<WebApprovalPolicy>,
    #[serde(default)]
    pub web_transport_allowlist: Vec<BrowserTransportKind>,
    pub requires_elevation: bool,
    pub side_effect_level: SideEffectLevel,
    pub trust_profile: Option<TrustProfile>,
}
