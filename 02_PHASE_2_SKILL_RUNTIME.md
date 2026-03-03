# Phase 2: L3 Skill Runtime & Cedar Policy Engine

## Objective
Implement strict security policies for tool execution and wrap the Extism/Wasmtime WebAssembly runtime to execute dynamic skills safely.

## Rules for LLM
- Tests must prove that a Wasm module *cannot* access the host filesystem unless strictly authorized.
- Cedar policy evaluation must block unauthorized actions before Wasm instantiation.

---

## 1. `aria-policy` (Cedar Engine)

### Components to Implement:
- **`CedarEvaluator`:** Loads `policies/default.cedar`.
- **AST Parser:** Convert an LLM string (e.g., `read_sensor(node="relay_01")`) into a Cedar `Request`.
- **Entities:** `Principal` (Agent ID), `Action` (Tool Name), `Resource` (Target Node/File).

### Testing Requirements (TDD):
1. Test that `Agent::"developer"` calling `Action::"read_file"` on `Resource::"/etc/shadow"` returns `Decision::Deny`.
2. Test that the same agent reading `Resource::"/workspace/main.rs"` returns `Decision::Allow`.

---

## 2. `aria-skill-runtime` (Wasm Executor)

### Components to Implement:
- **`WasmExecutor` Trait:** `fn execute(&self, module: &[u8], input: &str) -> Result<String, Error>`
- **`ExtismBackend`:** Implement the trait using the `extism` Rust crate. Set strict capability scopes (e.g., memory limits).

### Testing Requirements (TDD):
1. Compile a dummy Rust function to `wasm32-unknown-unknown` that returns "hello". Assert `execute` returns "hello".
2. **Security Test:** Compile a Wasm module that attempts to open a local file. Assert that `execute` panics or returns a specific capabilities error.
