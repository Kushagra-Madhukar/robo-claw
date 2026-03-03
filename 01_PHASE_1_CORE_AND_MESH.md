# Phase 1: Core Types & L4 Mesh Transport

## Objective
Establish the foundational data structures used across all layers and implement the Zenoh-based mesh networking layer for inter-node communication.

## Rules for LLM
- Implement `aria-core` first. It must compile with `#![no_std]` (with `alloc`).
- Write unit tests for serialization/deserialization of core types.
- Implement `aria-mesh` second. It depends on `aria-core`.

---

## 1. `aria-core` (The Foundation)

### Structs to Implement:
- `AgentRequest`: `request_id` (UUID), `session_id` (UUID), `user_id` (String), `content` (String), `timestamp_us` (u64).
- `AgentResponse`: `request_id` (UUID), `content` (String), `skill_trace` (Vec<String>).
- `ToolDefinition`: `name` (String), `description` (String), `parameters` (JSON Schema), `embedding` (Vec<f32>).
- `HardwareIntent`: `intent_id` (u32), `motor_id` (u8), `target_velocity` (f32).

### Testing Requirements:
1. Verify `HardwareIntent` can be serialized/deserialized without allocation errors in a `no_std` context (mocked).
2. Validate UUID generation formats for requests.

---

## 2. `aria-mesh` (L4 Transport)

### Components to Implement:
- **`ZenohRouter`:** Wraps a `zenoh::Session`.
- **Publisher:** Implement methods to publish to `aria/gateway/{channel}/inbound` and `aria/skill/{node}/call/{skill}`.
- **Subscriber:** Create an async stream listener for incoming MCP tool results on `aria/skill/+/result`.

### Testing Requirements (TDD):
1. **Integration Test:** Spin up an in-memory Zenoh router. Publish an `AgentRequest` on node A and verify it is received exactly once on node B.
2. **Failure Handling:** Test network partition simulation (timeout on wait).
