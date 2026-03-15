//! # aria-intelligence
//!
//! ARIA-X semantic routing and dynamic tool cache.
//!
//! ## SemanticRouter
//!
//! Routes queries to the best-matching agent using cosine similarity
//! on pre-computed embedding vectors. No external network calls —
//! embeddings are loaded in-memory.
//!
//! ## DynamicToolCache
//!
//! LRU-based tool cache with two limits:
//! - `context_cap`: soft limit — evicts least-recently-used tools
//! - `session_ceiling`: hard limit — returns `CeilingReached` error

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fastembed::{EmbeddingModel as FastEmbedModel, InitOptions, TextEmbedding};
use tracing::{debug, info};

use aria_core::{
    AgentRequest, GatewayChannel, MessageContent, ModelCapabilityProfile, SkillManifest,
    SkillRegistration, TelemetryLog, ToolCallingMode, ToolDefinition, ToolModality,
    ToolRuntimePolicy, Uuid,
};
use aria_skill_runtime::SignedModule;
use futures::future::join_all;
use serde::{Deserialize, Serialize};

pub mod backends;
pub use backends::{
    EgressCredentialBroker, EgressSecretAuditRecord, EgressSecretOutcome, LLMBackend,
    ModelMetadata, ModelProvider,
};

mod hardware;
mod orchestrator;
mod prompting;
mod router;
mod runtime;
mod scheduler;
mod telemetry;
#[cfg(test)]
mod tests;
mod tools;

pub(crate) use hardware::render_tool_result_for_model;
pub use hardware::*;
#[cfg(test)]
pub(crate) use orchestrator::maybe_finalize_after_scheduler_tools;
pub use orchestrator::*;
pub(crate) use orchestrator::{append_tool_results_to_context_pack, append_tool_results_to_prompt};
pub use prompting::*;
pub use router::*;
#[cfg(test)]
pub(crate) use runtime::balance_json;
pub use runtime::*;
pub(crate) use runtime::{approval_required_tool_name, extract_tool_name_candidate};
pub use scheduler::*;
pub use telemetry::*;
pub use tools::*;
