use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use aria_core::{
    AgentCapabilityProfile, AgentMailboxMessage, AgentRunEvent, AgentRunRecord, ApprovalRecord,
    BrowserActionAuditRecord, BrowserArtifactRecord, BrowserChallengeEvent,
    BrowserLoginStateRecord, BrowserProfile, BrowserProfileBindingRecord, BrowserSessionRecord,
    BrowserSessionStateRecord, CompactionState, ComputerActionAuditRecord,
    ComputerArtifactRecord, ComputerExecutionProfile, ComputerSessionRecord,
    ControlDocumentEntry, CrawlJob, DomainAccessDecision, ElevationGrant,
    ExecutionBackendProfile, ExecutionWorkerRecord, RobotRuntimeRecord,
    RoboticsSimulationRecord, Ros2BridgeProfile,
    ModelCapabilityProbeRecord, ModelCapabilityProfile, OutboundEnvelope,
    ProviderCapabilityProfile, ScopeDenialRecord, SkillActivationRecord, SkillBinding,
    SkillPackageManifest, WatchJobRecord, WebsiteMemoryRecord,
};
#[cfg(feature = "mcp-runtime")]
use aria_core::{
    McpBindingRecord, McpImportCacheRecord, McpImportedPrompt, McpImportedResource,
    McpImportedTool, McpServerProfile,
};
use aria_intelligence::CachedTool;
use aria_learning::{
    build_candidate_promotion_record, build_macro_compilation_dataset,
    build_prompt_optimization_dataset, build_wasm_compilation_dataset,
    compile_macro_candidate_from_dataset, compile_prompt_candidate_from_dataset,
    compile_wasm_candidate_from_dataset, evaluate_candidate_against_replay,
    synthesize_candidate_for_cluster, task_fingerprint_matches_request,
    train_selector_models_for_samples, CandidateArtifactRecord, CandidateEvaluationRun,
    CandidatePromotionAction, CandidatePromotionRecord, CandidatePromotionStatus, ExecutionTrace,
    FingerprintCluster, FingerprintEvaluationSummary, LearningDerivativeEvent,
    LearningDerivativeKind, LearningMetricsSnapshot, MacroCompilationDataset,
    PromptOptimizationDataset, ReplaySample, RewardEvent, RewardKind, SelectorModelKind,
    SelectorModelRecord, TraceOutcome, WasmCompilationDataset,
};
use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone)]
pub struct RuntimeStore {
    path: PathBuf,
}

struct RuntimeStorePool {
    path: PathBuf,
    available: Mutex<Vec<Connection>>,
    total: AtomicUsize,
    max_size: usize,
    waiters: Condvar,
}

struct StoreConnection {
    pool: Arc<RuntimeStorePool>,
    conn: Option<Connection>,
}

impl std::ops::Deref for StoreConnection {
    type Target = Connection;

    fn deref(&self) -> &Self::Target {
        self.conn
            .as_ref()
            .expect("pooled sqlite connection missing")
    }
}

impl std::ops::DerefMut for StoreConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.conn
            .as_mut()
            .expect("pooled sqlite connection missing")
    }
}

impl RuntimeStore {
    pub(crate) fn cache_key(&self) -> String {
        self.path.display().to_string()
    }
}

impl Drop for StoreConnection {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut guard = self
                .pool
                .available
                .lock()
                .expect("runtime store pool poisoned");
            guard.push(conn);
            self.pool.waiters.notify_one();
        }
    }
}

const DEFAULT_SKILL_SIGNATURE_RETENTION_ROWS: u32 = 5_000;
const DEFAULT_SHELL_EXEC_AUDIT_RETENTION_ROWS: u32 = 20_000;
const DEFAULT_SCOPE_DENIAL_RETENTION_ROWS: u32 = 20_000;
const DEFAULT_REQUEST_POLICY_AUDIT_RETENTION_ROWS: u32 = 10_000;
const DEFAULT_REPAIR_FALLBACK_AUDIT_RETENTION_ROWS: u32 = 5_000;
const DEFAULT_STREAMING_DECISION_AUDIT_RETENTION_ROWS: u32 = 10_000;
const DEFAULT_BROWSER_ACTION_AUDIT_RETENTION_ROWS: u32 = 20_000;
const DEFAULT_BROWSER_CHALLENGE_EVENT_RETENTION_ROWS: u32 = 5_000;
static SKILL_SIGNATURE_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_SKILL_SIGNATURE_RETENTION_ROWS);
static SHELL_EXEC_AUDIT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_SHELL_EXEC_AUDIT_RETENTION_ROWS);
static SCOPE_DENIAL_RETENTION_ROWS: AtomicU32 = AtomicU32::new(DEFAULT_SCOPE_DENIAL_RETENTION_ROWS);
static REQUEST_POLICY_AUDIT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_REQUEST_POLICY_AUDIT_RETENTION_ROWS);
static REPAIR_FALLBACK_AUDIT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_REPAIR_FALLBACK_AUDIT_RETENTION_ROWS);
static STREAMING_DECISION_AUDIT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_STREAMING_DECISION_AUDIT_RETENTION_ROWS);
static BROWSER_ACTION_AUDIT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_BROWSER_ACTION_AUDIT_RETENTION_ROWS);
static BROWSER_CHALLENGE_EVENT_RETENTION_ROWS: AtomicU32 =
    AtomicU32::new(DEFAULT_BROWSER_CHALLENGE_EVENT_RETENTION_ROWS);

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundDeliveryRecord {
    pub envelope_id: String,
    pub channel: String,
    pub recipient_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobSnapshotRecord<T> {
    pub job: T,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkillSignatureRecord {
    pub record_id: String,
    pub skill_id: String,
    pub version: String,
    pub algorithm: String,
    pub payload_sha256_hex: String,
    pub public_key_hex: String,
    pub signature_hex: String,
    pub source: String,
    pub verified: bool,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableQueueKind {
    Ingress,
    Run,
    Outbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurableQueueStatus {
    Pending,
    Claimed,
    Acked,
    DeadLetter,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableQueueMessage {
    pub message_id: String,
    pub queue: DurableQueueKind,
    pub tenant_id: String,
    pub workspace_scope: String,
    pub dedupe_key: Option<String>,
    pub payload_json: String,
    pub attempt_count: u32,
    pub last_error: Option<String>,
    pub status: DurableQueueStatus,
    pub visible_at_us: u64,
    pub claimed_by: Option<String>,
    pub claimed_until_us: Option<u64>,
    pub created_at_us: u64,
    pub updated_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DurableQueueDlqRecord {
    pub dlq_id: String,
    pub message_id: String,
    pub queue: DurableQueueKind,
    pub tenant_id: String,
    pub workspace_scope: String,
    pub payload_json: String,
    pub final_error: String,
    pub attempt_count: u32,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ShellExecutionAuditRecord {
    pub audit_id: String,
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
    pub execution_backend_id: Option<String>,
    pub command: String,
    pub cwd: Option<String>,
    pub os_containment_requested: bool,
    pub containment_backend: Option<String>,
    pub timeout_seconds: u64,
    pub cpu_seconds: u64,
    pub memory_kb: u64,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output_truncated: bool,
    pub error: Option<String>,
    pub duration_ms: u64,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RequestPolicyAuditRecord {
    pub audit_id: String,
    pub request_id: String,
    pub session_id: String,
    pub user_id: String,
    pub agent_id: Option<String>,
    pub channel: String,
    pub tool_runtime_policy: aria_core::ToolRuntimePolicy,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepairFallbackAuditRecord {
    pub audit_id: String,
    pub request_id: String,
    pub session_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub tool_name: String,
    pub created_at_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StreamingDecisionAuditRecord {
    pub audit_id: String,
    pub request_id: String,
    pub session_id: String,
    pub user_id: String,
    pub agent_id: String,
    pub phase: String,
    pub mode: String,
    pub model_ref: Option<String>,
    pub created_at_us: u64,
}

#[path = "runtime_store/approvals.rs"]
mod approvals;
#[path = "runtime_store/audits.rs"]
mod audits;
#[path = "runtime_store/browser.rs"]
mod browser;
#[path = "runtime_store/computer.rs"]
mod computer;
#[path = "runtime_store/execution_backends.rs"]
mod execution_backends;
#[path = "runtime_store/connection.rs"]
mod connection;
#[path = "runtime_store/control.rs"]
mod control;
#[path = "runtime_store/working_set.rs"]
mod working_set;
#[path = "runtime_store/crawl.rs"]
mod crawl;
#[path = "runtime_store/helpers.rs"]
mod helpers;
#[path = "runtime_store/learning.rs"]
mod learning;
#[cfg(feature = "mcp-runtime")]
#[path = "runtime_store/mcp.rs"]
mod mcp;
#[path = "runtime_store/outbound.rs"]
mod outbound;
#[path = "runtime_store/queues.rs"]
mod queues;
#[path = "runtime_store/robotics.rs"]
mod robotics;
#[path = "runtime_store/runs.rs"]
mod runs;
#[path = "runtime_store/schema.rs"]
mod schema;
#[path = "runtime_store/skills.rs"]
mod skills;
use helpers::*;
#[cfg(test)]
#[path = "runtime_store/tests.rs"]
mod tests;
