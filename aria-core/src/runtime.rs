use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAvailabilityState {
    Available,
    Busy,
    Degraded,
    Paused,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentPresenceRecord {
    pub agent_id: String,
    pub availability: AgentAvailabilityState,
    #[serde(default)]
    pub active_run_count: u32,
    #[serde(default)]
    pub status_summary: Option<String>,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunParentLink {
    pub parent_run_id: String,
    pub child_run_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentSpawnRequest {
    pub agent_id: String,
    pub prompt: String,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub max_runtime_seconds: Option<u32>,
    #[serde(default)]
    pub approved_attachment_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunResult {
    #[serde(default)]
    pub response_summary: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub completed_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunRecord {
    pub run_id: String,
    #[serde(default)]
    pub parent_run_id: Option<String>,
    #[serde(default)]
    pub origin_kind: Option<AgentRunOriginKind>,
    #[serde(default)]
    pub lineage_run_id: Option<String>,
    pub session_id: Uuid,
    pub user_id: String,
    #[serde(default)]
    pub requested_by_agent: Option<String>,
    pub agent_id: String,
    pub status: AgentRunStatus,
    pub request_text: String,
    pub inbox_on_completion: bool,
    #[serde(default)]
    pub max_runtime_seconds: Option<u32>,
    pub created_at_us: u64,
    #[serde(default)]
    pub started_at_us: Option<u64>,
    #[serde(default)]
    pub finished_at_us: Option<u64>,
    #[serde(default)]
    pub result: Option<AgentRunResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunOriginKind {
    Spawned,
    Retry,
    Takeover,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRunEventKind {
    Queued,
    Started,
    Retried,
    TakeoverQueued,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    InboxNotification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunEvent {
    pub event_id: String,
    pub run_id: String,
    pub kind: AgentRunEventKind,
    pub summary: String,
    pub created_at_us: u64,
    #[serde(default)]
    pub related_run_id: Option<String>,
    #[serde(default)]
    pub actor_agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentMailboxMessage {
    pub message_id: String,
    pub run_id: String,
    pub session_id: Uuid,
    #[serde(default)]
    pub from_agent_id: Option<String>,
    #[serde(default)]
    pub to_agent_id: Option<String>,
    pub body: String,
    pub created_at_us: u64,
    #[serde(default)]
    pub delivered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunTreeNode {
    pub run: AgentRunRecord,
    #[serde(default)]
    pub child_run_ids: Vec<String>,
    #[serde(default)]
    pub mailbox_count: u32,
    #[serde(default)]
    pub last_event_kind: Option<AgentRunEventKind>,
    #[serde(default)]
    pub last_event_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunTransition {
    pub kind: AgentRunEventKind,
    pub source_run_id: String,
    pub target_run_id: String,
    pub summary: String,
    pub created_at_us: u64,
    #[serde(default)]
    pub actor_agent_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunTreeSnapshot {
    pub session_id: Uuid,
    #[serde(default)]
    pub root_run_ids: Vec<String>,
    #[serde(default)]
    pub orphan_parent_refs: Vec<String>,
    #[serde(default)]
    pub nodes: Vec<AgentRunTreeNode>,
    #[serde(default)]
    pub transitions: Vec<AgentRunTransition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillActivationPolicy {
    Manual,
    AutoSuggest,
    AutoLoadLowRisk,
    ApprovalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillProvenanceKind {
    Local,
    Imported,
    Generated,
    CompatibilityImport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillProvenance {
    pub kind: SkillProvenanceKind,
    #[serde(default)]
    pub source_ref: Option<String>,
    #[serde(default)]
    pub imported_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillPackageManifest {
    pub skill_id: String,
    pub name: String,
    pub description: String,
    pub version: String,
    pub entry_document: String,
    #[serde(default)]
    pub tool_names: Vec<String>,
    #[serde(default)]
    pub mcp_server_dependencies: Vec<String>,
    #[serde(default)]
    pub retrieval_hints: Vec<String>,
    #[serde(default)]
    pub wasm_module_ref: Option<String>,
    #[serde(default)]
    pub config_schema: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub provenance: Option<SkillProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillBinding {
    pub binding_id: String,
    pub agent_id: String,
    pub skill_id: String,
    pub activation_policy: SkillActivationPolicy,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillActivationRecord {
    pub activation_id: String,
    pub skill_id: String,
    pub agent_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<Uuid>,
    pub active: bool,
    pub activated_at_us: u64,
    #[serde(default)]
    pub deactivated_at_us: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerProfile {
    pub server_id: String,
    pub display_name: String,
    pub transport: String,
    pub endpoint: String,
    #[serde(default)]
    pub auth_ref: Option<String>,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPrimitiveKind {
    Tool,
    Prompt,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpImportedTool {
    pub import_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub description: String,
    pub parameters_schema: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpImportedPrompt {
    pub import_id: String,
    pub server_id: String,
    pub prompt_name: String,
    pub description: String,
    #[serde(default)]
    pub arguments_schema: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpImportedResource {
    pub import_id: String,
    pub server_id: String,
    pub resource_uri: String,
    pub description: String,
    #[serde(default)]
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpBindingRecord {
    pub binding_id: String,
    pub agent_id: String,
    pub server_id: String,
    pub primitive_kind: McpPrimitiveKind,
    pub target_name: String,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpImportCacheRecord {
    pub server_id: String,
    pub transport: String,
    pub tool_count: u32,
    pub prompt_count: u32,
    pub resource_count: u32,
    pub refreshed_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlDocumentKind {
    Instructions,
    Skills,
    Tools,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleScope {
    Org,
    User,
    Project,
    Path,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleSourceKind {
    HiveClaw,
    AgentsMd,
    ClaudeMd,
    UserRulesFile,
    OrgRulesFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleDecision {
    Applied,
    NotApplicable,
    Shadowed,
    Duplicate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleEntry {
    pub rule_id: String,
    pub scope: RuleScope,
    pub source_kind: RuleSourceKind,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub applies_to_path: Option<String>,
    pub title: String,
    pub body: String,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuleInspectionRecord {
    pub rule_id: String,
    pub scope: RuleScope,
    pub source_kind: RuleSourceKind,
    pub title: String,
    #[serde(default)]
    pub source_path: Option<String>,
    #[serde(default)]
    pub applies_to_path: Option<String>,
    pub decision: RuleDecision,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct RuleResolution {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub resolved_target_path: Option<String>,
    #[serde(default)]
    pub active_rules: Vec<RuleEntry>,
    #[serde(default)]
    pub records: Vec<RuleInspectionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlDocumentEntry {
    pub document_id: String,
    pub workspace_root: String,
    pub relative_path: String,
    pub kind: ControlDocumentKind,
    pub sha256_hex: String,
    pub body: String,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlDocumentSnapshot {
    pub snapshot_id: String,
    pub workspace_root: String,
    #[serde(default)]
    pub entries: Vec<ControlDocumentEntry>,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompactionStatus {
    Idle,
    Queued,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionMetadata {
    #[serde(default)]
    pub summary_hash: Option<String>,
    #[serde(default)]
    pub summary_version: u32,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactionState {
    pub session_id: Uuid,
    pub status: CompactionStatus,
    #[serde(default)]
    pub last_started_at_us: Option<u64>,
    #[serde(default)]
    pub last_completed_at_us: Option<u64>,
    pub metadata: CompactionMetadata,
}
