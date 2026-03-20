# Provider Tool-Call Implementation TODO

This document is the execution companion to:

- [PROVIDER_TOOL_CALL_ARCHITECTURE_PLAN.md](/Users/kushagramadhukar/coding/anima/docs/PROVIDER_TOOL_CALL_ARCHITECTURE_PLAN.md)

It converts the architecture recommendation into concrete implementation work for ARIA.

## Goal

Build one provider-agnostic internal tool/runtime contract and adapt it correctly for:

- OpenAI
- Anthropic Claude
- Gemini
- OpenAI-compatible providers such as OpenRouter
- Ollama-native local models

The implementation must:

- remove prompt/schema duplication on native-tool providers
- keep MCP as an integration subsystem, not the universal wire protocol
- enforce deterministic tool-loop behavior
- make provider payloads inspectable

## Non-Goals

- Do not rewrite the entire LLM stack in one pass.
- Do not make MCP the mandatory transport for all providers.
- Do not hardcode prompt-specific or tool-specific string heuristics to compensate for broken architecture.

## Execution Order

Follow this order strictly. Later phases depend on earlier ones.

1. `P0` canonical contracts
2. `P1` context assembly split
3. `P2` provider adapter parity
4. `P3` tool-choice policy engine
5. `P4` MCP normalization boundary
6. `P5` inspection and observability
7. `P6` cleanup and rollout hardening

## Progress Snapshot (March 13, 2026)

- Completed:
  - provider payload inspection is implemented in OpenAI, Anthropic, and Gemini backends
  - context inspection persistence includes provider request payload in runtime tests
  - top-level operator subcommands added:
    - `aria-x inspect context ...`
    - `aria-x inspect provider-payloads ...`
    - `aria-x explain context ...`
    - `aria-x explain provider-payloads ...`
  - singular alias support added:
    - `provider-payload` (inspect/explain)
  - help text and shell completions updated for inspect/explain subcommands

- In progress:
  - remaining cleanup/debt removal under `P6`
  - broader live-provider validation matrix refresh after latest routing/tooling changes

## P0: Canonical Contracts

### Objective

Define one internal tool and tool-result contract that all providers and runtimes use.

### Changes

#### `aria-core`

Files:

- [aria-core/src/app.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/app.rs)
- [aria-core/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/runtime.rs)
- [aria-core/src/lib.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/lib.rs)
- [aria-core/src/tests.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/tests.rs)

Add:

- `CanonicalToolSpec`
- `ToolExecutionKind`
  - `Native`
  - `Skill`
  - `McpImported`
  - `ProviderBuiltIn`
- `CanonicalToolSchema`
- `CanonicalToolResultEnvelope`
- `ToolApprovalClass`
- `ToolSideEffectLevel`
- `ProviderCompatibilityHints`
- `ToolSelectionDecision`
- `ToolInvocationEnvelope`
- `ToolResultEnvelope`

Required fields:

- stable tool id
- user-facing name
- provider-facing name
- long description
- short description
- canonical parameters schema
- canonical result contract
- modality requirements
- approval requirements
- side-effect class
- parallel-safety
- streaming-safety
- provider compatibility hints

#### `aria-intelligence`

Files:

- [aria-intelligence/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/tools.rs)
- [aria-intelligence/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/runtime.rs)

Refactor:

- move runtime decisions off ad hoc `CachedTool` behavior where possible
- introduce conversion from existing `CachedTool` to `CanonicalToolSpec`

### Tests

- schema round-trip
- result envelope round-trip
- canonical tool compatibility checks
- provider compatibility hints parsing

### Exit Criteria

- all tool execution and provider adapters can depend on one internal tool contract
- no new provider logic depends on raw prompt text conventions

## P1: Context Assembly Split

### Objective

Stop flattening tool instructions, RAG, history, and request into one undifferentiated string before provider shaping.

### Changes

#### `aria-core`

Files:

- [aria-core/src/app.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/app.rs)

Extend or refine:

- `ExecutionContextPack`
- `ContextBlock`
- `ContextBlockKind`
- `PromptContextMessage`

Add explicit sections for:

- `system_sections`
- `control_document_sections`
- `recent_history_messages`
- `compacted_memory_sections`
- `retrieval_evidence_sections`
- `page_index_sections`
- `tool_policy_sections`
- `tool_history_sections`
- `current_user_request`

#### `aria-intelligence`

Files:

- [aria-intelligence/src/prompting.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/prompting.rs)
- [aria-intelligence/src/orchestrator.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/orchestrator.rs)

Add:

- `ContextAssembler`
- strict separation between:
  - context assembly
  - provider message shaping
  - plain-text fallback rendering

Refactor:

- remove full tool-schema dumps from prompt text for native-tool providers
- keep tool prompt text only for:
  - text fallback
  - repair mode
  - explicit provider-specific descriptive guidance

#### `aria-x`

Files:

- [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs)

Refactor:

- produce typed retrieval/context sections instead of one generic `rag_context` blob
- separate:
  - live session history
  - compacted memory
  - session retrieval
  - workspace retrieval
  - control-doc retrieval
  - page-index hints

### Tests

- native-tool path omits text schema duplication
- text fallback path still renders textual tool contract
- context sections are preserved in inspection
- history and retrieval remain distinct in final provider payload

### Exit Criteria

- provider backends consume structured context, not a giant prompt string

## P2: Provider Adapter Parity

### Objective

Make every provider adapter own its actual schema translation and message flow.

### Changes

#### Shared adapter layer

Files:

- [aria-intelligence/src/backends/mod.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/mod.rs)

Add or harden:

- provider-specific `translate_tool_schema`
- provider-specific `translate_tool_definition`
- provider-specific `build_initial_messages`
- provider-specific `build_tool_result_messages`
- provider-specific `parse_tool_calls`
- provider-specific `tool choice` mapping

Add explicit adapter capabilities:

- `supports_parallel_tool_calls`
- `supports_allowed_tool_lists`
- `supports_strict_schema`
- `supports_tool_result_blocks`
- `supports_remote_mcp`

#### OpenAI-compatible

Files:

- [aria-intelligence/src/backends/openai.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/openai.rs)
- [aria-intelligence/src/backends/openrouter.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/openrouter.rs)

Work:

- preserve strict schemas
- preserve `call_id`
- preserve `function_call_output`
- narrow tools using allowed-tools and specific-tool forcing

#### Anthropic

Files:

- [aria-intelligence/src/backends/anthropic.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/anthropic.rs)

Work:

- map canonical tool spec to `tools + input_schema`
- preserve descriptive tool text
- keep block-based `tool_use` / `tool_result`
- add optional token-efficient mode toggle

#### Gemini

Files:

- [aria-intelligence/src/backends/gemini.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/gemini.rs)

Work:

- keep reduced schema translator
- keep `functionCall` / `functionResponse`
- support `AUTO`, `ANY`, `NONE`
- support `allowed_function_names`
- keep compositional/parallel call handling where provider supports it

#### Ollama

Files:

- [aria-intelligence/src/backends/ollama.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/ollama.rs)

Work:

- keep reduced compatibility mode
- preserve native local-tool wire behavior where available
- maintain text/repair fallback when not

### Tests

- provider conformance matrix
- tool translation per family
- one tool round-trip per provider
- multi-tool round-trip where supported
- unsupported schema field rejection handled by translator, not by runtime failure

### Exit Criteria

- each provider file owns its actual tool/message protocol
- no provider relies on another provider's schema assumptions

## P3: Tool-Choice Policy Engine

### Objective

Turn “should the model use tools?” into an explicit runtime policy decision.

### Changes

#### `aria-intelligence`

Files:

- [aria-intelligence/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/runtime.rs)
- [aria-intelligence/src/orchestrator.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/orchestrator.rs)
- [aria-intelligence/src/router.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/router.rs)
- [aria-intelligence/src/hardware.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/hardware.rs)

Add:

- `ToolObligationDecision`
  - `NoTools`
  - `ToolsOptional`
  - `ToolsRequired`
  - `SpecificToolRequired`
- provider-agnostic tool relevance scoring
- tool-window ranking
- provider-specific mapping:
  - OpenAI `required` / specific / allowed-tools / none
  - Gemini `ANY` / `AUTO` / `NONE` with allowed names
  - Anthropic loop-level enforcement

Refactor:

- stop accepting first-round prose as final answer when the runtime has determined a tool is required
- keep this generic and policy-driven, not string-driven

#### `aria-x`

Files:

- [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs)

Refactor:

- improve request-to-tool active set reduction
- keep low-entropy prompts like `Hi` from exposing irrelevant tool sets

### Tests

- greeting requests do not expose browser/scheduler tools unnecessarily
- external-fact requests force tool-capable paths when policy requires
- side-effect requests narrow to safe relevant tools only
- repair mode still respects obligation decisions

### Exit Criteria

- tool forcing is based on runtime policy and relevance, not prompt accidents

## P4: MCP Normalization Boundary

### Objective

Keep MCP important, but put it in the correct place in the stack.

### Changes

#### `aria-mcp`

Files:

- [aria-mcp/src/lib.rs](/Users/kushagramadhukar/coding/anima/aria-mcp/src/lib.rs)

Add:

- canonical import path from MCP primitive to `CanonicalToolSpec`
- explicit imported prompt/resource/tool metadata
- provenance and trust metadata

#### `aria-x`

Files:

- [aria-x/src/runtime_store/mcp.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/mcp.rs)
- [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs)
- [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs)

Work:

- persist imported MCP tools/resources/prompts as normalized runtime objects
- ensure MCP imports go through the same capability/policy checks as native tools
- keep provider-native MCP integrations optional and isolated behind adapters

### Tests

- MCP-imported tool round-trip through canonical tool spec
- scope denial for MCP-imported primitive
- provider-native MCP path does not bypass ARIA policy checks

### Exit Criteria

- MCP is a normalized integration source, not a special bypass path

## P5: Inspection and Observability

### Objective

Make provider payloads and tool-loop state inspectable enough to debug failures quickly.

### Changes

#### `aria-core`

Files:

- [aria-core/src/app.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/app.rs)

Add:

- `ProviderPayloadInspection`
- `TranslatedToolDeclaration`
- `ToolLoopTrace`

#### `aria-x`

Files:

- [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs)
- [aria-x/src/runtime_store/audits.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/audits.rs)
- [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs)
- [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs)

Add:

- durable provider payload inspection storage
- exact translated tool declarations as sent
- exact tool-choice policy applied
- exact provider message role sequence
- tool call/result sequence per round
- CLI/operator inspection command, for example:
  - `aria-x inspect provider-payload <session> [agent]`

### Tests

- inspection round-trip
- inspection reflects actual provider payload
- secret-sensitive values remain redacted

### Exit Criteria

- “what exactly did the provider receive?” is answerable from runtime artifacts

## P6: Cleanup and Rollout Hardening

### Objective

Remove old architectural debt once the new tool/runtime path is stable.

### Changes

#### `aria-intelligence`

Files:

- [aria-intelligence/src/prompting.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/prompting.rs)
- [aria-intelligence/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/runtime.rs)
- [aria-intelligence/src/orchestrator.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/orchestrator.rs)

Cleanup:

- remove obsolete prompt-schema injection branches
- remove duplicated tool fallback branches that are no longer needed
- simplify repair-mode handling after provider adapters are mature

#### `aria-x`

Files:

- [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs)
- [aria-x/tests/e2e_pipeline.rs](/Users/kushagramadhukar/coding/anima/aria-x/tests/e2e_pipeline.rs)

Cleanup:

- remove old stopgap compatibility logic that the canonical engine replaces
- expand end-to-end test coverage to current provider defaults

#### Documentation

Files:

- [README.md](/Users/kushagramadhukar/coding/anima/README.md)
- [aria-x/config.toml](/Users/kushagramadhukar/coding/anima/aria-x/config.toml)
- [aria-x/config.example.toml](/Users/kushagramadhukar/coding/anima/aria-x/config.example.toml)

Update:

- provider/tool architecture overview
- MCP positioning
- debugging/inspection commands
- provider-specific caveats

### Tests

- full crate tests
- live provider validation:
  - OpenAI/OpenRouter
  - Gemini
  - Anthropic
- no prompt-level schema duplication on native-tool providers

### Exit Criteria

- old duplicate tool/prompt paths are removed or clearly deprecated

## File Map Summary

### Core contracts

- [aria-core/src/app.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/app.rs)
- [aria-core/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/runtime.rs)

### Tool/runtime engine

- [aria-intelligence/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/tools.rs)
- [aria-intelligence/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/runtime.rs)
- [aria-intelligence/src/orchestrator.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/orchestrator.rs)
- [aria-intelligence/src/prompting.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/prompting.rs)
- [aria-intelligence/src/router.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/router.rs)

### Provider adapters

- [aria-intelligence/src/backends/mod.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/mod.rs)
- [aria-intelligence/src/backends/openai.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/openai.rs)
- [aria-intelligence/src/backends/openrouter.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/openrouter.rs)
- [aria-intelligence/src/backends/anthropic.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/anthropic.rs)
- [aria-intelligence/src/backends/gemini.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/gemini.rs)
- [aria-intelligence/src/backends/ollama.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/backends/ollama.rs)

### Runtime integration

- [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs)
- [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs)
- [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs)
- [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs)
- [aria-x/src/runtime_store/audits.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/audits.rs)
- [aria-x/src/runtime_store/mcp.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/mcp.rs)

### MCP

- [aria-mcp/src/lib.rs](/Users/kushagramadhukar/coding/anima/aria-mcp/src/lib.rs)

### Tests

- [aria-core/src/tests.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/tests.rs)
- [aria-intelligence/src/tests.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/tests.rs)
- [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs)
- [aria-x/tests/e2e_pipeline.rs](/Users/kushagramadhukar/coding/anima/aria-x/tests/e2e_pipeline.rs)

## Recommended Start Point

Start with:

1. `P0` canonical contracts
2. `P2` provider schema/message parity for all providers
3. `P3` tool-choice policy engine

Reason:

- provider breakage is the current blocking pain
- canonical contracts reduce future drift
- tool-choice policy controls whether tools are used correctly in live runs

After that:

4. `P1` deeper context assembly cleanup
5. `P5` inspection hardening
6. `P4` MCP normalization refinements
7. `P6` cleanup
