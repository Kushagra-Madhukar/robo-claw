# ARIA-X: Master Implementation Guide

This project builds **ARIA-X** (Adaptive Runtime for Intelligence Agents), a 4-layer, Rust-based, hardware-agnostic AI agent architecture.

This guide is designed to be fed into an LLM. It dictates a strict **Test-Driven Development (TDD)** approach. You must not proceed to the next module until the unit and integration tests for the current module pass.

## Architecture Overview
ARIA-X consists of 4 layers:
1. **L4 (Mesh Transport):** Zenoh-based UDP/QUIC pub/sub mesh.
2. **L3 (Skill Runtime):** Extism/Wasmtime/WAMR sandboxed WebAssembly execution + Cedar Policy Engine for AST zero-trust evaluation.
3. **L2 (Intelligence):** BGE-m3 Semantic Router, Dynamic Tool Cache, and SSMU (PageIndex + Vector RAG).
4. **L1 (Gateway):** Ingestion adapters (Telegram, WebSocket, etc.) normalizing to `AgentRequest`.

## Implementation Methodology (LLM Instructions)
For every step in the split-down guides:
1. **Write the Tests First:** Define the expected behavior, structs, and boundary conditions in `#[cfg(test)]` modules.
2. **Implement the Core Logic:** Write the bare minimum Rust code to make the tests pass.
3. **Enforce Safety:** Ensure no `unwrap()` or `panic!()` exists in production paths. Use `Result` and custom error types.
4. **Compile & Verify:** Run `cargo test` for the specific crate.

---

## The Build Sequence
To execute this build, feed the following markdown files to the LLM sequentially:

1. **[Phase 1: Core Types & L4 Mesh Transport](./01_PHASE_1_CORE_AND_MESH.md)**
2. **[Phase 2: L3 Skill Runtime & Cedar Policy Engine](./02_PHASE_2_SKILL_RUNTIME.md)**
3. **[Phase 3: SSMU RAG & L2 Intelligence (Semantic Router)](./03_PHASE_3_INTELLIGENCE.md)**
4. **[Phase 4: Orchestrator Loop & L1 Gateway](./04_PHASE_4_GATEWAY_AND_ORCHESTRATOR.md)**
5. **[Phase 5: E2E Integration & Deployment](./05_PHASE_5_INTEGRATION.md)**

Start with `01_PHASE_1_CORE_AND_MESH.md`.
