use core::f32;
use serde::{Deserialize, Serialize};
use tracing::debug;

// ---------------------------------------------------------------------------
// Tantivy-backed keyword index for BM25 sparse search
// ---------------------------------------------------------------------------

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, STORED, TEXT};
use tantivy::{doc, Index, IndexWriter, ReloadPolicy};

/// In-memory BM25 keyword index backed by Tantivy.
///
/// Documents are indexed by `(doc_id, content)`. Searches return ranked
/// doc IDs scored by BM25 relevance.
#[allow(dead_code)]
pub struct KeywordIndex {
    index: Index,
    schema: Schema,
    doc_id_field: tantivy::schema::Field,
    content_field: tantivy::schema::Field,
}

impl KeywordIndex {
    /// Create a new in-memory keyword index.
    pub fn new() -> Result<Self, String> {
        let mut schema_builder = Schema::builder();
        let doc_id_field = schema_builder.add_text_field("doc_id", TEXT | STORED);
        let content_field = schema_builder.add_text_field("content", TEXT | STORED);
        let schema = schema_builder.build();

        let index = Index::create_in_ram(schema.clone());

        Ok(Self {
            index,
            schema,
            doc_id_field,
            content_field,
        })
    }

    /// Index a document for keyword search.
    pub fn add_document(&self, doc_id: &str, content: &str) -> Result<(), String> {
        let mut writer: IndexWriter = self
            .index
            .writer(15_000_000) // 15MB heap for indexing
            .map_err(|e| format!("tantivy writer: {}", e))?;

        writer
            .add_document(doc!(
                self.doc_id_field => doc_id,
                self.content_field => content,
            ))
            .map_err(|e| format!("tantivy add_document: {}", e))?;

        writer
            .commit()
            .map_err(|e| format!("tantivy commit: {}", e))?;
        Ok(())
    }

    /// Bulk-index multiple documents in a single commit.
    pub fn add_documents_batch(&self, docs: &[(String, String)]) -> Result<(), String> {
        let mut writer: IndexWriter = self
            .index
            .writer(15_000_000)
            .map_err(|e| format!("tantivy writer: {}", e))?;

        for (doc_id, content) in docs {
            writer
                .add_document(doc!(
                    self.doc_id_field => doc_id.as_str(),
                    self.content_field => content.as_str(),
                ))
                .map_err(|e| format!("tantivy add_document: {}", e))?;
        }

        writer
            .commit()
            .map_err(|e| format!("tantivy commit: {}", e))?;
        Ok(())
    }

    /// Search the keyword index using BM25 ranking.
    /// Returns `(doc_id, score)` pairs sorted by relevance.
    pub fn search(&self, query_text: &str, top_k: usize) -> Result<Vec<(String, f32)>, String> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .map_err(|e| format!("tantivy reader: {}", e))?;

        let searcher = reader.searcher();
        let query_parser = QueryParser::for_index(&self.index, vec![self.content_field]);

        // Escape special characters in user query to prevent parse errors
        let sanitized = query_text.replace([':', '(', ')', '[', ']', '{', '}', '"', '\\'], " ");

        let query = query_parser
            .parse_query(&sanitized)
            .map_err(|e| format!("tantivy parse: {}", e))?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(top_k))
            .map_err(|e| format!("tantivy search: {}", e))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (score, doc_address) in top_docs {
            let retrieved: tantivy::TantivyDocument = searcher
                .doc(doc_address)
                .map_err(|e| format!("tantivy doc: {}", e))?;

            if let Some(tantivy::schema::OwnedValue::Str(id)) =
                retrieved.get_first(self.doc_id_field)
            {
                results.push((id.to_string(), score));
            }
        }

        Ok(results)
    }
}

// ---------------------------------------------------------------------------
// Hybrid search result (merged cosine + BM25 via RRF)
// ---------------------------------------------------------------------------

/// A search result from hybrid vector + keyword search, merged via RRF.
#[derive(Debug, Clone, PartialEq)]
pub struct HybridSearchResult {
    /// The document ID.
    pub id: String,
    /// The document content.
    pub content: String,
    /// The fused RRF score (higher = more relevant).
    pub rrf_score: f32,
    /// The cosine similarity score (0 if not matched by vector search).
    pub vector_score: f32,
    /// The BM25 keyword score (0 if not matched by keyword search).
    pub keyword_score: f32,
    /// Metadata of the document.
    pub metadata: ChunkMetadata,
}

/// Merge two ranked lists via Reciprocal Rank Fusion.
///
/// `rrf_score(d) = Σ 1/(k + rank_i(d))` where `k` is a smoothing constant
/// (typically 60) and `rank_i` is the 1-based rank in list `i`.
pub fn reciprocal_rank_fusion(
    vector_results: &[(f32, String)],  // (score, doc_id) from cosine
    keyword_results: &[(String, f32)], // (doc_id, score) from BM25
    k: f32,
) -> Vec<(String, f32, f32, f32)> {
    // doc_id -> (rrf_score, vector_score, keyword_score)
    let mut scores: std::collections::HashMap<String, (f32, f32, f32)> =
        std::collections::HashMap::new();

    // Process vector results (already sorted by score desc)
    for (rank_0, (vscore, doc_id)) in vector_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(doc_id.clone()).or_insert((0.0, 0.0, 0.0));
        entry.0 += 1.0 / (k + rank);
        entry.1 = *vscore;
    }

    // Process keyword results (already sorted by score desc)
    for (rank_0, (doc_id, kscore)) in keyword_results.iter().enumerate() {
        let rank = (rank_0 + 1) as f32;
        let entry = scores.entry(doc_id.clone()).or_insert((0.0, 0.0, 0.0));
        entry.0 += 1.0 / (k + rank);
        entry.2 = *kscore;
    }

    // Sort by RRF score descending
    let mut merged: Vec<(String, f32, f32, f32)> = scores
        .into_iter()
        .map(|(id, (rrf, vs, ks))| (id, rrf, vs, ks))
        .collect();
    merged.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    merged
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ChunkKind {
    Document,
    SessionSummary,
    ToolDescription,
    SensorAnnotation,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChunkMetadata {
    pub kind: ChunkKind,
    pub source_id: String,
    pub tags: Vec<String>,
    pub is_structured: bool,
    /// If this is a micro-chunk, holds the ID of its parent document chunk.
    #[serde(default)]
    pub parent_id: Option<String>,
}

impl Default for ChunkMetadata {
    fn default() -> Self {
        Self {
            kind: ChunkKind::Other,
            source_id: String::new(),
            tags: Vec::new(),
            is_structured: false,
            parent_id: None,
        }
    }
}

impl ChunkMetadata {
    pub fn for_document(
        source_id: impl Into<String>,
        tags: Vec<String>,
        is_structured: bool,
    ) -> Self {
        Self {
            kind: ChunkKind::Document,
            source_id: source_id.into(),
            tags,
            is_structured,
            parent_id: None,
        }
    }

    pub fn for_session_summary(session_id: impl Into<String>, tags: Vec<String>) -> Self {
        Self {
            kind: ChunkKind::SessionSummary,
            source_id: session_id.into(),
            tags,
            is_structured: false,
            parent_id: None,
        }
    }

    pub fn for_tool(tool_name: impl Into<String>, tags: Vec<String>) -> Self {
        Self {
            kind: ChunkKind::ToolDescription,
            source_id: tool_name.into(),
            tags,
            is_structured: false,
            parent_id: None,
        }
    }

    pub fn for_sensor_annotation(sensor_id: impl Into<String>, tags: Vec<String>) -> Self {
        Self {
            kind: ChunkKind::SensorAnnotation,
            source_id: sensor_id.into(),
            tags,
            is_structured: false,
            parent_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorDoc {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub metadata: ChunkMetadata,
}

#[derive(Debug)]
pub enum VectorError {
    DimensionMismatch,
    NotFound,
}

impl std::fmt::Display for VectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VectorError::DimensionMismatch => write!(f, "dimension mismatch in embedding search"),
            VectorError::NotFound => write!(f, "document not found in vector store"),
        }
    }
}

impl std::error::Error for VectorError {}

/// A simple in-memory vector store matching exact similarity searches
/// using cosine. Fulfills the "embedded" RAG component requirement dynamically
/// decoupled from internal index tree formats.
#[derive(Serialize, Deserialize)]
pub struct VectorStore {
    docs: Vec<VectorDoc>,
}

impl VectorStore {
    /// Serialize the store to a JSON string.
    pub fn to_json(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| format!("serialize vector store: {}", e))
    }

    /// Deserialize a store from a JSON string.
    /// On corrupt input, returns an empty store (startup recovery).
    pub fn from_json_or_empty(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_else(|_| Self::new())
    }

    /// Number of indexed documents.
    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn docs(&self) -> &[VectorDoc] {
        &self.docs
    }
}

#[allow(clippy::new_without_default)]
impl VectorStore {
    pub fn new() -> Self {
        Self { docs: Vec::new() }
    }

    pub fn insert(&mut self, id: String, content: String, embedding: Vec<f32>) {
        self.insert_with_metadata(id, content, embedding, ChunkMetadata::default());
    }

    pub fn insert_with_metadata(
        &mut self,
        id: String,
        content: String,
        embedding: Vec<f32>,
        metadata: ChunkMetadata,
    ) {
        self.docs.push(VectorDoc {
            id,
            content,
            embedding,
            metadata,
        });
    }

    /// Index a document chunk with metadata.
    pub fn index_document(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        embedding: Vec<f32>,
        source_id: impl Into<String>,
        tags: Vec<String>,
        is_structured: bool,
    ) {
        self.insert_with_metadata(
            id.into(),
            content.into(),
            embedding,
            ChunkMetadata::for_document(source_id, tags, is_structured),
        );
    }

    /// Index a session summary chunk with metadata.
    pub fn index_session_summary(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        embedding: Vec<f32>,
        session_id: impl Into<String>,
        tags: Vec<String>,
    ) {
        self.insert_with_metadata(
            id.into(),
            content.into(),
            embedding,
            ChunkMetadata::for_session_summary(session_id, tags),
        );
    }

    /// Index a tool description with metadata.
    pub fn index_tool_description(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        embedding: Vec<f32>,
        tool_name: impl Into<String>,
        tags: Vec<String>,
    ) {
        self.insert_with_metadata(
            id.into(),
            content.into(),
            embedding,
            ChunkMetadata::for_tool(tool_name, tags),
        );
    }

    /// Index a sensor annotation with metadata.
    pub fn index_sensor_annotation(
        &mut self,
        id: impl Into<String>,
        content: impl Into<String>,
        embedding: Vec<f32>,
        sensor_id: impl Into<String>,
        tags: Vec<String>,
    ) {
        self.insert_with_metadata(
            id.into(),
            content.into(),
            embedding,
            ChunkMetadata::for_sensor_annotation(sensor_id, tags),
        );
    }

    /// Index a document using the parent-child chunking strategy.
    ///
    /// Stores two chunks:
    /// 1. A micro-chunk (`micro_id`, ~75 tokens) with `parent_id` set to `parent_id`.
    /// 2. A full parent-chunk (`parent_id`, ~375 tokens) without a parent.
    ///
    /// When searching with `search_with_parent`, the micro-chunk match will return
    /// the parent chunk so that the LLM sees the broader context.
    #[allow(clippy::too_many_arguments)]
    pub fn index_document_with_parent(
        &mut self,
        micro_id: impl Into<String>,
        micro_content: impl Into<String>,
        micro_embedding: Vec<f32>,
        parent_id: impl Into<String>,
        parent_content: impl Into<String>,
        parent_embedding: Vec<f32>,
        source_id: impl Into<String>,
        tags: Vec<String>,
    ) {
        let parent_id_str = parent_id.into();
        let source_id_str = source_id.into();
        // Store parent chunk first
        self.insert_with_metadata(
            parent_id_str.clone(),
            parent_content.into(),
            parent_embedding,
            ChunkMetadata {
                kind: ChunkKind::Document,
                source_id: source_id_str.clone(),
                tags: tags.clone(),
                is_structured: false,
                parent_id: None,
            },
        );
        // Store micro chunk, referencing parent
        self.insert_with_metadata(
            micro_id.into(),
            micro_content.into(),
            micro_embedding,
            ChunkMetadata {
                kind: ChunkKind::Document,
                source_id: source_id_str,
                tags,
                is_structured: false,
                parent_id: Some(parent_id_str),
            },
        );
    }

    /// Search by cosine similarity, then resolve parent chunks for micro-chunk hits.
    ///
    /// Returns `(score, micro_doc, Option<parent_doc>)` triples. When the matched
    /// doc has a `parent_id`, the parent doc is resolved from the store. Callers
    /// should inject `parent_doc.content` (not `micro_doc.content`) into the LLM
    /// prompt to provide full context.
    pub fn search_with_parent(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Vec<(f32, &VectorDoc, Option<&VectorDoc>)> {
        let id_map: std::collections::HashMap<&str, &VectorDoc> =
            self.docs.iter().map(|d| (d.id.as_str(), d)).collect();

        let hits = self.search(query_embedding, top_k).unwrap_or_default();
        hits.into_iter()
            .map(|(score, doc)| {
                let parent = doc
                    .metadata
                    .parent_id
                    .as_deref()
                    .and_then(|pid| id_map.get(pid).copied());
                (score, doc, parent)
            })
            .collect()
    }

    pub fn search(
        &self,
        query_embedding: &[f32],
        top_k: usize,
    ) -> Result<Vec<(f32, &VectorDoc)>, VectorError> {
        if self.docs.is_empty() {
            return Ok(Vec::new());
        }

        let query_dim = query_embedding.len();
        if self
            .docs
            .first()
            .map(|d| d.embedding.len())
            .unwrap_or(query_dim)
            != query_dim
        {
            return Err(VectorError::DimensionMismatch);
        }

        let mut results: Vec<(f32, &VectorDoc)> = self
            .docs
            .iter()
            .map(|doc| {
                let score = cosine_similarity(query_embedding, &doc.embedding);
                (score, doc)
            })
            .collect();

        // Sort descending by score
        results.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(top_k);

        Ok(results)
    }

    /// Hybrid search combining cosine similarity with BM25 keyword matching,
    /// merged via Reciprocal Rank Fusion (RRF).
    ///
    /// - `query_embedding`: dense vector from the embedding model.
    /// - `keyword_index`: Tantivy-backed keyword index (pass `None` to fallback
    ///   to vector-only search).
    /// - `query_text`: raw user query for BM25 keyword matching.
    /// - `top_k`: maximum number of results to return.
    /// - `min_rrf_score`: minimum RRF score threshold; results below this are
    ///   filtered out to avoid injecting irrelevant context.
    /// - `rrf_k`: RRF smoothing constant (default: 60.0).
    pub fn hybrid_search(
        &self,
        query_embedding: &[f32],
        keyword_index: Option<&KeywordIndex>,
        query_text: &str,
        top_k: usize,
        min_rrf_score: f32,
        rrf_k: f32,
    ) -> Vec<HybridSearchResult> {
        // 1. Vector search (cosine similarity)
        let vector_hits = self
            .search(query_embedding, top_k * 2) // fetch more for fusion
            .unwrap_or_default();

        let vector_for_rrf: Vec<(f32, String)> = vector_hits
            .iter()
            .map(|(score, doc)| (*score, doc.id.clone()))
            .collect();

        // 2. Keyword search (BM25 via Tantivy)
        let keyword_hits = if let Some(kw_index) = keyword_index {
            kw_index.search(query_text, top_k * 2).unwrap_or_default()
        } else {
            Vec::new()
        };

        debug!(
            vector_hits = vector_for_rrf.len(),
            keyword_hits = keyword_hits.len(),
            query = %query_text,
            "hybrid_search: pre-fusion"
        );

        // 3. Reciprocal Rank Fusion
        let fused = reciprocal_rank_fusion(&vector_for_rrf, &keyword_hits, rrf_k);

        // 4. Build results with metadata, filter by threshold, and truncate
        let doc_map: std::collections::HashMap<&str, &VectorDoc> =
            self.docs.iter().map(|d| (d.id.as_str(), d)).collect();

        let mut results: Vec<HybridSearchResult> = fused
            .into_iter()
            .filter(|(_, rrf_score, _, _)| *rrf_score >= min_rrf_score)
            .filter_map(|(id, rrf_score, vs, ks)| {
                doc_map.get(id.as_str()).map(|doc| HybridSearchResult {
                    id: doc.id.clone(),
                    content: doc.content.clone(),
                    rrf_score,
                    vector_score: vs,
                    keyword_score: ks,
                    metadata: doc.metadata.clone(),
                })
            })
            .collect();

        results.truncate(top_k);

        debug!(
            final_results = results.len(),
            top_rrf = results.first().map(|r| r.rrf_score).unwrap_or(0.0),
            "hybrid_search: post-fusion"
        );

        results
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (x, y) in a.iter().zip(b.iter()) {
        dot_product += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a.sqrt() * norm_b.sqrt())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vector_search_top_k() {
        let mut store = VectorStore::new();

        store.insert("A".into(), "doc A".into(), vec![1.0, 0.0, 0.0]);
        store.insert("B".into(), "doc B".into(), vec![0.0, 1.0, 0.0]);
        store.insert("C".into(), "doc C".into(), vec![0.8, 0.2, 0.0]);

        // query matches mostly A and C
        let query = vec![0.9, 0.1, 0.0];
        let results = store.search(&query, 2).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(results[0].1.id, "A");
        // A (1,0,0) dot (0.9,0.1,0) = 0.9. Norm A=1, Norm Q=sqrt(0.82)=0.905 -> Score ~ 0.99
        // C (0.8,0.2,0) dot (0.9,0.1,0) = 0.74. Norm C=sqrt(0.68)=0.82 -> Score ~ 0.98
    }

    #[test]
    fn test_vector_dimension_mismatch() {
        let mut store = VectorStore::new();
        store.insert("A".into(), "doc A".into(), vec![1.0, 0.0]);

        let result = store.search(&[1.0, 0.1, 0.0], 1);
        assert!(result.is_err());
    }

    #[test]
    fn test_vector_store_json_persistence() {
        let mut store = VectorStore::new();
        store.index_document(
            "doc.1",
            "workspace source files",
            vec![1.0, 0.0, 0.0],
            "workspace",
            vec!["files".into()],
            false,
        );
        store.index_tool_description(
            "tool.read_file",
            "Read file contents",
            vec![0.0, 1.0, 0.0],
            "read_file",
            vec![],
        );

        let json = store.to_json().expect("serialize");
        let restored = VectorStore::from_json_or_empty(&json);

        assert_eq!(restored.len(), 2);
        let results = restored.search(&[1.0, 0.0, 0.0], 2).expect("search");
        assert_eq!(results[0].1.id, "doc.1");
        assert_eq!(results[0].1.metadata.kind, ChunkKind::Document);
    }

    #[test]
    fn test_vector_store_corrupted_fallback_to_empty() {
        let recovered = VectorStore::from_json_or_empty("{corrupted!!}");
        assert_eq!(recovered.len(), 0);
    }

    #[test]
    fn test_hybrid_retrieve_via_vector_store() {
        let mut store = VectorStore::new();
        store.index_document("a", "alpha content", vec![1.0, 0.0], "src", vec![], false);
        store.index_document("b", "beta content", vec![0.0, 1.0], "src", vec![], false);

        let results = store.search(&[0.9, 0.1], 1).expect("search");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].1.id, "a");
    }

    #[test]
    fn test_chunk_metadata_indexing() {
        let mut store = VectorStore::new();
        store.index_document(
            "doc.1",
            "chapter content",
            vec![1.0, 0.0],
            "source.pdf",
            vec!["chapter".into()],
            true,
        );
        store.index_session_summary(
            "session.abc",
            "user: hi\nassistant: hello",
            vec![0.5, 0.5],
            "abc",
            vec!["chat".into()],
        );
        store.index_tool_description(
            "tool.read_file",
            "Read file from path",
            vec![0.0, 1.0],
            "read_file",
            vec!["file".into()],
        );
        store.index_sensor_annotation(
            "sensor.imu.1",
            "accel x=0.1",
            vec![0.2, 0.8],
            "imu",
            vec!["telemetry".into()],
        );

        let results = store.search(&[1.0, 0.0], 4).unwrap();
        assert_eq!(results.len(), 4);
        let ids: Vec<_> = results.iter().map(|(_, d)| d.id.as_str()).collect();
        assert!(ids.contains(&"doc.1"));
        assert!(ids.contains(&"session.abc"));
        assert!(ids.contains(&"tool.read_file"));
        assert!(ids.contains(&"sensor.imu.1"));

        let doc1 = results
            .iter()
            .find(|(_, d)| d.id == "doc.1")
            .map(|(_, d)| d)
            .unwrap();
        assert_eq!(doc1.metadata.kind, ChunkKind::Document);
        assert_eq!(doc1.metadata.source_id, "source.pdf");
        assert!(doc1.metadata.is_structured);
    }

    // =========================================================================
    // Item 2: Parent-Child Semantic Chunking
    // =========================================================================

    #[test]
    fn parent_child_both_chunks_indexed() {
        let mut store = VectorStore::new();
        store.index_document_with_parent(
            "micro.1",
            "short snippet of text",
            vec![1.0, 0.0],
            "parent.1",
            "full surrounding paragraph with more context",
            vec![0.95, 0.05],
            "source.txt",
            vec![],
        );
        // Both micro and parent should be in the store
        assert_eq!(store.len(), 2);
        let ids: Vec<&str> = store.docs.iter().map(|d| d.id.as_str()).collect();
        assert!(ids.contains(&"micro.1"));
        assert!(ids.contains(&"parent.1"));
    }

    #[test]
    fn search_with_parent_resolves_parent_on_micro_hit() {
        let mut store = VectorStore::new();
        store.index_document_with_parent(
            "micro.1",
            "short snippet",
            vec![1.0, 0.0],
            "parent.1",
            "full paragraph context",
            vec![0.9, 0.1],
            "source.txt",
            vec![],
        );
        // Query close to micro chunk
        let hits = store.search_with_parent(&[1.0, 0.0], 2);
        // Find the micro chunk hit
        let micro_hit = hits.iter().find(|(_, d, _)| d.id == "micro.1");
        assert!(micro_hit.is_some(), "micro.1 should be in results");
        let (_, micro_doc, parent_doc) = micro_hit.unwrap();
        assert_eq!(micro_doc.id, "micro.1");
        assert!(
            parent_doc.is_some(),
            "parent should be resolved for micro chunk"
        );
        assert_eq!(parent_doc.unwrap().id, "parent.1");
    }

    #[test]
    fn search_with_parent_returns_none_for_top_level_doc() {
        let mut store = VectorStore::new();
        store.index_document(
            "top.1",
            "standalone doc",
            vec![1.0, 0.0],
            "src",
            vec![],
            false,
        );
        let hits = store.search_with_parent(&[1.0, 0.0], 1);
        assert_eq!(hits.len(), 1);
        let (_, doc, parent) = &hits[0];
        assert_eq!(doc.id, "top.1");
        assert!(parent.is_none(), "top-level doc should have no parent");
    }

    #[test]
    fn parent_id_field_set_on_micro_but_not_parent() {
        let mut store = VectorStore::new();
        store.index_document_with_parent(
            "micro.2",
            "micro text",
            vec![0.5, 0.5],
            "parent.2",
            "parent text",
            vec![0.5, 0.5],
            "src",
            vec![],
        );
        let micro = store.docs.iter().find(|d| d.id == "micro.2").unwrap();
        let parent = store.docs.iter().find(|d| d.id == "parent.2").unwrap();
        assert_eq!(micro.metadata.parent_id, Some("parent.2".to_string()));
        assert_eq!(parent.metadata.parent_id, None);
    }
}
