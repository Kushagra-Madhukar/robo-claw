use super::*;

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

/// Routing decision output that aligns with the HiveClaw architecture:
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
    pub filesystem_scopes: Vec<aria_core::FilesystemScope>,
    #[serde(default)]
    pub retrieval_scopes: Vec<aria_core::RetrievalScope>,
    #[serde(default)]
    pub delegation_scope: Option<aria_core::DelegationScope>,
    #[serde(default)]
    pub web_domain_allowlist: Vec<String>,
    #[serde(default)]
    pub web_domain_blocklist: Vec<String>,
    #[serde(default)]
    pub browser_profile_allowlist: Vec<String>,
    #[serde(default)]
    pub browser_action_scope: Option<aria_core::BrowserActionScope>,
    #[serde(default)]
    pub computer_profile_allowlist: Vec<String>,
    #[serde(default)]
    pub computer_action_scope: Option<aria_core::ComputerActionScope>,
    #[serde(default)]
    pub browser_session_scope: Option<aria_core::BrowserSessionScope>,
    #[serde(default)]
    pub crawl_scope: Option<aria_core::CrawlScope>,
    #[serde(default)]
    pub web_approval_policy: Option<aria_core::WebApprovalPolicy>,
    #[serde(default)]
    pub web_transport_allowlist: Vec<aria_core::BrowserTransportKind>,
    #[serde(default)]
    pub requires_elevation: bool,
    #[serde(default = "default_agent_class")]
    pub class: aria_core::AgentClass,
    #[serde(default = "default_side_effect_level")]
    pub side_effect_level: aria_core::SideEffectLevel,
    #[serde(default)]
    pub trust_profile: Option<aria_core::TrustProfile>,
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
fn default_agent_class() -> aria_core::AgentClass {
    aria_core::AgentClass::Generalist
}
fn default_side_effect_level() -> aria_core::SideEffectLevel {
    aria_core::SideEffectLevel::StatefulWrite
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
