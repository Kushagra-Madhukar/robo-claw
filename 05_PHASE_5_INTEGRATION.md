# Phase 5: E2E Integration & Deployment

## Objective
Wire all the crates together into the final `orchestrator` binary and verify end-to-end functionality. 

## Rules for LLM
- The final binary must correctly initialize the Zenoh session, the LLM backend (easiest to test with a mock backend or local Ollama), and the Cedar Engine.
- Write a high-level integration test that traces a request from the Gateway to the Wasm Executor and back.

---

## 1. `aria-x/nodes/orchestrator` (The Binary)

### Components to Implement:
- `main.rs`: Read TOML configuration.
- Wire dependencies: Gateway (CLI) -> Semantic Router -> Agent Orchestrator -> Cedar Policy -> Wasm/Mesh.
- Graceful shutdown on `SIGINT`.

### Testing Requirements (E2E):
1. **Full Pipeline Test (Mocked):**
   - Inject an `AgentRequest` via the CLI adapter (`"List contents of workspace"`).
   - Ensure the Semantic Router picks the `developer` agent.
   - Ensure the `developer` agent calls `list_directory`.
   - Ensure Cedar allows it (assuming default workspace policy).
   - Ensure the Wasm module executes and returns "file1.txt, file2.rs".
   - Ensure the final output is printed to the console.

---

## 2. Documentation and Cleanup

### Requirements:
- Run `cargo fmt` and `cargo clippy -- -D warnings`.
- Ensure a `README.md` exists detailing how to spin up multiple ARIA-X nodes (Orchestrator + Relay) using standard `cargo run` commands over Zenoh.
