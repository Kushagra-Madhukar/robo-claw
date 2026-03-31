<div align="center">

<img src="docs/HiveClawFavicon.png" alt="HiveClaw logo" width="180" />

</div>

# HiveClaw

![Rust](https://img.shields.io/badge/Rust-Workspace-000000?logo=rust)
![Status](https://img.shields.io/badge/status-alpha-orange)
![Architecture](https://img.shields.io/badge/architecture-local--first-blue)
![Runtime](https://img.shields.io/badge/runtime-SQLite--first-2ea44f)
![Security](https://img.shields.io/badge/security-policy--gated-critical)
![Swarm](https://img.shields.io/badge/swarm-hive--mind-black)
![Robotics](https://img.shields.io/badge/robotics-ROS2%20ready-6f42c1)
![Mesh](https://img.shields.io/badge/mesh-multi--system-0a7ea4)

HiveClaw is a local-first, multi-agent runtime, gateway, and emerging hive-mind control plane written in Rust. It combines agent orchestration, policy enforcement, tool execution, retrieval, scheduling, browser automation, MCP integration, robotics interfaces, and multi-channel ingress into one cohesive system.

If you are evaluating projects like OpenClaw, agent gateways, coding-agent runtimes, swarm orchestrators, robotics control planes, or local-first alternatives to hosted AI wrappers, HiveClaw is the project to inspect.

It is designed to become a **hive mind for parallel systems**:

- one runtime coordinating many agents
- one control plane spanning many channels
- one operator surface spanning many machines
- one policy boundary spanning tools, browsers, files, MCP, and future robot executors
- one mesh-capable architecture that can grow into swarm and fleet operations

The project is designed around a practical constraint set:

- low-end-device-friendly by default
- SQLite-first for local and node deployments
- strict capability boundaries for agents, tools, files, retrieval, and delegation
- human-in-the-loop approvals for risky actions
- multi-channel operation without turning the core runtime into channel-specific code

> HiveClaw is not a chat wrapper. It is an agent runtime with durable state, explicit policy boundaries, tool orchestration, background jobs, and operator visibility.

## Why HiveClaw

Most AI agent projects stop at one of these layers:

- a chatbot wrapper
- a single-agent CLI
- a provider-specific tool loop
- a hosted SaaS orchestration UI
- a robotics stack with no modern agent runtime

HiveClaw is aiming at the harder target: a **local-first agent platform** that can coordinate tools, channels, scheduled work, browsers, operator approvals, mesh-connected runtimes, and eventually robot/ROS2 execution under one system design.

That makes it relevant if you want:

- a serious OpenClaw alternative in Rust
- a local-first coding-agent runtime
- a swarm-ready agent orchestration base
- a bridge between software agents and robotics control
- a system that can run on laptops, nodes, edge devices, and future robot companions

## Positioning

HiveClaw is being built as:

- a local-first agent runtime
- a multi-agent gateway
- a capability-gated tool execution platform
- a future hive-mind layer for multi-system coordination
- a robotics-aware runtime with ROS2 and mesh expansion paths

It is not being built as:

- a thin chat frontend over a hosted model
- a prompt-only automation wrapper
- a Python-only orchestration script pile
- a one-surface bot with hardcoded transport logic
- a robotics stack that ignores modern LLM and tool-loop constraints

## Table of Contents

- [Why HiveClaw](#why-hiveclaw)
- [Positioning](#positioning)
- [What HiveClaw Is](#what-hiveclaw-is)
- [Who It Is For](#who-it-is-for)
- [Current Platform Scope](#current-platform-scope)
- [Hive Mind Direction](#hive-mind-direction)
- [Architecture](#architecture)
- [Runtime Flow](#runtime-flow)
- [Workspace Layout](#workspace-layout)
- [Key Capabilities](#key-capabilities)
- [Channels and Interaction Surfaces](#channels-and-interaction-surfaces)
- [Getting Started](#getting-started)
- [Configuration](#configuration)
- [Security Model](#security-model)
- [Docs Map](#docs-map)
- [Validation Status](#validation-status)
- [Known Boundaries](#known-boundaries)
- [Development](#development)
- [License](#license)

## What HiveClaw Is

HiveClaw is a Rust workspace for building and running:

- multi-agent systems with explicit capability profiles
- policy-gated tool execution
- local and remote channel adapters
- durable session memory and retrieval
- scheduled jobs and reminders
- browser-assisted web access and automation
- MCP client-side integration
- Chrome DevTools MCP onboarding for Chrome-backed browser access through MCP
- optional mesh-connected node topologies
- ROS2-adjacent and robotics-aware execution surfaces
- learning and audit traces for future self-improvement workflows

It is built as a modular workspace so the runtime can evolve without collapsing into one large binary with implicit behavior.

## Who It Is For

HiveClaw is for builders who need more than a single coding assistant:

- engineers building local-first agent products
- teams that want one runtime behind TUI, Telegram, WebSocket, and future channels
- operators who need approvals, audits, retrieval, and durable runs
- researchers experimenting with agent swarms and multi-agent delegation
- robotics and embodied-AI builders who want a path from software agents to ROS2 and robot fleets
- edge-device and low-resource deployments that cannot afford bloated cloud-first stacks

## Current Platform Scope

### Implemented baseline

- multi-agent runtime with agent overrides and scoped routing
- approval-gated file, shell, browser, and high-risk operations
- provider-aware LLM orchestration with capability-aware tool exposure
- durable session state, compaction state, approvals, runs, mailbox, audits, and runtime metrics in SQLite
- TUI interaction mode
- Telegram and WebSocket runtime support
- background runs, reminders, and scheduler flow
- browser profile/session state with encrypted persistence support
- MCP subsystem boundary and runtime integration
- Chrome DevTools MCP can be registered and synced as a browser-facing MCP provider, with managed-launch and attach-to-existing-session modes
- skills/runtime foundation with policy gating

### Current maturity

- production-style architecture, alpha product stage
- core local/node runtime validated
- cluster-scale backend remains intentionally deferred

## Hive Mind Direction

HiveClaw is intentionally being shaped toward a bigger target than a single-node assistant runtime.

### Near-term direction

- stronger multi-agent coordination
- better multi-node execution over the mesh layer
- more capable background runs and delegated work
- better operator visibility over concurrent runs and approvals
- deeper MCP and external-system integration

### Strategic direction

- act as a hive mind across multiple systems and surfaces
- coordinate parallel workers across machines, devices, and channels
- operate as a swarm runtime rather than a single-session assistant
- mediate between software agents, external tools, browser actors, and robotics executors
- support robot-adjacent and ROS2-based workflows without abandoning the local-first runtime core

### Robotics and fleet direction

The long-term intent is for HiveClaw to work not only as a software agent runtime, but also as a robotics-aware coordination layer:

- ROS2 bridge integration
- robot-state-aware planning
- high-level robotics contracts instead of unsafe direct actuator prompting
- policy-gated robot operations
- mesh-connected robot and companion-node communication
- future native deployment paths on robot-class hardware and constrained edge systems

## Architecture

For the maintained multi-diagram reference set, see [`docs/architecture/README.md`](docs/architecture/README.md).

### High-level system view

```mermaid
flowchart TD
    U["User / Operator"] --> C["Channels<br/>TUI / Telegram / WebSocket / others"]
    C --> G["aria-gateway<br/>Normalization + transport adapters"]
    G --> X["aria-x<br/>Runtime wiring + ingress + operator surfaces"]
    X --> I["aria-intelligence<br/>Routing + orchestration + prompting + tool policy"]
    I --> P["aria-policy<br/>Cedar policy enforcement"]
    I --> S["aria-ssmu<br/>Session memory + retrieval + compaction state"]
    I --> K["aria-skill-runtime<br/>Wasm skill execution boundary"]
    I --> M["aria-mcp<br/>MCP client/runtime integration"]
    I --> V["aria-vault<br/>Secrets and protected state"]
    I --> L["aria-learning<br/>Traces, fingerprints, evaluation records"]
    X --> R["SQLite runtime store<br/>runs, approvals, queues, audits, compaction, telemetry"]
    X --> B["Browser / Web / Shell / File / Native tools"]
    X --> Z["aria-mesh<br/>Optional topology / transport layer"]
```

### Request lifecycle

```mermaid
sequenceDiagram
    participant User
    participant Channel
    participant Gateway as aria-gateway
    participant Runtime as aria-x
    participant Orch as aria-intelligence
    participant Policy as aria-policy
    participant Store as SQLite runtime store
    participant Tools as Native tools / MCP / Skills

    User->>Channel: message / command / prompt
    Channel->>Gateway: raw provider payload
    Gateway->>Runtime: normalized AgentRequest
    Runtime->>Policy: ingress + capability checks
    Runtime->>Store: session / approvals / queue / state load
    Runtime->>Orch: build prompt context + tool window
    Orch->>Policy: tool eligibility / approval requirements
    Orch->>Tools: execute tool call(s)
    Tools->>Orch: tool result / approval-needed / denial
    Orch->>Runtime: final response or background-run update
    Runtime->>Store: persist history, audits, metrics, compaction state
    Runtime->>Channel: user-visible response
```

### Control and background execution model

```mermaid
flowchart LR
    A["Primary session"] --> B["Control intents<br/>agent / approvals / runs / sessions"]
    A --> C["Prompt request"]
    C --> D["Orchestrator loop"]
    D --> E["Direct tool execution"]
    D --> F["spawn_agent"]
    F --> G["Background child run"]
    G --> H["Run events"]
    G --> I["Mailbox / status"]
    H --> A
    I --> A
```

## Runtime Flow

The current runtime path is:

1. Inbound payload is normalized into a common request shape.
2. Firewall and ingress safety checks run.
3. Session scoping and override resolution run.
4. Capability-aware tool exposure is computed for the active model and agent.
5. Retrieval builds a structured context pack from session state, control docs, and memory.
6. The orchestrator sends that context to the provider and runs the tool loop.
7. Approval-gated actions pause safely and persist approval state.
8. Responses, audits, queue state, metrics, and compaction state are persisted.
9. Outbound channel rendering sends the normalized response back to the originating surface.

## Workspace Layout

| Path | Purpose |
|---|---|
| `aria-core` | Shared contracts: requests, responses, agents, browser/tool/runtime types |
| `aria-gateway` | Channel adapters, normalization, transport-specific logic |
| `aria-intelligence` | Orchestrator, routing, prompting, provider adapters, tool policy |
| `aria-policy` | Cedar-backed policy and capability evaluation |
| `aria-ssmu` | Session state, retrieval, compaction, memory indexing |
| `aria-skill-runtime` | Wasm execution boundary for deterministic skills |
| `aria-mcp` | MCP client-side subsystem and runtime integration |
| `aria-learning` | Execution traces, fingerprints, evaluation/promotion records |
| `aria-safety` | Leak pattern scanning and safety utilities |
| `aria-vault` | Encrypted secret storage and protected state access |
| `aria-mesh` | Optional distributed topology / mesh transport layer |
| `aria-x` | Main binary: runtime composition, operator surfaces, TUI, scheduler |
| `agents/` | Agent capability profiles and role configuration |
| `nodes/` | Node-role configuration examples |
| `docs/` | Architecture, migration, audit, and planning documents |
| `scripts/` | Validation, stress, soak, and build helper scripts |

## Key Capabilities

### Agent runtime

- explicit agent selection and override support
- scoped delegation to child runs
- parent/child run graph and mailbox persistence
- bounded background execution
- capability ceilings enforced in code
- execution model designed to scale toward swarm and hive-mind coordination

### Tool execution

- provider-aware tool calling path
- approval-aware execution flow
- deterministic native-tool fast paths for critical operations
- runtime policy checks before file, shell, browser, retrieval, and MCP access
- tool exposure filtered by active model capabilities

### Browser and web access

- browser profile persistence
- default profile reuse
- login state and session persistence
- screenshot capture and browser action execution
- browser activity auditing
- optional Chrome DevTools MCP integration for live DevTools-backed browser access

See [docs/CHROME_DEVTOOLS_MCP.md](docs/CHROME_DEVTOOLS_MCP.md) for the setup flow.

Operator commands:

```bash
hiveclaw doctor mcp
hiveclaw doctor mcp --live
hiveclaw doctor mcp --live --mode auto_connect
hiveclaw setup chrome-devtools-mcp --agent developer
hiveclaw setup chrome-devtools-mcp --mode auto_connect --agent developer
```

### Scheduling and background work

- reminders and recurring jobs
- durable scheduler state
- background child runs
- status and mailbox inspection

### Multi-channel support

- shared runtime with multiple adapters
- WebSocket channel for structured local and remote clients
- Telegram integration
- TUI client over the shared runtime
- channel onboarding/status commands
- architecture intended to let one HiveClaw runtime coordinate many surfaces in parallel

### Mesh, swarm, and robotics direction

- optional mesh transport layer for distributed node topologies
- ROS2 bridge surface already present in the workspace
- robotics prompt and contract primitives already present in core/intelligence layers
- path toward multi-device, multi-robot, and companion-node coordination
- long-term target: one HiveClaw runtime acting as the hive mind across many systems

### Operator visibility

- run inspection
- approvals and approval handles
- channel health
- compaction state
- queue / outbox / DLQ visibility
- retrieval traces
- scope denials and secret usage audits
- STT doctor/setup commands

## Channels and Interaction Surfaces

HiveClaw separates transport concerns from core runtime behavior.

### Implemented and used in the current platform

- `tui`
- `telegram`
- `websocket`

### Adapters present in the workspace

- `cli`
- `telegram`
- `websocket`
- `whatsapp`
- `slack`
- `discord`
- `imessage`
- `ros2`

Adapter maturity is not uniform. The core validated local runtime path today is TUI + WebSocket + Telegram.

### TUI

The project now includes a real terminal UI with:

- transcript pane
- operator workbench tabs for runs, approvals, tools/context, and system health
- approval picker
- agent switcher
- searchable command palette
- inspect/explain shortcuts for context, provider payloads, MCP, and workspace diagnostics
- failure summaries that explain common operator-visible issues without hiding raw logs
- runtime status summaries
- runtime log tail
- persisted context inspection records for prompt review
- keyboard and mouse navigation

Operator workflow docs:

- [docs/HIVECLAW_OPERATOR_WORKBENCH.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_OPERATOR_WORKBENCH.md)

## Getting Started

### Prerequisites

- Rust stable toolchain
- Cargo
- optional `.env` for local secrets
- a provider API key; the default repo setup uses Gemini Flash via `GEMINI_API_KEY`

### 1. Build the workspace

```bash
cargo build --workspace
```

### 1.5. Configure the default provider

```bash
cp .env.example .env
```

Then set:

```bash
GEMINI_API_KEY=your_key_here
```

The checked-in runtime defaults use:

- `backend = "gemini"`
- `model = "gemini-3-flash-preview"`

### 1.6. Initialize local HiveClaw workspace files (recommended)

```bash
target/debug/hiveclaw init
```

This bootstraps `.hiveclaw/` with starter config, policy, and guidance files for local runs.

For the full onboarding and operator workflow, see:

- [docs/HIVECLAW_OPERATOR_WORKBENCH.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_OPERATOR_WORKBENCH.md)

### 2. Run tests

```bash
cargo test --workspace
```

### 3. Start the runtime

```bash
cargo run -p aria-x --bin hiveclaw -- run aria-x/config.toml
```

### 4. Start the TUI

```bash
cargo run -p aria-x --bin hiveclaw -- tui aria-x/config.toml
```

### 5. Attach the TUI to a shared runtime

If you are already running a shared gateway with WebSocket enabled:

```bash
target/debug/hiveclaw tui aria-x/config.toml --attach ws://127.0.0.1:8090/ws
```

### 6. Multi-node examples

```bash
cargo run -p aria-x --bin hiveclaw -- nodes/orchestrator.toml
cargo run -p aria-x --bin hiveclaw -- nodes/relay.toml
cargo run -p aria-x --bin hiveclaw -- nodes/companion.toml
cargo run -p aria-x --bin hiveclaw -- nodes/micro.toml
```

### 7. Verify speech-to-text setup

```bash
target/debug/hiveclaw doctor stt
target/debug/hiveclaw setup stt --local
```

## Configuration

The main example config is:

- [`aria-x/config.example.toml`](aria-x/config.example.toml)

Primary configuration areas:

- `llm`: backend, model, tool-loop limits
- `gateway`: adapters, ports, transport modes, fanout rules
- `router`: confidence thresholds and tie-break behavior
- `ssmu`: session store and retention knobs
- `scheduler`: runtime scheduling
- `node`: node role and tier
- `cluster`: deployment profile and backend boundary
- `rollout`: canary feature gates
- `telemetry`: logging/observability
- `ui`: local UI controls

### Default provider

The repository defaults are configured for Gemini:

- `backend = "gemini"`
- `model = "gemini-3-flash-preview"`

If you want to switch providers later, change the `[llm]` block in [`aria-x/config.toml`](aria-x/config.toml) and provide the matching credential in `.env`.

### Speech-to-text modes

HiveClaw supports:

- `auto`: prefer local STT when available, otherwise use configured cloud STT, otherwise stay off
- `local`: require a valid local Whisper runtime
- `cloud`: require a configured cloud STT endpoint
- `off`: disable voice/video transcription

For local Whisper, HiveClaw expects:

- `WHISPER_CPP_MODEL`
- `WHISPER_CPP_BIN`
- `FFMPEG_BIN`
- optional `WHISPER_CPP_LANGUAGE`

### Secret handling

Use local env files for secrets:

```bash
cp .env.example .env
```

For the default Gemini setup, populate:

```bash
GEMINI_API_KEY=your_key_here
```

Do not place live secrets in tracked config files. Generated runtime config files and local env files are intentionally ignored.

### STT onboarding

If you want local voice transcription:

```bash
target/debug/hiveclaw doctor
target/debug/hiveclaw doctor stt
target/debug/hiveclaw setup stt --local
```

`doctor stt` reports whether the local runtime is operational.

`setup stt --local` bootstraps a Homebrew-based local STT setup when possible and writes detected local STT paths into your local `.env`.

`doctor` reports the current runtime status, install-path status, configured channels, and STT readiness in one operator summary.

Additional doctor scopes:

- `doctor env`
- `doctor gateway`
- `doctor browser`

`install` copies the current binary into `~/.local/bin/hiveclaw` by default so you can run HiveClaw from anywhere once that directory is on your shell `PATH`. The legacy `aria-x` command remains available for compatibility.

You can also seed the standard application config path during install:

```bash
target/debug/hiveclaw install --with-default-config
```

Shell completions are generated on demand:

```bash
target/debug/hiveclaw completion zsh
target/debug/hiveclaw completion bash
target/debug/hiveclaw completion fish
```

### Typical local setup

```bash
cargo run -p aria-x --bin hiveclaw -- run aria-x/config.toml
```

### Typical shared runtime setup

Configure both Telegram and WebSocket in the gateway section, then run:

```bash
./dev.sh aria-x/config.toml
```

And attach the TUI from another terminal:

```bash
target/debug/hiveclaw tui aria-x/config.toml --attach ws://127.0.0.1:8090/ws
```

## Security Model

HiveClaw is built around runtime-enforced boundaries, not prompt-only instructions.

### Enforcement layers

- Cedar policy evaluation
- capability profiles per agent
- approval-gated sensitive tools
- filesystem scopes
- retrieval scopes
- MCP allowlists
- delegation ceilings for child runs
- secret usage auditing
- ingress and egress safety filters

### Security posture

- unknown or unsupported model capabilities degrade conservatively
- low-capability agents cannot escalate via prompt injection alone
- browser/session persistence requires explicit master key configuration
- local secrets and runtime artifacts are expected to stay out of Git

## Docs Map

Start here if you want the deeper architecture and planning trail:

- [`docs/HIVECLAW_EXECUTION_ROADMAP.md`](docs/HIVECLAW_EXECUTION_ROADMAP.md)
- [`docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md`](docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md)
- [`docs/HIVECLAW_RULES_HOOKS_SKILLS.md`](docs/HIVECLAW_RULES_HOOKS_SKILLS.md)
- [`docs/HIVECLAW_COMPUTER_RUNTIME.md`](docs/HIVECLAW_COMPUTER_RUNTIME.md)
- [`docs/HIVECLAW_EDGE_MODE.md`](docs/HIVECLAW_EDGE_MODE.md)
- [`docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md`](docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md)
- [`docs/HIVECLAW_DISTRIBUTED_EXECUTION.md`](docs/HIVECLAW_DISTRIBUTED_EXECUTION.md)
- [`docs/HIVECLAW_EVALS_TELEMETRY.md`](docs/HIVECLAW_EVALS_TELEMETRY.md)
- [`docs/HIVECLAW_OPERATOR_WORKBENCH.md`](docs/HIVECLAW_OPERATOR_WORKBENCH.md)
- [`docs/CHROME_DEVTOOLS_MCP.md`](docs/CHROME_DEVTOOLS_MCP.md)
- [`docs/architecture/README.md`](docs/architecture/README.md)
- [`docs/REPO_CONTEXT_MAP.md`](docs/REPO_CONTEXT_MAP.md)
- [`docs/ARCHITECTURAL_CHANGES.md`](docs/ARCHITECTURAL_CHANGES.md)
- [`docs/ARCHITECTURE_REMAINING_WORK.md`](docs/ARCHITECTURE_REMAINING_WORK.md)
- [`docs/ARCHITECTURE_STRESS_TEST_AND_TARGET_STATE.md`](docs/ARCHITECTURE_STRESS_TEST_AND_TARGET_STATE.md)
- [`docs/OPENCLAW_DEEP_ARCHITECTURE_COMPARISON.md`](docs/OPENCLAW_DEEP_ARCHITECTURE_COMPARISON.md)
- [`docs/AGENT_PLATFORM_EXPANSION_PLAN.md`](docs/AGENT_PLATFORM_EXPANSION_PLAN.md)
- [`docs/WEB_ACCESS_PLATFORM_PLAN.md`](docs/WEB_ACCESS_PLATFORM_PLAN.md)
- [`docs/RUST_SYSTEMS_REVIEW.md`](docs/RUST_SYSTEMS_REVIEW.md)
- [`docs/OPERATIONAL_ALERTS_RUNBOOK.md`](docs/OPERATIONAL_ALERTS_RUNBOOK.md)

## Validation Status

The repo has gone through:

- workspace builds
- crate-level tests
- targeted integration tests
- live runtime validation for core flows
- stress suite
- soak suite
- acceptance gate checks

Validated baseline areas include:

- prompt execution
- approvals
- file and shell tool flows
- scheduler/reminders
- browser profile reuse
- browser login/manual auth flow
- browser screenshot and browser action flow
- sub-agent spawn
- multi-gateway shared-state validation
- TUI interaction and attach flow

## Known Boundaries

These are the honest current limits:

- cluster-grade Postgres runtime store is intentionally deferred
- dedicated coordination service is intentionally deferred until measured need
- full adaptive mixed-transport orchestration is deferred
- adapter maturity is uneven across non-primary channels
- the platform is architecturally broad, but still alpha as a product

## Development

### Common commands

```bash
# Build
cargo build --workspace

# Test
cargo test --workspace

# Run main runtime
cargo run -p aria-x --bin hiveclaw -- run aria-x/config.toml

# Installed-style run command
target/debug/hiveclaw run aria-x/config.toml

# Run TUI
cargo run -p aria-x --bin hiveclaw -- tui aria-x/config.toml

# Runtime lifecycle
target/debug/hiveclaw status
target/debug/hiveclaw stop
target/debug/hiveclaw doctor
target/debug/hiveclaw doctor stt
target/debug/hiveclaw doctor env
target/debug/hiveclaw doctor gateway
target/debug/hiveclaw doctor browser
target/debug/hiveclaw --inspect-context <session_id> [agent_id]
target/debug/hiveclaw --inspect-provider-payloads <session_id> [agent_id]
target/debug/hiveclaw --explain-context <session_id> [agent_id]
target/debug/hiveclaw --explain-provider-payloads <session_id> [agent_id]
target/debug/hiveclaw inspect context [session_id] [agent_id]
target/debug/hiveclaw inspect provider-payloads [session_id] [agent_id]
target/debug/hiveclaw explain context [session_id] [agent_id]
target/debug/hiveclaw explain provider-payloads [session_id] [agent_id]
target/debug/hiveclaw install
target/debug/hiveclaw install --with-default-config
target/debug/hiveclaw completion zsh

# Dev wrapper
./dev.sh aria-x/config.toml

# Stress suite
bash scripts/run-stress-suite.sh

# Soak suite
bash scripts/run-soak-suite.sh
```

### Repository hygiene

Before pushing:

```bash
git status --short
git diff --cached --stat
```

Keep these local-only:

- `.env`
- `vault.json`
- runtime DBs and session logs
- generated `config.runtime.json` files
- `config.live*` files
- local TUI runtime artifacts

## License

HiveClaw is licensed under the GNU Affero General Public License v3.0 or later.

See [LICENSE](LICENSE) for the full license text.
