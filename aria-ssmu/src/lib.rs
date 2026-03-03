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

use serde::{Deserialize, Serialize};

#[cfg(feature = "vector-store")]
pub mod vector;

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

/// Thread-safe, per-session message history.
///
/// Internally uses `Arc<RwLock<HashMap<Uuid, VecDeque<Message>>>>`.
/// Each session maintains a sliding window of the most recent `window_size`
/// messages.
#[derive(Clone)]
pub struct SessionMemory {
    window_size: usize,
    store: Arc<RwLock<HashMap<uuid::Uuid, VecDeque<Message>>>>,
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

        let history = store.entry(session_id).or_insert_with(VecDeque::new);
        history.push_back(message);

        if history.len() > self.window_size {
            history.pop_front();
        }

        Ok(())
    }

    /// Get the message history for a session, in chronological order.
    pub fn get_history(&self, session_id: &uuid::Uuid) -> Result<Vec<Message>, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;

        Ok(store
            .get(session_id)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default())
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> Result<usize, String> {
        let store = self
            .store
            .read()
            .map_err(|e| format!("lock poisoned: {}", e))?;
        Ok(store.len())
    }
}

// ---------------------------------------------------------------------------
// Tests — TDD
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
}
