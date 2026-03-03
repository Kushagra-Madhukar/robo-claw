# Phase 3: SSMU RAG & L2 Intelligence

## Objective
Build the memory engine (PageIndex + Vector Search) and the zero-token Semantic Router using BGE-m3 embeddings.

## Rules for LLM
- The Semantic Router must not make external network calls; use a local ONNX runtime or `candle-rs` for computing embeddings.
- Ensure the `DynamicToolCache` strictly enforces the context cap.

---

## 1. `aria-ssmu` (RAG Engine)

### Components to Implement:
- **`PageIndexTree`:** A hierarchical graph structure for JSON table-of-contents navigation. read more at https://github.com/VectifyAI/PageIndex and https://pageindex.ai/developer
- **`VectorStore`:** Wrap `qdrant-client` or embedded `usearch-rs`.
- **`SessionMemory`:** Thread-safe structure holding the last `N` messages of a UUID session.

### Testing Requirements (TDD):
1. **LRU Eviction Test:** Insert 33 nodes into the PageIndexTree (cap is 32). Verify the oldest node is written to disk and removed from RAM.
2. **Session Append:** Append 5 messages to a session, verify they are stored chronologically.

---

## 2. `aria-intelligence` (Routing & Tool Cache)

### Components to Implement:
- **`SemanticRouter`:** Load pre-computed agent embeddings into a `RouterIndex`. Implement cosine similarity.
- **`DynamicToolCache`:** LRU queue of `ToolDefinition`.

### Testing Requirements (TDD):
1. **Routing Test:** Given two agents ("financial_analyst" and "robot_controller"), verify that the phrase "buy AAPL stock" scores higher for the analyst.
2. **Cache Eviction Test:** Add 9 tools to a cache with a `context_cap` of 8. Verify the 1st tool is evicted.
3. **Ceiling Test:** Add 16 unique tools to a cache with a `session_ceiling` of 15. Verify it returns a `CeilingReached` error.
