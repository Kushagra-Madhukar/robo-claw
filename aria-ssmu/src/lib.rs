//! # aria-ssmu
//!
//! ARIA-X RAG engine implementing the PageIndex tree structure and
//! thread-safe session memory.
//!
//! ## PageIndex
//!
//! Inspired by [VectifyAI/PageIndex](https://github.com/VectifyAI/PageIndex),
//! this module provides a **vectorless, reasoning-based** document index.
//! Documents are represented as hierarchical JSON trees (semantic
//! table-of-contents) rather than vector embeddings. An LLM navigates
//! the tree top-down via reasoning to find relevant sections.
//!
//! The in-memory tree enforces an LRU capacity limit, evicting the
//! least-recently-accessed nodes when full.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, RwLock};
use std::{fs, path::Path};

use serde::{Deserialize, Serialize};
use tracing::debug;

pub mod persistence;
pub mod vector;
use vector::VectorStore;

// ---------------------------------------------------------------------------
// PageNode
// ---------------------------------------------------------------------------

/// A single node in the PageIndex tree.
///
/// Mirrors the VectifyAI PageIndex JSON schema:
/// ```json
/// {
///   "title": "Financial Stability",
///   "node_id": "0006",
///   "start_index": 21,
///   "end_index": 22,
///   "summary": "The Federal Reserve ...",
///   "nodes": [...]
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PageNode {
    /// Unique identifier for this node (e.g. "0006").
    pub node_id: String,
    /// Section title.
    pub title: String,
    /// LLM-generated summary of this section.
    pub summary: String,
    /// Start page index (inclusive).
    pub start_index: u32,
    /// End page index (exclusive).
    pub end_index: u32,
    /// IDs of child nodes.
    pub children: Vec<String>,
}

// ---------------------------------------------------------------------------
// PageIndexTree
// ---------------------------------------------------------------------------

/// Error type for PageIndexTree operations.
#[derive(Debug)]
pub enum TreeError {
    /// A node with this ID already exists.
    DuplicateNode(String),
    /// The referenced node was not found.
    NodeNotFound(String),
    /// Serialization/deserialization failure.
    SerializationError(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::DuplicateNode(id) => write!(f, "duplicate node: {}", id),
            TreeError::NodeNotFound(id) => write!(f, "node not found: {}", id),
            TreeError::SerializationError(msg) => write!(f, "serialization error: {}", msg),
        }
    }
}

impl std::error::Error for TreeError {}

/// Hierarchical document index with LRU eviction.
///
/// Maintains an in-memory tree of [`PageNode`]s capped at `capacity`.
/// When the tree is full, the least-recently-accessed node is evicted
/// and moved to the `evicted` buffer (simulating disk spill).
pub struct PageIndexTree {
    /// Maximum number of nodes kept in memory.
    capacity: usize,
    /// Node storage keyed by node_id.
    nodes: HashMap<String, PageNode>,
    /// LRU order: front = oldest, back = most recent.
    lru_order: VecDeque<String>,
    /// Nodes that were evicted from memory (disk spill simulation).
    evicted: Vec<PageNode>,
}

impl PageIndexTree {
    /// Create a new tree with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            nodes: HashMap::with_capacity(capacity),
            lru_order: VecDeque::with_capacity(capacity),
            evicted: Vec::new(),
        }
    }

    /// Insert a node into the tree. Returns the evicted node if the tree
    /// was at capacity, or `None` otherwise.
    pub fn insert(&mut self, node: PageNode) -> Result<Option<PageNode>, TreeError> {
        if self.nodes.contains_key(&node.node_id) {
            return Err(TreeError::DuplicateNode(node.node_id.clone()));
        }

        let evicted = if self.nodes.len() >= self.capacity {
            self.evict_oldest()
        } else {
            None
        };

        self.lru_order.push_back(node.node_id.clone());
        self.nodes.insert(node.node_id.clone(), node);

        Ok(evicted)
    }

    /// Get a node by ID, touching it for LRU purposes.
    pub fn get(&mut self, node_id: &str) -> Option<&PageNode> {
        if self.nodes.contains_key(node_id) {
            self.touch(node_id);
            self.nodes.get(node_id)
        } else {
            None
        }
    }

    /// Get a node by ID without touching LRU (read-only peek).
    pub fn peek(&self, node_id: &str) -> Option<&PageNode> {
        self.nodes.get(node_id)
    }

    /// Get all child nodes of the given node.
    pub fn get_children(&self, node_id: &str) -> Result<Vec<&PageNode>, TreeError> {
        let parent = self
            .nodes
            .get(node_id)
            .ok_or_else(|| TreeError::NodeNotFound(node_id.to_string()))?;

        Ok(parent
            .children
            .iter()
            .filter_map(|cid| self.nodes.get(cid))
            .collect())
    }

    /// Retrieve nodes relevant to a natural-language query using
    /// vectorless lexical scoring over the PageIndex hierarchy.
    ///
    /// This keeps PageIndex as the primary retrieval mechanism:
    /// we score by token overlap against node titles and summaries.
    pub fn retrieve_relevant(&self, query: &str, top_k: usize) -> Vec<PageNode> {
        if top_k == 0 || query.trim().is_empty() {
            return Vec::new();
        }

        let query_terms = tokenize(query);
        if query_terms.is_empty() {
            return Vec::new();
        }

        let mut ranked: Vec<(usize, &PageNode)> = self
            .nodes
            .values()
            .map(|node| {
                let searchable = format!("{} {}", node.title, node.summary);
                let score = overlap_score(&tokenize(&searchable), &query_terms);
                (score, node)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        ranked.sort_by(|a, b| b.0.cmp(&a.0));
        ranked
            .into_iter()
            .take(top_k)
            .map(|(_, node)| node.clone())
            .collect()
    }

    /// Number of nodes currently in memory.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Nodes that were evicted from memory.
    pub fn evicted_nodes(&self) -> &[PageNode] {
        &self.evicted
    }

    /// Serialize the tree to JSON.
    pub fn to_json(&self) -> Result<String, TreeError> {
        let nodes: Vec<&PageNode> = self
            .lru_order
            .iter()
            .filter_map(|id| self.nodes.get(id))
            .collect();
        serde_json::to_string_pretty(&nodes)
            .map_err(|e| TreeError::SerializationError(format!("{}", e)))
    }

    /// Deserialize nodes from JSON into a new tree.
    pub fn from_json(json: &str, capacity: usize) -> Result<Self, TreeError> {
        let nodes: Vec<PageNode> = serde_json::from_str(json)
            .map_err(|e| TreeError::SerializationError(format!("{}", e)))?;

        let mut tree = Self::new(capacity);
        for node in nodes {
            // Ignore eviction during bulk load — caller chose capacity
            let _ = tree.insert(node);
        }
        Ok(tree)
    }

    // -- internal helpers --

    fn touch(&mut self, node_id: &str) {
        if let Some(pos) = self.lru_order.iter().position(|id| id == node_id) {
            self.lru_order.remove(pos);
            self.lru_order.push_back(node_id.to_string());
        }
    }

    fn evict_oldest(&mut self) -> Option<PageNode> {
        if let Some(oldest_id) = self.lru_order.pop_front() {
            if let Some(node) = self.nodes.remove(&oldest_id) {
                self.evicted.push(node.clone());
                return Some(node);
            }
        }
        None
    }
}

fn tokenize(input: &str) -> Vec<String> {
    input
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|t| {
            let normalized = t.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                None
            } else {
                Some(normalized)
            }
        })
        .collect()
}

fn overlap_score(haystack_terms: &[String], needle_terms: &[String]) -> usize {
    needle_terms
        .iter()
        .filter(|term| haystack_terms.contains(term))
        .count()
}

// ---------------------------------------------------------------------------
// Hybrid memory planner + retrieval
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryPlan {
    VectorOnly,
    VectorPlusPageIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryPlannerConfig {
    pub structured_query_token_threshold: usize,
}

impl Default for QueryPlannerConfig {
    fn default() -> Self {
        Self {
            structured_query_token_threshold: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridRetrieval {
    pub plan: QueryPlan,
    pub vector_context: Vec<String>,
    pub page_context: Vec<PageNode>,
}

pub struct HybridMemoryEngine<'a> {
    vector: &'a VectorStore,
    page_index: &'a PageIndexTree,
    keyword_index: Option<&'a vector::KeywordIndex>,
    planner: QueryPlannerConfig,
}

impl<'a> HybridMemoryEngine<'a> {
    pub fn new(
        vector: &'a VectorStore,
        page_index: &'a PageIndexTree,
        planner: QueryPlannerConfig,
    ) -> Self {
        Self {
            vector,
            page_index,
            keyword_index: None,
            planner,
        }
    }

    /// Set the keyword index for hybrid (cosine + BM25 + RRF) retrieval.
    pub fn with_keyword_index(mut self, kw: &'a vector::KeywordIndex) -> Self {
        self.keyword_index = Some(kw);
        self
    }

    pub fn plan_query(&self, query: &str) -> QueryPlan {
        let terms = tokenize(query);
        let has_structured_hint = terms.iter().any(|t| {
            matches!(
                t.as_str(),
                "section" | "chapter" | "page" | "pdf" | "document" | "manual"
            )
        });
        if has_structured_hint || terms.len() >= self.planner.structured_query_token_threshold {
            QueryPlan::VectorPlusPageIndex
        } else {
            QueryPlan::VectorOnly
        }
    }

    /// Original vector-only retrieval (backward compatible).
    pub fn retrieve(
        &self,
        query: &str,
        query_embedding: &[f32],
        vector_top_k: usize,
        page_top_k: usize,
    ) -> HybridRetrieval {
        let plan = self.plan_query(query);
        debug!(query = %query, plan = ?plan, "RAG: plan_query");

        let vector_context = self
            .vector
            .search(query_embedding, vector_top_k)
            .unwrap_or_default()
            .into_iter()
            .map(|(score, doc)| format!("- {:.3} {}: {}", score, doc.id, doc.content))
            .collect::<Vec<_>>();

        debug!(
            vector_hits = vector_context.len(),
            vector_preview = vector_context
                .join("\n")
                .chars()
                .take(300)
                .collect::<String>(),
            "RAG: VectorStore search"
        );

        let page_context = if plan == QueryPlan::VectorPlusPageIndex {
            let pages = self.page_index.retrieve_relevant(query, page_top_k);
            debug!(
                page_hits = pages.len(),
                page_titles = pages
                    .iter()
                    .map(|n| n.title.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                "RAG: PageIndex retrieval"
            );
            pages
        } else {
            debug!("RAG: PageIndex skipped (VectorOnly plan)");
            Vec::new()
        };

        HybridRetrieval {
            plan,
            vector_context,
            page_context,
        }
    }

    /// Hybrid retrieval using cosine similarity + BM25 keyword search,
    /// merged via Reciprocal Rank Fusion (RRF).
    ///
    /// Falls back to vector-only search if no `KeywordIndex` was configured.
    pub fn retrieve_hybrid(
        &self,
        query: &str,
        query_embedding: &[f32],
        vector_top_k: usize,
        page_top_k: usize,
        min_rrf_score: f32,
    ) -> HybridRetrieval {
        let plan = self.plan_query(query);
        debug!(query = %query, plan = ?plan, "RAG: plan_query (hybrid)");

        let vector_context = if self.keyword_index.is_some() {
            // Full hybrid: cosine + BM25 → RRF
            let results = self.vector.hybrid_search(
                query_embedding,
                self.keyword_index,
                query,
                vector_top_k,
                min_rrf_score,
                60.0,
            );
            results
                .into_iter()
                .map(|r| {
                    format!(
                        "- [RRF:{:.4} V:{:.3} K:{:.1}] {}: {}",
                        r.rrf_score, r.vector_score, r.keyword_score, r.id, r.content
                    )
                })
                .collect::<Vec<_>>()
        } else {
            // Fallback: vector-only
            self.vector
                .search(query_embedding, vector_top_k)
                .unwrap_or_default()
                .into_iter()
                .map(|(score, doc)| format!("- {:.3} {}: {}", score, doc.id, doc.content))
                .collect::<Vec<_>>()
        };

        debug!(
            vector_hits = vector_context.len(),
            vector_preview = vector_context
                .join("\n")
                .chars()
                .take(300)
                .collect::<String>(),
            "RAG: hybrid search"
        );

        let page_context = if plan == QueryPlan::VectorPlusPageIndex {
            let pages = self.page_index.retrieve_relevant(query, page_top_k);
            debug!(page_hits = pages.len(), "RAG: PageIndex retrieval");
            pages
        } else {
            Vec::new()
        };

        HybridRetrieval {
            plan,
            vector_context,
            page_context,
        }
    }
}

// ---------------------------------------------------------------------------
// SessionMemory
// ---------------------------------------------------------------------------

/// A message in the session history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    /// Role of the message sender (e.g. "user", "assistant").
    pub role: String,
    /// Text content of the message.
    pub content: String,
    /// Microsecond timestamp.
    pub timestamp_us: u64,
}

#[derive(Debug, Clone, Default)]
pub struct SessionState {
    pub history: VecDeque<Message>,
    pub durable_constraints: Vec<String>,
    pub current_agent: Option<String>,
    pub current_model: Option<String>,
}

/// Thread-safe, per-session message history and state overrides.
///
/// Internally uses `Arc<RwLock<HashMap<Uuid, SessionState>>>`.
/// Each session maintains a sliding window of the most recent `window_size`
/// messages, plus optional overrides.
#[derive(Clone)]
pub struct SessionMemory {
    window_size: usize,
    store: Arc<RwLock<HashMap<uuid::Uuid, SessionState>>>,
}

impl SessionMemory {
    /// Create a new session memory with the given window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            window_size,
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Append a message to the given session.
    ///
    /// If the session's history exceeds `window_size`, the oldest
    /// message is removed.
    pub fn append(&self, session_id: uuid::Uuid, message: Message) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let state = store
            .entry(session_id)
            .or_insert_with(SessionState::default);
        let content_preview: String = message.content.chars().take(100).collect();
        debug!(
            session_id = %session_id,
            role = %message.role,
            content_len = message.content.len(),
            content_preview = %content_preview,
            "SessionMemory: append"
        );
        state.history.push_back(message);

        if state.history.len() > self.window_size {
            state.history.pop_front();
        }

        Ok(())
    }

    /// Append a durable constraint to the session memory.
    pub fn add_durable_constraint(
        &self,
        session_id: uuid::Uuid,
        constraint: String,
    ) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        let state = store
            .entry(session_id)
            .or_insert_with(SessionState::default);
        state.durable_constraints.push(constraint);
        Ok(())
    }

    /// Get all durable constraints for a session.
    pub fn get_durable_constraints(&self, session_id: &uuid::Uuid) -> Result<Vec<String>, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        if let Some(state) = store.get(session_id) {
            Ok(state.durable_constraints.clone())
        } else {
            Ok(Vec::new())
        }
    }

    /// Get the message history for a session, in chronological order.
    pub fn get_history(&self, session_id: &uuid::Uuid) -> Result<Vec<Message>, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let hist: Vec<Message> = store
            .get(session_id)
            .map(|s| s.history.iter().cloned().collect())
            .unwrap_or_default();
        debug!(
            session_id = %session_id,
            message_count = hist.len(),
            "SessionMemory: get_history"
        );
        Ok(hist)
    }

    /// Clears the message history for a given session.
    pub fn clear_history(&self, session_id: &uuid::Uuid) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        if let Some(state) = store.get_mut(session_id) {
            state.history.clear();
        }
        Ok(())
    }

    /// Set the current agent/model overrides for a session.
    pub fn update_overrides(
        &self,
        session_id: uuid::Uuid,
        agent: Option<String>,
        model: Option<String>,
    ) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        let state = store
            .entry(session_id)
            .or_insert_with(SessionState::default);
        if let Some(a) = agent {
            state.current_agent = Some(a);
        }
        if let Some(m) = model {
            state.current_model = Some(m);
        }
        Ok(())
    }

    /// Get the current overrides (agent, model) for a session.
    pub fn get_overrides(
        &self,
        session_id: &uuid::Uuid,
    ) -> Result<(Option<String>, Option<String>), String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        if let Some(state) = store.get(session_id) {
            Ok((state.current_agent.clone(), state.current_model.clone()))
        } else {
            Ok((None, None))
        }
    }

    /// Replaces the oldest `remove_count` messages in the session history with
    /// a single summary message, keeping the session window compact.
    pub fn replace_old_history(
        &self,
        session_id: uuid::Uuid,
        remove_count: usize,
        summary: Message,
    ) -> Result<(), String> {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        let state = store
            .entry(session_id)
            .or_insert_with(SessionState::default);

        let rc = remove_count.min(state.history.len());
        for _ in 0..rc {
            state.history.pop_front();
        }
        state.history.push_front(summary);

        while state.history.len() > self.window_size {
            state.history.pop_front();
        }
        Ok(())
    }

    /// Condense the session history when it exceeds `threshold` messages.
    ///
    /// The `summarizer` closure receives all current messages and returns a
    /// single summary string. That string replaces the history with one
    /// synthetic `"summary"` message, keeping the session window compact.
    pub fn summarize_if_over_threshold<F>(
        &self,
        session_id: uuid::Uuid,
        threshold: usize,
        timestamp_us: u64,
        summarizer: F,
    ) -> Result<bool, String>
    where
        F: FnOnce(&[Message]) -> String,
    {
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let history = match store.get_mut(&session_id) {
            Some(state) if state.history.len() > threshold => &mut state.history,
            _ => return Ok(false),
        };

        let all_messages: Vec<Message> = history.iter().cloned().collect();
        let summary = summarizer(&all_messages);
        history.clear();
        history.push_back(Message {
            role: "summary".into(),
            content: summary,
            timestamp_us,
        });
        Ok(true)
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> Result<usize, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        Ok(store.len())
    }

    /// Index all session histories as summary chunks into the vector store.
    /// Uses the provided embed function to compute embeddings.
    pub fn index_session_summaries_to<E>(
        &self,
        vector_store: &mut VectorStore,
        embed_fn: E,
    ) -> Result<usize, String>
    where
        E: Fn(&str) -> Vec<f32>,
    {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        let mut count = 0usize;
        for (session_id, state) in store.iter() {
            let summary: String = state
                .history
                .iter()
                .map(|m| format!("{}: {}", m.role, m.content))
                .collect::<Vec<_>>()
                .join("\n");
            let truncated = if summary.len() > 500 {
                format!("{}...", &summary[..500])
            } else {
                summary.clone()
            };
            let id = format!("session.{}", session_id);
            vector_store.index_session_summary(
                id.clone(),
                summary,
                embed_fn(&truncated),
                session_id.to_string(),
                vec!["session".into(), "history".into()],
            );
            debug!(
                session_id = %session_id,
                chunk_id = %id,
                summary_len = truncated.len(),
                "RAG: indexed session summary to VectorStore"
            );
            count += 1;
        }
        debug!(
            total_indexed = count,
            "RAG: index_session_summaries_to complete"
        );
        Ok(count)
    }

    pub fn save_to_dir<P: AsRef<Path>>(&self, dir: P) -> Result<usize, String> {
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| format!("create sessions dir: {}", e))?;

        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let mut count = 0usize;
        for (session_id, state) in store.iter() {
            let payload = SessionFile {
                session_id: session_id.to_string(),
                history: state.history.iter().cloned().collect(),
                durable_constraints: state.durable_constraints.clone(),
                current_agent: state.current_agent.clone(),
                current_model: state.current_model.clone(),
            };
            let json = serde_json::to_vec_pretty(&payload)
                .map_err(|e| format!("serialize session {}: {}", session_id, e))?;
            // Save state snapshot
            let target = dir.join(format!("{}_state.json", session_id));
            let tmp = dir.join(format!("{}_state.json.tmp", session_id));
            fs::write(&tmp, &json).map_err(|e| format!("write temp session file: {}", e))?;
            fs::rename(&tmp, &target).map_err(|e| format!("atomic rename session file: {}", e))?;
            count += 1;
        }

        Ok(count)
    }

    /// Save all active sessions into a single unified SQLite database.
    pub fn save_to_sqlite<P: AsRef<Path>>(&self, path: P) -> Result<usize, String> {
        let mut db = persistence::SqlitePersistence::open(path.as_ref())
            .map_err(|e| format!("sqlite open failed: {}", e))?;

        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let mut count = 0usize;
        for (session_id, state) in store.iter() {
            db.save_session(*session_id, state)
                .map_err(|e| format!("sqlite save failed for {}: {}", session_id, e))?;
            count += 1;
        }

        Ok(count)
    }

    /// Append a single message event to the session's JSONL audit log.
    pub fn append_audit_event<P: AsRef<Path>>(
        &self,
        dir: P,
        session_id: &uuid::Uuid,
        message: &Message,
    ) -> Result<(), String> {
        use std::io::Write;
        let dir = dir.as_ref();
        fs::create_dir_all(dir).map_err(|e| format!("create sessions dir: {}", e))?;
        let target = dir.join(format!("{}.jsonl", session_id));

        // Write the single message as a JSON line
        let mut json =
            serde_json::to_vec(message).map_err(|e| format!("serialize audit event: {}", e))?;
        json.push(b'\n');

        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)
            .map_err(|e| format!("open jsonl file: {}", e))?;

        file.write_all(&json)
            .map_err(|e| format!("write jsonl entry: {}", e))?;

        Ok(())
    }

    /// Load session files from a directory and merge into in-memory store.
    ///
    /// Existing in-memory sessions are replaced if the same session ID exists
    /// on disk. Invalid files are ignored and returned as warnings.
    pub fn load_from_dir<P: AsRef<Path>>(&self, dir: P) -> Result<LoadReport, String> {
        let dir = dir.as_ref();
        if !dir.exists() {
            return Ok(LoadReport {
                loaded_sessions: 0,
                skipped_files: 0,
            });
        }

        let mut loaded = 0usize;
        let mut skipped = 0usize;
        let mut store = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        let entries = fs::read_dir(dir).map_err(|e| format!("read sessions dir: {}", e))?;
        for entry in entries {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let path = entry.path();
            let is_state = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .ends_with("_state.json");

            // For backwards compatibility, also load old ".json" files if they don't have "_state" suffix,
            // but prioritize the state files.
            if !is_state && path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let raw = match fs::read(&path) {
                Ok(v) => v,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let parsed: SessionFile = match serde_json::from_slice(&raw) {
                Ok(v) => v,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let sid = match uuid::Uuid::parse_str(&parsed.session_id) {
                Ok(v) => v,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };

            let mut history: VecDeque<Message> = parsed.history.into_iter().collect();
            while history.len() > self.window_size {
                history.pop_front();
            }
            store.insert(
                sid,
                SessionState {
                    history,
                    durable_constraints: parsed.durable_constraints,
                    current_agent: parsed.current_agent,
                    current_model: parsed.current_model,
                },
            );
            loaded += 1;
        }

        Ok(LoadReport {
            loaded_sessions: loaded,
            skipped_files: skipped,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadReport {
    pub loaded_sessions: usize,
    pub skipped_files: usize,
}

#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    session_id: String,
    history: Vec<Message>,
    #[serde(default)]
    durable_constraints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_model: Option<String>,
}

// ---------------------------------------------------------------------------
// UserPreferences — persistent KV store
// ---------------------------------------------------------------------------

/// Thread-safe, file-backed key-value store for per-user preferences.
///
/// Values are arbitrary strings. Persisted as a flat JSON object.
#[derive(Clone)]
pub struct UserPreferences {
    store: Arc<RwLock<HashMap<String, String>>>,
}

impl UserPreferences {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn set(&self, key: impl Into<String>, value: impl Into<String>) -> Result<(), String> {
        let mut guard = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        guard.insert(key.into(), value.into());
        Ok(())
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.store.read().ok()?.get(key).cloned()
    }

    pub fn remove(&self, key: &str) -> Result<(), String> {
        let mut guard = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        guard.remove(key);
        Ok(())
    }

    /// Persist to a JSON file atomically (temp file + rename).
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("create prefs dir: {}", e))?;
        }
        let guard = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        let json =
            serde_json::to_vec_pretty(&*guard).map_err(|e| format!("serialize prefs: {}", e))?;
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, &json).map_err(|e| format!("write prefs tmp: {}", e))?;
        fs::rename(&tmp, path).map_err(|e| format!("rename prefs: {}", e))?;
        Ok(())
    }

    /// Load from a JSON file, merging into the current store.
    /// Returns `Ok(false)` if the file doesn't exist (non-fatal).
    /// Returns `Ok(true)` if loaded successfully.
    /// Corrupted files are replaced with an empty store on recovery.
    pub fn load<P: AsRef<Path>>(&self, path: P) -> Result<bool, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(false);
        }
        let raw = fs::read(path).map_err(|e| format!("read prefs: {}", e))?;
        let parsed: HashMap<String, String> = serde_json::from_slice(&raw).unwrap_or_else(|_| {
            // corrupted file: fall back to empty store and overwrite on next save
            HashMap::new()
        });
        let mut guard = self
            .store
            .write()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        *guard = parsed;
        Ok(true)
    }
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{ChunkKind, ChunkMetadata};

    fn make_node(id: &str, title: &str, children: Vec<&str>) -> PageNode {
        PageNode {
            node_id: id.to_string(),
            title: title.to_string(),
            summary: format!("Summary of {}", title),
            start_index: 0,
            end_index: 1,
            children: children.into_iter().map(String::from).collect(),
        }
    }

    // =====================================================================
    // PageIndexTree tests
    // =====================================================================

    #[test]
    fn lru_eviction_at_capacity() {
        let mut tree = PageIndexTree::new(32);

        // Insert 33 nodes (cap is 32)
        for i in 0..33 {
            let node = make_node(&format!("{:04}", i), &format!("Section {}", i), vec![]);
            tree.insert(node).expect("insert");
        }

        // Tree should have exactly 32 nodes
        assert_eq!(tree.len(), 32);

        // The oldest node (0000) should have been evicted
        assert!(tree.peek("0000").is_none(), "node 0000 should be evicted");
        assert_eq!(tree.evicted_nodes().len(), 1);
        assert_eq!(tree.evicted_nodes()[0].node_id, "0000");

        // Most recent node should still be present
        assert!(tree.peek("0032").is_some());
    }

    #[test]
    fn lru_touch_prevents_eviction() {
        let mut tree = PageIndexTree::new(3);

        tree.insert(make_node("a", "A", vec![])).unwrap();
        tree.insert(make_node("b", "B", vec![])).unwrap();
        tree.insert(make_node("c", "C", vec![])).unwrap();

        // Touch "a" so it becomes most recent
        tree.get("a");

        // Insert "d" — should evict "b" (now oldest), not "a"
        tree.insert(make_node("d", "D", vec![])).unwrap();

        assert!(tree.peek("a").is_some(), "a was touched, should survive");
        assert!(tree.peek("b").is_none(), "b should be evicted");
        assert!(tree.peek("c").is_some());
        assert!(tree.peek("d").is_some());
    }

    #[test]
    fn duplicate_node_error() {
        let mut tree = PageIndexTree::new(10);
        tree.insert(make_node("x", "X", vec![])).unwrap();
        let result = tree.insert(make_node("x", "X again", vec![]));
        assert!(result.is_err());
        match result {
            Err(TreeError::DuplicateNode(id)) => assert_eq!(id, "x"),
            _ => panic!("expected DuplicateNode error"),
        }
    }

    #[test]
    fn get_children_returns_child_nodes() {
        let mut tree = PageIndexTree::new(10);

        tree.insert(make_node("root", "Root", vec!["c1", "c2"]))
            .unwrap();
        tree.insert(make_node("c1", "Child 1", vec![])).unwrap();
        tree.insert(make_node("c2", "Child 2", vec![])).unwrap();

        let children = tree.get_children("root").unwrap();
        assert_eq!(children.len(), 2);

        let child_ids: Vec<&str> = children.iter().map(|c| c.node_id.as_str()).collect();
        assert!(child_ids.contains(&"c1"));
        assert!(child_ids.contains(&"c2"));
    }

    #[test]
    fn get_children_of_missing_node() {
        let tree = PageIndexTree::new(10);
        let result = tree.get_children("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn retrieve_relevant_prefers_matching_nodes() {
        let mut tree = PageIndexTree::new(10);
        tree.insert(PageNode {
            node_id: "finance".into(),
            title: "Financial Stability".into(),
            summary: "Risk and market analysis for banking".into(),
            start_index: 1,
            end_index: 2,
            children: vec![],
        })
        .unwrap();
        tree.insert(PageNode {
            node_id: "robot".into(),
            title: "Robot Diagnostics".into(),
            summary: "Actuator telemetry and motor controls".into(),
            start_index: 3,
            end_index: 4,
            children: vec![],
        })
        .unwrap();

        let results = tree.retrieve_relevant("market risk in finance", 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].node_id, "finance");
    }

    #[test]
    fn retrieve_relevant_empty_query_returns_none() {
        let mut tree = PageIndexTree::new(10);
        tree.insert(make_node("n1", "Any", vec![])).unwrap();
        assert!(tree.retrieve_relevant("   ", 5).is_empty());
    }

    #[test]
    fn json_round_trip() {
        let mut tree = PageIndexTree::new(10);
        tree.insert(make_node("root", "Root Section", vec!["ch1"]))
            .unwrap();
        tree.insert(make_node("ch1", "Chapter 1", vec![])).unwrap();

        let json = tree.to_json().unwrap();
        let restored = PageIndexTree::from_json(&json, 10).unwrap();

        assert_eq!(restored.len(), 2);
        assert_eq!(restored.peek("root").unwrap().title, "Root Section");
        assert_eq!(restored.peek("ch1").unwrap().title, "Chapter 1");
    }

    #[test]
    fn tree_error_display() {
        let e = TreeError::DuplicateNode("001".into());
        assert!(format!("{}", e).contains("duplicate node"));

        let e = TreeError::NodeNotFound("999".into());
        assert!(format!("{}", e).contains("node not found"));

        let e = TreeError::SerializationError("bad json".into());
        assert!(format!("{}", e).contains("serialization error"));
    }

    #[test]
    fn hybrid_query_planner_vector_only_for_short_queries() {
        let page = PageIndexTree::new(4);
        let mut vec_store = VectorStore::new();
        vec_store.insert_with_metadata(
            "a".into(),
            "quick status answer".into(),
            vec![1.0, 0.0],
            ChunkMetadata {
                kind: ChunkKind::ToolDescription,
                source_id: "tool.a".into(),
                tags: vec!["status".into()],
                is_structured: false,
                parent_id: None,
            },
        );
        let engine = HybridMemoryEngine::new(&vec_store, &page, QueryPlannerConfig::default());
        let out = engine.retrieve("status?", &[1.0, 0.0], 2, 2);
        assert_eq!(out.plan, QueryPlan::VectorOnly);
        assert_eq!(out.page_context.len(), 0);
        assert_eq!(out.vector_context.len(), 1);
    }

    #[test]
    fn hybrid_query_planner_deep_dive_for_structured_queries() {
        let mut page = PageIndexTree::new(4);
        page.insert(PageNode {
            node_id: "doc.section.1".into(),
            title: "Section 1".into(),
            summary: "Document chapter with architecture constraints".into(),
            start_index: 1,
            end_index: 2,
            children: vec![],
        })
        .unwrap();
        let mut vec_store = VectorStore::new();
        vec_store.insert("docv".into(), "architecture summary".into(), vec![1.0, 0.0]);
        let engine = HybridMemoryEngine::new(&vec_store, &page, QueryPlannerConfig::default());
        let out = engine.retrieve(
            "find section in this document chapter about constraints",
            &[1.0, 0.0],
            2,
            2,
        );
        assert_eq!(out.plan, QueryPlan::VectorPlusPageIndex);
        assert_eq!(out.vector_context.len(), 1);
        assert_eq!(out.page_context.len(), 1);
    }

    // =====================================================================
    // SessionMemory tests
    // =====================================================================

    #[test]
    fn session_append_chronological_order() {
        let mem = SessionMemory::new(100);
        let sid = uuid::Uuid::new_v4();

        for i in 0..5 {
            mem.append(
                sid,
                Message {
                    role: "user".into(),
                    content: format!("msg {}", i),
                    timestamp_us: i as u64 * 1000,
                },
            )
            .unwrap();
        }

        let history = mem.get_history(&sid).unwrap();
        assert_eq!(history.len(), 5);

        // Verify chronological order
        for (i, msg) in history.iter().enumerate() {
            assert_eq!(msg.content, format!("msg {}", i));
            assert_eq!(msg.timestamp_us, i as u64 * 1000);
        }
    }

    #[test]
    fn session_window_cap_enforcement() {
        let mem = SessionMemory::new(3); // window of 3
        let sid = uuid::Uuid::new_v4();

        for i in 0..5 {
            mem.append(
                sid,
                Message {
                    role: "user".into(),
                    content: format!("msg {}", i),
                    timestamp_us: i as u64,
                },
            )
            .unwrap();
        }

        let history = mem.get_history(&sid).unwrap();
        assert_eq!(history.len(), 3, "should cap at window_size=3");
        // Should keep the 3 most recent: msg 2, 3, 4
        assert_eq!(history[0].content, "msg 2");
        assert_eq!(history[1].content, "msg 3");
        assert_eq!(history[2].content, "msg 4");
    }

    #[test]
    fn session_empty_history_returns_empty() {
        let mem = SessionMemory::new(10);
        let sid = uuid::Uuid::new_v4();
        let history = mem.get_history(&sid).unwrap();
        assert!(history.is_empty());
    }

    #[test]
    fn session_multiple_sessions_independent() {
        let mem = SessionMemory::new(10);
        let s1 = uuid::Uuid::new_v4();
        let s2 = uuid::Uuid::new_v4();

        mem.append(
            s1,
            Message {
                role: "user".into(),
                content: "s1 msg".into(),
                timestamp_us: 1,
            },
        )
        .unwrap();

        mem.append(
            s2,
            Message {
                role: "user".into(),
                content: "s2 msg".into(),
                timestamp_us: 2,
            },
        )
        .unwrap();

        assert_eq!(mem.get_history(&s1).unwrap().len(), 1);
        assert_eq!(mem.get_history(&s2).unwrap().len(), 1);
        assert_eq!(mem.session_count().unwrap(), 2);
    }

    #[test]
    fn session_thread_safety() {
        let mem = SessionMemory::new(1000);
        let sid = uuid::Uuid::new_v4();

        let handles: Vec<_> = (0..4)
            .map(|t| {
                let mem = mem.clone();
                std::thread::spawn(move || {
                    for i in 0..25 {
                        mem.append(
                            sid,
                            Message {
                                role: format!("thread-{}", t),
                                content: format!("msg-{}-{}", t, i),
                                timestamp_us: (t * 100 + i) as u64,
                            },
                        )
                        .unwrap();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().unwrap();
        }

        let history = mem.get_history(&sid).unwrap();
        assert_eq!(history.len(), 100, "4 threads × 25 messages = 100");
    }

    #[test]
    fn session_summarization_replaces_history_when_over_threshold() {
        let mem = SessionMemory::new(100);
        let sid = uuid::Uuid::new_v4();
        for i in 0..5 {
            mem.append(
                sid,
                Message {
                    role: "user".into(),
                    content: format!("msg {}", i),
                    timestamp_us: i,
                },
            )
            .unwrap();
        }
        let condensed = mem
            .summarize_if_over_threshold(sid, 3, 99, |msgs| {
                format!("Summary of {} messages", msgs.len())
            })
            .unwrap();
        assert!(condensed, "should have summarized");
        let hist = mem.get_history(&sid).unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].role, "summary");
        assert!(hist[0].content.contains("5 messages"));
    }

    #[test]
    fn session_summarization_skips_when_under_threshold() {
        let mem = SessionMemory::new(100);
        let sid = uuid::Uuid::new_v4();
        for i in 0..3 {
            mem.append(
                sid,
                Message {
                    role: "user".into(),
                    content: format!("msg {}", i),
                    timestamp_us: i,
                },
            )
            .unwrap();
        }
        let condensed = mem
            .summarize_if_over_threshold(sid, 10, 0, |_| "Summary".into())
            .unwrap();
        assert!(!condensed, "should not summarize when under threshold");
        assert_eq!(mem.get_history(&sid).unwrap().len(), 3);
    }

    #[test]
    fn user_preferences_set_get_remove() {
        let prefs = UserPreferences::new();
        prefs.set("theme", "dark").unwrap();
        prefs.set("lang", "en").unwrap();
        assert_eq!(prefs.get("theme"), Some("dark".into()));
        prefs.remove("theme").unwrap();
        assert_eq!(prefs.get("theme"), None);
        assert_eq!(prefs.get("lang"), Some("en".into()));
    }

    #[test]
    fn user_preferences_persistence_round_trip() {
        let prefs = UserPreferences::new();
        prefs.set("model", "llama3").unwrap();
        prefs.set("voice", "off").unwrap();

        let tmp_path =
            std::env::temp_dir().join(format!("aria_prefs_{}.json", uuid::Uuid::new_v4()));
        prefs.save(&tmp_path).unwrap();

        let restored = UserPreferences::new();
        assert!(restored.load(&tmp_path).unwrap());
        assert_eq!(restored.get("model"), Some("llama3".into()));
        assert_eq!(restored.get("voice"), Some("off".into()));

        std::fs::remove_file(&tmp_path).ok();
    }

    #[test]
    fn user_preferences_corrupted_file_recovers_to_empty() {
        let tmp_path =
            std::env::temp_dir().join(format!("aria_prefs_bad_{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&tmp_path, b"{{corrupted}}").unwrap();

        let prefs = UserPreferences::new();
        assert!(prefs.load(&tmp_path).is_ok());
        assert!(prefs.get("anything").is_none());
        std::fs::remove_file(&tmp_path).ok();
    }

    #[test]
    fn session_persistence_round_trip() {
        let mem = SessionMemory::new(10);
        let sid = uuid::Uuid::new_v4();
        mem.append(
            sid,
            Message {
                role: "user".into(),
                content: "hello persistence".into(),
                timestamp_us: 1,
            },
        )
        .unwrap();

        let test_dir =
            std::env::temp_dir().join(format!("aria_ssmu_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let written = mem.save_to_dir(&test_dir).unwrap();
        assert_eq!(written, 1);

        let restored = SessionMemory::new(10);
        let report = restored.load_from_dir(&test_dir).unwrap();
        assert_eq!(report.loaded_sessions, 1);
        assert_eq!(report.skipped_files, 0);
        let history = restored.get_history(&sid).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "hello persistence");

        std::fs::remove_dir_all(&test_dir).unwrap();
    }
}
