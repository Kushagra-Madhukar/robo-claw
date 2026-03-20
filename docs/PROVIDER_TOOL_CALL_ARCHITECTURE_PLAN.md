# Provider Tool-Call Architecture Plan

## Purpose

This document consolidates the official tool-use guidance for:

- OpenAI
- Anthropic Claude
- Google Gemini

and turns that research into one implementation plan for ARIA.

The goal is not to copy any one provider's wire format into the core runtime. The goal is to build one strong internal tool/runtime contract and then adapt it correctly per provider.

## Executive Summary

ARIA should not make MCP the primary universal tool contract for all providers.

ARIA should use this architecture:

1. `Canonical internal tool contract`
2. `Provider-specific schema/message translators`
3. `Structured context assembly`
4. `Deterministic tool loop state machine`
5. `MCP as an integration source, not the core tool contract`
6. `Per-request inspection of the final provider payload`

This is the only design that fits all three providers cleanly.

## Official Provider Guidance

### OpenAI

Official docs:

- [Function calling guide](https://platform.openai.com/docs/guides/function-calling/how-do-i-ensure-the-model-calls-the-correct-function)
- [Function calling lifecycle](https://platform.openai.com/docs/guides/function-calling/lifecycle)
- [Tools guide](https://platform.openai.com/docs/guides/tools/tool-choice)
- [OpenAI Docs MCP](https://platform.openai.com/docs/docs-mcp)

Observed guidance from official docs:

- Tools are passed in the `tools` array.
- Custom functions use:
  - `type = "function"`
  - `name`
  - `description`
  - `parameters`
  - optional `strict`
- Responses API tool calling is a multi-step loop:
  1. send request with tools
  2. receive `function_call`
  3. execute tool
  4. send `function_call_output` back using `call_id`
  5. receive final answer or more calls
- The model may emit zero, one, or multiple tool calls.
- `tool_choice` supports:
  - `auto`
  - `required`
  - specific function
  - `allowed_tools`
  - `none`
- `parallel_tool_calls` can be disabled.
- OpenAI recommends `strict: true` for reliable schema adherence.
- OpenAI exposes remote MCP as a tool surface in the Responses/tooling ecosystem, but not as the only tool architecture.

Implication for ARIA:

- OpenAI is the cleanest fit for a strict canonical JSON-schema tool contract.
- ARIA should preserve:
  - `call_id`
  - parallel-call support
  - strict schema mode
  - allowed-tool narrowing without rebuilding the full registry

### Anthropic Claude

Official docs:

- [How to implement tool use](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/implement-tool-use)
- [Token-efficient tool use](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/token-efficient-tool-use)
- [Model Context Protocol overview](https://docs.anthropic.com/en/docs/mcp)
- [MCP connector](https://docs.anthropic.com/en/docs/agents-and-tools/mcp-connector)

Observed guidance from official docs:

- Tools are passed in the top-level `tools` field.
- Each tool includes:
  - `name`
  - `description`
  - `input_schema`
- Anthropic explicitly says it constructs a special system prompt from:
  - tool definitions
  - tool configuration
  - any user system prompt
- Anthropic strongly emphasizes very detailed plaintext tool descriptions.
- Anthropic message flow is block-based:
  - model emits `tool_use`
  - application responds with `tool_result`
- Anthropic exposes token-efficient tool use as a provider-specific optimization.
- Anthropic MCP connector is real, but constrained:
  - remote HTTP servers only
  - tool calls only
  - multiple remote servers supported
  - not full local MCP parity

Implication for ARIA:

- Claude does not want OpenAI wire assumptions.
- Tool descriptions matter more for Claude than minimal schemas alone.
- ARIA should preserve provider-specific advantages:
  - richer description text
  - block-based tool/result flow
  - optional token-efficient mode
- Anthropic MCP connector should be treated as an optional provider optimization, not the baseline architecture.

### Google Gemini

Official docs:

- [Function calling with the Gemini API](https://ai.google.dev/gemini-api/docs/function-calling)

Observed guidance from official docs:

- Function declarations are sent in `tools`.
- Function calling modes:
  - `AUTO`
  - `ANY`
  - `NONE`
- `allowed_function_names` can constrain the active set.
- Gemini supports:
  - parallel function calling
  - compositional function calling
- Tool results are returned with `functionResponse` parts and must be sent back in order.
- Gemini supports MCP through the SDK, but the core API still centers on function declarations.
- Gemini docs explicitly state only a subset of the OpenAPI schema is supported.
- Gemini docs also recommend:
  - very clear function and parameter descriptions
  - strong typing
  - limiting the active tool set to roughly 10-20 relevant tools
  - explicit error handling
  - user validation for consequential actions

Implication for ARIA:

- Gemini must not receive raw strict JSON Schema blindly.
- Gemini needs its own schema translator and active-tool reduction.
- Gemini SDK MCP support is useful, but it is not a replacement for a strong internal tool architecture.

## Cross-Provider Conclusions

### 1. MCP should not be ARIA's primary internal tool contract

Reason:

- OpenAI supports MCP in a provider-specific tool ecosystem.
- Anthropic supports MCP connector with important limitations.
- Gemini SDK supports MCP integration, but the base API still expects function declarations.

So the provider ecosystem position is:

- MCP is an important integration boundary
- MCP is not the universal wire contract across providers

ARIA should therefore:

- keep `aria-mcp` as a first-class subsystem
- import MCP tools/resources/prompts into ARIA's internal registry
- not force every provider request through MCP as the primary tool path

### 2. ARIA needs one canonical internal tool model

Every tool should exist once in a provider-agnostic runtime representation.

Recommended internal model:

- `tool_id`
- `name`
- `description_long`
- `description_short`
- `parameter_schema_canonical`
- `schema_constraints`
- `execution_kind`
  - native
  - skill
  - mcp
  - built-in provider tool
- `result_contract`
- `modalities`
- `requires_approval`
- `parallel_safe`
- `streaming_safe`
- `side_effect_level`
- `provider_compatibility_hints`

Provider adapters then translate from this internal representation into:

- OpenAI function tools
- Anthropic tools/input_schema
- Gemini functionDeclarations

### 3. Prompt text must not duplicate native tool contracts

Best practice across providers is:

- send tools as first-class tool objects
- keep prompt tool guidance short and policy-focused
- do not dump full schemas into the prompt when native tool calling is available

ARIA should therefore:

- use native tool schemas as the primary tool contract
- keep prompt text limited to:
  - when to use tools
  - how to ask for clarification
  - safety/approval expectations
  - response style after tool execution

This is especially important because Anthropic already synthesizes system instructions from tools, and OpenAI/Gemini both already consume tool objects structurally.

### 4. Tool results need one canonical result envelope

All providers require the runtime to execute the tool and send a result back.

ARIA should standardize tool results internally before adapting them per provider.

Recommended internal result envelope:

```json
{
  "ok": true,
  "summary": "Short user-safe summary",
  "data": {},
  "artifacts": [],
  "error": null,
  "retryable": false,
  "approval_required": false
}
```

Then provider adapters render that to:

- OpenAI `function_call_output`
- Anthropic `tool_result`
- Gemini `functionResponse`

### 5. Tool exposure must be runtime-filtered before every request

All three providers encourage or benefit from a small active tool set.

ARIA should expose tools by:

1. capability profile
2. agent policy/capability scope
3. request relevance
4. provider compatibility
5. current runtime conditions
   - approvals
   - browser availability
   - network policy
   - session state

The active request set should be small and relevant, not the whole registry.

### 6. Clarification and enforcement should be policy-driven, not keyword-driven

The runtime should decide whether a tool is required by evaluating:

- request-to-tool relevance
- whether the task requires external data or side effects
- whether the answer would otherwise be speculative

That decision should then map to provider-specific tool choice controls:

- OpenAI:
  - `required`
  - specific function
  - allowed tools
- Gemini:
  - `ANY`
  - `allowed_function_names`
- Anthropic:
  - explicit runtime re-prompt/interrupt policy, since the provider contract is different

## Recommended ARIA Target Architecture

### A. Context Assembly

Introduce a single `ContextAssembler` that emits a typed `ContextPack`.

It should contain:

- `system_sections`
- `control_documents`
- `recent_history`
- `compacted_memory`
- `retrieval_evidence`
- `page_index_refs`
- `tool_policy_guidance`
- `user_request`
- `tool_history`

Then provider adapters transform that into provider-native message shapes.

### B. Provider Adapters

Each provider adapter must own:

- schema translation
- message shaping
- tool-call parsing
- tool-result formatting
- tool choice enforcement
- streaming event parsing
- provider-specific optimizations

This must not be partially split between prompting code and backend code.

### C. Tool Loop State Machine

Implement one deterministic loop:

1. build `ContextPack`
2. select active tools
3. translate tools for provider
4. make provider request
5. parse provider result
6. if tool calls:
   - execute tools
   - append structured tool results
   - continue loop
7. if final answer:
   - apply completion policy
8. persist context inspection

### D. MCP Position

ARIA should keep MCP in two places only:

1. `Internal integration layer`
   - import MCP tools/resources/prompts into ARIA registry
   - subject them to ARIA policy and capability controls

2. `Optional provider optimization`
   - OpenAI remote MCP
   - Anthropic MCP connector
   - Gemini SDK MCP session integration

But:

- do not make provider MCP the baseline execution architecture
- do not couple ARIA core tool execution to provider-specific MCP features

### E. Observability

Per run, ARIA should persist:

- final provider
- final model
- active tools
- translated tool payload shape
- context sections and token counts
- tool choice policy applied
- provider message sequence
- tool calls emitted
- tool results returned
- provider-specific errors

This is required to debug the class of failures we have already seen.

## Current Gaps In ARIA

Based on the current implementation, ARIA still has these gaps:

1. provider-specific schema translation is incomplete
   - Gemini already failed here

2. prompt/tool duplication still exists in some paths
   - some prompts still inline tool text unnecessarily

3. tool exposure is still too broad or poorly ranked in some flows
   - irrelevant tools appear for simple prompts

4. provider message shaping is not yet perfectly symmetric across providers

5. MCP and native tool architecture are not fully separated conceptually in all paths

6. context inspection exists now, but it needs stronger operator UX and stable per-provider reporting

## Recommended Changes

### Phase 1: Canonical Tool Contract

- formalize one internal `ToolSpec`
- formalize one internal `ToolResultEnvelope`
- add provider-compatibility metadata explicitly
- ensure every tool carries:
  - side-effect level
  - approval class
  - provider compatibility
  - schema strictness

### Phase 2: Provider Schema Translators

- OpenAI:
  - keep strict schema as baseline
  - support `strict: true`
- Anthropic:
  - map to `input_schema`
  - preserve richer descriptions
- Gemini:
  - use reduced schema translator
  - strip unsupported fields deterministically
  - enforce active-tool-set reduction more aggressively

### Phase 3: Provider Message Shaping

- OpenAI:
  - use proper `function_call_output` lifecycle
  - preserve `call_id`
- Anthropic:
  - use native `tool_use` and `tool_result` block structure
- Gemini:
  - use `functionCall` / `functionResponse` parts
  - preserve order for multiple calls

### Phase 4: Tool Choice Policy Engine

- add one provider-agnostic tool obligation decision
- map that decision to:
  - OpenAI `tool_choice`
  - Gemini `function_calling_config`
  - Anthropic runtime loop policy

### Phase 5: MCP Boundary Hardening

- keep MCP import and execution in `aria-mcp`
- normalize imported MCP tools to internal `ToolSpec`
- do not let provider-specific MCP replace internal policy enforcement
- support provider-native remote MCP only as an optional optimization path

### Phase 6: Context/Prompt Cleanup

- remove full schema dumps from prompts for native-tool providers
- keep concise tool policy instructions only
- keep retrieval evidence separate from tool instructions
- keep recent history separate from retrieved history

### Phase 7: Inspection and Debuggability

- add provider payload inspection per run
- store translated tool declarations exactly as sent
- store provider tool-call outputs exactly as returned
- expose this via operator CLI

## Test Plan

### Unit tests

- provider schema translation:
  - OpenAI strict schema preserved
  - Anthropic input schema mapping valid
  - Gemini unsupported fields stripped

- tool choice mapping:
  - required tool path
  - allowed-tools path
  - forced specific-tool path
  - none path

- provider result adaptation:
  - OpenAI `function_call_output`
  - Anthropic `tool_result`
  - Gemini `functionResponse`

### Integration tests

- same internal `ToolSpec` works against all three adapters
- one tool call round-trip per provider
- multiple tool calls per provider where supported
- tool-result re-entry works and final answer follows
- inspection record matches actual outgoing provider payload

### Live validation

- OpenAI:
  - required tool call
  - parallel tool call
  - allowed-tools restriction
- Anthropic:
  - detailed description improves call quality
  - tool_result continuation works
- Gemini:
  - reduced schema accepted
  - `ANY` mode forces function calls
  - compositional function calling works on Gemini Flash

## Recommended Decisions

### Decision 1

Keep ARIA's internal tool system provider-agnostic.

### Decision 2

Use provider adapters to shape tools and tool loops, not prompt hacks.

### Decision 3

Treat MCP as:

- a first-class import/integration subsystem
- not the universal provider-facing wire protocol

### Decision 4

Use structured context packs and small active tool windows.

### Decision 5

Persist exact provider payload inspections for every debug-critical run.

## Recommended Next Implementation Track

Priority order:

1. finish provider-specific schema/message parity
2. formalize canonical `ToolSpec` and `ToolResultEnvelope`
3. tighten tool exposure relevance and provider-specific tool forcing
4. clean prompt duplication out of native-tool paths
5. strengthen operator inspection for provider payloads
6. add optional provider-native MCP paths behind the internal MCP subsystem

## References

- OpenAI:
  - [Function calling guide](https://platform.openai.com/docs/guides/function-calling/how-do-i-ensure-the-model-calls-the-correct-function)
  - [Function calling lifecycle](https://platform.openai.com/docs/guides/function-calling/lifecycle)
  - [Tools guide](https://platform.openai.com/docs/guides/tools/tool-choice)
  - [Docs MCP](https://platform.openai.com/docs/docs-mcp)

- Anthropic:
  - [How to implement tool use](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/implement-tool-use)
  - [Token-efficient tool use](https://docs.anthropic.com/en/docs/agents-and-tools/tool-use/token-efficient-tool-use)
  - [Model Context Protocol](https://docs.anthropic.com/en/docs/mcp)
  - [MCP connector](https://docs.anthropic.com/en/docs/agents-and-tools/mcp-connector)

- Gemini:
  - [Function calling with the Gemini API](https://ai.google.dev/gemini-api/docs/function-calling)
