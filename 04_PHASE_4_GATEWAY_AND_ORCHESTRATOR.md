# Phase 4: Orchestrator Loop & L1 Gateway

## Objective
Implement the main ReAct (Reasoning and Acting) Agent Orchestrator loop, and normalize inbound signals through the Gateway adapters.

## Rules for LLM
- The Orchestrator must support multiple tool calls per turn (parallel tool execution).
- The LLM Backend must use a generic Trait, enabling swapping between Claude, Ollama, and `llama.cpp`.

---

## 1. `aria-intelligence` (Orchestrator Loop)

### Components to Implement:
- **`LLMBackend` Trait:** `async fn query(&self, prompt: &str, tools: &[ToolDefinition]) -> Result<LLMResponse, Error>`
- **`AgentOrchestrator`:**
  1. Receives `AgentRequest`.
  2. Queries `SemanticRouter` for agent ID.
  3. Loads `AgentConfig`.
  4. Retrieves history from SSMU.
  5. Initializes `DynamicToolCache`.
  6. Enters ReAct loop (`max_tool_rounds`).
  7. Evaluates tool calls via `CedarEngine`.
  8. Executes via `aria-mesh` (Zenoh) or local `WasmExecutor`.

### Testing Requirements (TDD):
1. **Mock LLM Valid Loop:** Provide a mock LLM that first returns a tool call `read_file`, and on the second loop iteration returns a final text answer. Verify the Orchestrator executes both rounds and outputs the final text.
2. **Infinite Loop Prevention:** Provide a mock LLM that returns tool calls 6 times. If `max_tool_rounds` is 5, verify the Orchestrator aborts and returns an error.

---

## 2. `aria-gateway` (L1 Normalization)

### Components to Implement:
- **`GatewayAdapter` Trait:** `async fn receive(&self) -> Result<AgentRequest, Error>`
- **CLI Adapter:** Read `stdin`, construct `AgentRequest`, push to inbound queue. Wait for `AgentResponse`.

### Testing Requirements (TDD):
1. **Normalization:** Input a mock Telegram JSON payload. Verify it correctly maps to the internal `AgentRequest` struct with channel metadata stripped.
