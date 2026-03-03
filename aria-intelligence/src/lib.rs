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

use futures::future::join_all;

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
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RouterError::NoAgents => write!(f, "no agents registered"),
            RouterError::DimensionMismatch { expected, got } => {
                write!(f, "dimension mismatch: expected {}, got {}", expected, got)
            }
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

    /// Route raw text by embedding it locally and then performing cosine routing.
    pub fn route_text<E: EmbeddingModel>(
        &self,
        query_text: &str,
        embedder: &E,
    ) -> Result<(String, f32), RouterError> {
        let query_embedding = embedder.embed(query_text);
        self.route(&query_embedding)
    }

    /// Get the similarity score for a specific agent.
    pub fn score_agent(&self, agent_name: &str, query_embedding: &[f32]) -> Option<f32> {
        self.agents
            .iter()
            .find(|a| a.name == agent_name)
            .map(|a| cosine_similarity(&a.embedding, query_embedding))
    }
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
}

// ---------------------------------------------------------------------------
// Orchestrator types
// ---------------------------------------------------------------------------

/// A tool call requested by the LLM.
#[derive(Debug, Clone)]
pub struct ToolCall {
    /// Name of the tool to invoke.
    pub name: String,
    /// JSON-encoded arguments for the tool.
    pub arguments: String,
}

/// Response from the LLM backend.
#[derive(Debug, Clone)]
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
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::LLMError(msg) => write!(f, "LLM error: {}", msg),
            OrchestratorError::ToolError(msg) => write!(f, "tool error: {}", msg),
            OrchestratorError::MaxRoundsExceeded { limit } => {
                write!(f, "max tool rounds exceeded: limit {}", limit)
            }
        }
    }
}

impl std::error::Error for OrchestratorError {}

// ---------------------------------------------------------------------------
// LLMBackend trait
// ---------------------------------------------------------------------------

/// Async trait for LLM backends (Claude, Ollama, llama.cpp, etc.).
///
/// Implementations must be `Send + Sync` to support concurrent usage.
#[async_trait::async_trait]
pub trait LLMBackend: Send + Sync {
    /// Query the LLM with a prompt and available tools.
    async fn query(
        &self,
        prompt: &str,
        tools: &[CachedTool],
    ) -> Result<LLMResponse, OrchestratorError>;
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
    ) -> Result<String, OrchestratorError> {
        let mut prompt = initial_prompt.to_string();
        let mut rounds = 0_usize;

        loop {
            let response = self.llm.query(&prompt, tools).await?;

            match response {
                LLMResponse::TextAnswer(answer) => {
                    return Ok(answer);
                }
                LLMResponse::ToolCalls(calls) => {
                    if calls.is_empty() {
                        // Empty tool calls treated as implicit text answer
                        return Ok(String::new());
                    }

                    rounds += 1;
                    if rounds > max_tool_rounds {
                        return Err(OrchestratorError::MaxRoundsExceeded {
                            limit: max_tool_rounds,
                        });
                    }

                    // Execute all tool calls in parallel for this turn.
                    let executions = calls.iter().map(|call| async {
                        let result = self.tool_executor.execute(call).await;
                        (call.name.clone(), result)
                    });
                    let resolved = join_all(executions).await;

                    let mut tool_results = Vec::with_capacity(resolved.len());
                    for (tool_name, result) in resolved {
                        let output = result?;
                        tool_results.push(format!("[Tool: {}] Result: {}", tool_name, output));
                    }

                    // Append tool results to prompt for next LLM query
                    prompt = format!(
                        "{}\n\n--- Tool Results ---\n{}",
                        prompt,
                        tool_results.join("\n")
                    );
                }
            }
        }
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

    // =====================================================================
    // Orchestrator tests (mock-based)
    // =====================================================================

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Mock LLM: returns tool calls on first query, text answer on second.
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

    #[tokio::test]
    async fn orchestrator_valid_two_round_loop() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let llm = MockLLMValidLoop {
            call_count: call_count.clone(),
        };
        let executor = MockToolExecutor;
        let orchestrator = AgentOrchestrator::new(llm, executor);

        let tools = vec![make_tool("read_file")];
        let result = orchestrator.run("read file main.rs", &tools, 5).await;

        assert!(result.is_ok());
        let answer = result.unwrap();
        assert_eq!(answer, "File contents: fn main() {}");

        // LLM should have been called exactly 2 times
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn orchestrator_max_rounds_exceeded() {
        let llm = MockLLMInfiniteLoop;
        let executor = MockToolExecutor;
        let orchestrator = AgentOrchestrator::new(llm, executor);

        let tools = vec![make_tool("some_tool")];
        let result = orchestrator.run("do something", &tools, 5).await;

        assert!(result.is_err());
        match result {
            Err(OrchestratorError::MaxRoundsExceeded { limit: 5 }) => {}
            other => panic!("expected MaxRoundsExceeded, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn orchestrator_empty_tool_calls_returns_empty() {
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
        let result = orchestrator.run("test", &[], 5).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "");
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

        let result = orchestrator.run("test", &[], 5).await;
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
        let result = orchestrator.run("parallel test", &[], 1).await;
        let elapsed = started.elapsed();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "done");
        // Parallel execution should complete near max(single call) instead of sum.
        assert!(
            elapsed < Duration::from_millis(90),
            "expected parallel execution under 90ms, got {:?}",
            elapsed
        );
    }

    #[test]
    fn orchestrator_error_display() {
        let e = OrchestratorError::LLMError("timeout".into());
        assert!(format!("{}", e).contains("LLM error"));

        let e = OrchestratorError::ToolError("failed".into());
        assert!(format!("{}", e).contains("tool error"));

        let e = OrchestratorError::MaxRoundsExceeded { limit: 5 };
        assert!(format!("{}", e).contains("max tool rounds exceeded"));
    }
}
