use core::f32;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VectorDoc {
    pub id: String,
    pub content: String,
    pub embedding: Vec<f32>,
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
/// decoupled from PageIndex Tree formats.
pub struct VectorStore {
    docs: Vec<VectorDoc>,
}

#[allow(clippy::new_without_default)]
impl VectorStore {
    pub fn new() -> Self {
        Self { docs: Vec::new() }
    }

    pub fn insert(&mut self, id: String, content: String, embedding: Vec<f32>) {
        self.docs.push(VectorDoc {
            id,
            content,
            embedding,
        });
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
}
