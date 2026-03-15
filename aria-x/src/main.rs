//! ARIA-X Orchestrator — the final binary that wires all crates together.
//!
//! Reads TOML configuration, initializes all subsystems, and runs the
//! ReAct agent loop with graceful SIGINT shutdown via a CLI gateway.

use std::io::{self, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    sync::atomic::Ordering,
    time::Duration,
};
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use axum::{
    extract::{ws::Message as AxumWsMessage, ws::WebSocketUpgrade, Path as AxumPath, Query, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
#[cfg(test)]
use base64::Engine;
use directories::ProjectDirs;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use governor::Quota;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use aria_core::{
    AgentCapabilityProfile, AgentMailboxMessage, AgentRequest, AgentRunEvent, AgentRunEventKind,
    AgentRunRecord, AgentRunStatus, AgentSpawnRequest, GatewayChannel, MessageContent,
    ScopeDenialKind, ScopeDenialRecord, SkillActivationPolicy, SkillActivationRecord, SkillBinding,
    SkillPackageManifest,
};
#[cfg(feature = "mcp-runtime")]
use aria_core::{
    McpBindingRecord, McpImportCacheRecord, McpImportedPrompt, McpImportedResource,
    McpImportedTool, McpPrimitiveKind, McpServerProfile,
};
use aria_gateway::{
    GatewayAdapter, GatewayError, TelegramNormalizer, WebSocketNormalizer, WhatsAppNormalizer,
};
use aria_intelligence::{
    backends::{
        self, ollama::OllamaBackend, resolve_capability_profile, ProviderRegistry, SecretRef,
    },
    AgentConfigStore, AgentOrchestrator, CachedTool, DynamicToolCache, EmbeddingModel,
    FastEmbedder, LLMBackend, LLMResponse, LlmBackendPool, OrchestratorError, OrchestratorEvent,
    OrchestratorEventSink, PromptManager, RouteConfig, RouterIndex, ScheduleSpec, ScheduledJobKind,
    ScheduledPromptJob, SemanticRouter, ToolCall, ToolExecutionResult, ToolExecutor,
    ToolManifestStore,
};
use aria_learning::{
    CandidateArtifactKind, CandidateArtifactRecord, ExecutionTrace, RewardEvent, RewardKind,
    SelectorModelKind, SelectorModelRecord, TaskFingerprint, TraceOutcome,
};
#[cfg(feature = "mcp-runtime")]
use aria_mcp::{McpClient, McpRegistry, TransportSelector};
use aria_ssmu::{
    vector::{KeywordIndex, VectorStore},
    HybridMemoryEngine, PageNode, QueryPlannerConfig,
};
use aria_vault::CredentialVault;
mod channel_health;
mod ingress;
mod outbound;
mod robotics_bridge;
mod runtime_store;
mod stt;
mod telegram_support;
mod tui;
use ingress::PartitionedIngressQueueBridge;
use outbound::{
    deterministic_outbound_envelope_id, dispatch_outbound,
    envelope_from_text_response_with_correlation, parse_media_response,
};
use runtime_store::{
    RepairFallbackAuditRecord, RequestPolicyAuditRecord, RuntimeStore, ShellExecutionAuditRecord,
    SkillSignatureRecord, StreamingDecisionAuditRecord,
};
use stt::{build_stt_backend, SpeechToTextBackend};
use telegram_support::{
    ChannelMetrics, DedupeWindow, TELEGRAM_UPDATE_QUEUE_CAPACITY, TELEGRAM_WORKER_COUNT,
};

#[cfg(test)]
static BROWSER_ENV_LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RagCorpus {
    Session,
    Workspace,
    PolicyRuntime,
    External,
    Social,
}

include!("config.rs");
include!("workspace_lock.rs");
include!("gateway_runtime.rs");
include!("tools.rs");
include!("browser.rs");
include!("web.rs");
include!("crawl.rs");
include!("scheduler.rs");
include!("approvals.rs");
include!("operator.rs");
include!("bootstrap.rs");
#[cfg(test)]
include!("test_support.rs");

fn main() {
    run_main();
}
