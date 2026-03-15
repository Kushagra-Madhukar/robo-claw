# Architecture Backlog

This file tracks deferred architectural work that is intentionally not in the current active implementation plan.

Rules:
- Items are indexed and append-only.
- Each item should state why it is deferred, what problem it solves, expected blast radius, and what would justify picking it up.
- Backlog items are not active commitments. They are candidates for future planning.

## 1. True PageIndex Retrieval Mode

Status: Deferred

Priority: Backlog

Summary:
- Add a true PageIndex-style retrieval mode for long, structured document corpora.
- This is not a rename or a documentation cleanup task. It means implementing actual PageIndex-like document-tree generation and retrieval behavior, then routing into it selectively where it outperforms current hybrid RAG.

Why Deferred:
- Current hybrid retrieval is already working and remains the correct default for most ARIA workflows.
- True PageIndex adds significant complexity and should only be taken on if long structured-document analysis becomes a first-class product requirement.
- The effort is substantial enough that it needs its own benchmark and acceptance track, not opportunistic incremental edits.

Why It Might Be Worth Doing:
- Better retrieval quality for large structured documents such as PDFs, manuals, filings, legal/regulatory documents, and long reports.
- Better section-aware provenance and explainability than plain chunk-based retrieval.
- Better fit for queries that require navigating document hierarchy instead of nearest-neighbor chunk lookup.

Why It Should Not Replace Current RAG:
- Session memory, control documents, web extracts, tool instructions, and short/medium unstructured corpora are still better served by the current hybrid RAG path.
- PageIndex should be an additional retrieval mode, not a platform-wide replacement.

Target Architecture:
- Keep current hybrid RAG as the default retrieval engine.
- Add a structured-document retrieval mode based on:
  - document tree generation
  - stable node ids
  - parent/child section hierarchy
  - section summaries
  - document-span metadata
- Add retrieval routing with three modes:
  - hybrid RAG
  - PageIndex/document-tree retrieval
  - hybrid + PageIndex combined

Expected Crates Affected:
- `aria-ssmu`
  - document tree model
  - tree persistence
  - tree traversal and retrieval
  - hybrid document-tree retrieval mode
- `aria-core`
  - retrieval mode enums
  - tree metadata types
  - retrieval trace fields
- `aria-x`
  - ingestion/update hooks
  - retrieval planner/router integration
  - operator inspection/debug views
- `aria-intelligence`
  - optional traversal-step prompt/context shaping if LLM-guided tree search is used
- `aria-learning`
  - benchmark/eval harness and retrieval-quality comparisons

Expected Scope:
- Estimated implementation size: medium-to-large
- Estimated code change: roughly 1,500 to 3,500 lines for a production-quality version
- Estimated implementation style: separate phased project, not opportunistic maintenance work

Implementation Outline:
1. Add document-tree generation for structured local documents.
2. Persist and version document trees.
3. Add retrieval router that chooses between hybrid RAG and PageIndex mode based on corpus and request shape.
4. Add PageIndex-only and hybrid+PageIndex retrieval execution paths.
5. Add operator/debug surfaces for tree inspection and retrieval provenance.
6. Add benchmark harness comparing:
   - hybrid RAG only
   - PageIndex only
   - hybrid + PageIndex

Benchmark Criteria:
- answer accuracy
- retrieval relevance
- section/page provenance correctness
- latency
- token cost
- tree generation/update cost
- behavior on large structured documents versus unstructured corpora

Acceptance Criteria For Picking This Up:
- Long structured-document analysis becomes an explicit product priority.
- There is a real target corpus set where current hybrid retrieval is measurably insufficient.
- We commit to building both the implementation and the benchmark harness together.

Primary Decision:
- Do not replace current RAG with PageIndex.
- If implemented later, PageIndex should be introduced as a selective retrieval mode for structured long-document workflows only.
