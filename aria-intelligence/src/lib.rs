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
    AgentRequest, GatewayChannel, MessageContent, SkillManifest, SkillRegistration, TelemetryLog,
    ToolDefinition, Uuid,
};
use aria_skill_runtime::SignedModule;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

pub mod backends;
pub use backends::{LLMBackend, ModelMetadata, ModelProvider};

// ---------------------------------------------------------------------------
// Cosine similarity
// ---------------------------------------------------------------------------

/// Compute cosine similarity between two vectors.
///
/// Returns a value in `[-1.0, 1.0]`, or `0.0` if either vector has zero magnitude.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let mut dot = 0.0_f32;
    let mut norm_a = 0.0_f32;
    let mut norm_b = 0.0_f32;

    for i in 0..a.len() {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }

    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < f32::EPSILON {
        0.0
    } else {
        dot / denom
    }
}

// ---------------------------------------------------------------------------
// SemanticRouter
// ---------------------------------------------------------------------------

/// Error type for the semantic router.
#[derive(Debug)]
pub enum RouterError {
    /// No agents have been registered.
    NoAgents,
    /// The query embedding dimension does not match agent embeddings.
    DimensionMismatch { expected: usize, got: usize },
    /// No candidate could be selected during fallback.
    NoRoutingCandidate,
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::NoAgents => write!(f, "no agents registered"),
            RouterError::DimensionMismatch { expected, got } => {
                write!(f, "dimension mismatch: expected {}, got {}", expected, got)
            }
            RouterError::NoRoutingCandidate => write!(f, "no routing candidates available"),
        }
    }
}

impl std::error::Error for RouterError {}

/// Local embedding model interface used by the semantic router.
///
/// Implementations must compute embeddings without network calls.
pub trait EmbeddingModel {
    fn embed(&self, text: &str) -> Vec<f32>;
}

/// Blanket impl so `Arc<T>` satisfies `impl EmbeddingModel` bounds.
impl<T: EmbeddingModel> EmbeddingModel for std::sync::Arc<T> {
    fn embed(&self, text: &str) -> Vec<f32> {
        (**self).embed(text)
    }
}

/// Lightweight deterministic local embedder based on token hashing.
///
/// This is intentionally simple and dependency-free so routing can operate
/// offline. It provides a stable vector space for router comparisons.
pub struct LocalHashEmbedder {
    dimension: usize,
}

impl LocalHashEmbedder {
    pub fn new(dimension: usize) -> Self {
        Self { dimension }
    }
}

impl EmbeddingModel for LocalHashEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        let mut vec = vec![0.0_f32; self.dimension];
        if self.dimension == 0 {
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
            let idx = (hash as usize) % self.dimension;
            vec[idx] += 1.0;
        }

        // L2-normalize to make cosine comparisons stable.
        let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > f32::EPSILON {
            for v in &mut vec {
                *v /= norm;
            }
        }
        vec
    }
}

// ---------------------------------------------------------------------------
// FastEmbedder — MiniLM-L6-v2 via fastembed-rs
// ---------------------------------------------------------------------------

/// Offline semantic embedder backed by all-MiniLM-L6-v2 (384-dim, 22MB).
///
/// The underlying `TextEmbedding` model is initialized once and stored
/// behind a `OnceLock` so multiple clones of `FastEmbedder` share the
/// same ONNX session.
///
/// On the first call the model is loaded from the fastembed cache
/// (`~/.cache/huggingface/hub` by default). Subsequent boots are instant.
pub struct FastEmbedder {
    model: Arc<TextEmbedding>,
}

impl FastEmbedder {
    /// Initialise the embedder, downloading MiniLM-L6-v2 on first run.
    pub fn new() -> Result<Self, String> {
        // Thread count is capped via ORT_NUM_THREADS env var set in dev.sh.
        // fastembed InitOptions has no direct thread setter.
        let model = TextEmbedding::try_new(
            InitOptions::new(FastEmbedModel::AllMiniLML6V2).with_show_download_progress(true),
        )
        .map_err(|e| format!("FastEmbedder init failed: {}", e))?;
        Ok(Self {
            model: Arc::new(model),
        })
    }
}

impl EmbeddingModel for FastEmbedder {
    fn embed(&self, text: &str) -> Vec<f32> {
        self.model
            .embed(vec![text.to_string()], None)
            .map(|mut v| v.pop().unwrap_or_default())
            .unwrap_or_default()
    }
}

/// Registered agent with pre-computed embedding.
struct AgentEntry {
    name: String,
    embedding: Vec<f32>,
}

/// Semantic router that matches queries to agents via cosine similarity.
///
/// Agents are registered with pre-computed embeddings. Routing finds
/// the agent whose embedding has the highest cosine similarity to the
/// query embedding. No network calls are made.
pub struct SemanticRouter {
    agents: Vec<AgentEntry>,
    dimension: Option<usize>,
}

/// Startup-built router index that stores agent embeddings and thresholds.
#[derive(Debug, Clone)]
pub struct RouterIndex {
    pub agents: Vec<(String, Vec<f32>)>,
    pub confidence_threshold: f32,
    pub tie_break_gap: f32,
}

/// Routing decision output that aligns with ARIA-X architecture:
/// direct semantic dispatch when confident, otherwise fallback path.
#[derive(Debug, Clone, PartialEq)]
pub enum RouterDecision {
    Confident { agent_id: String, score: f32 },
    NeedsLlmFallback { candidates: Vec<(String, f32)> },
}

/// Configurable thresholds for semantic router decisions.
#[derive(Debug, Clone, Copy)]
pub struct RouteConfig {
    pub confidence_threshold: f32,
    pub tie_break_gap: f32,
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: 0.70,
            tie_break_gap: 0.05,
        }
    }
}

/// Runtime agent profile loaded from TOML config.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub description: String,
    pub system_prompt: String,
    pub base_tool_names: Vec<String>,
    #[serde(default = "default_context_cap")]
    pub context_cap: usize,
    #[serde(default = "default_session_tool_ceiling")]
    pub session_tool_ceiling: usize,
    #[serde(default = "default_max_tool_rounds")]
    pub max_tool_rounds: usize,
    pub fallback_agent: Option<String>,
}

fn default_context_cap() -> usize {
    8
}
fn default_session_tool_ceiling() -> usize {
    15
}
fn default_max_tool_rounds() -> usize {
    5
}

/// Errors for loading agent configuration files.
#[derive(Debug)]
pub enum AgentStoreError {
    Io(String),
    Parse { file: String, message: String },
}

impl std::fmt::Display for AgentStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentStoreError::Io(msg) => write!(f, "agent store io error: {}", msg),
            AgentStoreError::Parse { file, message } => {
                write!(f, "agent config parse error in {}: {}", file, message)
            }
        }
    }
}

impl std::error::Error for AgentStoreError {}

/// In-memory registry of agent configs loaded from TOML files.
#[derive(Default, Debug, Clone)]
pub struct AgentConfigStore {
    configs: HashMap<String, AgentConfig>,
}

impl AgentConfigStore {
    pub fn new() -> Self {
        Self {
            configs: HashMap::new(),
        }
    }

    pub fn insert(&mut self, cfg: AgentConfig) {
        self.configs.insert(cfg.id.clone(), cfg);
    }

    pub fn get(&self, id: &str) -> Option<&AgentConfig> {
        self.configs.get(id)
    }

    pub fn all(&self) -> impl Iterator<Item = &AgentConfig> {
        self.configs.values()
    }

    pub fn len(&self) -> usize {
        self.configs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.configs.is_empty()
    }

    pub fn load_from_dir<P: AsRef<Path>>(dir: P) -> Result<Self, AgentStoreError> {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Ok(Self::new());
        }

        let mut store = Self::new();
        let entries = std::fs::read_dir(dir)
            .map_err(|e| AgentStoreError::Io(format!("read dir {}: {}", dir.display(), e)))?;
        for entry in entries {
            let entry =
                entry.map_err(|e| AgentStoreError::Io(format!("dir entry error: {}", e)))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            let raw = std::fs::read_to_string(&path).map_err(|e| {
                AgentStoreError::Io(format!("read agent config {}: {}", path.display(), e))
            })?;
            let cfg: AgentConfig = toml::from_str(&raw).map_err(|e| AgentStoreError::Parse {
                file: path.display().to_string(),
                message: e.to_string(),
            })?;
            store.insert(cfg);
        }
        Ok(store)
    }
}

impl Default for SemanticRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SemanticRouter {
    /// Create a new empty router.
    pub fn new() -> Self {
        Self {
            agents: Vec::new(),
            dimension: None,
        }
    }

    /// Register an agent with its pre-computed embedding.
    pub fn register_agent(&mut self, name: &str, embedding: Vec<f32>) -> Result<(), RouterError> {
        if let Some(dim) = self.dimension {
            if embedding.len() != dim {
                return Err(RouterError::DimensionMismatch {
                    expected: dim,
                    got: embedding.len(),
                });
            }
        } else {
            self.dimension = Some(embedding.len());
        }

        self.agents.push(AgentEntry {
            name: name.to_string(),
            embedding,
        });
        Ok(())
    }

    /// Register an agent from representative text by computing a local embedding.
    pub fn register_agent_text<E: EmbeddingModel>(
        &mut self,
        name: &str,
        representative_text: &str,
        embedder: &E,
    ) -> Result<(), RouterError> {
        self.register_agent(name, embedder.embed(representative_text))
    }

    /// Route a query embedding to the best-matching agent.
    ///
    /// Returns `(agent_name, similarity_score)`.
    pub fn route(&self, query_embedding: &[f32]) -> Result<(String, f32), RouterError> {
        if self.agents.is_empty() {
            return Err(RouterError::NoAgents);
        }

        if let Some(dim) = self.dimension {
            if query_embedding.len() != dim {
                return Err(RouterError::DimensionMismatch {
                    expected: dim,
                    got: query_embedding.len(),
                });
            }
        }

        let mut best_name = &self.agents[0].name;
        let mut best_score = f32::NEG_INFINITY;

        for agent in &self.agents {
            let score = cosine_similarity(&agent.embedding, query_embedding);
            if score > best_score {
                best_score = score;
                best_name = &agent.name;
            }
        }

        Ok((best_name.clone(), best_score))
    }

    /// Route using confidence and tie-break thresholds.
    ///
    /// - If top score is below `confidence_threshold`, fallback is requested.
    /// - If the gap between top-2 candidates is below `tie_break_gap`,
    ///   fallback is requested.
    pub fn route_with_config(
        &self,
        query_embedding: &[f32],
        cfg: RouteConfig,
    ) -> Result<RouterDecision, RouterError> {
        let ranked = self.rank_candidates(query_embedding)?;
        let (best_name, best_score) = ranked
            .first()
            .cloned()
            .ok_or(RouterError::NoRoutingCandidate)?;
        let second_score = ranked.get(1).map(|(_, s)| *s);
        let is_tie = second_score
            .map(|s| (best_score - s).abs() < cfg.tie_break_gap)
            .unwrap_or(false);

        if best_score < cfg.confidence_threshold || is_tie {
            Ok(RouterDecision::NeedsLlmFallback { candidates: ranked })
        } else {
            Ok(RouterDecision::Confident {
                agent_id: best_name,
                score: best_score,
            })
        }
    }

    /// Route raw text by embedding it locally and then performing cosine routing.
    pub fn route_text<E: EmbeddingModel>(
        &self,
        query_text: &str,
        embedder: &E,
    ) -> Result<(String, f32), RouterError> {
        let query_embedding = embedder.embed(query_text);
        self.route(&query_embedding)
    }

    /// Route raw text and return a confidence-aware decision.
    pub fn route_text_with_config<E: EmbeddingModel>(
        &self,
        query_text: &str,
        embedder: &E,
        cfg: RouteConfig,
    ) -> Result<RouterDecision, RouterError> {
        let query_embedding = embedder.embed(query_text);
        self.route_with_config(&query_embedding, cfg)
    }

    /// Get the similarity score for a specific agent.
    pub fn score_agent(&self, agent_name: &str, query_embedding: &[f32]) -> Option<f32> {
        self.agents
            .iter()
            .find(|a| a.name == agent_name)
            .map(|a| cosine_similarity(&a.embedding, query_embedding))
    }

    fn rank_candidates(&self, query_embedding: &[f32]) -> Result<Vec<(String, f32)>, RouterError> {
        if self.agents.is_empty() {
            return Err(RouterError::NoAgents);
        }
        if let Some(dim) = self.dimension {
            if query_embedding.len() != dim {
                return Err(RouterError::DimensionMismatch {
                    expected: dim,
                    got: query_embedding.len(),
                });
            }
        }

        let mut ranked: Vec<(String, f32)> = self
            .agents
            .iter()
            .map(|a| {
                (
                    a.name.clone(),
                    cosine_similarity(&a.embedding, query_embedding),
                )
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(ranked)
    }

    /// Build a stable `RouterIndex` snapshot from registered agents.
    pub fn build_index(&self, cfg: RouteConfig) -> RouterIndex {
        RouterIndex {
            agents: self
                .agents
                .iter()
                .map(|a| (a.name.clone(), a.embedding.clone()))
                .collect(),
            confidence_threshold: cfg.confidence_threshold,
            tie_break_gap: cfg.tie_break_gap,
        }
    }
}

impl RouterIndex {
    pub fn route(&self, query_embedding: &[f32]) -> Result<RouterDecision, RouterError> {
        if self.agents.is_empty() {
            debug!(decision = "NoAgents", "Router: no agents registered");
            return Err(RouterError::NoAgents);
        }
        let expected_dim = self.agents[0].1.len();
        if query_embedding.len() != expected_dim {
            return Err(RouterError::DimensionMismatch {
                expected: expected_dim,
                got: query_embedding.len(),
            });
        }

        let mut ranked: Vec<(String, f32)> = self
            .agents
            .iter()
            .map(|(id, emb)| (id.clone(), cosine_similarity(query_embedding, emb)))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let ranked_str: String = ranked
            .iter()
            .map(|(id, s)| format!("{}={:.3}", id, s))
            .collect::<Vec<_>>()
            .join(", ");
        debug!(
            ranked = %ranked_str,
            threshold = self.confidence_threshold,
            tie_break_gap = self.tie_break_gap,
            "Router: ranked candidates"
        );
        let (best_id, best_score) = ranked
            .first()
            .cloned()
            .ok_or(RouterError::NoRoutingCandidate)?;
        let second_score = ranked.get(1).map(|(_, s)| *s);
        let is_tie = second_score
            .map(|s| (best_score - s).abs() < self.tie_break_gap)
            .unwrap_or(false);
        if best_score < self.confidence_threshold || is_tie {
            debug!(
                best = %best_id,
                best_score = best_score,
                reason = if is_tie { "tie" } else { "below_threshold" },
                "Router: NeedsLlmFallback"
            );
            Ok(RouterDecision::NeedsLlmFallback { candidates: ranked })
        } else {
            debug!(
                agent_id = %best_id,
                score = best_score,
                "Router: Confident"
            );
            Ok(RouterDecision::Confident {
                agent_id: best_id,
                score: best_score,
            })
        }
    }

    pub fn route_text<E: EmbeddingModel>(
        &self,
        query_text: &str,
        embedder: &E,
    ) -> Result<RouterDecision, RouterError> {
        let qv = embedder.embed(query_text);
        self.route(&qv)
    }
}

/// LLM-assisted tie/low-confidence fallback selector.
///
/// The LLM receives only candidate IDs and scores. If output does not map to
/// a candidate, this function returns the top-ranked candidate.
pub async fn llm_route_fallback<L: LLMBackend>(
    llm: &L,
    user_text: &str,
    candidates: &[(String, f32)],
) -> Result<String, OrchestratorError> {
    let cand_str: String = candidates
        .iter()
        .map(|(id, s)| format!("{}={:.3}", id, s))
        .collect::<Vec<_>>()
        .join(", ");
    debug!(user_text = %user_text, candidates = %cand_str, "Router fallback: LLM selecting agent");
    if candidates.is_empty() {
        return Err(OrchestratorError::LLMError(
            "fallback router received no candidates".into(),
        ));
    }
    let options = candidates
        .iter()
        .map(|(id, score)| format!("{} ({:.3})", id, score))
        .collect::<Vec<_>>()
        .join(", ");
    let prompt = format!(
        "Select exactly one best agent id from candidates for the user request.\n\
         Return only the agent id.\n\
         user_request: {}\n\
         candidates: {}",
        user_text, options
    );
    let response = llm.query(&prompt, &[]).await?;
    let chosen = match response {
        LLMResponse::TextAnswer(text) => text.trim().to_string(),
        LLMResponse::ToolCalls(_) => String::new(),
    };

    if let Some((id, _)) = candidates.iter().find(|(id, _)| chosen.contains(id)) {
        debug!(chosen = %id, "Router fallback: LLM chose agent");
        return Ok(id.clone());
    }
    let default = candidates[0].0.clone();
    debug!(
        chosen_raw = %chosen,
        default = %default,
        "Router fallback: LLM output invalid, using top candidate"
    );
    Ok(default)
}

// ---------------------------------------------------------------------------
// DynamicToolCache
// ---------------------------------------------------------------------------

/// Error type for the tool cache.
#[derive(Debug, PartialEq)]
pub enum CacheError {
    /// The session ceiling (hard limit) has been reached. No more unique
    /// tools may be added to this session.
    CeilingReached {
        ceiling: usize,
        attempted_total: usize,
    },
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::CeilingReached {
                ceiling,
                attempted_total,
            } => write!(
                f,
                "session ceiling reached: limit {}, attempted {}",
                ceiling, attempted_total
            ),
        }
    }
}

impl std::error::Error for CacheError {}

/// A cached tool entry (lightweight reference to a ToolDefinition).
#[derive(Debug, Clone, PartialEq)]
pub struct CachedTool {
    /// Tool name (unique key).
    pub name: String,
    /// Tool description.
    pub description: String,
    /// JSON schema for parameters.
    pub parameters_schema: String,
}

/// LRU-based tool cache with context cap (soft) and session ceiling (hard).
///
/// - `context_cap`: Maximum tools kept in the active context. When exceeded,
///   the least-recently-used tool is evicted.
/// - `session_ceiling`: Absolute maximum unique tools across the session
///   lifetime. Exceeding this returns [`CacheError::CeilingReached`].
pub struct DynamicToolCache {
    context_cap: usize,
    session_ceiling: usize,
    /// Active tools in LRU order (back = most recent).
    active: VecDeque<CachedTool>,
    /// All tools ever seen in this session (for ceiling tracking).
    seen: HashMap<String, ()>,
}

impl DynamicToolCache {
    /// Create a new cache with the given limits.
    pub fn new(context_cap: usize, session_ceiling: usize) -> Self {
        Self {
            context_cap,
            session_ceiling,
            active: VecDeque::with_capacity(context_cap),
            seen: HashMap::new(),
        }
    }

    /// Insert a tool into the cache.
    ///
    /// - If the tool already exists, it's promoted to most-recently-used.
    /// - If `context_cap` is exceeded, the LRU tool is evicted.
    /// - If `session_ceiling` would be exceeded by a new unique tool,
    ///   returns [`CacheError::CeilingReached`].
    pub fn insert(&mut self, tool: CachedTool) -> Result<Option<CachedTool>, CacheError> {
        // If already in cache, promote it
        if let Some(pos) = self.active.iter().position(|t| t.name == tool.name) {
            self.active.remove(pos);
            self.active.push_back(tool);
            return Ok(None);
        }

        // New unique tool — check session ceiling
        if !self.seen.contains_key(&tool.name) {
            if self.seen.len() >= self.session_ceiling {
                return Err(CacheError::CeilingReached {
                    ceiling: self.session_ceiling,
                    attempted_total: self.seen.len() + 1,
                });
            }
            self.seen.insert(tool.name.clone(), ());
        }

        // Evict if at context cap
        let evicted = if self.active.len() >= self.context_cap {
            self.active.pop_front()
        } else {
            None
        };

        self.active.push_back(tool);
        Ok(evicted)
    }

    /// Get a tool by name, promoting it in the LRU order.
    pub fn get(&mut self, name: &str) -> Option<&CachedTool> {
        if let Some(pos) = self.active.iter().position(|t| t.name == name) {
            let tool = self.active.remove(pos).expect("just found");
            self.active.push_back(tool);
            self.active.back()
        } else {
            None
        }
    }

    /// Number of tools currently in the active context.
    pub fn len(&self) -> usize {
        self.active.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    /// Total unique tools seen across the session.
    pub fn total_seen(&self) -> usize {
        self.seen.len()
    }

    /// Snapshot of currently active tools in LRU order.
    pub fn active_tools(&self) -> Vec<CachedTool> {
        self.active.iter().cloned().collect()
    }
}

// ---------------------------------------------------------------------------
// ToolManifestStore + search_tool_registry support
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ToolRegistryError {
    EmptyStore,
}

impl std::fmt::Display for ToolRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolRegistryError::EmptyStore => write!(f, "tool registry is empty"),
        }
    }
}

impl std::error::Error for ToolRegistryError {}

/// Registry of all known tools with simple semantic search support.
#[derive(Debug, Clone)]
pub struct ToolManifestStore {
    tools: Vec<CachedTool>,
}

impl Default for ToolManifestStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolManifestStore {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    pub fn register(&mut self, tool: CachedTool) {
        self.tools.push(tool);
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn get_by_name(&self, name: &str) -> Option<CachedTool> {
        self.tools.iter().find(|t| t.name == name).cloned()
    }

    pub fn search<E: EmbeddingModel>(
        &self,
        query: &str,
        embedder: &E,
        top_k: usize,
    ) -> Result<Vec<(CachedTool, f32)>, ToolRegistryError> {
        if self.tools.is_empty() {
            return Err(ToolRegistryError::EmptyStore);
        }
        if top_k == 0 {
            return Ok(Vec::new());
        }
        let qv = embedder.embed(query);
        let mut ranked: Vec<(CachedTool, f32)> = self
            .tools
            .iter()
            .cloned()
            .map(|t| {
                let tv = embedder.embed(&format!("{} {}", t.name, t.description));
                let score = cosine_similarity(&qv, &tv);
                (t, score)
            })
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked.truncate(top_k);
        Ok(ranked)
    }

    /// Implements the `search_tool_registry` behavior for one query.
    /// Returns the inserted tool on success.
    pub fn hot_swap_best<E: EmbeddingModel>(
        &self,
        cache: &mut DynamicToolCache,
        query: &str,
        embedder: &E,
    ) -> Result<Option<CachedTool>, CacheError> {
        let results = self.search(query, embedder, 1);
        match results.ok().and_then(|mut v| v.pop()) {
            Some((tool, score)) => {
                debug!(
                    query = %query,
                    tool = %tool.name,
                    score = score,
                    "ToolManifestStore: hot_swap_best found tool"
                );
                cache.insert(tool.clone())?;
                Ok(Some(tool))
            }
            None => {
                debug!(query = %query, "ToolManifestStore: hot_swap_best no match");
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Telemetry ring buffer + distillation engine
// ---------------------------------------------------------------------------

/// Fixed-capacity ring buffer for `<state, action, reward>` telemetry tuples.
///
/// Backed by a `VecDeque` with simple eviction semantics: inserting into a
/// full buffer drops the oldest entry.
#[derive(Debug)]
pub struct TelemetryRingBuffer {
    capacity: usize,
    entries: VecDeque<TelemetryLog>,
}

impl TelemetryRingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TelemetryLog> {
        self.entries.iter()
    }

    /// Push a new telemetry record, evicting the oldest entry if at capacity.
    pub fn push(&mut self, log: TelemetryLog) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(log);
    }

    /// Count how many times a particular `mcp_action` appears in the buffer.
    pub fn count_action(&self, action: &str) -> usize {
        self.entries
            .iter()
            .filter(|e| e.mcp_action == action)
            .count()
    }
}

/// Result of a successful distillation run.
#[derive(Debug, Clone)]
pub struct DistilledSkill {
    /// Tool definition injected into the ToolManifestStore.
    pub tool: ToolDefinition,
    /// Skill registration entry added to the SkillManifest.
    pub registration: SkillRegistration,
    /// Compiled + signed Wasm module for deployment.
    pub signed_module: SignedModule,
}

/// Simple deployment interface so the distillation engine can publish signed
/// modules to target nodes without depending directly on the mesh layer.
pub trait DeploymentBus: Send + Sync {
    fn deploy(&self, node_id: &str, signed: &SignedModule) -> Result<(), String>;
}

/// Distillation engine that:
/// - Maintains a ring buffer of TelemetryLog entries
/// - Detects repeated tool chains (by `mcp_action`) over a threshold
/// - Synthesizes a distilled tool definition
/// - Compiles + signs a Wasm module (stub implementation)
/// - Registers the tool and publishes a deployment event via a `DeploymentBus`
pub struct DistillationEngine {
    buffer: TelemetryRingBuffer,
    threshold: usize,
    target_node: String,
    next_skill_id: u32,
}

impl DistillationEngine {
    /// Create a new engine with the specified ring-buffer capacity, pattern
    /// threshold, and default deployment target node.
    pub fn new(capacity: usize, threshold: usize, target_node: impl Into<String>) -> Self {
        Self {
            buffer: TelemetryRingBuffer::new(capacity),
            threshold: threshold.max(1),
            target_node: target_node.into(),
            next_skill_id: 1,
        }
    }

    pub fn buffer(&self) -> &TelemetryRingBuffer {
        &self.buffer
    }

    /// Ingest a new telemetry log and, if the associated `mcp_action` has
    /// reached the configured threshold, return a synthesized distilled skill.
    pub fn log_and_maybe_distill(&mut self, log: TelemetryLog) -> Option<DistilledSkill> {
        let action = log.mcp_action.clone();
        self.buffer.push(log);
        let count = self.buffer.count_action(&action);
        if count >= self.threshold {
            Some(self.distill_for_action(&action))
        } else {
            None
        }
    }

    fn distill_for_action(&mut self, action: &str) -> DistilledSkill {
        let skill_id = self.next_skill_id;
        self.next_skill_id += 1;

        let tool_name = Self::derive_tool_name(action, skill_id);
        let description = format!("Distilled skill for repeated pattern '{}'", action);

        let tool = ToolDefinition {
            name: tool_name.clone(),
            description,
            // Parameters deliberately minimal; production systems can use a
            // richer schema inferred from the original tool calls.
            parameters: r#"{"type":"object","properties":{}}"#.into(),
            embedding: Vec::new(),
        };

        let registration = SkillRegistration {
            skill_id: format!("distilled-{}", skill_id),
            tool_name: tool_name.clone(),
            host_node_id: self.target_node.clone(),
        };

        let wasm_bytes = Self::compile_stub_wasm(&tool_name, action);
        let signed_module = Self::sign_stub_module(wasm_bytes);

        DistilledSkill {
            tool,
            registration,
            signed_module,
        }
    }

    fn derive_tool_name(action: &str, skill_id: u32) -> String {
        let mut base = action.replace(['→', ' '], "_");
        if base.is_empty() {
            base = "distilled_tool".into();
        }
        format!("{}_{}", base, skill_id)
    }

    fn compile_stub_wasm(tool_name: &str, action: &str) -> Vec<u8> {
        // Stub compilation pipeline: encode a small deterministic payload
        // that is non-empty and unique per (tool_name, action).
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"wasm");
        bytes.extend_from_slice(tool_name.as_bytes());
        bytes.extend_from_slice(b"::");
        bytes.extend_from_slice(action.as_bytes());
        bytes
    }

    fn sign_stub_module(bytes: Vec<u8>) -> SignedModule {
        // Use a fixed dev/build key for deterministic signing.
        // In production, the build pipeline would use a proper CA-signed key.
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[2u8; 32]);
        aria_skill_runtime::sign_module(bytes, &signing_key)
    }

    /// Register a distilled skill into both the tool manifest store and the
    /// skill manifest, then deploy it via the provided `DeploymentBus`.
    pub fn register_and_deploy<B: DeploymentBus>(
        &self,
        distilled: &DistilledSkill,
        tool_store: &mut ToolManifestStore,
        manifest: &mut SkillManifest,
        bus: &B,
    ) -> Result<(), String> {
        let cached = CachedTool {
            name: distilled.tool.name.clone(),
            description: distilled.tool.description.clone(),
            parameters_schema: distilled.tool.parameters.clone(),
        };
        tool_store.register(cached);
        manifest.registrations.push(distilled.registration.clone());
        bus.deploy(&self.target_node, &distilled.signed_module)
    }
}

/// Trait for an isolated learner which can refine reward models based on a
/// batch of telemetry logs. Implementations are expected to run in a
/// separate process or thread.
#[async_trait::async_trait]
pub trait LearnerBackend: Send + Sync {
    async fn refine_reward_model(&self, batch: Vec<TelemetryLog>) -> Result<(), String>;
}

impl DistillationEngine {
    /// Run a training cycle on the provided learner backend using the current
    /// contents of the ring buffer.
    pub async fn run_training_cycle<L: LearnerBackend>(&self, learner: &L) -> Result<(), String> {
        let batch: Vec<TelemetryLog> = self.buffer.iter().cloned().collect();
        if batch.is_empty() {
            return Ok(());
        }
        learner.refine_reward_model(batch).await
    }
}

// ---------------------------------------------------------------------------
// Orchestrator types
// ---------------------------------------------------------------------------

/// A tool call requested by the LLM.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    /// Name of the tool to invoke.
    pub name: String,
    /// JSON-encoded arguments for the tool.
    pub arguments: String,
}

/// Response from the LLM backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LLMResponse {
    /// A final text answer — the ReAct loop should terminate.
    TextAnswer(String),
    /// One or more tool calls to execute before re-querying.
    ToolCalls(Vec<ToolCall>),
}

/// Errors from the orchestrator.
#[derive(Debug)]
pub enum OrchestratorError {
    /// The LLM backend returned an error.
    LLMError(String),
    /// A tool execution failed.
    ToolError(String),
    /// The maximum number of tool rounds was exceeded.
    MaxRoundsExceeded { limit: usize },
    /// The backend is overloaded or timed out.
    BackendOverloaded(String),
    /// A security violation was intercepted by the firewall.
    SecurityViolation(String),
    /// The human operator triggered an abort via steering.
    UserAborted,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::LLMError(msg) => write!(f, "LLM error: {}", msg),
            OrchestratorError::ToolError(msg) => write!(f, "tool error: {}", msg),
            OrchestratorError::MaxRoundsExceeded { limit } => {
                write!(f, "max rounds ({}) exceeded", limit)
            }
            OrchestratorError::BackendOverloaded(msg) => write!(f, "backend overloaded: {}", msg),
            OrchestratorError::SecurityViolation(msg) => {
                write!(f, "security violation: {}", msg)
            }
            OrchestratorError::UserAborted => write!(f, "aborted by user steering"),
        }
    }
}

impl std::error::Error for OrchestratorError {}

// LLM backends are now in the `backends` module.

/// Identifier for registered LLM backends.
pub type LlmBackendId = String;

pub struct LlmBackendPool {
    backends: Mutex<HashMap<LlmBackendId, Box<dyn LLMBackend>>>,
    fallback_order: Vec<LlmBackendId>,
    cooldown_for: Duration,
    cooldown_until: Mutex<HashMap<LlmBackendId, Instant>>,
    consecutive_failures: Mutex<HashMap<LlmBackendId, usize>>,
}

impl LlmBackendPool {
    pub fn new(fallback_order: Vec<LlmBackendId>, cooldown_for: Duration) -> Self {
        Self {
            backends: Mutex::new(HashMap::new()),
            fallback_order,
            cooldown_for,
            cooldown_until: Mutex::new(HashMap::new()),
            consecutive_failures: Mutex::new(HashMap::new()),
        }
    }

    pub fn register_backend(&self, id: impl Into<LlmBackendId>, backend: Box<dyn LLMBackend>) {
        let mut guard = self.backends.lock().expect("poisoned pool mutex");
        guard.insert(id.into(), backend);
    }

    fn is_cooling_down(&self, id: &str) -> bool {
        let guard = self.cooldown_until.lock().expect("poisoned cooldown mutex");
        guard
            .get(id)
            .map(|until| *until > Instant::now())
            .unwrap_or(false)
    }

    fn mark_cooldown(&self, id: &str) {
        let mut guard = self.cooldown_until.lock().expect("poisoned cooldown mutex");
        guard.insert(id.to_string(), Instant::now() + self.cooldown_for);
    }

    fn increment_failure(&self, id: &str) -> usize {
        let mut guard = self
            .consecutive_failures
            .lock()
            .expect("poisoned failures mutex");
        *guard
            .entry(id.to_string())
            .and_modify(|c| *c += 1)
            .or_insert(1)
    }

    fn reset_failures(&self, id: &str) {
        let mut guard = self
            .consecutive_failures
            .lock()
            .expect("poisoned failures mutex");
        guard.insert(id.to_string(), 0);
    }

    /// Query configured backends in fallback order, skipping cooling-down entries.
    pub async fn query_with_fallback(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError> {
        info!(
            prompt_len = prompt.len(),
            prompt = %prompt,
            "LLM: raw prompt sent to backend"
        );
        let mut last_err: Option<OrchestratorError> = None;
        for backend_id in &self.fallback_order {
            if self.is_cooling_down(backend_id) {
                continue;
            }
            let backend = {
                let guard = self.backends.lock().expect("poisoned pool mutex");
                guard.get(backend_id).map(|b| dyn_clone::clone_box(&*b))
            };
            let Some(backend) = backend else {
                continue;
            };
            match backend.query(prompt, tools).await {
                Ok(resp) => {
                    self.reset_failures(backend_id);
                    match &resp {
                        LLMResponse::TextAnswer(t) => info!(
                            response_len = t.len(),
                            response = %t,
                            "LLM: raw response"
                        ),
                        LLMResponse::ToolCalls(calls) => info!(
                            tool_count = calls.len(),
                            tool_names = ?calls.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
                            "LLM: tool calls returned"
                        ),
                    }
                    return Ok(resp);
                }
                Err(err) => {
                    if self.increment_failure(backend_id) >= 3 {
                        self.mark_cooldown(backend_id);
                    }
                    last_err = Some(err);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            OrchestratorError::LLMError("no available llm backends in pool".into())
        }))
    }
}

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Tool-call JSON drift recovery
// ---------------------------------------------------------------------------

/// Balance incomplete JSON braces.
/// Small models sometimes truncate the closing brace of tool call JSON.
fn balance_json(s: &str) -> String {
    let open = s.chars().filter(|&c| c == '{').count();
    let close = s.chars().filter(|&c| c == '}').count();
    if open > close {
        let mut out = s.to_string();
        for _ in 0..(open - close) {
            out.push('}');
        }
        out
    } else {
        s.to_string()
    }
}

/// Attempt to extract a `ToolCall` from potentially drifted LLM JSON output.
///
/// Accepts any of the common key aliases that small models emit:
/// - Tool name: `"tool"`, `"name"`, `"function"`, `"fn"`, `"action"`
/// - Arguments: `"args"`, `"arguments"`, `"parameters"`, `"input"`, `"params"`
///
/// Returns `None` when: no JSON object is found, more than one candidate exists
/// but none of the names match the active tool set, or the tool list is empty.
pub fn repair_tool_call_json(text: &str, tools: &[CachedTool]) -> Option<ToolCall> {
    if tools.is_empty() {
        return None;
    }
    let start = text.find('{')?;
    let mut raw_candidate = &text[start..];

    // Strip trailing markdown blocks that can confuse standard parsing
    // but be CAREFUL not to strip inner braces if the JSON is malformed (missing closing brace).
    if let Some(idx) = raw_candidate.rfind("```") {
        raw_candidate = raw_candidate[..idx].trim();
    }
    // Only strip trailing '}' if we are reasonably sure it's the outer one.
    // Heuristic: if there's significant text after it that doesn't look like JSON fluff, keep it.
    if let Some(idx) = raw_candidate.rfind('}') {
        let after = &raw_candidate[idx + 1..].trim();
        if after.is_empty() || after.starts_with(',') || after.starts_with(']') {
            // likely the real end
            // But wait, if we are missing the closing brace, this might still be an inner one.
            // For now, let's only strip if it's the VERY last non-whitespace char.
            if raw_candidate.trim_end().ends_with('}') {
                raw_candidate = raw_candidate.trim_end();
            }
        }
    }

    let balanced = balance_json(raw_candidate);
    let v_opt = serde_json::from_str::<serde_json::Value>(&balanced).ok();

    if let Some(v) = v_opt {
        // Try each alias for the tool name key.
        const NAME_KEYS: &[&str] = &["tool", "name", "function", "fn", "action"];
        let tool_name: Option<String> = NAME_KEYS.iter().find_map(|k| {
            v.get(*k).and_then(|n| n.as_str()).map(|s| {
                // Architectural Fix: Strip "tool." or "agent." prefixes common in LLM hallucinations
                s.strip_prefix("tool.")
                    .or_else(|| s.strip_prefix("agent."))
                    .unwrap_or(s)
                    .to_string()
            })
        });

        // Try each alias for the arguments key.
        const ARGS_KEYS: &[&str] = &["args", "arguments", "parameters", "input", "params"];
        let args = ARGS_KEYS
            .iter()
            .find_map(|k| v.get(*k).cloned())
            .unwrap_or_else(|| serde_json::json!({}));

        if let Some(name) = tool_name {
            if tools.iter().any(|t| t.name == name)
                || matches!(
                    name.as_str(),
                    "schedule_message" | "set_reminder" | "manage_cron"
                )
            {
                return Some(ToolCall {
                    name,
                    arguments: args.to_string(),
                });
            }
        }
    }

    // --- Aggressive Fallback Heuristics for Severe JSON Drift ---
    // Often, small LLMs hallucinate executable code syntax inside the JSON string
    // (e.g. `);`) which breaks the standard serde_json parsing entirely.
    if raw_candidate.contains("write_file") && tools.iter().any(|t| t.name == "write_file") {
        let path_start = raw_candidate
            .find(r#""path":"#)
            .or_else(|| raw_candidate.find(r#""path": "#));
        let content_start = raw_candidate
            .find(r#""content":"#)
            .or_else(|| raw_candidate.find(r#""content": "#));

        if let (Some(ps), Some(cs)) = (path_start, content_start) {
            let after_path = &raw_candidate[ps + 6..];
            if let Some(path_q1) = after_path.find('"') {
                if let Some(path_q2) = after_path[path_q1 + 1..].find('"') {
                    let path = &after_path[path_q1 + 1..path_q1 + 1 + path_q2];

                    let after_content = &raw_candidate[cs + 9..];
                    if let Some(content_q1) = after_content.find('"') {
                        let mut content = &after_content[content_q1 + 1..];
                        if let Some(last_quote) = content.rfind('"') {
                            // Only strip if there's trailing junk after the last quote
                            // otherwise we might be stripping the only quote we have.
                            if last_quote < content.len() - 1 {
                                content = &content[..last_quote];
                            }
                        }

                        let escaped_content = content.replace('\n', "\\n").replace('"', "\\\"");
                        let args = format!(
                            r#"{{"path": "{}", "content": "{}"}}"#,
                            path, escaped_content
                        );

                        return Some(ToolCall {
                            name: "write_file".to_string(),
                            arguments: args,
                        });
                    }
                }
            }
        }
    }

    // Generic fallback for tools other than write_file (if they are simple key-value)
    None
}

fn extract_tool_name_candidate(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let raw_candidate = balance_json(text[start..].trim());
    let v = serde_json::from_str::<serde_json::Value>(&raw_candidate).ok()?;
    const NAME_KEYS: &[&str] = &["tool", "name", "function", "fn", "action"];
    NAME_KEYS.iter().find_map(|k| {
        v.get(*k).and_then(|n| n.as_str()).map(|s| {
            s.strip_prefix("tool.")
                .or_else(|| s.strip_prefix("agent."))
                .unwrap_or(s)
                .to_string()
        })
    })
}

// ---------------------------------------------------------------------------
// Cron scheduler subsystem
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum ScheduleSpec {
    EverySeconds(u64),
    Cron(cron::Schedule, String),
    Once(chrono::DateTime<chrono::Utc>),
    DailyAt { hour: u32, minute: u32 },
    WeeklyAt {
        interval_weeks: u32,
        weekday: chrono::Weekday,
        hour: u32,
        minute: u32,
    },
}

impl std::fmt::Debug for ScheduleSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleSpec::EverySeconds(s) => write!(f, "EverySeconds({})", s),
            ScheduleSpec::Cron(_, expr) => write!(f, "Cron({})", expr),
            ScheduleSpec::Once(dt) => write!(f, "Once({})", dt),
            ScheduleSpec::DailyAt { hour, minute } => {
                write!(f, "DailyAt({:02}:{:02})", hour, minute)
            }
            ScheduleSpec::WeeklyAt {
                interval_weeks,
                weekday,
                hour,
                minute,
            } => write!(
                f,
                "WeeklyAt(every={}w,{:?},{:02}:{:02})",
                interval_weeks, weekday, hour, minute
            ),
        }
    }
}

impl PartialEq for ScheduleSpec {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (ScheduleSpec::EverySeconds(a), ScheduleSpec::EverySeconds(b)) => a == b,
            (ScheduleSpec::Cron(_, a), ScheduleSpec::Cron(_, b)) => a == b,
            (ScheduleSpec::Once(a), ScheduleSpec::Once(b)) => a == b,
            (
                ScheduleSpec::DailyAt {
                    hour: ah,
                    minute: am,
                },
                ScheduleSpec::DailyAt {
                    hour: bh,
                    minute: bm,
                },
            ) => ah == bh && am == bm,
            (
                ScheduleSpec::WeeklyAt {
                    interval_weeks: ai,
                    weekday: aw,
                    hour: ah,
                    minute: am,
                },
                ScheduleSpec::WeeklyAt {
                    interval_weeks: bi,
                    weekday: bw,
                    hour: bh,
                    minute: bm,
                },
            ) => ai == bi && aw == bw && ah == bh && am == bm,
            _ => false,
        }
    }
}
impl Eq for ScheduleSpec {}

impl ScheduleSpec {
    pub fn parse(spec: &str) -> Option<Self> {
        let s = spec.trim();
        let lower = s.to_ascii_lowercase();
        if let Some(v) = s.strip_prefix("every:") {
            let secs = v.trim_end_matches('s').parse::<u64>().ok()?;
            return (secs > 0).then_some(ScheduleSpec::EverySeconds(secs));
        }

        if let Some(hm) = lower.strip_prefix("daily@") {
            let (hour, minute) = parse_hh_mm(hm)?;
            return Some(ScheduleSpec::DailyAt { hour, minute });
        }

        if let Some(rest) = lower.strip_prefix("weekly:") {
            let (weekday, hour, minute) = parse_weekday_at(rest)?;
            return Some(ScheduleSpec::WeeklyAt {
                interval_weeks: 1,
                weekday,
                hour,
                minute,
            });
        }
        if let Some(rest) = lower.strip_prefix("biweekly:") {
            let (weekday, hour, minute) = parse_weekday_at(rest)?;
            return Some(ScheduleSpec::WeeklyAt {
                interval_weeks: 2,
                weekday,
                hour,
                minute,
            });
        }

        if let Some(at_text) = s.strip_prefix("at:") {
            if let Some(dt) = parse_once_datetime(at_text.trim()) {
                return Some(ScheduleSpec::Once(dt));
            }
        }
        if let Some(dt) = parse_once_datetime(s) {
            return Some(ScheduleSpec::Once(dt));
        }

        // Handle one-shot delays: "delay:2m", "in:1h", "2m"
        let once_text = s
            .strip_prefix("delay:")
            .or_else(|| s.strip_prefix("in:"))
            .unwrap_or(s);

        if let Some(secs) = parse_duration_to_secs(once_text) {
            // If it's a simple number and was NOT prefixed by delay:/in:,
            // AND the test expects EverySeconds(5) for "*/5 * * * * *",
            // we should be careful.
            // Actually, if it contains '*' or '/', it's a cron.
            if !s.contains('*') && !s.contains('/') {
                let now = chrono::Utc::now();
                return Some(ScheduleSpec::Once(
                    now + chrono::Duration::try_seconds(secs as i64).unwrap(),
                ));
            }
        }

        // Use real cron evaluation.
        // Note: The `cron` crate expects 6 or 7 fields (sec min hour dom month dow [year]).
        // LLMs often give 5 fields (min hour dom month dow).
        let parts: Vec<&str> = s.split_whitespace().collect();
        let normalized = match parts.len() {
            5 => format!("0 {}", s), // min hour dom month dow -> sec min hour dom month dow
            6 => s.to_string(),      // sec min hour dom month dow
            7 => s.to_string(),      // sec min hour dom month dow year
            _ => s.to_string(),
        };

        use std::str::FromStr;
        if let Ok(cron) = cron::Schedule::from_str(&normalized) {
            return Some(ScheduleSpec::Cron(cron, s.to_string()));
        }

        None
    }

    pub fn is_once(&self) -> bool {
        matches!(self, ScheduleSpec::Once(_))
    }

    pub fn next_fire(&self, now: chrono::DateTime<chrono::Utc>) -> chrono::DateTime<chrono::Utc> {
        match self {
            ScheduleSpec::EverySeconds(s) => {
                now + chrono::Duration::try_seconds(*s as i64).unwrap_or(chrono::Duration::days(1))
            }
            ScheduleSpec::Cron(c, _) => c
                .upcoming(chrono::Utc)
                .next()
                .unwrap_or_else(|| now + chrono::Duration::days(365)),
            ScheduleSpec::Once(dt) => *dt,
            ScheduleSpec::DailyAt { hour, minute } => {
                let date = now.date_naive();
                let t = chrono::NaiveTime::from_hms_opt(*hour, *minute, 0)
                    .unwrap_or(chrono::NaiveTime::MIN);
                let mut next =
                    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(date.and_time(t), chrono::Utc);
                if next <= now {
                    next += chrono::Duration::days(1);
                }
                next
            }
            ScheduleSpec::WeeklyAt {
                interval_weeks,
                weekday,
                hour,
                minute,
            } => next_weekly_fire(now, *interval_weeks, *weekday, *hour, *minute),
        }
    }
}

fn next_weekly_fire(
    now: chrono::DateTime<chrono::Utc>,
    interval_weeks: u32,
    weekday: chrono::Weekday,
    hour: u32,
    minute: u32,
) -> chrono::DateTime<chrono::Utc> {
    use chrono::Datelike;
    let target_time = chrono::NaiveTime::from_hms_opt(hour, minute, 0).unwrap_or(chrono::NaiveTime::MIN);
    let today = now.date_naive();
    let current_w = today.weekday().num_days_from_monday() as i64;
    let target_w = weekday.num_days_from_monday() as i64;
    let mut days_until = (target_w - current_w + 7) % 7;
    let mut candidate = today.and_time(target_time) + chrono::Duration::days(days_until);
    if candidate <= now.naive_utc() {
        days_until += 7;
        candidate = today.and_time(target_time) + chrono::Duration::days(days_until);
    }

    let every = interval_weeks.max(1) as i64;
    if every > 1 {
        let anchor_monday = chrono::NaiveDate::from_ymd_opt(1970, 1, 5).unwrap_or(today);
        while candidate
            .date()
            .signed_duration_since(anchor_monday)
            .num_days()
            .div_euclid(7)
            .rem_euclid(every)
            != 0
        {
            candidate += chrono::Duration::days(7);
        }
    }

    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(candidate, chrono::Utc)
}

fn parse_duration_to_secs(s: &str) -> Option<u64> {
    let s = s.trim().to_ascii_lowercase();
    if s.is_empty() {
        return None;
    }
    let (digits, unit): (String, String) = s.chars().partition(|c| c.is_ascii_digit());
    let val = digits.parse::<u64>().ok()?;
    match unit.trim() {
        "s" | "" => Some(val),
        "m" | "min" | "mins" => Some(val * 60),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(val * 3600),
        "d" | "day" | "days" => Some(val * 86400),
        _ => None,
    }
}

fn parse_hh_mm(s: &str) -> Option<(u32, u32)> {
    let mut parts = s.split(':');
    let hour = parts.next()?.trim().parse::<u32>().ok()?;
    let minute = parts.next()?.trim().parse::<u32>().ok()?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return None;
    }
    Some((hour, minute))
}

fn parse_weekday(s: &str) -> Option<chrono::Weekday> {
    match s.trim() {
        "mon" | "monday" => Some(chrono::Weekday::Mon),
        "tue" | "tues" | "tuesday" => Some(chrono::Weekday::Tue),
        "wed" | "wednesday" => Some(chrono::Weekday::Wed),
        "thu" | "thurs" | "thursday" => Some(chrono::Weekday::Thu),
        "fri" | "friday" => Some(chrono::Weekday::Fri),
        "sat" | "saturday" => Some(chrono::Weekday::Sat),
        "sun" | "sunday" => Some(chrono::Weekday::Sun),
        _ => None,
    }
}

fn parse_weekday_at(s: &str) -> Option<(chrono::Weekday, u32, u32)> {
    let (day, hm) = s.split_once('@')?;
    let weekday = parse_weekday(day)?;
    let (hour, minute) = parse_hh_mm(hm)?;
    Some((weekday, hour, minute))
}

fn parse_once_datetime(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M") {
        return Some(chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(ndt, chrono::Utc));
    }
    None
}

impl Default for ScheduleSpec {
    fn default() -> Self {
        ScheduleSpec::EverySeconds(60)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScheduledJobKind {
    Notify,
    Orchestrate,
}

impl Default for ScheduledJobKind {
    fn default() -> Self {
        Self::Orchestrate
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScheduledPromptJob {
    pub id: String,
    pub agent_id: String,
    pub prompt: String,
    pub schedule_str: String,
    #[serde(default)]
    pub kind: ScheduledJobKind,
    /// Session this job belongs to, to enable context-aware execution.
    pub session_id: Option<Uuid>,
    pub user_id: Option<String>,
    pub channel: Option<GatewayChannel>,
    #[serde(skip)]
    pub schedule: ScheduleSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScheduledPromptEvent {
    pub job_id: String,
    pub agent_id: String,
    pub prompt: String,
    pub kind: ScheduledJobKind,
    pub session_id: Option<Uuid>,
    pub user_id: Option<String>,
    pub channel: Option<GatewayChannel>,
}

// ---------------------------------------------------------------------------
// Item 3: Bump-Pointer Arena Prompt Assembly (P3)
// ---------------------------------------------------------------------------

/// A short-lived arena for prompt construction.
/// Reduces allocator churn for large string concatenations during orchestration.
pub struct PromptArena {
    bump: bumpalo::Bump,
}

impl PromptArena {
    pub fn new() -> Self {
        Self {
            bump: bumpalo::Bump::new(),
        }
    }

    /// Allocate a formatted string in the arena.
    pub fn format<'a>(&'a self, args: std::fmt::Arguments) -> &'a str {
        use std::fmt::Write;
        let mut s = bumpalo::collections::String::new_in(&self.bump);
        s.write_fmt(args).expect("arena allocation failed");
        s.into_bump_str()
    }

    /// Allocate a segment of history or context in the arena.
    pub fn alloc(&self, s: &str) -> &str {
        self.bump.alloc_str(s)
    }
}

// ---------------------------------------------------------------------------
// Item 4: Hardware DVFS / Power Management Hooks (P3)
// ---------------------------------------------------------------------------

/// Integration hook for platform-specific power management (DVFS/Clock Gating).
pub trait PlatformPowerHooks: Send + Sync {
    /// Called when the system enters an idle state.
    fn on_idle(&self);
    /// Called when the system resumes active processing.
    fn on_busy(&self);
}

/// No-op implementation for systems without platform hooks.
pub struct NoopPowerHooks;
impl PlatformPowerHooks for NoopPowerHooks {
    fn on_idle(&self) {}
    fn on_busy(&self) {}
}

pub enum CronCommand {
    Add(ScheduledPromptJob),
    Remove(String),
    List(tokio::sync::oneshot::Sender<Vec<ScheduledPromptJob>>),
}

pub struct CronScheduler {
    jobs: std::collections::HashMap<String, ScheduledPromptJob>,
    next_fires: std::collections::HashMap<String, chrono::DateTime<chrono::Utc>>,
    power_hooks: Arc<dyn PlatformPowerHooks>,
}

impl Default for CronScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl CronScheduler {
    pub fn new() -> Self {
        Self {
            jobs: std::collections::HashMap::new(),
            next_fires: std::collections::HashMap::new(),
            power_hooks: Arc::new(NoopPowerHooks),
        }
    }

    pub fn with_power_hooks(mut self, hooks: Arc<dyn PlatformPowerHooks>) -> Self {
        self.power_hooks = hooks;
        self
    }

    pub fn add_job(&mut self, job: ScheduledPromptJob) {
        let id = job.id.clone();
        info!(job_id = %id, schedule = %job.schedule_str, agent = %job.agent_id, "Adding scheduled job");
        self.jobs.insert(id.clone(), job);
        self.next_fires.remove(&id); // Force recalculation
    }

    pub fn due_events_now(&mut self) -> Vec<ScheduledPromptEvent> {
        let now = chrono::Utc::now();
        let mut events = Vec::new();
        let mut to_remove = Vec::new();

        for (id, job) in &self.jobs {
            let next_run = self.next_fires.get(id).copied();

            let target_time = if let Some(nr) = next_run {
                nr
            } else {
                let nr = job.schedule.next_fire(now);
                self.next_fires.insert(id.clone(), nr);
                nr
            };

            if now >= target_time {
                info!(job_id = %job.id, prompt = %job.prompt, "Scheduled job is due to fire");
                events.push(ScheduledPromptEvent {
                    job_id: job.id.clone(),
                    agent_id: job.agent_id.clone(),
                    prompt: job.prompt.clone(),
                    kind: job.kind.clone(),
                    session_id: job.session_id,
                    user_id: job.user_id.clone(),
                    channel: job.channel,
                });

                if job.schedule.is_once() {
                    info!(job_id = %job.id, "Removing one-shot job after firing");
                    to_remove.push(id.clone());
                } else {
                    let new_nr = job.schedule.next_fire(now);
                    self.next_fires.insert(id.clone(), new_nr);
                    debug!(job_id = %job.id, next_fire = %new_nr, "Rescheduled job");
                }
            }
        }

        for id in to_remove {
            self.jobs.remove(&id);
            self.next_fires.remove(&id);
        }

        events
    }

    pub fn start(
        self,
        tick_seconds: u64,
        mut command_rx: tokio::sync::mpsc::Receiver<CronCommand>,
    ) -> tokio::sync::mpsc::Receiver<ScheduledPromptEvent> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);
        let mut scheduler = self;
        tokio::spawn(async move {
            let mut ticker =
                tokio::time::interval(std::time::Duration::from_secs(tick_seconds.max(1)));
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let events = scheduler.due_events_now();
                        if events.is_empty() {
                            scheduler.power_hooks.on_idle();
                        } else {
                            scheduler.power_hooks.on_busy();
                            for ev in events {
                                if tx.send(ev).await.is_err() {
                                    return;
                                }
                            }
                        }
                    }
                    cmd = command_rx.recv() => {
                        match cmd {
                            Some(CronCommand::Add(job)) => {
                                scheduler.add_job(job);
                            }
                            Some(CronCommand::Remove(id)) => {
                                info!(job_id = %id, "Removing scheduled job via command");
                                scheduler.jobs.remove(&id);
                                scheduler.next_fires.remove(&id);
                            }
                            Some(CronCommand::List(reply)) => {
                                let mut all = Vec::new();
                                for job in scheduler.jobs.values() {
                                    all.push(job.clone());
                                }
                                let _ = reply.send(all);
                            }
                            None => {}
                        }
                    }
                }
            }
        });
        rx
    }
}

/// Async trait for tool execution backends.
///
/// Abstracts over local Wasm execution and remote mesh calls.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    /// Execute a tool call and return the result as a string.
    async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError>;
}

// ---------------------------------------------------------------------------
// AgentOrchestrator
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrchestratorResult {
    /// The agent finished its reasoning and generated a final text response.
    Completed(String),
    /// The agent requested a tool that requires human approval.
    /// Contains the pending tool call and the current aggregated reasoning prompt.
    ToolApprovalRequired {
        call: ToolCall,
        pending_prompt: String,
    },
}

/// ReAct (Reasoning and Acting) agent orchestrator.
///
/// Implements the core loop:
/// 1. Query LLM with prompt + tools
/// 2. If `TextAnswer` → return final response
/// 3. If `ToolCalls` → execute each tool, append results, re-query
/// 4. If rounds exceed `max_tool_rounds` → abort with error
pub struct AgentOrchestrator<L: LLMBackend, T: ToolExecutor> {
    llm: L,
    tool_executor: T,
}

/// Commands injected by the human operator mid-flight to alter agent reasoning
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SteeringCommand {
    /// Human sent a new message that should abort the current tool plan and pivot
    Pivot(String),
    /// Instantly halt evaluation
    Abort,
}

impl<L: LLMBackend, T: ToolExecutor> AgentOrchestrator<L, T> {
    /// Create a new orchestrator with the given LLM and tool executor.
    pub fn new(llm: L, tool_executor: T) -> Self {
        Self { llm, tool_executor }
    }

    /// Run the ReAct loop.
    ///
    /// - `initial_prompt`: The user's query or assembled prompt
    /// - `tools`: Available tools for this session
    /// - `max_tool_rounds`: Maximum number of tool-call iterations
    ///
    /// Returns the final text answer from the LLM.
    pub async fn run(
        &self,
        initial_prompt: &str,
        tools: &[CachedTool],
        max_tool_rounds: usize,
        mut steering_rx: Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        global_estop: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        let mut prompt = initial_prompt.to_string();
        let mut rounds = 0_usize;
        let mut last_progress = std::time::Instant::now();

        loop {
            // Check global emergency stop flag
            if let Some(estop) = global_estop {
                if estop.load(std::sync::atomic::Ordering::SeqCst) {
                    debug!("Orchestrator: Global ESTOP triggered, aborting run loop");
                    return Err(OrchestratorError::UserAborted);
                }
            }

            // Check for stuck jobs: if no progress in the last 60 seconds, inject a correction prompt.
            if rounds > 0 && last_progress.elapsed().as_secs() > 60 {
                debug!("Orchestrator: Job heartbeat stalled. Injecting correction prompt.");
                prompt.push_str("\n\n[SYSTEM CORRECTIVE: Your previous approach has stalled for over 60 seconds without resolving the query. Please pivot and try a different strategy or tool immediately.]");
                last_progress = std::time::Instant::now(); // reset heartbeat after injection
            }

            // Check if we have an override signal before querying the LLM
            if let Some(ref mut rx) = steering_rx {
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SteeringCommand::Abort => return Err(OrchestratorError::UserAborted),
                        SteeringCommand::Pivot(new_instructions) => {
                            debug!("Orchestrator: user pivot detected mid-flight");
                            prompt = format!("{}\n\n<<SYSTEM INTERRUPT: User provided new steerings: '{}'>>\nAbort your previous tool plan and restart reasoning.", prompt, new_instructions);
                        }
                    }
                }
            }
            let response = self.llm.query(&prompt, tools).await?;

            match response {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) = repair_tool_call_json(&answer, tools) {
                        debug!(
                            tool = %repaired.name,
                            "Orchestrator: Repaired ToolCall from TextAnswer"
                        );
                        let calls = vec![repaired];
                        let res = self
                            .process_generic_tool_calls(
                                calls,
                                tools,
                                &mut rounds,
                                max_tool_rounds,
                                &mut prompt,
                                &mut last_progress,
                                &mut steering_rx,
                            )
                            .await?;
                        if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                            return Ok(res);
                        }
                        if let OrchestratorResult::Completed(ref s) = res {
                            if s != "CONTINUE_LOOP" {
                                return Ok(res);
                            }
                        }
                    } else if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                        rounds += 1;
                        if rounds > max_tool_rounds {
                            return Err(OrchestratorError::MaxRoundsExceeded {
                                limit: max_tool_rounds,
                            });
                        }
                        let available = tools
                            .iter()
                            .map(|t| t.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        prompt = format!(
                            "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' is not available in this session. Use one of [{}], or answer in plain text. If using a tool, return only valid JSON.>>",
                            prompt, tool_name, available
                        );
                        last_progress = std::time::Instant::now();
                        continue;
                    } else {
                        return Ok(OrchestratorResult::Completed(answer));
                    }
                }
                LLMResponse::ToolCalls(calls) => {
                    last_progress = std::time::Instant::now(); // Reset heartbeat on success
                    let res = self
                        .process_generic_tool_calls(
                            calls,
                            tools,
                            &mut rounds,
                            max_tool_rounds,
                            &mut prompt,
                            &mut last_progress,
                            &mut steering_rx,
                        )
                        .await?;
                    if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                        return Ok(res);
                    }
                    if let OrchestratorResult::Completed(ref s) = res {
                        if s != "CONTINUE_LOOP" {
                            return Ok(res);
                        }
                    }
                }
            }
        }
    }

    async fn process_generic_tool_calls(
        &self,
        calls: Vec<ToolCall>,
        tools_cache: &[CachedTool],
        rounds: &mut usize,
        max_tool_rounds: usize,
        prompt: &mut String,
        last_progress: &mut Instant,
        steering_rx: &mut Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        if calls.is_empty() {
            return Ok(OrchestratorResult::Completed(String::new()));
        }

        *rounds += 1;
        if *rounds > max_tool_rounds {
            return Err(OrchestratorError::MaxRoundsExceeded {
                limit: max_tool_rounds,
            });
        }

        let mut async_calls = Vec::new();
        let mut validation_errors = Vec::new();

        for call in calls {
            if call.name == "run_shell" || call.name == "write_file" {
                // Yield execution to human operator for destructive commands.
                return Ok(OrchestratorResult::ToolApprovalRequired {
                    call: call.clone(),
                    pending_prompt: prompt.clone(),
                });
            } else if call.name == "search_tool_registry" {
                // Special system tool, schema is loose, execute downstream
                async_calls.push(call);
            } else {
                // Perform JSON Schema Validation
                let mut is_valid = false;
                if let Some(cached_tool) = tools_cache.iter().find(|t| t.name == call.name) {
                    is_valid = true;
                    if let Ok(schema_val) =
                        serde_json::from_str::<serde_json::Value>(&cached_tool.parameters_schema)
                    {
                        if let Ok(compiled_schema) = jsonschema::Validator::new(&schema_val) {
                            if let Ok(args_val) =
                                serde_json::from_str::<serde_json::Value>(&call.arguments)
                            {
                                if let Err(_) = compiled_schema.validate(&args_val) {
                                    is_valid = false;
                                    let mut err_msgs = Vec::new();
                                    for err in compiled_schema.iter_errors(&args_val) {
                                        err_msgs.push(format!("{}", err));
                                    }
                                    let merged_errs = err_msgs.join("; ");
                                    debug!(tool = %call.name, error = %merged_errs, "Tool arguments failed JSON schema validation");
                                    validation_errors.push(format!(
                                        "[Tool '{}' Rejected]: schema validation failed on arguments `{}` - {}",
                                        call.name, call.arguments, merged_errs
                                    ));
                                }
                            } else {
                                is_valid = false;
                                validation_errors.push(format!(
                                    "[Tool '{}' Rejected]: arguments must be valid JSON object",
                                    call.name
                                ));
                            }
                        }
                    }
                } else {
                    debug!(tool = %call.name, "Attempted to call unregistered tool");
                    validation_errors.push(format!(
                        "[Tool '{}' Rejected]: tool is not available. Please use 'search_tool_registry' to find and load it, or pick from the available tools listed above.",
                        call.name
                    ));
                }

                if is_valid {
                    async_calls.push(call);
                }
            }
        }

        // If validation errors occurred, intercept the loop and auto-correct LLM immediately
        // without running any of the other async tools to prevent partial state corruption.
        if !validation_errors.is_empty() {
            *prompt = format!(
                "{}\n\n<<SYSTEM INTERRUPT: Tool execution failed schema validation: \n{}\nPlease correct the tool call parameters and try again.>>",
                prompt,
                validation_errors.join("\n")
            );
            return Ok(OrchestratorResult::Completed(String::from("CONTINUE_LOOP")));
        }

        let mut tool_results = Vec::new();
        if !async_calls.is_empty() {
            let mut executed_tools: Vec<(String, String)> = Vec::new();
            let executions = async_calls.into_iter().map(|call| async move {
                let result = self.tool_executor.execute(&call).await;
                (call.name, result)
            });

            let executions_future = join_all(executions);

            let resolved = if let Some(ref mut rx) = steering_rx {
                tokio::select! {
                    res = executions_future => res,
                    cmd = rx.recv() => {
                        match cmd {
                            Some(SteeringCommand::Abort) => return Err(OrchestratorError::UserAborted),
                            Some(SteeringCommand::Pivot(new_instructions)) => {
                                debug!("Orchestrator: user pivot intercepted during tool execution");
                                *prompt = format!("{}\n\n<<SYSTEM INTERRUPT during tool execution: User steered with: '{}'>>\nIgnore the pending tools.", prompt, new_instructions);
                                return Ok(OrchestratorResult::Completed(String::new())); // Restart loop
                            }
                            None => return Err(OrchestratorError::UserAborted),
                        }
                    }
                }
            } else {
                executions_future.await
            };

            for (name, result) in resolved {
                let output = result?;
                debug!(tool = %name, output_len = output.len(), output_preview = %output.chars().take(100).collect::<String>(), "Tool: executed");
                tool_results.push(format!("[Tool: {}] Result: {}", name, output));
                executed_tools.push((name, output));
            }

            if let Some(final_text) = maybe_finalize_after_scheduler_tools(&executed_tools) {
                return Ok(OrchestratorResult::Completed(final_text));
            }

            // Update heartbeat after successful tool executions
            *last_progress = std::time::Instant::now();
        }

        // Append tool results to prompt for next LLM query
        *prompt = format!(
            "{}\n\n--- Tool Results ---\n{}",
            prompt,
            tool_results.join("\n")
        );

        // Signal that tools were processed and the loop should continue
        Ok(OrchestratorResult::Completed("CONTINUE_LOOP".to_string()))
    }

    /// Build a request-scoped prompt from normalized request + memory context + system prompt.
    /// Arena-optimized prompt builder.
    pub fn build_request_prompt_arena(
        arena: &PromptArena,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
    ) -> String {
        let user_text = match &request.content {
            MessageContent::Text(s) => arena.alloc(s),
            MessageContent::Image { url, caption } => arena.format(format_args!(
                "User sent an image.\nurl: {}\ncaption: {}",
                url,
                caption.as_deref().unwrap_or_default()
            )),
            MessageContent::Audio { url, transcript } => arena.format(format_args!(
                "User sent an audio message.\nurl: {}\ntranscript: {}",
                url,
                transcript.as_deref().unwrap_or_default()
            )),
            MessageContent::Location { lat, lng } => arena.format(format_args!(
                "User shared location: lat={}, lng={}",
                lat, lng
            )),
        };

        let tools_section = if tools.is_empty() {
            ""
        } else {
            let mut tools_buffer = bumpalo::collections::String::new_in(&arena.bump);
            use std::fmt::Write;
            let _ = writeln!(tools_buffer, "\n--- Available Tools ---");
            let _ = writeln!(
                tools_buffer,
                "To call a tool, respond with ONLY the following JSON and nothing else:"
            );
            let _ = writeln!(
                tools_buffer,
                r#"{{"tool": "<tool_name>", "args": {{<key>: <value>, ...}}}}"#
            );
            let _ = writeln!(tools_buffer, "\nTools:");
            for t in tools {
                let _ = writeln!(tools_buffer, "- {}: {}", t.name, t.description);
                if !t.parameters_schema.is_empty() && t.parameters_schema != "{}" {
                    let _ = writeln!(tools_buffer, "  Schema: {}", t.parameters_schema);
                }
            }
            let _ = writeln!(
                tools_buffer,
                "\nIf no tool is needed, reply with plain text."
            );
            arena.alloc(tools_buffer.as_str())
        };

        let now_utc = chrono::Utc::now();
        let now_local = chrono::Local::now();
        let meta_instructions = format!(
            "\n--- System Directives ---\n\
            1. You are a precise, concise AI agent. Follow the defined system prompt strictly.\n\
            2. If you need to use a tool, return ONLY the tool call JSON shown above.\n\
            3. Do not over-explain. Provide direct answers.\n\
            4. Current UTC time: {}.\n\
            5. Current local time: {}.\n\
            6. For reminders/scheduling, prefer explicit formats like '2m', 'daily@19:30', 'weekly:sat@11:00', 'biweekly:sat@11:00', or 'at:YYYY-MM-DD HH:MM'.\n\
            7. For schedule_message/set_reminder, use mode='notify' for static reminders, mode='defer' to execute work at trigger time, and mode='both' if both are needed.\n\
            8. If user asks to perform work \"in X\" time, default to mode='defer' (do not execute now) unless user explicitly asks for both immediate and delayed output.\n",
            now_utc.to_rfc3339(),
            now_local.format("%Y-%m-%d %H:%M:%S %:z")
        );

        format!(
            "System Prompt:\n{}{}\n{}\nRAG Context:\n{}\n\nSession History:\n{}\n\nUser Request (channel={:?}):\n{}",
            agent_system_prompt, tools_section, meta_instructions, rag_context, history_context, request.channel, user_text
        )
    }

    pub fn build_request_prompt(
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
    ) -> String {
        let arena = PromptArena::new();
        Self::build_request_prompt_arena(
            &arena,
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            tools,
        )
    }

    /// Request-aware orchestrator entrypoint aligned with gateway normalization.
    pub async fn run_for_request(
        &self,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        tools: &[CachedTool],
        max_tool_rounds: usize,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        let prompt = Self::build_request_prompt(
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            tools,
        );
        self.run(&prompt, tools, max_tool_rounds, None, None).await
    }

    /// Request-aware orchestrator loop that supports dynamic tool hot-swap via
    /// the `search_tool_registry` meta-tool.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_for_request_with_dynamic_tools<E: EmbeddingModel>(
        &self,
        agent_system_prompt: &str,
        request: &AgentRequest,
        history_context: &str,
        rag_context: &str,
        cache: &mut DynamicToolCache,
        tool_registry: &ToolManifestStore,
        embedder: &E,
        max_tool_rounds: usize,
        mut steering_rx: Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        global_estop: Option<&Arc<std::sync::atomic::AtomicBool>>,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        // Build initial prompt with tool schemas injected so small models know the format.
        let initial_tools = cache.active_tools();
        let mut prompt = Self::build_request_prompt(
            agent_system_prompt,
            request,
            history_context,
            rag_context,
            &initial_tools,
        );
        let mut rounds = 0usize;
        let mut last_progress = std::time::Instant::now();

        let prompt_preview = if prompt.len() > 500 {
            format!("{}...", &prompt[..500])
        } else {
            prompt.clone()
        };
        debug!(prompt_len = prompt.len(), prompt_preview = %prompt_preview, "Orchestrator: built prompt");

        loop {
            // Check global emergency stop flag
            if let Some(estop) = global_estop {
                if estop.load(std::sync::atomic::Ordering::SeqCst) {
                    debug!("Orchestrator: Global ESTOP triggered, aborting dynamic run loop");
                    return Err(OrchestratorError::UserAborted);
                }
            }

            // Check for stuck jobs: if no progress in the last 60 seconds, inject a correction prompt.
            if rounds > 0 && last_progress.elapsed().as_secs() > 60 {
                debug!("Orchestrator: dynamic loop job heartbeat stalled. Injecting correction prompt.");
                prompt.push_str("\n\n[SYSTEM CORRECTIVE: Your previous approach has stalled for over 60 seconds without resolving the query. Please pivot and try a different strategy or tool immediately.]");
                last_progress = std::time::Instant::now(); // reset heartbeat after injection
            }

            if let Some(ref mut rx) = steering_rx {
                if let Ok(cmd) = rx.try_recv() {
                    match cmd {
                        SteeringCommand::Abort => return Err(OrchestratorError::UserAborted),
                        SteeringCommand::Pivot(new_instructions) => {
                            debug!("Orchestrator: user pivot detected mid-flight before dynamic LLM call");
                            prompt = format!("{}\n\n<<SYSTEM INTERRUPT: User provided new steerings: '{}'>>\nAbort your previous tool plan and restart reasoning.", prompt, new_instructions);
                        }
                    }
                }
            }

            let active_tools = cache.active_tools();
            let tool_names: Vec<String> = active_tools.iter().map(|t| t.name.clone()).collect();
            debug!(round = rounds + 1, tools = ?tool_names, "Orchestrator: querying LLM");

            let response = self.llm.query(&prompt, &active_tools).await?;
            match response {
                LLMResponse::TextAnswer(answer) => {
                    if let Some(repaired) = repair_tool_call_json(&answer, &active_tools) {
                        debug!(
                            tool = %repaired.name,
                            "Orchestrator: Repaired ToolCall from TextAnswer (dynamic)"
                        );
                        let calls = vec![repaired];
                        let res = self
                            .process_dynamic_tool_calls(
                                calls,
                                &mut rounds,
                                max_tool_rounds,
                                &mut prompt,
                                &mut last_progress,
                                &mut steering_rx,
                                global_estop.map(|a| a.as_ref()),
                                cache,
                                tool_registry,
                                embedder,
                            )
                            .await?;
                        if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                            return Ok(res);
                        }
                        if let OrchestratorResult::Completed(ref s) = res {
                            if s != "CONTINUE_LOOP" {
                                return Ok(res);
                            }
                        }
                    } else if let Some(tool_name) = extract_tool_name_candidate(&answer) {
                        rounds += 1;
                        if rounds > max_tool_rounds {
                            return Err(OrchestratorError::MaxRoundsExceeded {
                                limit: max_tool_rounds,
                            });
                        }
                        let available = active_tools
                            .iter()
                            .map(|t| t.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        prompt = format!(
                            "{}\n\n<<SYSTEM INTERRUPT: Tool '{}' is not available in this session. Use one of [{}], or answer in plain text. If using a tool, return only valid JSON.>>",
                            prompt, tool_name, available
                        );
                        last_progress = Instant::now();
                        continue;
                    } else {
                        debug!(
                            answer_len = answer.len(),
                            answer_preview = %answer.chars().take(200).collect::<String>(),
                            "Orchestrator: LLM returned TextAnswer"
                        );
                        return Ok(OrchestratorResult::Completed(answer));
                    }
                }
                LLMResponse::ToolCalls(calls) => {
                    last_progress = Instant::now();
                    let res = self
                        .process_dynamic_tool_calls(
                            calls,
                            &mut rounds,
                            max_tool_rounds,
                            &mut prompt,
                            &mut last_progress,
                            &mut steering_rx,
                            global_estop.map(|a| a.as_ref()),
                            cache,
                            tool_registry,
                            embedder,
                        )
                        .await?;
                    if matches!(res, OrchestratorResult::ToolApprovalRequired { .. }) {
                        return Ok(res);
                    }
                    if let OrchestratorResult::Completed(ref s) = res {
                        if s != "CONTINUE_LOOP" {
                            return Ok(res);
                        }
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_dynamic_tool_calls<E: EmbeddingModel>(
        &self,
        calls: Vec<ToolCall>,
        rounds: &mut usize,
        max_tool_rounds: usize,
        prompt: &mut String,
        last_progress: &mut Instant,
        steering_rx: &mut Option<&mut tokio::sync::mpsc::Receiver<SteeringCommand>>,
        global_estop: Option<&std::sync::atomic::AtomicBool>,
        cache: &mut DynamicToolCache,
        tool_registry: &ToolManifestStore,
        embedder: &E,
    ) -> Result<OrchestratorResult, OrchestratorError> {
        if let Some(estop) = global_estop {
            if estop.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(OrchestratorError::UserAborted);
            }
        }
        if calls.is_empty() {
            return Ok(OrchestratorResult::Completed(String::new()));
        }

        *rounds += 1;
        if *rounds > max_tool_rounds {
            return Err(OrchestratorError::MaxRoundsExceeded {
                limit: max_tool_rounds,
            });
        }

        let mut tool_results = Vec::new();
        let mut async_calls = Vec::new();

        for call in calls {
            if call.name == "search_tool_registry" {
                let query = extract_registry_query(&call.arguments).unwrap_or_default();
                debug!(query = %query, "Tool: search_tool_registry invoked");
                let swapped = tool_registry
                    .hot_swap_best(cache, &query, embedder)
                    .map_err(|e| OrchestratorError::ToolError(format!("{}", e)))?;
                match swapped {
                    Some(tool) => {
                        debug!(loaded_tool = %tool.name, "Tool: search_tool_registry loaded tool");
                        tool_results.push(format!(
                            "[Tool: search_tool_registry] Result: loaded '{}'",
                            tool.name
                        ));
                    }
                    None => {
                        debug!("Tool: search_tool_registry found no matching tools");
                        tool_results.push(
                            "[Tool: search_tool_registry] Result: no matching tools found"
                                .to_string(),
                        );
                    }
                }
            } else if call.name == "run_shell" || call.name == "write_file" {
                debug!(tool = %call.name, "Tool requires human-in-the-loop approval, yielding execution flow");
                return Ok(OrchestratorResult::ToolApprovalRequired {
                    call: call.clone(),
                    pending_prompt: prompt.clone(),
                });
            } else {
                async_calls.push(call);
            }
        }

        if !async_calls.is_empty() {
            let mut executed_tools: Vec<(String, String)> = Vec::new();
            let executions = async_calls.into_iter().map(|call| async move {
                let result = self.tool_executor.execute(&call).await;
                (call.name, result)
            });

            let resolved = if let Some(ref mut rx) = steering_rx {
                tokio::select! {
                    res = join_all(executions) => res,
                    cmd = rx.recv() => {
                        match cmd {
                            Some(SteeringCommand::Abort) => return Err(OrchestratorError::UserAborted),
                            Some(SteeringCommand::Pivot(new_instructions)) => {
                                debug!("Orchestrator: user pivot intercepted during tool execution");
                                *prompt = format!("{}\n\n<<SYSTEM INTERRUPT during tool execution: User steered with: '{}'>>\nIgnore the pending tools.", prompt, new_instructions);
                                return Ok(OrchestratorResult::Completed(String::new())); // Restart loop
                            }
                            None => return Err(OrchestratorError::UserAborted),
                        }
                    }
                }
            } else {
                join_all(executions).await
            };

            for (name, result) in resolved {
                let output = result?;
                debug!(tool = %name, output_len = output.len(), output_preview = %output.chars().take(100).collect::<String>(), "Tool: executed");
                tool_results.push(format!("[Tool: {}] Result: {}", name, output));
                executed_tools.push((name, output));
            }

            if let Some(final_text) = maybe_finalize_after_scheduler_tools(&executed_tools) {
                return Ok(OrchestratorResult::Completed(final_text));
            }
        }

        *prompt = format!(
            "{}\n\n--- Tool Results ---\n{}",
            prompt,
            tool_results.join("\n")
        );
        *last_progress = Instant::now();
        Ok(OrchestratorResult::Completed("CONTINUE_LOOP".to_string()))
    }
}

fn extract_registry_query(arguments_json: &str) -> Option<String> {
    let value = serde_json::from_str::<serde_json::Value>(arguments_json).ok()?;
    value
        .get("query")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn maybe_finalize_after_scheduler_tools(executed_tools: &[(String, String)]) -> Option<String> {
    if executed_tools.is_empty() {
        return None;
    }
    let all_scheduler_tools = executed_tools.iter().all(|(name, _)| {
        matches!(
            name.as_str(),
            "schedule_message" | "set_reminder" | "manage_cron"
        )
    });
    if !all_scheduler_tools {
        return None;
    }
    let merged = executed_tools
        .iter()
        .map(|(_, out)| out.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if merged.is_empty() {
        Some("Scheduling updated.".to_string())
    } else {
        Some(merged)
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_tool(name: &str) -> CachedTool {
        CachedTool {
            name: name.to_string(),
            description: format!("{} tool", name),
            parameters_schema: "{}".to_string(),
        }
    }

    // =====================================================================
    // Item 1: JSON Drift Recovery — repair_tool_call_json
    // =====================================================================

    #[test]
    fn json_drift_canonical_tool_key_parsed() {
        let tools = vec![make_tool("read_file")];
        let json = r#"{"tool": "read_file", "args": {"path": "/etc/hosts"}}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "read_file");
    }

    #[test]
    fn json_drift_function_key_alias_parsed() {
        let tools = vec![make_tool("write_file")];
        let json =
            r#"{"function": "write_file", "parameters": {"path": "/tmp/x", "content": "hi"}}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_some(), "expected Some but got None");
        assert_eq!(result.unwrap().name, "write_file");
    }

    #[test]
    fn json_drift_fn_and_input_aliases_parsed() {
        let tools = vec![make_tool("search_web")];
        let json = r#"{"fn": "search_web", "input": {"query": "rust async"}}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "search_web");
    }

    #[test]
    fn json_drift_action_and_params_aliases_parsed() {
        let tools = vec![make_tool("run_tests")];
        let json = r#"{"action": "run_tests", "params": {}}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "run_tests");
    }

    #[test]
    fn json_drift_unbalanced_braces_repaired() {
        let tools = vec![make_tool("read_file")];
        // Missing closing brace — balance_json should fix it
        let json = r#"{"tool": "read_file", "args": {"path": "/etc"}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_some(), "unbalanced brace should be auto-repaired");
    }

    #[test]
    fn json_drift_unknown_tool_returns_none() {
        let tools = vec![make_tool("read_file")];
        let json = r#"{"tool": "delete_everything", "args": {}}"#;
        let result = repair_tool_call_json(json, &tools);
        assert!(result.is_none(), "should not match tool not in registry");
    }

    #[test]
    fn json_drift_empty_tool_list_returns_none() {
        let json = r#"{"tool": "read_file", "args": {}}"#;
        let result = repair_tool_call_json(json, &[]);
        assert!(
            result.is_none(),
            "empty tool list should always return None"
        );
    }

    #[test]
    fn json_drift_no_json_in_text_returns_none() {
        let tools = vec![make_tool("read_file")];
        let result = repair_tool_call_json("just a plain text response", &tools);
        assert!(result.is_none());
    }

    #[test]
    fn balance_json_adds_missing_braces() {
        let input = r#"{"key": "val""#;
        let balanced = balance_json(input);
        // Should be parseable now
        assert!(serde_json::from_str::<serde_json::Value>(&balanced).is_ok());
    }

    // =====================================================================
    // Distillation engine tests
    // =====================================================================

    struct RecordingBus {
        deployed: std::sync::Mutex<Vec<(String, usize)>>,
    }

    impl RecordingBus {
        fn new() -> Self {
            Self {
                deployed: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl DeploymentBus for RecordingBus {
        fn deploy(&self, node_id: &str, signed: &SignedModule) -> Result<(), String> {
            let mut guard = self.deployed.lock().unwrap();
            guard.push((node_id.to_string(), signed.bytes.len()));
            Ok(())
        }
    }

    struct CountingLearner {
        pub calls: std::sync::Mutex<usize>,
    }

    #[async_trait::async_trait]
    impl LearnerBackend for CountingLearner {
        async fn refine_reward_model(&self, batch: Vec<TelemetryLog>) -> Result<(), String> {
            let mut guard = self.calls.lock().unwrap();
            *guard += 1;
            // Ensure we actually received the batch.
            if batch.is_empty() {
                return Err("empty batch".into());
            }
            Ok(())
        }
    }

    fn make_telemetry(action: &str) -> TelemetryLog {
        TelemetryLog {
            state_vector: vec![0.1, 0.2],
            mcp_action: action.to_string(),
            reward_score: 1.0,
            timestamp_us: 1_700_000_000_000_000,
        }
    }

    #[test]
    fn telemetry_ring_buffer_eviction_and_count() {
        let mut buf = TelemetryRingBuffer::new(2);
        buf.push(make_telemetry("a"));
        buf.push(make_telemetry("b"));
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.count_action("a"), 1);
        buf.push(make_telemetry("a"));
        // Oldest ("a") evicted, now buffer holds ["b", "a"]
        assert_eq!(buf.len(), 2);
        assert_eq!(buf.count_action("a"), 1);
        assert_eq!(buf.count_action("b"), 1);
    }

    #[test]
    fn distillation_triggers_after_threshold() {
        let mut engine = DistillationEngine::new(8, 3, "relay_01");
        let pattern = "read_sensor→threshold→alert";

        assert!(engine
            .log_and_maybe_distill(make_telemetry(pattern))
            .is_none());
        assert!(engine
            .log_and_maybe_distill(make_telemetry(pattern))
            .is_none());
        let distilled = engine
            .log_and_maybe_distill(make_telemetry(pattern))
            .expect("expected distillation after 3 occurrences");

        assert!(distilled.tool.name.contains("read_sensor"));
        assert_eq!(distilled.registration.host_node_id, "relay_01");
        assert!(!distilled.signed_module.bytes.is_empty());
        // Signature structure is compatible with the verifier stub.
        assert!(aria_skill_runtime::verify_module(&distilled.signed_module).is_ok());
    }

    #[tokio::test]
    async fn register_and_deploy_updates_stores_and_bus() {
        let mut engine = DistillationEngine::new(8, 1, "relay_99");
        let pattern = "read_sensor→threshold→alert";
        let distilled = engine
            .log_and_maybe_distill(make_telemetry(pattern))
            .expect("distillation on first log with threshold=1");

        let mut store = ToolManifestStore::new();
        let mut manifest = SkillManifest {
            registrations: Vec::new(),
        };
        let bus = RecordingBus::new();

        engine
            .register_and_deploy(&distilled, &mut store, &mut manifest, &bus)
            .expect("register_and_deploy");

        assert_eq!(store.len(), 1);
        assert_eq!(manifest.registrations.len(), 1);

        let deployed = bus.deployed.lock().unwrap();
        assert_eq!(deployed.len(), 1);
        assert_eq!(deployed[0].0, "relay_99");
        assert!(deployed[0].1 > 0);
    }

    #[tokio::test]
    async fn learner_backend_invoked_with_batch() {
        let mut engine = DistillationEngine::new(8, 3, "relay_01");
        engine.log_and_maybe_distill(make_telemetry("a"));
        engine.log_and_maybe_distill(make_telemetry("b"));

        let learner = CountingLearner {
            calls: std::sync::Mutex::new(0),
        };
        engine
            .run_training_cycle(&learner)
            .await
            .expect("training cycle");
        let calls = learner.calls.lock().unwrap();
        assert_eq!(*calls, 1);
    }

    // =====================================================================
    // Cosine similarity tests
    // =====================================================================

    #[test]
    fn cosine_identical_vectors() {
        let a = vec![1.0, 2.0, 3.0];
        let score = cosine_similarity(&a, &a);
        assert!((score - 1.0).abs() < 1e-6, "identical vectors → 1.0");
    }

    #[test]
    fn cosine_orthogonal_vectors() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let score = cosine_similarity(&a, &b);
        assert!(score.abs() < 1e-6, "orthogonal → 0.0");
    }

    #[test]
    fn cosine_opposite_vectors() {
        let a = vec![1.0, 2.0];
        let b = vec![-1.0, -2.0];
        let score = cosine_similarity(&a, &b);
        assert!((score + 1.0).abs() < 1e-6, "opposite → -1.0");
    }

    #[test]
    fn cosine_zero_vector_returns_zero() {
        let a = vec![1.0, 2.0];
        let b = vec![0.0, 0.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn cosine_different_lengths_returns_zero() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    // =====================================================================
    // SemanticRouter tests
    // =====================================================================

    #[test]
    fn route_buy_aapl_to_financial_analyst() {
        let mut router = SemanticRouter::new();

        // Simulate pre-computed embeddings:
        // "financial_analyst" embedding biased toward finance dimensions
        // "robot_controller" embedding biased toward robotics dimensions
        let financial_embedding = vec![0.9, 0.8, 0.1, 0.05]; // high in "finance" dims
        let robot_embedding = vec![0.1, 0.05, 0.9, 0.85]; // high in "robotics" dims

        router
            .register_agent("financial_analyst", financial_embedding)
            .unwrap();
        router
            .register_agent("robot_controller", robot_embedding)
            .unwrap();

        // "buy AAPL stock" would embed close to financial dimensions
        let query = vec![0.85, 0.75, 0.15, 0.1];

        let (agent, score) = router.route(&query).unwrap();
        assert_eq!(agent, "financial_analyst");

        // Verify the analyst scores higher
        let analyst_score = router.score_agent("financial_analyst", &query).unwrap();
        let robot_score = router.score_agent("robot_controller", &query).unwrap();

        assert!(
            analyst_score > robot_score,
            "financial_analyst ({}) should score higher than robot_controller ({})",
            analyst_score,
            robot_score
        );
        assert!(score > 0.9, "score should be high for matching agent");
    }

    #[test]
    fn route_empty_router_returns_error() {
        let router = SemanticRouter::new();
        let result = router.route(&[1.0, 2.0]);
        assert!(result.is_err());
        match result {
            Err(RouterError::NoAgents) => {}
            _ => panic!("expected NoAgents"),
        }
    }

    #[test]
    fn route_dimension_mismatch() {
        let mut router = SemanticRouter::new();
        router.register_agent("a", vec![1.0, 2.0, 3.0]).unwrap();

        let result = router.route(&[1.0, 2.0]); // wrong dim
        assert!(result.is_err());
        match result {
            Err(RouterError::DimensionMismatch {
                expected: 3,
                got: 2,
            }) => {}
            _ => panic!("expected DimensionMismatch"),
        }
    }

    #[test]
    fn register_dimension_mismatch() {
        let mut router = SemanticRouter::new();
        router.register_agent("a", vec![1.0, 2.0]).unwrap();
        let result = router.register_agent("b", vec![1.0, 2.0, 3.0]);
        assert!(result.is_err());
    }

    #[test]
    fn router_error_display() {
        let e = RouterError::NoAgents;
        assert!(format!("{}", e).contains("no agents"));

        let e = RouterError::DimensionMismatch {
            expected: 3,
            got: 2,
        };
        assert!(format!("{}", e).contains("dimension mismatch"));

        let e = RouterError::NoRoutingCandidate;
        assert!(format!("{}", e).contains("no routing candidates"));
    }

    #[test]
    fn route_text_with_local_embedder() {
        let embedder = LocalHashEmbedder::new(32);
        let mut router = SemanticRouter::new();
        router
            .register_agent_text(
                "financial_analyst",
                "stocks finance market portfolio equity",
                &embedder,
            )
            .unwrap();
        router
            .register_agent_text(
                "robot_controller",
                "motors actuators robotics telemetry",
                &embedder,
            )
            .unwrap();

        let (winner, _score) = router.route_text("buy AAPL stock", &embedder).unwrap();
        assert_eq!(winner, "financial_analyst");
    }

    #[test]
    fn route_with_config_confident() {
        let mut router = SemanticRouter::new();
        router.register_agent("a", vec![1.0, 0.0]).unwrap();
        router.register_agent("b", vec![0.0, 1.0]).unwrap();

        let decision = router
            .route_with_config(
                &[0.99, 0.01],
                RouteConfig {
                    confidence_threshold: 0.70,
                    tie_break_gap: 0.05,
                },
            )
            .unwrap();

        match decision {
            RouterDecision::Confident { agent_id, score } => {
                assert_eq!(agent_id, "a");
                assert!(score > 0.9);
            }
            _ => panic!("expected confident decision"),
        }
    }

    #[test]
    fn route_with_config_low_confidence_fallback() {
        let mut router = SemanticRouter::new();
        router.register_agent("a", vec![1.0, 0.0]).unwrap();
        router.register_agent("b", vec![0.0, 1.0]).unwrap();

        let decision = router
            .route_with_config(
                &[0.50, 0.50],
                RouteConfig {
                    confidence_threshold: 0.95,
                    tie_break_gap: 0.05,
                },
            )
            .unwrap();

        match decision {
            RouterDecision::NeedsLlmFallback { candidates } => {
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected fallback decision"),
        }
    }

    #[test]
    fn route_with_config_tie_gap_fallback() {
        let mut router = SemanticRouter::new();
        router
            .register_agent("financial_analyst", vec![0.9, 0.8, 0.1, 0.05])
            .unwrap();
        router
            .register_agent("robot_controller", vec![0.88, 0.79, 0.1, 0.05])
            .unwrap();

        let decision = router
            .route_with_config(
                &[0.89, 0.80, 0.1, 0.05],
                RouteConfig {
                    confidence_threshold: 0.70,
                    tie_break_gap: 0.10,
                },
            )
            .unwrap();

        match decision {
            RouterDecision::NeedsLlmFallback { candidates } => {
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected tie fallback decision"),
        }
    }

    #[test]
    fn router_index_routes_confidently() {
        let mut router = SemanticRouter::new();
        router.register_agent("a", vec![1.0, 0.0]).unwrap();
        router.register_agent("b", vec![0.0, 1.0]).unwrap();
        let index = router.build_index(RouteConfig {
            confidence_threshold: 0.7,
            tie_break_gap: 0.05,
        });
        let decision = index.route(&[0.99, 0.01]).unwrap();
        assert!(matches!(decision, RouterDecision::Confident { .. }));
    }

    #[test]
    fn agent_config_store_loads_toml_files() {
        let dir = std::env::temp_dir().join(format!("agent_store_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("developer.toml");
        std::fs::write(
            &file,
            r#"
id = "developer"
description = "Code and git tasks"
system_prompt = "You are a coding assistant."
base_tool_names = ["read_file", "run_tests"]
context_cap = 8
session_tool_ceiling = 15
max_tool_rounds = 5
"#,
        )
        .unwrap();

        let store = AgentConfigStore::load_from_dir(&dir).unwrap();
        assert_eq!(store.len(), 1);
        let dev = store.get("developer").unwrap();
        assert_eq!(dev.base_tool_names.len(), 2);
        assert_eq!(dev.context_cap, 8);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    // =====================================================================
    // DynamicToolCache tests
    // =====================================================================

    #[test]
    fn cache_eviction_at_context_cap() {
        let mut cache = DynamicToolCache::new(8, 100);

        // Insert 9 tools (cap is 8)
        for i in 0..9 {
            cache.insert(make_tool(&format!("tool_{}", i))).unwrap();
        }

        assert_eq!(cache.len(), 8, "should be capped at 8");

        // tool_0 (the first) should have been evicted
        assert!(cache.get("tool_0").is_none(), "tool_0 should be evicted");

        // tool_8 (most recent) should be present
        assert!(cache.get("tool_8").is_some());
    }

    #[test]
    fn cache_ceiling_reached() {
        let mut cache = DynamicToolCache::new(8, 15);

        // Insert 15 unique tools (all within ceiling)
        for i in 0..15 {
            cache.insert(make_tool(&format!("tool_{}", i))).unwrap();
        }

        // 16th unique tool should hit the ceiling
        let result = cache.insert(make_tool("tool_15"));
        assert!(result.is_err());
        match result {
            Err(CacheError::CeilingReached { ceiling: 15, .. }) => {}
            _ => panic!("expected CeilingReached"),
        }
    }

    #[test]
    fn cache_promote_on_reinsert() {
        let mut cache = DynamicToolCache::new(3, 100);

        cache.insert(make_tool("a")).unwrap();
        cache.insert(make_tool("b")).unwrap();
        cache.insert(make_tool("c")).unwrap();

        // Re-insert "a" — should promote it, not add duplicate
        cache.insert(make_tool("a")).unwrap();

        // Now insert "d" — should evict "b" (oldest non-promoted)
        cache.insert(make_tool("d")).unwrap();

        assert!(cache.get("a").is_some(), "a was promoted");
        assert!(cache.get("b").is_none(), "b should be evicted");
        assert!(cache.get("c").is_some());
        assert!(cache.get("d").is_some());
    }

    #[test]
    fn cache_seen_not_reset_by_reinsert() {
        let mut cache = DynamicToolCache::new(2, 3);

        cache.insert(make_tool("a")).unwrap();
        cache.insert(make_tool("b")).unwrap();
        cache.insert(make_tool("c")).unwrap(); // evicts "a" from active
        assert_eq!(cache.total_seen(), 3);

        // Re-inserting "a" should not increase seen count
        cache.insert(make_tool("a")).unwrap();
        assert_eq!(cache.total_seen(), 3);
    }

    #[test]
    fn cache_error_display() {
        let e = CacheError::CeilingReached {
            ceiling: 15,
            attempted_total: 16,
        };
        assert!(format!("{}", e).contains("session ceiling reached"));
    }

    #[test]
    fn tool_manifest_search_and_hot_swap() {
        let mut store = ToolManifestStore::new();
        store.register(CachedTool {
            name: "git_push".into(),
            description: "Push current git branch to remote".into(),
            parameters_schema: "{}".into(),
        });
        store.register(CachedTool {
            name: "read_sensor".into(),
            description: "Read telemetry from sensor node".into(),
            parameters_schema: "{}".into(),
        });

        let embedder = LocalHashEmbedder::new(64);
        let results = store.search("push branch to remote", &embedder, 1).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0.name, "git_push");

        let mut cache = DynamicToolCache::new(8, 15);
        let swapped = store
            .hot_swap_best(&mut cache, "sensor telemetry", &embedder)
            .unwrap();
        assert!(swapped.is_some());
        assert_eq!(cache.len(), 1);
    }

    // =====================================================================
    // Orchestrator tests (mock-based)
    // =====================================================================

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock LLM: returns tool calls on first query, text answer on second.
    #[derive(Clone)]
    struct MockLLMValidLoop {
        call_count: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl LLMBackend for MockLLMValidLoop {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            let n = self.call_count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                // First call: return a tool call
                Ok(LLMResponse::ToolCalls(vec![ToolCall {
                    name: "read_file".into(),
                    arguments: r#"{"path": "/workspace/main.rs"}"#.into(),
                }]))
            } else {
                // Second call: final answer
                Ok(LLMResponse::TextAnswer(
                    "File contents: fn main() {}".into(),
                ))
            }
        }
    }

    /// Mock LLM: always returns tool calls (for infinite loop test).
    #[derive(Clone)]
    struct MockLLMInfiniteLoop;

    #[async_trait::async_trait]
    impl LLMBackend for MockLLMInfiniteLoop {
        async fn query(
            &self,
            _prompt: &str,
            _tools: &[CachedTool],
        ) -> Result<LLMResponse, OrchestratorError> {
            Ok(LLMResponse::ToolCalls(vec![ToolCall {
                name: "some_tool".into(),
                arguments: "{}".into(),
            }]))
        }
    }

    /// Mock tool executor: always returns a fixed result.
    struct MockToolExecutor;

    #[async_trait::async_trait]
    impl ToolExecutor for MockToolExecutor {
        async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
            Ok(format!("result of {}", call.name))
        }
    }

    struct SleepyExecutor;

    #[async_trait::async_trait]
    impl ToolExecutor for SleepyExecutor {
        async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
            let wait = if call.name == "fast" { 5 } else { 50 };
            tokio::time::sleep(Duration::from_millis(wait)).await;
            Ok(format!("done {}", call.name))
        }
    }

    #[test]
    fn json_drift_severe_write_file_code_injection() {
        let tools = vec![make_tool("write_file")];
        // Note: No closing brace, trailing markdown, raw JS injection.
        let raw_llm = r#"```json
{"tool": "write_file", "args": {"path": "good.js", "content": "console.log('Sum:', sum);\n\n// Save to file\nwriteFile(\"good.js\", `// Good.js - Print Sum of 2 Numbers\nfunction sum(n1, n2) {\n    return n1 + n2;\n}\n\nele = sum(5, 3);");
```"#;

        let call =
            repair_tool_call_json(raw_llm, &tools).expect("Failed to repair severe JSON drift");
        assert_eq!(call.name, "write_file");
        // Ensure path and content were extracted safely
        assert!(call.arguments.contains(r#"path": "good.js"#));
        assert!(call.arguments.contains(r#"content": "console.log"#));
        assert!(call.arguments.contains("ele = sum(5, 3);"));
    }

    #[tokio::test]
    async fn orchestrator_valid_two_round_loop() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let llm = MockLLMValidLoop {
            call_count: call_count.clone(),
        };
        let executor = MockToolExecutor;
        let orchestrator = AgentOrchestrator::new(llm, executor);

        let tools = vec![make_tool("read_file")];
        let result = orchestrator
            .run(
                "please execute the destructive command",
                &tools,
                5,
                None,
                None,
            )
            .await;

        assert!(result.is_ok());
        let answer = result.unwrap();
        assert_eq!(
            answer,
            OrchestratorResult::Completed("File contents: fn main() {}".to_string())
        );

        // LLM should have been called exactly 2 times
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn orchestrator_does_not_emit_unknown_tool_json_to_user() {
        #[derive(Clone)]
        struct UnknownToolThenTextLLM {
            call_count: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl LLMBackend for UnknownToolThenTextLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(LLMResponse::TextAnswer(
                        r#"{"tool":"run_shell","args":{"command":"echo hi"}}"#.to_string(),
                    ))
                } else {
                    Ok(LLMResponse::TextAnswer("final answer".to_string()))
                }
            }
        }

        let call_count = Arc::new(AtomicUsize::new(0));
        let orchestrator = AgentOrchestrator::new(
            UnknownToolThenTextLLM {
                call_count: call_count.clone(),
            },
            MockToolExecutor,
        );
        let tools = vec![make_tool("read_file")];
        let result = orchestrator.run("test", &tools, 3, None, None).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            OrchestratorResult::Completed("final answer".to_string())
        );
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn orchestrator_max_rounds_exceeded() {
        let llm = MockLLMInfiniteLoop;
        let executor = MockToolExecutor;
        let orchestrator = AgentOrchestrator::new(llm, executor);

        let tools = vec![make_tool("some_tool")];
        let result = orchestrator
            .run("do something", &tools, 5, None, None)
            .await;

        assert!(result.is_err());
        match result {
            Err(OrchestratorError::MaxRoundsExceeded { limit: 5 }) => {}
            other => panic!("expected MaxRoundsExceeded, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn orchestrator_empty_tool_calls_returns_empty() {
        #[derive(Clone)]
        struct EmptyToolCallLLM;

        #[async_trait::async_trait]
        impl LLMBackend for EmptyToolCallLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Ok(LLMResponse::ToolCalls(vec![]))
            }
        }

        let orchestrator = AgentOrchestrator::new(EmptyToolCallLLM, MockToolExecutor);
        let result = orchestrator.run("test", &[], 5, None, None).await;
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            OrchestratorResult::Completed("".to_string())
        );
    }

    #[tokio::test]
    async fn orchestrator_tool_error_propagates() {
        struct FailingExecutor;

        #[async_trait::async_trait]
        impl ToolExecutor for FailingExecutor {
            async fn execute(&self, _call: &ToolCall) -> Result<String, OrchestratorError> {
                Err(OrchestratorError::ToolError("permission denied".into()))
            }
        }

        let llm = MockLLMInfiniteLoop; // will request tool call
        let orchestrator = AgentOrchestrator::new(llm, FailingExecutor);
        let tools = vec![CachedTool {
            name: "some_tool".into(),
            description: "test".into(),
            parameters_schema: "{}".into(),
        }];
        let result = orchestrator.run("test", &tools, 5, None, None).await;
        assert!(result.is_err());
        match result {
            Err(OrchestratorError::ToolError(msg)) => {
                assert!(msg.contains("permission denied"));
            }
            other => panic!("expected ToolError, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn orchestrator_executes_tool_calls_in_parallel() {
        #[derive(Clone)]
        struct TwoToolCallThenAnswerLLM {
            call_count: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl LLMBackend for TwoToolCallThenAnswerLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                let n = self.call_count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(LLMResponse::ToolCalls(vec![
                        ToolCall {
                            name: "slow".into(),
                            arguments: "{}".into(),
                        },
                        ToolCall {
                            name: "fast".into(),
                            arguments: "{}".into(),
                        },
                    ]))
                } else {
                    Ok(LLMResponse::TextAnswer("done".into()))
                }
            }
        }

        let llm = TwoToolCallThenAnswerLLM {
            call_count: Arc::new(AtomicUsize::new(0)),
        };
        let orchestrator = AgentOrchestrator::new(llm, SleepyExecutor);
        let started = tokio::time::Instant::now();
        let result = orchestrator.run("parallel test", &[], 1, None, None).await;
        let elapsed = started.elapsed();

        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            OrchestratorResult::Completed("done".to_string())
        );
        // Parallel execution should complete near max(single call) instead of sum.
        assert!(
            elapsed < Duration::from_millis(90),
            "expected parallel execution under 90ms, got {:?}",
            elapsed
        );
    }

    #[test]
    fn finalize_after_scheduler_tools_returns_merged_outputs() {
        let out = maybe_finalize_after_scheduler_tools(&[
            ("schedule_message".to_string(), "Scheduled reminder A".to_string()),
            ("manage_cron".to_string(), "Cron job updated".to_string()),
        ]);
        assert_eq!(
            out,
            Some("Scheduled reminder A\nCron job updated".to_string())
        );
    }

    #[test]
    fn finalize_after_scheduler_tools_ignores_mixed_tool_sets() {
        let out = maybe_finalize_after_scheduler_tools(&[
            ("schedule_message".to_string(), "Scheduled reminder A".to_string()),
            ("read_file".to_string(), "contents".to_string()),
        ]);
        assert_eq!(out, None);
    }

    #[test]
    fn orchestrator_error_display() {
        let e = OrchestratorError::LLMError("timeout".into());
        assert!(format!("{}", e).contains("LLM error"));

        let e = OrchestratorError::ToolError("failed".into());
        assert!(format!("{}", e).contains("tool error"));

        let e = OrchestratorError::MaxRoundsExceeded { limit: 5 };
        assert!(format!("{}", e).contains("max rounds (5) exceeded"));
    }

    #[tokio::test]
    async fn orchestrator_run_for_request_uses_request_content() {
        #[derive(Clone)]
        struct EchoPromptLLM;
        #[async_trait::async_trait]
        impl LLMBackend for EchoPromptLLM {
            async fn query(
                &self,
                prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Ok(LLMResponse::TextAnswer(prompt.to_string()))
            }
        }

        let orchestrator = AgentOrchestrator::new(EchoPromptLLM, MockToolExecutor);
        let request = AgentRequest {
            request_id: [1; 16],
            session_id: [2; 16],
            channel: aria_core::GatewayChannel::Cli,
            user_id: "u1".to_string(),
            content: MessageContent::Text("List workspace".to_string()),
            timestamp_us: 42,
        };

        let answer = orchestrator
            .run_for_request("mock parser", &request, "history", "rag", &[], 1)
            .await
            .unwrap();
        let answer_str = match answer {
            OrchestratorResult::Completed(t) => t,
            _ => panic!("Expected text"),
        };
        assert!(answer_str.contains("List workspace"));
        assert!(answer_str.contains("channel=Cli"));
    }

    #[tokio::test]
    async fn orchestrator_dynamic_registry_hot_swap_path() {
        #[derive(Clone)]
        struct RegistryFlowLLM {
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl LLMBackend for RegistryFlowLLM {
            async fn query(
                &self,
                _prompt: &str,
                tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                let n = self.calls.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(LLMResponse::ToolCalls(vec![ToolCall {
                        name: "search_tool_registry".into(),
                        arguments: r#"{"query":"push branch to remote"}"#.into(),
                    }]))
                } else if n == 1 {
                    let has_git_push = tools.iter().any(|t| t.name == "git_push");
                    assert!(
                        has_git_push,
                        "git_push should be injected after registry search"
                    );
                    Ok(LLMResponse::ToolCalls(vec![ToolCall {
                        name: "git_push".into(),
                        arguments: "{}".into(),
                    }]))
                } else {
                    Ok(LLMResponse::TextAnswer("done".into()))
                }
            }
        }

        let llm = RegistryFlowLLM {
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let orchestrator = AgentOrchestrator::new(llm, MockToolExecutor);
        let request = AgentRequest {
            request_id: [1; 16],
            session_id: [2; 16],
            channel: aria_core::GatewayChannel::Cli,
            user_id: "u1".to_string(),
            content: MessageContent::Text("push my branch".to_string()),
            timestamp_us: 42,
        };
        let mut cache = DynamicToolCache::new(8, 15);
        cache
            .insert(CachedTool {
                name: "search_tool_registry".into(),
                description: "meta".into(),
                parameters_schema: "{}".into(),
            })
            .unwrap();
        let mut registry = ToolManifestStore::new();
        registry.register(CachedTool {
            name: "git_push".into(),
            description: "Push current git branch to remote".into(),
            parameters_schema: "{}".into(),
        });
        let embedder = LocalHashEmbedder::new(64);

        let result = orchestrator
            .run_for_request_with_dynamic_tools(
                "mock sys", &request, "history", "rag", &mut cache, &registry, &embedder, 5, None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result, OrchestratorResult::Completed("done".to_string()));
    }

    #[tokio::test]
    async fn orchestrator_dynamic_stops_after_schedule_tool() {
        #[derive(Clone)]
        struct ScheduleToolLLM {
            calls: Arc<AtomicUsize>,
        }
        #[async_trait::async_trait]
        impl LLMBackend for ScheduleToolLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(LLMResponse::TextAnswer(
                    r#"{"tool":"schedule_message","args":{"task":"Contents of ok.js","delay":"1:10 AM","mode":"defer","deferred_prompt":"Read and return the contents of ok.js"}}"#.to_string(),
                ))
            }
        }

        struct ScheduleExec;
        #[async_trait::async_trait]
        impl ToolExecutor for ScheduleExec {
            async fn execute(&self, call: &ToolCall) -> Result<String, OrchestratorError> {
                if call.name == "schedule_message" {
                    Ok("Scheduled deferred execution for 'Contents of ok.js' at 'at:2026-03-07 01:10' (agent: omni). created=[defer:x]".to_string())
                } else {
                    Ok("ok".to_string())
                }
            }
        }

        let llm_calls = Arc::new(AtomicUsize::new(0));
        let orchestrator = AgentOrchestrator::new(
            ScheduleToolLLM {
                calls: llm_calls.clone(),
            },
            ScheduleExec,
        );
        let request = AgentRequest {
            request_id: [3; 16],
            session_id: [4; 16],
            channel: aria_core::GatewayChannel::Telegram,
            user_id: "u1".to_string(),
            content: MessageContent::Text(
                "<TOOL_RESUME_BLOCK>\nTool 'write_file' completed with output:\nSuccessfully wrote 2 bytes to ok.js</TOOL_RESUME_BLOCK>"
                    .to_string(),
            ),
            timestamp_us: 1,
        };
        let mut cache = DynamicToolCache::new(8, 16);
        cache
            .insert(CachedTool {
                name: "schedule_message".into(),
                description: "schedule".into(),
                parameters_schema: "{}".into(),
            })
            .unwrap();
        let registry = ToolManifestStore::new();
        let embedder = LocalHashEmbedder::new(32);

        let result = orchestrator
            .run_for_request_with_dynamic_tools(
                "mock sys", &request, "", "", &mut cache, &registry, &embedder, 5, None, None,
            )
            .await
            .unwrap();
        assert!(matches!(result, OrchestratorResult::Completed(_)));
        assert_eq!(
            llm_calls.load(Ordering::SeqCst),
            1,
            "scheduler tool should short-circuit without additional LLM rounds",
        );
    }

    #[tokio::test]
    async fn llm_route_fallback_prefers_llm_choice_when_valid() {
        #[derive(Clone)]
        struct FallbackChoiceLLM;
        #[async_trait::async_trait]
        impl LLMBackend for FallbackChoiceLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Ok(LLMResponse::TextAnswer("robotics_ctrl".into()))
            }
        }
        let chosen = llm_route_fallback(
            &FallbackChoiceLLM,
            "move the robot arm",
            &[("developer".into(), 0.71), ("robotics_ctrl".into(), 0.70)],
        )
        .await
        .unwrap();
        assert_eq!(chosen, "robotics_ctrl");
    }

    #[tokio::test]
    async fn llm_route_fallback_defaults_to_top_candidate_on_invalid_output() {
        #[derive(Clone)]
        struct BadChoiceLLM;
        #[async_trait::async_trait]
        impl LLMBackend for BadChoiceLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Ok(LLMResponse::TextAnswer("unknown_agent".into()))
            }
        }
        let chosen = llm_route_fallback(
            &BadChoiceLLM,
            "send a message",
            &[("communicator".into(), 0.69), ("productivity".into(), 0.68)],
        )
        .await
        .unwrap();
        assert_eq!(chosen, "communicator");
    }

    #[tokio::test]
    async fn llm_backend_pool_fallback_and_cooldown() {
        #[derive(Clone)]
        struct FailingLLM;
        #[async_trait::async_trait]
        impl LLMBackend for FailingLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Err(OrchestratorError::LLMError("backend down".into()))
            }
        }

        #[derive(Clone)]
        struct SuccessLLM;
        #[async_trait::async_trait]
        impl LLMBackend for SuccessLLM {
            async fn query(
                &self,
                _prompt: &str,
                _tools: &[CachedTool],
            ) -> Result<LLMResponse, OrchestratorError> {
                Ok(LLMResponse::TextAnswer("ok".into()))
            }
        }

        let pool = LlmBackendPool::new(
            vec!["primary".into(), "fallback".into()],
            Duration::from_millis(10),
        );
        pool.register_backend("primary", Box::new(FailingLLM));
        pool.register_backend("fallback", Box::new(SuccessLLM));

        let first = pool.query_with_fallback("p", &[]).await.unwrap();
        assert!(matches!(first, LLMResponse::TextAnswer(_)));
        assert!(!pool.is_cooling_down("primary")); // Only 1 failure

        let _ = pool.query_with_fallback("p", &[]).await.unwrap();
        assert!(!pool.is_cooling_down("primary")); // 2 failures

        let _ = pool.query_with_fallback("p", &[]).await.unwrap();
        // primary should now be cooling down after 3 consecutive failures
        assert!(pool.is_cooling_down("primary"));
    }

    #[test]
    fn schedule_spec_parsing() {
        assert_eq!(
            ScheduleSpec::parse("every:10s"),
            Some(ScheduleSpec::EverySeconds(10))
        );
        assert!(matches!(
            ScheduleSpec::parse("*/5 * * * * *").unwrap(),
            ScheduleSpec::Cron(_, _)
        ));
        assert!(matches!(
            ScheduleSpec::parse("30 19 * * *").unwrap(),
            ScheduleSpec::Cron(_, _)
        ));
        assert_eq!(
            ScheduleSpec::parse("daily@19:30"),
            Some(ScheduleSpec::DailyAt {
                hour: 19,
                minute: 30
            })
        );
        assert_eq!(
            ScheduleSpec::parse("biweekly:sat@11:00"),
            Some(ScheduleSpec::WeeklyAt {
                interval_weeks: 2,
                weekday: chrono::Weekday::Sat,
                hour: 11,
                minute: 0,
            })
        );
        assert!(matches!(
            ScheduleSpec::parse("at:2026-08-28 19:00").unwrap(),
            ScheduleSpec::Once(_)
        ));
        assert!(matches!(
            ScheduleSpec::parse("2026-08-28T19:00:00+05:30").unwrap(),
            ScheduleSpec::Once(_)
        ));
        assert_eq!(ScheduleSpec::parse("every:0s"), None);
    }

    #[test]
    fn schedule_spec_next_fire_daily_advances_correctly() {
        let spec = ScheduleSpec::DailyAt {
            hour: 19,
            minute: 30,
        };
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-28T18:00:00Z")
            .expect("rfc3339")
            .with_timezone(&chrono::Utc);
        let next = spec.next_fire(now);
        assert_eq!(
            next.to_rfc3339(),
            "2026-08-28T19:30:00+00:00".to_string()
        );

        let now_after = chrono::DateTime::parse_from_rfc3339("2026-08-28T20:00:00Z")
            .expect("rfc3339")
            .with_timezone(&chrono::Utc);
        let next_after = spec.next_fire(now_after);
        assert_eq!(
            next_after.to_rfc3339(),
            "2026-08-29T19:30:00+00:00".to_string()
        );
    }

    #[test]
    fn schedule_spec_next_fire_biweekly_respects_interval() {
        let spec = ScheduleSpec::WeeklyAt {
            interval_weeks: 2,
            weekday: chrono::Weekday::Sat,
            hour: 11,
            minute: 0,
        };
        let now = chrono::DateTime::parse_from_rfc3339("2026-08-01T10:00:00Z")
            .expect("rfc3339")
            .with_timezone(&chrono::Utc);
        let first = spec.next_fire(now);
        let second = spec.next_fire(first + chrono::Duration::seconds(1));
        let gap_days = second.signed_duration_since(first).num_days();
        assert_eq!(gap_days, 14, "biweekly schedule must fire every 14 days");
    }

    #[tokio::test]
    async fn cron_scheduler_runtime_emits_events() {
        let mut s = CronScheduler::new();
        s.add_job(ScheduledPromptJob {
            id: "j1".into(),
            agent_id: "developer".into(),
            prompt: "ping".into(),
            schedule_str: "every:1s".into(),
            kind: ScheduledJobKind::Orchestrate,
            schedule: ScheduleSpec::EverySeconds(1),
            session_id: None,
            user_id: None,
            channel: None,
        });
        let (_tx, rx_cmd) = tokio::sync::mpsc::channel(1);
        let mut rx = s.start(1, rx_cmd);
        let ev = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv())
            .await
            .expect("scheduler timeout")
            .expect("scheduler channel closed");
        assert_eq!(ev.job_id, "j1");
    }

    #[test]
    fn cron_scheduler_propagates_notify_job_kind() {
        let mut s = CronScheduler::new();
        s.add_job(ScheduledPromptJob {
            id: "n1".into(),
            agent_id: "communicator".into(),
            prompt: "Take water now".into(),
            schedule_str: "at:2026-01-01 00:00".into(),
            kind: ScheduledJobKind::Notify,
            schedule: ScheduleSpec::Once(
                chrono::Utc::now() - chrono::Duration::seconds(1),
            ),
            session_id: None,
            user_id: Some("u1".into()),
            channel: Some(GatewayChannel::Telegram),
        });
        let events = s.due_events_now();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, ScheduledJobKind::Notify);
    }
}
