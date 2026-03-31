use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeDenialKind {
    ToolAllowlist,
    DelegationScope,
    FilesystemScope,
    ExecutionProfile,
    SkillScope,
    DomainPolicy,
    NetworkEgress,
    SecretEgress,
    ContentFirewall,
    BrowserProfileScope,
    BrowserSessionScope,
    BrowserActionScope,
    ComputerProfileScope,
    ComputerActionScope,
    CrawlScope,
    McpToolScope,
    McpPromptScope,
    McpResourceScope,
    RetrievalScope,
    ElevationRequired,
}

impl ScopeDenialKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::ToolAllowlist => "tool_allowlist",
            Self::DelegationScope => "delegation_scope",
            Self::FilesystemScope => "filesystem_scope",
            Self::ExecutionProfile => "execution_profile",
            Self::SkillScope => "skill_scope",
            Self::DomainPolicy => "domain_policy",
            Self::NetworkEgress => "network_egress",
            Self::SecretEgress => "secret_egress",
            Self::ContentFirewall => "content_firewall",
            Self::BrowserProfileScope => "browser_profile_scope",
            Self::BrowserSessionScope => "browser_session_scope",
            Self::BrowserActionScope => "browser_action_scope",
            Self::ComputerProfileScope => "computer_profile_scope",
            Self::ComputerActionScope => "computer_action_scope",
            Self::CrawlScope => "crawl_scope",
            Self::McpToolScope => "mcp_tool_scope",
            Self::McpPromptScope => "mcp_prompt_scope",
            Self::McpResourceScope => "mcp_resource_scope",
            Self::RetrievalScope => "retrieval_scope",
            Self::ElevationRequired => "elevation_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeDenialRecord {
    pub denial_id: String,
    pub kind: ScopeDenialKind,
    pub agent_id: String,
    #[serde(default)]
    pub session_id: Option<Uuid>,
    pub target: String,
    pub reason: String,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretUsageOutcome {
    Allowed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretUsageAuditRecord {
    pub audit_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub session_id: Option<Uuid>,
    pub tool_name: String,
    pub key_name: String,
    pub target_domain: String,
    pub outcome: SecretUsageOutcome,
    pub detail: String,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievalTraceRecord {
    pub trace_id: String,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub agent_id: String,
    pub query_text: String,
    pub latency_ms: u64,
    pub session_hits: u32,
    pub workspace_hits: u32,
    pub policy_hits: u32,
    pub external_hits: u32,
    pub social_hits: u32,
    pub document_context_hits: u32,
    pub history_tokens: u32,
    pub rag_tokens: u32,
    pub control_tokens: u32,
    pub tool_count: u32,
    pub control_document_conflicts: u32,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextBlockKind {
    Retrieval,
    ControlDocument,
    RuleContext,
    DurableConstraint,
    SubAgentResult,
    ToolInstructions,
    ToolResult,
    WorkingSet,
    Ambiguity,
    PromptAsset,
    ResourceContext,
    CapabilityIndex,
    DocumentIndex,
    ContractRequirements,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextBlock {
    pub kind: ContextBlockKind,
    pub label: String,
    pub content: String,
    pub token_estimate: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PromptContextMessage {
    pub role: String,
    pub content: String,
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionContextPack {
    pub system_prompt: String,
    pub history_messages: Vec<PromptContextMessage>,
    pub context_blocks: Vec<ContextBlock>,
    pub user_request: String,
    pub channel: GatewayChannel,
    #[serde(default)]
    pub execution_contract: Option<ExecutionContract>,
    #[serde(default)]
    pub retrieved_context: Option<RetrievedContextBundle>,
    #[serde(default)]
    pub working_set: Option<WorkingSet>,
    #[serde(default)]
    pub context_plan: Option<ContextPlan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionContractKind {
    AnswerOnly,
    ToolAssisted,
    ScheduleCreate,
    BrowserRead,
    BrowserAct,
    McpInvoke,
    SubAgentSpawn,
    ArtifactCreate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionContract {
    pub kind: ExecutionContractKind,
    #[serde(default)]
    pub allowed_tool_classes: Vec<String>,
    #[serde(default)]
    pub required_artifact_kinds: Vec<ExecutionArtifactKind>,
    #[serde(default)]
    pub forbidden_completion_modes: Vec<String>,
    #[serde(default)]
    pub fallback_mode: Option<String>,
    #[serde(default)]
    pub approval_required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionArtifactKind {
    Schedule,
    Browser,
    Computer,
    File,
    Mcp,
    SubAgent,
    ToolSearch,
    PlainAnswer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionArtifact {
    pub kind: ExecutionArtifactKind,
    pub label: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub locator: Option<String>,
    #[serde(default)]
    pub origin_tool: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSetEntryKind {
    Artifact,
    Resource,
    PendingApproval,
    ToolOutput,
    WorkspaceTarget,
    ExternalReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingSetStatus {
    Pending,
    Active,
    Completed,
    Failed,
    Superseded,
    Resolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingSetEntry {
    pub entry_id: String,
    pub kind: WorkingSetEntryKind,
    #[serde(default)]
    pub artifact_kind: Option<ExecutionArtifactKind>,
    #[serde(default)]
    pub locator: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub origin_tool: Option<String>,
    #[serde(default)]
    pub channel: Option<GatewayChannel>,
    #[serde(default)]
    pub session_id: Option<Uuid>,
    pub status: WorkingSetStatus,
    pub created_at_us: u64,
    #[serde(default)]
    pub updated_at_us: Option<u64>,
    pub summary: String,
    #[serde(default)]
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub approval_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceResolutionOutcome {
    Resolved,
    Ambiguous,
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReferenceResolution {
    pub query_text: String,
    pub outcome: ReferenceResolutionOutcome,
    #[serde(default)]
    pub matched_entry_ids: Vec<String>,
    #[serde(default)]
    pub active_target_entry_id: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct WorkingSet {
    #[serde(default)]
    pub entries: Vec<WorkingSetEntry>,
    #[serde(default)]
    pub active_target_entry_id: Option<String>,
    #[serde(default)]
    pub reference_resolution: Option<ReferenceResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextPlanDecision {
    Included,
    DroppedEmpty,
    DroppedDuplicate,
    DroppedBudget,
    DroppedPolicy,
    DroppedAmbiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionBlockRecord {
    pub kind: ContextBlockKind,
    pub label: String,
    pub decision: ContextPlanDecision,
    pub token_estimate: u32,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ContextPlan {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub block_records: Vec<InspectionBlockRecord>,
    #[serde(default)]
    pub ambiguity: Option<ReferenceResolution>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractFailureReason {
    MissingRequiredArtifact,
    ForbiddenCompletionMode,
    ProviderCapabilityMismatch,
    ToolPolicyMismatch,
    ApprovalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalSourceKind {
    SessionHistory,
    SessionMemory,
    Workspace,
    PolicyRuntime,
    External,
    Social,
    ControlDocument,
    SubAgentResult,
    PromptAsset,
    McpResource,
    CapabilityIndex,
    DocumentIndex,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RetrievedContextBlock {
    pub source_kind: RetrievalSourceKind,
    pub source_id: String,
    pub label: String,
    pub content: String,
    #[serde(default)]
    pub trust_class: Option<String>,
    #[serde(default)]
    pub score: Option<f32>,
    #[serde(default)]
    pub rank: Option<u32>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
    #[serde(default)]
    pub recency_us: Option<u64>,
    pub token_estimate: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct RetrievedContextBundle {
    #[serde(default)]
    pub plan_summary: Option<String>,
    #[serde(default)]
    pub blocks: Vec<RetrievedContextBlock>,
    #[serde(default)]
    pub dropped_blocks: Vec<RetrievedContextBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityIndexEntry {
    pub entry_id: String,
    pub title: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentIndexEntry {
    pub entry_id: String,
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub start_index: Option<u32>,
    #[serde(default)]
    pub end_index: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextInspectionRecord {
    pub context_id: String,
    pub request_id: Uuid,
    pub session_id: Uuid,
    pub agent_id: String,
    pub channel: GatewayChannel,
    pub provider_model: Option<String>,
    pub prompt_mode: String,
    pub history_tokens: u32,
    pub context_tokens: u32,
    pub system_tokens: u32,
    pub user_tokens: u32,
    pub tool_count: u32,
    #[serde(default)]
    pub active_tool_names: Vec<String>,
    #[serde(default)]
    pub tool_runtime_policy: Option<ToolRuntimePolicy>,
    #[serde(default)]
    pub tool_selection: Option<ToolSelectionDecision>,
    #[serde(default)]
    pub provider_request_payload: Option<serde_json::Value>,
    #[serde(default)]
    pub selected_tool_catalog: Vec<ToolCatalogEntry>,
    #[serde(default)]
    pub hidden_tool_messages: Vec<String>,
    #[serde(default)]
    pub emitted_artifacts: Vec<ExecutionArtifact>,
    #[serde(default)]
    pub tool_provider_readiness: Vec<crate::ToolProviderReadiness>,
    pub pack: ExecutionContextPack,
    pub rendered_prompt: String,
    pub created_at_us: u64,
}

/// Channel-agnostic inbound envelope used by adapters before request projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboundEnvelope {
    /// Unique identifier for the inbound provider event.
    pub envelope_id: Uuid,
    /// Session this message belongs to.
    pub session_id: Uuid,
    /// Source gateway channel.
    pub channel: GatewayChannel,
    /// Channel-scoped user identifier.
    pub user_id: String,
    /// Provider-specific message/update identifier.
    pub provider_message_id: Option<String>,
    /// Normalized content payload.
    pub content: MessageContent,
    /// Optional attachment references from the provider payload.
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    /// Ingress timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Channel-agnostic outbound envelope used before transport-specific fanout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutboundEnvelope {
    /// Unique identifier for the outbound message.
    pub envelope_id: Uuid,
    /// Session this message belongs to.
    pub session_id: Uuid,
    /// Destination channel.
    pub channel: GatewayChannel,
    /// Channel-scoped recipient identifier (chat/user/thread id).
    pub recipient_id: String,
    /// Optional provider-specific correlation ID.
    pub provider_message_id: Option<String>,
    /// Outbound payload.
    pub content: MessageContent,
    /// Optional attachment references to send.
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    /// Emit timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Normalized attachment metadata shared across channels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AttachmentRef {
    pub kind: AttachmentKind,
    pub url: String,
    pub mime_type: Option<String>,
    pub caption: Option<String>,
}

/// Coarse media type for attachment routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    Image,
    Audio,
    Video,
    Document,
    Other,
}

/// Unified payload model used by both requests and responses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    Image {
        url: String,
        caption: Option<String>,
    },
    Audio {
        url: String,
        transcript: Option<String>,
    },
    Video {
        url: String,
        caption: Option<String>,
        transcript: Option<String>,
    },
    Document {
        url: String,
        caption: Option<String>,
        mime_type: Option<String>,
    },
    Location {
        lat: f64,
        lng: f64,
    },
}

impl MessageContent {
    /// Returns inner text if this is a `Text` payload.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            MessageContent::Text(text) => Some(text.as_str()),
            _ => None,
        }
    }
}

/// Source channel type used by the gateway layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GatewayChannel {
    Telegram,
    WhatsApp,
    Discord,
    Slack,
    IMessage,
    Cli,
    WebSocket,
    Ros2,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelCapabilityProfile {
    pub supports_callbacks: bool,
    pub supports_inline_buttons: bool,
    pub supports_rich_media: bool,
    pub supports_typing_indicator: bool,
    pub supports_thread_context: bool,
    pub supports_command_aliases: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelFallbackMode {
    NativeOnly,
    TextFallback,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelPluginManifest {
    pub channel: GatewayChannel,
    pub plugin_id: String,
    pub transport: String,
    pub control_capabilities: ChannelCapabilityProfile,
    pub approval_capable: bool,
    pub fallback_mode: ChannelFallbackMode,
}

pub fn channel_capability_profile(channel: GatewayChannel) -> ChannelCapabilityProfile {
    match channel {
        GatewayChannel::Telegram => ChannelCapabilityProfile {
            supports_callbacks: true,
            supports_inline_buttons: true,
            supports_rich_media: true,
            supports_typing_indicator: true,
            supports_thread_context: true,
            supports_command_aliases: true,
        },
        GatewayChannel::WhatsApp => ChannelCapabilityProfile {
            supports_callbacks: true,
            supports_inline_buttons: false,
            supports_rich_media: true,
            supports_typing_indicator: true,
            supports_thread_context: true,
            supports_command_aliases: true,
        },
        GatewayChannel::Discord
        | GatewayChannel::Slack
        | GatewayChannel::IMessage
        | GatewayChannel::WebSocket => ChannelCapabilityProfile {
            supports_callbacks: true,
            supports_inline_buttons: true,
            supports_rich_media: true,
            supports_typing_indicator: true,
            supports_thread_context: true,
            supports_command_aliases: true,
        },
        GatewayChannel::Cli | GatewayChannel::Ros2 => ChannelCapabilityProfile {
            supports_callbacks: false,
            supports_inline_buttons: false,
            supports_rich_media: false,
            supports_typing_indicator: false,
            supports_thread_context: false,
            supports_command_aliases: true,
        },
        GatewayChannel::Unknown => ChannelCapabilityProfile {
            supports_callbacks: false,
            supports_inline_buttons: false,
            supports_rich_media: false,
            supports_typing_indicator: false,
            supports_thread_context: false,
            supports_command_aliases: false,
        },
    }
}

pub fn builtin_channel_plugin_manifest(channel: GatewayChannel) -> ChannelPluginManifest {
    let (plugin_id, transport, approval_capable, fallback_mode) = match channel {
        GatewayChannel::Telegram => (
            "builtin.telegram",
            "telegram",
            true,
            ChannelFallbackMode::TextFallback,
        ),
        GatewayChannel::WhatsApp => (
            "builtin.whatsapp",
            "whatsapp",
            true,
            ChannelFallbackMode::TextFallback,
        ),
        GatewayChannel::Cli => ("builtin.cli", "cli", true, ChannelFallbackMode::NativeOnly),
        GatewayChannel::WebSocket => (
            "builtin.websocket",
            "websocket",
            true,
            ChannelFallbackMode::TextFallback,
        ),
        GatewayChannel::Discord => (
            "builtin.discord",
            "discord",
            false,
            ChannelFallbackMode::Unsupported,
        ),
        GatewayChannel::Slack => (
            "builtin.slack",
            "slack",
            false,
            ChannelFallbackMode::Unsupported,
        ),
        GatewayChannel::IMessage => (
            "builtin.imessage",
            "imessage",
            false,
            ChannelFallbackMode::Unsupported,
        ),
        GatewayChannel::Ros2 => (
            "builtin.ros2",
            "ros2",
            false,
            ChannelFallbackMode::Unsupported,
        ),
        GatewayChannel::Unknown => (
            "builtin.unknown",
            "unknown",
            false,
            ChannelFallbackMode::Unsupported,
        ),
    };
    ChannelPluginManifest {
        channel,
        plugin_id: plugin_id.to_string(),
        transport: transport.to_string(),
        control_capabilities: channel_capability_profile(channel),
        approval_capable,
        fallback_mode,
    }
}

pub fn validate_channel_plugin_manifest(manifest: &ChannelPluginManifest) -> Result<(), String> {
    if manifest.plugin_id.trim().is_empty() {
        return Err("channel plugin manifest requires plugin_id".into());
    }
    if manifest.transport.trim().is_empty() {
        return Err("channel plugin manifest requires transport".into());
    }
    if manifest.approval_capable
        && matches!(manifest.fallback_mode, ChannelFallbackMode::Unsupported)
    {
        return Err("approval-capable channel cannot declare unsupported fallback".into());
    }
    if manifest.control_capabilities.supports_inline_buttons
        && !manifest.control_capabilities.supports_callbacks
    {
        return Err("inline buttons require callback support".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionScopePolicy {
    Main,
    Peer,
    ChannelPeer,
    AccountChannelPeer,
}

impl Default for SessionScopePolicy {
    fn default() -> Self {
        Self::Main
    }
}

pub fn derive_scoped_session_id(
    original_session_id: Uuid,
    channel: GatewayChannel,
    user_id: &str,
    policy: SessionScopePolicy,
) -> Uuid {
    match policy {
        SessionScopePolicy::Main => original_session_id,
        SessionScopePolicy::Peer => scoped_session_hash(&["peer", user_id]),
        SessionScopePolicy::ChannelPeer => {
            scoped_session_hash(&["channel_peer", gateway_channel_name(channel), user_id])
        }
        SessionScopePolicy::AccountChannelPeer => scoped_session_hash(&[
            "account_channel_peer",
            gateway_channel_name(channel),
            user_id,
            &hex_session_id(original_session_id),
        ]),
    }
}

fn scoped_session_hash(parts: &[&str]) -> Uuid {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0u8]);
    }
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes
}

fn gateway_channel_name(channel: GatewayChannel) -> &'static str {
    match channel {
        GatewayChannel::Telegram => "telegram",
        GatewayChannel::WhatsApp => "whatsapp",
        GatewayChannel::Discord => "discord",
        GatewayChannel::Slack => "slack",
        GatewayChannel::IMessage => "imessage",
        GatewayChannel::Cli => "cli",
        GatewayChannel::WebSocket => "websocket",
        GatewayChannel::Ros2 => "ros2",
        GatewayChannel::Unknown => "unknown",
    }
}

fn hex_session_id(session_id: Uuid) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for byte in session_id {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalResolutionDecision {
    Approve,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "intent", rename_all = "snake_case")]
pub enum ControlIntent {
    ListAgents,
    SwitchAgent {
        agent_id: Option<String>,
    },
    ListRuns,
    InspectRunTree {
        session_id: Option<String>,
    },
    InspectRun {
        run_id: Option<String>,
    },
    InspectRunEvents {
        run_id: Option<String>,
    },
    CancelRun {
        run_id: Option<String>,
    },
    RetryRun {
        run_id: Option<String>,
    },
    TakeoverRun {
        run_id: Option<String>,
        agent_id: Option<String>,
    },
    InspectMailbox {
        run_id: Option<String>,
    },
    ListModels,
    SwitchModel {
        model_ref: Option<String>,
    },
    SetTimezone {
        timezone: Option<String>,
    },
    InstallSkill {
        signed_module_json: Option<String>,
    },
    StopCurrent,
    Pivot {
        instructions: Option<String>,
    },
    InspectSession,
    ClearSession,
    ListProviderHealth,
    ListWorkspaceLocks,
    ListApprovals,
    ResolveApproval {
        decision: ApprovalResolutionDecision,
        target: Option<String>,
        tool_hint: Option<String>,
    },
}

fn control_aliases_for_channel(channel: GatewayChannel) -> &'static [(&'static str, &'static str)] {
    match channel {
        GatewayChannel::Telegram | GatewayChannel::WhatsApp => &[
            ("/a", "/approve"),
            ("/d", "/deny"),
            ("/ag", "/agent"),
            ("/tz", "/timezone"),
        ],
        GatewayChannel::Cli | GatewayChannel::WebSocket => {
            &[(":approve", "/approve"), (":deny", "/deny")]
        }
        _ => &[],
    }
}

fn normalize_control_alias(channel: GatewayChannel, command: &str) -> &str {
    control_aliases_for_channel(channel)
        .iter()
        .find_map(|(alias, canonical)| (*alias == command).then_some(*canonical))
        .unwrap_or(command)
}

pub fn parse_control_intent(text: &str, channel: GatewayChannel) -> Option<ControlIntent> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parts = trimmed.split_whitespace();
    let command = parts.next()?;
    let command = normalize_control_alias(channel, command);
    match command {
        "/agents" => Some(ControlIntent::ListAgents),
        "/agent" => Some(ControlIntent::SwitchAgent {
            agent_id: parts.next().map(ToString::to_string),
        }),
        "/runs" => Some(ControlIntent::ListRuns),
        "/run_tree" => Some(ControlIntent::InspectRunTree {
            session_id: parts.next().map(ToString::to_string),
        }),
        "/run" => Some(ControlIntent::InspectRun {
            run_id: parts.next().map(ToString::to_string),
        }),
        "/run_events" => Some(ControlIntent::InspectRunEvents {
            run_id: parts.next().map(ToString::to_string),
        }),
        "/run_cancel" => Some(ControlIntent::CancelRun {
            run_id: parts.next().map(ToString::to_string),
        }),
        "/run_retry" => Some(ControlIntent::RetryRun {
            run_id: parts.next().map(ToString::to_string),
        }),
        "/run_takeover" => Some(ControlIntent::TakeoverRun {
            run_id: parts.next().map(ToString::to_string),
            agent_id: parts.next().map(ToString::to_string),
        }),
        "/mailbox" => Some(ControlIntent::InspectMailbox {
            run_id: parts.next().map(ToString::to_string),
        }),
        "/models" => Some(ControlIntent::ListModels),
        "/model" => Some(ControlIntent::SwitchModel {
            model_ref: parts.next().map(ToString::to_string),
        }),
        "/timezone" => Some(ControlIntent::SetTimezone {
            timezone: parts.next().map(ToString::to_string),
        }),
        "/install_skill" => Some(ControlIntent::InstallSkill {
            signed_module_json: trimmed
                .strip_prefix(command)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
        }),
        "/stop" => Some(ControlIntent::StopCurrent),
        "/pivot" => Some(ControlIntent::Pivot {
            instructions: trimmed
                .strip_prefix(command)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string),
        }),
        "/session" => match parts.next() {
            Some(arg) if matches!(arg, "clear" | "reset") => Some(ControlIntent::ClearSession),
            _ => Some(ControlIntent::InspectSession),
        },
        "/provider_health" | "/providers" => Some(ControlIntent::ListProviderHealth),
        "/workspace_locks" | "/locks" => Some(ControlIntent::ListWorkspaceLocks),
        "/approvals" => Some(ControlIntent::ListApprovals),
        "/approve" => Some(ControlIntent::ResolveApproval {
            decision: ApprovalResolutionDecision::Approve,
            target: parts.next().map(ToString::to_string),
            tool_hint: parts.next().map(ToString::to_string),
        }),
        "/deny" => Some(ControlIntent::ResolveApproval {
            decision: ApprovalResolutionDecision::Deny,
            target: parts.next().map(ToString::to_string),
            tool_hint: parts.next().map(ToString::to_string),
        }),
        _ => None,
    }
}

/// Policy decision associated with each tool execution record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PolicyDecision {
    Allow,
    Deny,
    AskUser,
}

/// Lifecycle state for a human approval request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
}

/// Durable approval record for a pending or completed sensitive action.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub approval_id: String,
    pub session_id: Uuid,
    pub user_id: String,
    pub channel: GatewayChannel,
    pub agent_id: String,
    pub tool_name: String,
    pub arguments_json: String,
    pub pending_prompt: String,
    pub original_request: String,
    pub status: ApprovalStatus,
    pub created_at_us: u64,
    pub resolved_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ElevationGrant {
    pub session_id: Uuid,
    pub user_id: String,
    pub agent_id: String,
    pub granted_at_us: u64,
    pub expires_at_us: Option<u64>,
}

/// Telemetry record for an executed tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillExecutionRecord {
    /// Tool name (e.g. `read_file`).
    pub tool_name: String,
    /// JSON-encoded arguments used for invocation.
    pub arguments_json: String,
    /// Short summary of the result returned by runtime.
    pub result_summary: String,
    /// Runtime duration in milliseconds for this tool call.
    pub duration_ms: u32,
    /// Authorization decision observed before execution.
    pub policy_decision: PolicyDecision,
}

/// High-level telemetry log entry used by the distillation engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryLog {
    /// Encoded state vector for the trajectory at this step.
    pub state_vector: Vec<f32>,
    /// Tool or action identifier taken by the orchestrator.
    pub mcp_action: String,
    /// Reward score in \[-1.0, 1.0\] assigned by the reward model.
    pub reward_score: f32,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Constraint violation emitted by the HAL safety envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConstraintViolation {
    /// Node that attempted the unsafe actuation.
    pub node_id: String,
    /// Motor/actuator identifier that was targeted.
    pub motor_id: u8,
    /// Velocity requested by the upstream controller.
    pub requested_velocity: f32,
    /// Maximum safe envelope velocity.
    pub envelope_max: f32,
    /// Timestamp in microseconds since UNIX epoch.
    pub timestamp_us: u64,
}

/// Runtime agent profile as described in the architecture blueprint.
///
/// This struct is intentionally more general than the TOML-backed
/// configuration used by `aria-intelligence`; downstream crates are free
/// to project into lighter-weight views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub description: String,
    /// Optional pre-computed embedding of `description`.
    pub description_vec: Vec<f32>,
    /// Logical backend identifier (e.g. `"local"`, `"remote-llama"`).
    pub llm_backend: String,
    pub system_prompt: String,
    pub base_tool_names: Vec<String>,
    /// Maximum tools visible to the LLM at any time.
    pub context_cap: u8,
    /// Maximum unique tools ever loaded in this session.
    pub session_tool_ceiling: u8,
    /// Maximum LLM→tool cycles per request.
    pub max_tool_rounds: u8,
    /// Optional fallback agent id to delegate to on failure.
    pub fallback_agent: Option<String>,
}

/// Per-session dynamic tool cache state model.
///
/// The Intelligence layer maintains an in-memory implementation that uses
/// an LRU-backed cache; this struct captures the logical state for
/// telemetry and persistence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DynamicToolCacheState {
    /// Session this cache belongs to.
    pub session_id: Uuid,
    /// LRU-ordered tool names currently in the model context.
    pub active_tools: VecDeque<String>,
    /// All tools that have ever been loaded in this session.
    pub session_loaded: BTreeSet<String>,
    /// Whether `session_tool_ceiling` has been reached.
    pub ceiling_reached: bool,
}

/// Registration metadata for a single skill implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillRegistration {
    /// Globally-unique skill identifier.
    pub skill_id: String,
    /// Name of the tool exposed to the LLM.
    pub tool_name: String,
    /// Name of the host that owns this skill (e.g. node id).
    pub host_node_id: String,
}

/// Manifest snapshot of all skills available across the mesh.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    /// All registered skills, regardless of node.
    pub registrations: Vec<SkillRegistration>,
}

/// Metadata describing a tool available to the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Machine-readable tool name (e.g. `"read_file"`).
    pub name: String,
    /// Human-readable description of the tool's purpose.
    pub description: String,
    /// JSON Schema string describing the tool's parameters.
    pub parameters: String,
    /// Pre-computed embedding vector for semantic routing.
    pub embedding: Vec<f32>,
    /// Whether this tool must only be exposed to strict-schema models.
    #[serde(default)]
    pub requires_strict_schema: bool,
    /// Whether streamed tool-argument assembly is safe for this tool.
    #[serde(default)]
    pub streaming_safe: bool,
    /// Whether the tool may execute in parallel with peer tool calls.
    #[serde(default = "default_tool_parallel_safe")]
    pub parallel_safe: bool,
    /// Modalities required by the tool. Empty means text-only.
    #[serde(default)]
    pub modalities: Vec<ToolModality>,
}

const fn default_tool_parallel_safe() -> bool {
    true
}
