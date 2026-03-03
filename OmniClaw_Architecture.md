# OmniClaw — Unified AI Agent Orchestration Architecture

> **Comprehensive Architecture & Design Document**
> Version 1.0 | March 2026
> Derived from static analysis of: **PicoClaw** (Go) · **OpenClaw** (TypeScript) · **NanoBot** (Python)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Design Goals & Principles](#2-design-goals--principles)
3. [Core Concepts](#3-core-concepts)
4. [Architectural Overview — High-Level Design (HLD)](#4-architectural-overview--high-level-design-hld)
5. [System Topology & Deployment View](#5-system-topology--deployment-view)
6. [Low-Level Design (LLD)](#6-low-level-design-lld)
7. [Module Descriptions & Layer Architecture](#7-module-descriptions--layer-architecture)
8. [Data Model & Table Schemas](#8-data-model--table-schemas)
9. [End-to-End Request Flow](#9-end-to-end-request-flow)
10. [Sequence Diagrams](#10-sequence-diagrams)
11. [Class Diagrams](#11-class-diagrams)
12. [Data Flow Diagrams (DFDs)](#12-data-flow-diagrams-dfds)
13. [Functional Requirements](#13-functional-requirements)
14. [Non-Functional Requirements](#14-non-functional-requirements)
15. [Technical Stack](#15-technical-stack)
16. [Repository Structure](#16-repository-structure)
17. [Deployment Strategy](#17-deployment-strategy)
18. [Fault Tolerance & Resilience](#18-fault-tolerance--resilience)
19. [Future Scope — Robotics Adoption](#19-future-scope--robotics-adoption)
20. [Appendices](#20-appendices)

---

## 1. Executive Summary

OmniClaw is a next-generation, unified AI agent orchestration architecture synthesized from the deep static analysis of three production-grade open-source agent frameworks: **PicoClaw** (Go), **OpenClaw** (TypeScript), and **NanoBot** (Python). Each framework excels in a distinct dimension—PicoClaw in extreme hardware efficiency (<10 MB RAM, <1 s boot), OpenClaw in enterprise-grade gateway orchestration with multi-device mesh networking, and NanoBot in research-friendly modularity with a ~4,000-line Python core.

OmniClaw absorbs the strongest architectural patterns from all three while eliminating their respective anti-patterns. The result is a **lightweight, secure, performant, and extensible** platform that can run identically on a $10 embedded board and a cloud VM, with first-class support for future robotic actuator control via a Hardware Abstraction Layer (HAL).

This document provides the complete architectural blueprint—from high-level topology down to table schemas and sequence diagrams—to serve as the **single source of truth** for implementation.

---

## 2. Design Goals & Principles

### 2.1 Design Goals

| # | Goal | Description |
|---|------|-------------|
| G1 | **Extreme Portability** | Run on x86-64, ARM64, RISC-V, and MIPS with a single static binary. Target <15 MB resident memory. |
| G2 | **Defense-in-Depth Security** | Every tool invocation passes through an Approval Pipeline with configurable policies. Workspace chroot prevents filesystem escape. |
| G3 | **Sub-Second Cold Start** | The core daemon boots and accepts its first message in under 1 second on commodity hardware. |
| G4 | **Pluggable LLM Backends** | Provider interface abstracts OpenAI, Anthropic, Gemini, Ollama, and custom endpoints behind a unified `Chat()` contract. |
| G5 | **Channel Agnosticism** | Telegram, Discord, Slack, WhatsApp, iMessage, Web, CLI, and robotic peripheral buses are first-class channels. |
| G6 | **Horizontal Scalability** | Gateway mesh with node discovery (mDNS/Tailscale) enables multi-device agent fleets. |
| G7 | **Robotics-Ready Foundation** | HAL module exposes I2C, SPI, GPIO, CAN bus, and ROS2 bridge interfaces behind safe tool abstractions. |
| G8 | **Observable & Debuggable** | Structured JSON logging, OpenTelemetry traces, and a real-time Control UI for session inspection. |

### 2.2 Design Principles

- **Composition over Inheritance** — every subsystem is a pluggable module behind an interface.
- **Fail-Open for Reads, Fail-Closed for Writes** — read tools default-allow; write/exec tools default-deny.
- **Context is King** — the agent loop never mutates shared state; all mutation flows through the MessageBus.
- **Zero External Dependencies at Runtime** — no database server, no message broker; everything is embedded.
- **Convention over Configuration** — sensible defaults with opt-in overrides via `config.json`.

---

## 3. Core Concepts

| Concept | Definition |
|---------|-----------|
| **Agent** | An autonomous LLM-powered entity with its own system prompt, tool set, memory, and workspace. |
| **AgentLoop** | The iterative processing engine that polls the MessageBus, builds context, calls the LLM, executes tool calls, and publishes responses. |
| **MessageBus** | An in-process async pub/sub queue decoupling channel adapters from the AgentLoop. |
| **ToolRegistry** | A named map of Tool implementations exposing JSON Schema definitions to the LLM and executing validated calls. |
| **Session** | A conversation thread scoped by `channel:chat_id`, persisting message history and memory summaries. |
| **Subagent** | A child agent spawned by a parent for delegated tasks, inheriting workspace but isolated in session state. |
| **Skill** | A package of prompt instructions and optional tool bindings installable at runtime from a registry. |
| **Gateway** | The network-facing daemon exposing HTTP/WS APIs, managing auth, node discovery, and the Control UI. |
| **HAL** | Hardware Abstraction Layer — a standardized interface for robotic peripherals (sensors, actuators) exposed as tools. |
| **Approval Pipeline** | A chain of policy evaluators that gate dangerous tool invocations (exec, file-write, network). |

---

## 4. Architectural Overview — High-Level Design (HLD)

OmniClaw follows a **layered architecture** with four principal tiers, communicating exclusively through well-defined interfaces. Each tier can be deployed independently or co-located in a single binary.

| Layer | Components |
|-------|-----------|
| **Presentation** | Channel Adapters (Telegram, Discord, CLI, Web UI, ROS2 Bridge) |
| **Gateway** | HTTP/WS Server, Auth Manager, Node Discovery, Control UI, Config Reloader |
| **Agent** | AgentLoop, ContextBuilder, MemoryStore, ToolRegistry, SubagentManager, SkillLoader |
| **Infrastructure** | LLM Providers, MessageBus, Session Store, HAL, MCP Client, CronScheduler |

### 4.1 High-Level Component Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                     PRESENTATION LAYER                         │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────┐ ┌────────┐ │
│  │ Telegram │ │ Discord  │ │   CLI    │ │  Web   │ │ ROS2   │ │
│  │ Adapter  │ │ Adapter  │ │ Adapter  │ │  UI    │ │ Bridge │ │
│  └────┬─────┘ └────┬─────┘ └────┬─────┘ └───┬────┘ └───┬────┘ │
│       │             │            │            │          │      │
├───────┴─────────────┴────────────┴────────────┴──────────┴──────┤
│                       MESSAGE BUS                               │
│              InboundQueue ◄──► OutboundQueue                    │
├─────────────────────────────────────────────────────────────────┤
│                      GATEWAY LAYER                              │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐ ┌────────────────┐  │
│  │  HTTP/WS  │ │   Auth    │ │   Node    │ │   Control UI   │  │
│  │  Server   │ │  Manager  │ │ Discovery │ │   (SPA + API)  │  │
│  └───────────┘ └───────────┘ └───────────┘ └────────────────┘  │
├─────────────────────────────────────────────────────────────────┤
│                       AGENT LAYER                               │
│  ┌────────────────────────────────────────────────────────────┐ │
│  │                     AGENT LOOP                             │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌─────────────┐  │ │
│  │  │ Context  │ │  Memory  │ │   Tool   │ │  Subagent   │  │ │
│  │  │ Builder  │ │  Store   │ │ Registry │ │  Manager    │  │ │
│  │  └──────────┘ └──────────┘ └──────────┘ └─────────────┘  │ │
│  │  ┌──────────┐ ┌──────────┐ ┌──────────────────────────┐  │ │
│  │  │  Skill   │ │ Approval │ │    Fallback Chain        │  │ │
│  │  │  Loader  │ │ Pipeline │ │  (Multi-Provider LLM)    │  │ │
│  │  └──────────┘ └──────────┘ └──────────────────────────┘  │ │
│  └────────────────────────────────────────────────────────────┘ │
├─────────────────────────────────────────────────────────────────┤
│                   INFRASTRUCTURE LAYER                          │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌────────┐ ┌────────┐ │
│  │   LLM    │ │ Session  │ │   HAL    │ │  MCP   │ │  Cron  │ │
│  │ Provider │ │  Store   │ │ (HW I/O) │ │ Client │ │Schedule│ │
│  └──────────┘ └──────────┘ └──────────┘ └────────┘ └────────┘ │
└─────────────────────────────────────────────────────────────────┘
```

---

## 5. System Topology & Deployment View

```
                    ┌─────────────────┐
                    │   Cloud / VPS   │
                    │  ┌───────────┐  │
                    │  │ OmniClaw  │  │        ┌──────────────┐
                    │  │  Gateway  │◄─┼────────┤  Tailscale   │
                    │  │  :18789   │  │        │   Network    │
                    │  └─────┬─────┘  │        └──────┬───────┘
                    │        │        │               │
                    └────────┼────────┘               │
                             │                        │
              ┌──────────────┼────────────────────────┼──────┐
              │              │         LAN / Home     │      │
              │   ┌──────────▼─────────┐    ┌────────▼────┐ │
              │   │   Raspberry Pi     │    │   Laptop    │ │
              │   │  ┌──────────────┐  │    │  OmniClaw   │ │
              │   │  │  OmniClaw    │  │    │   CLI +     │ │
              │   │  │  Agent Node  │  │    │  Control UI │ │
              │   │  │  + HAL       │  │    └─────────────┘ │
              │   │  │  (I2C, SPI)  │  │                    │
              │   │  └──────────────┘  │                    │
              │   └────────────────────┘                    │
              └─────────────────────────────────────────────┘
```

---

## 6. Low-Level Design (LLD)

### 6.1 AgentLoop Internal State Machine

```
    ┌───────────┐    msg arrives    ┌──────────────┐
    │   IDLE    │──────────────────►│ BUILD_CONTEXT │
    └───────────┘                   └──────┬───────┘
         ▲                                 │
         │                                 ▼
         │                          ┌──────────────┐
         │     no tool calls        │   CALL_LLM   │
         │◄─────────────────────────┤              │
         │     (final answer)       └──────┬───────┘
         │                                 │ has tool calls
         │                                 ▼
         │                          ┌──────────────┐
         │                          │ EXECUTE_TOOL │
         │                          │  (approval?) │
         │                          └──────┬───────┘
         │                                 │
         │         iteration < max         │
         └─────────────────────────────────┘
```

### 6.2 Approval Pipeline Flow

```
  Tool Call ──► WorkspacePolicy ──► FileSystemPolicy ──► ExecPolicy ──► UserApproval
                   │                    │                   │               │
                   ▼                    ▼                   ▼               ▼
               ALLOW/DENY          ALLOW/DENY          ALLOW/DENY     ALLOW/DENY
```

### 6.3 Memory Consolidation Strategy

When the unconsolidated message count exceeds the `memory_window` threshold, the MemoryStore triggers an LLM-driven summarization pass. The summary is stored as a session-level prefix, and raw messages are archived to disk.

| Parameter | Default | Description |
|-----------|---------|-------------|
| `memory_window` | 100 | Max unconsolidated messages before triggering summarization |
| `max_iterations` | 40 | Max LLM tool-call iterations per request |
| `tool_result_max_chars` | 500 | Truncation limit for tool results stored in session |
| `consolidation_timeout` | 30s | Max time for a summarization LLM call |

### 6.4 Fallback Chain (Multi-Provider LLM)

```
  Request ──► Primary Provider ──[fail]──► Candidate 2 ──[fail]──► Candidate N
                   │                            │                      │
                   ▼ success                    ▼ success              ▼ success
              Return Response             Return Response        Return Response
                                                                       │
                                                                  [all fail]
                                                                       ▼
                                                                  Return Error
```

Cooldown tracking prevents repeatedly hitting a provider that returned 429/5xx errors within a configurable window.


---

## 7. Module Descriptions & Layer Architecture

### 7.1 Presentation Layer — Channel Adapters

Each channel adapter implements the `ChannelAdapter` interface, translating platform-specific APIs into `InboundMessage` / `OutboundMessage` bus events.

| Module | Origin | Responsibility |
|--------|--------|---------------|
| `TelegramAdapter` | NanoBot (Python) | Long-poll or webhook listener for Telegram Bot API |
| `DiscordAdapter` | OpenClaw (TS) | Gateway bot with slash-command registration and rich embeds |
| `CLIAdapter` | PicoClaw (Go) | Interactive terminal REPL with readline and progress hints |
| `WebAdapter` | OpenClaw (TS) | WebSocket bridge for browser-based Control UI |
| `ROS2Bridge` | New (OmniClaw) | Subscribes to ROS2 topics, publishes agent actions |
| `WhatsAppAdapter` | PicoClaw (Go) | WhatsApp Business API or linked-device bridge |
| `SlackAdapter` | OpenClaw (TS) | Slack Events API + interactive blocks |
| `iMessageAdapter` | OpenClaw (TS) | macOS-only AppleScript bridge for iMessage |

### 7.2 Gateway Layer

| Component | Package | Description |
|-----------|---------|-------------|
| `HTTPServer` | `gateway/http.go` | REST endpoints: `/v1/chat/completions`, `/health`, `/v1/responses` |
| `WSServer` | `gateway/ws.go` | Persistent WebSocket connections for Control UI and peer nodes |
| `AuthManager` | `gateway/auth.go` | Token-based auth with rate limiting and Tailscale identity |
| `NodeDiscovery` | `gateway/discovery.go` | mDNS/Bonjour + Tailscale peer discovery for multi-device mesh |
| `ConfigReloader` | `gateway/reload.go` | File-watch on `config.json`, hot-reload without restart |
| `ControlUI` | `gateway/ui/` | Embedded SPA for session inspection, model switching, logs |
| `TLSManager` | `gateway/tls.go` | Optional TLS termination with auto-generated self-signed certs |

### 7.3 Agent Layer

| Component | Package | Description |
|-----------|---------|-------------|
| `AgentLoop` | `agent/loop.go` | Core iteration: poll → context → LLM → tool exec → respond |
| `ContextBuilder` | `agent/context.go` | Assembles system prompt + history + memory summary + skills + runtime hints |
| `MemoryStore` | `agent/memory.go` | Sliding-window history with LLM-driven consolidation and archival |
| `ToolRegistry` | `agent/tools/registry.go` | Named tool map with JSON Schema generation for function calling |
| `SubagentManager` | `agent/subagent.go` | Spawns child agents with isolated sessions, manages lifecycle |
| `SkillLoader` | `agent/skills.go` | Discovers, installs, and injects skill prompt/tool packages |
| `ApprovalPipeline` | `agent/approval.go` | Chain of policy evaluators gating tool execution |
| `FallbackChain` | `agent/fallback.go` | Multi-provider LLM failover with cooldown tracking |
| `AgentRegistry` | `agent/registry.go` | Multi-agent routing: maps channel/account patterns to agent configs |

### 7.4 Infrastructure Layer

| Component | Package | Description |
|-----------|---------|-------------|
| `LLMProvider` (interface) | `providers/base.go` | `Chat(msgs, tools, model, opts) → LLMResponse` |
| `OpenAIProvider` | `providers/openai.go` | OpenAI / Azure completions + streaming |
| `AnthropicProvider` | `providers/anthropic.go` | Claude API with thinking-block support |
| `OllamaProvider` | `providers/ollama.go` | Local Ollama with auto-discovery |
| `GeminiProvider` | `providers/gemini.go` | Google Gemini API integration |
| `SessionStore` | `session/store.go` | Atomic JSON file persistence with crash-safe writes |
| `HALManager` | `hal/manager.go` | Unified peripheral bus: I2C, SPI, GPIO, CAN |
| `MCPClient` | `mcp/client.go` | Model Context Protocol client for external tool servers |
| `CronScheduler` | `cron/scheduler.go` | Persistent cron with file-backed job storage |
| `MessageBus` | `bus/queue.go` | Bounded async channel with backpressure and graceful drain |

---

## 8. Data Model & Table Schemas

OmniClaw uses a **zero-dependency embedded data store**. All state is persisted as JSON files under the workspace directory. The logical data model is described below as relational schemas for clarity.

### 8.1 `sessions`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `session_key` | TEXT | PRIMARY KEY | Composite key: `"{channel}:{chat_id}"` |
| `agent_id` | TEXT | NOT NULL | Owning agent identifier |
| `messages` | JSON[] | NOT NULL DEFAULT [] | Array of Message objects (role, content, tool_calls, timestamp) |
| `summary` | TEXT | NULLABLE | LLM-generated memory consolidation summary |
| `last_consolidated` | INTEGER | DEFAULT 0 | Index of last consolidated message |
| `created_at` | TIMESTAMP | NOT NULL | Session creation time |
| `updated_at` | TIMESTAMP | NOT NULL | Last activity time |

### 8.2 `agents`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `agent_id` | TEXT | PRIMARY KEY | Unique agent identifier |
| `model` | TEXT | NOT NULL | Default LLM model identifier |
| `system_prompt` | TEXT | NOT NULL | Base system prompt template |
| `workspace` | TEXT | NOT NULL | Absolute path to agent workspace directory |
| `max_iterations` | INTEGER | DEFAULT 40 | Max tool-call loop iterations |
| `max_tokens` | INTEGER | DEFAULT 4096 | Max LLM response tokens |
| `temperature` | REAL | DEFAULT 0.1 | LLM sampling temperature |
| `tools_enabled` | TEXT[] | DEFAULT ["*"] | Allowlist of tool names (* = all) |
| `candidates` | JSON[] | DEFAULT [] | Fallback provider chain configuration |

### 8.3 `skills`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `skill_id` | TEXT | PRIMARY KEY | Unique skill package identifier |
| `name` | TEXT | NOT NULL | Human-readable skill name |
| `version` | TEXT | NOT NULL | Semantic version |
| `source` | TEXT | NOT NULL | Registry URL or local path |
| `prompt_file` | TEXT | NULLABLE | Path to SKILL.md prompt instructions |
| `tools` | JSON[] | DEFAULT [] | Additional tool definitions bundled with skill |
| `installed_at` | TIMESTAMP | NOT NULL | Installation time |

### 8.4 `cron_jobs`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `job_id` | TEXT | PRIMARY KEY | UUID for the cron job |
| `schedule` | TEXT | NOT NULL | Cron expression (e.g., `"0 9 * * *"`) |
| `prompt` | TEXT | NOT NULL | Message to send to the agent |
| `channel` | TEXT | NOT NULL | Target channel for response delivery |
| `chat_id` | TEXT | NOT NULL | Target chat ID |
| `enabled` | BOOLEAN | DEFAULT TRUE | Whether the job is active |
| `last_run` | TIMESTAMP | NULLABLE | Last execution time |

### 8.5 `approval_policies`

| Column | Type | Constraints | Description |
|--------|------|-------------|-------------|
| `policy_id` | TEXT | PRIMARY KEY | Policy identifier |
| `tool_pattern` | TEXT | NOT NULL | Glob matching tool names (e.g., `"exec*"`) |
| `action` | TEXT | NOT NULL | `ALLOW` / `DENY` / `ASK_USER` |
| `conditions` | JSON | DEFAULT {} | Extra conditions (workspace_only, max_timeout, etc.) |
| `priority` | INTEGER | DEFAULT 100 | Lower number = higher priority |

### 8.6 Entity Relationship Diagram

```
  ┌──────────┐       1:N       ┌──────────────┐
  │  agents  │────────────────►│   sessions   │
  └────┬─────┘                 └──────┬───────┘
       │ 1:N                          │ 1:N
       ▼                              ▼
  ┌──────────┐                 ┌──────────────┐
  │  skills  │                 │   messages   │
  └──────────┘                 └──────────────┘
       │
       │ N:M
       ▼
  ┌──────────────────┐         ┌──────────────┐
  │ approval_policies│         │  cron_jobs   │
  └──────────────────┘         └──────────────┘
```

---

## 9. End-to-End Request Flow

| Step | Action |
|------|--------|
| 1 | User sends "Summarize my notes" via Telegram |
| 2 | `TelegramAdapter` receives webhook, creates `InboundMessage{channel:"telegram", chat_id:"12345", content:"Summarize my notes"}` |
| 3 | Adapter publishes `InboundMessage` to `MessageBus.inbound` queue |
| 4 | `AgentLoop.Run()` polls the bus, receives the message |
| 5 | `AgentRegistry.ResolveRoute()` determines `agent_id` and `session_key` from channel + metadata |
| 6 | `SessionStore` loads conversation history for `session_key` |
| 7 | `ContextBuilder` assembles: `[system_prompt, memory_summary, history..., user_message]` |
| 8 | `AgentLoop` calls `LLMProvider.Chat(messages, tools, model, opts)` |
| 9 | LLM responds with `tool_call: read_file(path="~/notes.md")` |
| 10 | `ApprovalPipeline` evaluates `read_file` → `WorkspacePolicy: ALLOW` (read-only, within workspace) |
| 11 | `ToolRegistry.Execute("read_file", {path:"~/notes.md"})` → returns file contents |
| 12 | `ContextBuilder` appends assistant + tool_result messages; loop re-calls LLM (iteration 2) |
| 13 | LLM responds with final text answer (no tool calls). Loop exits |
| 14 | Session history updated with new messages. `SessionStore.Save()` performs atomic write |
| 15 | `AgentLoop` publishes `OutboundMessage{channel:"telegram", chat_id:"12345", content:"..."}` |
| 16 | `TelegramAdapter` receives `OutboundMessage`, calls Telegram `sendMessage` API |

---

## 10. Sequence Diagrams

### 10.1 Standard Chat Flow

```
  User      Channel     MessageBus   AgentLoop    LLMProvider  ToolRegistry
   │          │             │            │             │            │
   │──msg────►│             │            │             │            │
   │          │──Inbound───►│            │             │            │
   │          │             │──consume──►│             │            │
   │          │             │            │──build_ctx  │            │
   │          │             │            │──Chat()────►│            │
   │          │             │            │◄─tool_call──│            │
   │          │             │            │──Execute()─────────────►│
   │          │             │            │◄─result─────────────────│
   │          │             │            │──Chat()────►│            │
   │          │             │            │◄─final_txt──│            │
   │          │             │◄─Outbound──│             │            │
   │          │◄─deliver────│            │             │            │
   │◄─reply───│             │            │             │            │
```

### 10.2 Subagent Spawn Flow

```
  ParentAgent  SpawnTool  SubagentMgr  ChildAgent   LLMProvider
       │           │           │            │            │
       │──spawn()─►│           │            │            │
       │           │──create()►│            │            │
       │           │           │──new_loop()►│           │
       │           │           │            │──Chat()───►│
       │           │           │            │◄─response──│
       │           │◄─result───│◄───────────│            │
       │◄─tool_res─│           │            │            │
```

### 10.3 Memory Consolidation Flow

```
  AgentLoop    MemoryStore    LLMProvider    SessionStore
      │             │              │              │
      │──check()───►│              │              │
      │             │──summarize()►│              │
      │             │◄─summary─────│              │
      │             │──archive()───────────────►│
      │             │──save()──────────────────►│
      │◄─done───────│              │              │
```

### 10.4 Approval-Gated Execution

```
  AgentLoop   ApprovalPipeline   ExecTool     User (Control UI)
      │              │              │              │
      │──evaluate()─►│              │              │
      │              │──check_wsp()─┤              │
      │              │──check_fs()──┤              │
      │              │──check_exec()┤              │
      │              │──ASK_USER────────────────►│
      │              │◄─────────APPROVE────────────│
      │◄─ALLOW───────│              │              │
      │──Execute()──────────────────►│             │
      │◄─result─────────────────────│              │
```


---

## 11. Class Diagrams

### 11.1 Provider Hierarchy

```
  ┌─────────────────────────────────────┐
  │          <<interface>>              │
  │           LLMProvider               │
  ├─────────────────────────────────────┤
  │ + Chat(msgs, tools, model) Response │
  │ + GetDefaultModel() string          │
  │ + ListModels() []string             │
  └───────────┬─────────────────────────┘
              │ implements
    ┌─────────┼──────────┬──────────────┐
    ▼         ▼          ▼              ▼
┌────────┐┌────────┐┌──────────┐┌───────────┐
│ OpenAI ││Anthropic││  Ollama  ││  Gemini   │
│Provider││Provider ││ Provider ││ Provider  │
└────────┘└────────┘└──────────┘└───────────┘
```

### 11.2 Tool Hierarchy

```
  ┌─────────────────────────────────────┐
  │          <<interface>>              │
  │              Tool                   │
  ├─────────────────────────────────────┤
  │ + Name() string                     │
  │ + Description() string              │
  │ + Schema() JSONSchema               │
  │ + Execute(args) (string, error)     │
  └───────────┬─────────────────────────┘
              │ implements
    ┌─────────┼─────────┬──────────┬──────────┐
    ▼         ▼         ▼          ▼          ▼
┌────────┐┌────────┐┌────────┐┌─────────┐┌────────┐
│ExecTool││ReadFile││WebSearch││ HALTool ││MCPTool │
└────────┘└────────┘└────────┘└─────────┘└────────┘
```

### 11.3 Core Composition

```
  ┌──────────────────────┐    owns    ┌────────────────┐
  │     AgentLoop        │───────────►│  ToolRegistry  │
  ├──────────────────────┤            ├────────────────┤
  │ - bus: MessageBus    │            │ - tools: map   │
  │ - provider: Provider │            │ + Register()   │
  │ - sessions: Store    │            │ + Execute()    │
  │ - context: Builder   │            │ + GetDefs()    │
  │ - registry: AgentReg │            └────────────────┘
  │ + Run()              │    owns    ┌────────────────┐
  │ + Stop()             │───────────►│ SubagentManager│
  │ + ProcessDirect()    │            ├────────────────┤
  └──────────────────────┘            │ + Spawn()      │
           │                          │ + Cancel()     │
           │ owns                     └────────────────┘
           ▼
  ┌────────────────┐      owns     ┌────────────────┐
  │ ContextBuilder │──────────────►│  MemoryStore   │
  ├────────────────┤               ├────────────────┤
  │ + BuildMsgs()  │               │ + Consolidate()│
  │ + AddAssistant │               │ + Archive()    │
  │ + AddToolRes() │               │ + GetSummary() │
  └────────────────┘               └────────────────┘
```

---

## 12. Data Flow Diagrams (DFDs)

### 12.1 Level-0 Context DFD

```
  ┌────────┐                              ┌────────────┐
  │External│    user message              │   LLM      │
  │ User   │──────────────►┌──────────┐──►│  Provider  │
  └────────┘               │ OmniClaw │   └──────┬─────┘
       ▲                   │  System  │          │
       │    response       └────┬─────┘    AI response
       └────────────────────────┘
```

### 12.2 Level-1 DFD

```
                           ┌─────────────────┐
  User ──msg──► [1.0       │  D1: Session    │
                Channel    │    Store        │
                Adapter]   └───────┬─────────┘
                   │               │ history
                   ▼               │
              [2.0 Agent    ◄──────┘
               Loop]────────────────────────► [3.0 Tool
                   │                           Executor]
                   │ context                      │
                   ▼                              │ result
              [4.0 LLM     ◄──────────────────────┘
               Provider]
                   │
                   ▼ response
              [5.0 Response
               Dispatcher] ──reply──► User
```

### 12.3 Level-2 DFD — Agent Layer Decomposition

```
  InboundMsg ──► [2.1 Router] ──► [2.2 Session Loader]
                                         │
                                         ▼
                                  [2.3 Context Builder] ◄── D2: Skills
                                         │
                                         ▼
                                  [2.4 LLM Caller] ──► D3: Provider Config
                                         │
                                    tool_call?
                                   ┌─────┴──────┐
                                   ▼             ▼
                            [2.5 Approval  [2.6 Response
                             Pipeline]     Formatter]
                                   │             │
                                   ▼             ▼
                            [2.7 Tool       OutboundMsg
                             Executor]
                                   │
                                   ▼
                              Loop back to 2.4
```

---

## 13. Functional Requirements

| ID | Category | Requirement |
|----|----------|-------------|
| FR-01 | Chat | System shall accept natural-language messages from any registered channel |
| FR-02 | Chat | System shall maintain per-session conversation history |
| FR-03 | Tools | System shall expose file read/write/edit, shell exec, web search, and web fetch as built-in tools |
| FR-04 | Tools | System shall allow runtime registration of additional tools via MCP or skill packages |
| FR-05 | Memory | System shall consolidate session history when message count exceeds configurable window |
| FR-06 | Memory | System shall persist memory summaries across daemon restarts |
| FR-07 | Subagents | System shall allow an agent to spawn child agents with isolated sessions |
| FR-08 | Subagents | System shall enforce configurable allowlists for subagent spawning |
| FR-09 | Cron | System shall execute scheduled prompts at user-defined intervals |
| FR-10 | Multi-Agent | System shall route messages to agents based on channel, account, and peer patterns |
| FR-11 | Skills | System shall discover, install, and activate skill packages from remote registries |
| FR-12 | Gateway | System shall expose an HTTP API compatible with OpenAI chat completions format |
| FR-13 | Gateway | System shall serve a browser-based Control UI for session management |
| FR-14 | Auth | System shall require token-based authentication for all gateway connections |
| FR-15 | HAL | System shall expose hardware I/O (I2C, SPI, GPIO) as standard tool interfaces |

---

## 14. Non-Functional Requirements

| ID | Category | Requirement | Target |
|----|----------|-------------|--------|
| NFR-01 | Performance | Cold-start to first-message-accepted | < 1 second |
| NFR-02 | Performance | Agent loop iteration latency (excluding LLM) | < 50 ms |
| NFR-03 | Memory | Resident memory at idle (single agent, no sessions) | < 15 MB |
| NFR-04 | Memory | Per-session memory overhead | < 100 KB |
| NFR-05 | Scalability | Concurrent active sessions | ≥ 100 |
| NFR-06 | Scalability | Peer nodes in mesh | ≥ 10 |
| NFR-07 | Reliability | Session data durability on crash | Zero loss (atomic writes) |
| NFR-08 | Reliability | LLM provider failover time | < 2 seconds |
| NFR-09 | Security | Tool execution without approval | Read-only tools only |
| NFR-10 | Security | Filesystem access outside workspace | Denied by default |
| NFR-11 | Portability | Supported architectures | x86-64, ARM64, RISC-V |
| NFR-12 | Portability | Supported OS | Linux, macOS, Windows (WSL) |
| NFR-13 | Observability | Log format | Structured JSON with levels |
| NFR-14 | Observability | Tracing | OpenTelemetry-compatible spans |

---

## 15. Technical Stack

| Layer | Technology | Rationale |
|-------|-----------|-----------|
| **Core Language** | Go 1.22+ | Static binary, goroutine concurrency, <10 MB binaries, cross-compilation |
| **CLI Framework** | cobra + readline | PicoClaw-proven, rich subcommand UX |
| **HTTP/WS Server** | net/http + gorilla/websocket | Zero-dependency, production-grade |
| **LLM Integration** | Custom provider interfaces | Uniform `Chat()` contract across OpenAI, Anthropic, Gemini, Ollama |
| **Session Persistence** | JSON files (atomic write) | No external DB dependency; crash-safe via rename |
| **Configuration** | JSON + env vars | Convention-over-config with `.env.example` |
| **Hardware I/O** | periph.io / direct syscall | I2C, SPI, GPIO on Linux; safe no-ops on other OS |
| **MCP Client** | JSON-RPC over stdio | Model Context Protocol for external tool servers |
| **Control UI** | Embedded SPA (Preact + Vite) | Lightweight browser UI served from binary |
| **Testing** | Go testing + testify | Unit + integration with mock providers |
| **CI/CD** | GitHub Actions + GoReleaser | Cross-platform binary releases |
| **Containerization** | Docker (multi-stage, scratch) | Minimal image: ~15 MB |

---

## 16. Repository Structure

```
omniclaw/
├── cmd/
│   └── omniclaw/
│       ├── main.go                    # Entry point + CLI setup
│       └── internal/
│           ├── agent/                 # Agent subcommand
│           ├── gateway/               # Gateway subcommand
│           ├── auth/                  # Auth subcommand
│           ├── cron/                  # Cron subcommand
│           ├── skills/                # Skills subcommand
│           ├── onboard/               # Onboarding wizard
│           └── version/               # Version subcommand
├── pkg/
│   ├── agent/
│   │   ├── loop.go                    # Core AgentLoop
│   │   ├── context.go                 # ContextBuilder
│   │   ├── memory.go                  # MemoryStore
│   │   ├── registry.go                # AgentRegistry
│   │   ├── subagent.go                # SubagentManager
│   │   ├── approval.go                # ApprovalPipeline
│   │   └── fallback.go                # FallbackChain
│   ├── bus/
│   │   └── queue.go                   # MessageBus
│   ├── channels/
│   │   ├── adapter.go                 # ChannelAdapter interface
│   │   ├── telegram/                  # Telegram implementation
│   │   ├── discord/                   # Discord implementation
│   │   ├── slack/                     # Slack implementation
│   │   ├── whatsapp/                  # WhatsApp implementation
│   │   ├── cli/                       # CLI REPL
│   │   └── web/                       # WebSocket bridge
│   ├── config/
│   │   ├── config.go                  # Config loading + validation
│   │   └── schema.go                  # Config struct definitions
│   ├── hal/
│   │   ├── manager.go                 # HAL Manager
│   │   ├── i2c.go                     # I2C tool
│   │   ├── spi.go                     # SPI tool
│   │   ├── gpio.go                    # GPIO tool
│   │   └── ros2_bridge.go             # ROS2 integration
│   ├── mcp/
│   │   └── client.go                  # MCP Client
│   ├── providers/
│   │   ├── base.go                    # LLMProvider interface
│   │   ├── openai.go                  # OpenAI provider
│   │   ├── anthropic.go               # Anthropic provider
│   │   ├── ollama.go                  # Ollama provider
│   │   └── gemini.go                  # Gemini provider
│   ├── routing/
│   │   └── router.go                  # Multi-agent routing
│   ├── session/
│   │   └── store.go                   # Session persistence
│   ├── skills/
│   │   ├── loader.go                  # Skill discovery + loading
│   │   └── registry.go                # Remote skill registry client
│   ├── tools/
│   │   ├── registry.go                # ToolRegistry
│   │   ├── exec.go                    # ExecTool
│   │   ├── filesystem.go              # Read/Write/Edit/ListDir
│   │   ├── web.go                     # WebSearch + WebFetch
│   │   ├── message.go                 # Cross-channel messaging
│   │   ├── spawn.go                   # Subagent spawner
│   │   └── mcp_tool.go               # MCP-proxied tools
│   └── gateway/
│       ├── server.go                  # HTTP/WS server
│       ├── auth.go                    # Auth + rate limiting
│       ├── discovery.go               # mDNS + Tailscale
│       ├── reload.go                  # Config hot-reload
│       └── ui/                        # Embedded Control UI
├── config/
│   └── config.example.json            # Example configuration
├── workspace/                          # Default agent workspace
├── scripts/                            # Build + release scripts
├── docs/                               # Documentation
├── Makefile                            # Build targets
├── go.mod / go.sum                     # Go modules
├── Dockerfile                          # Multi-stage container build
└── README.md                           # Project README
```

---

## 17. Deployment Strategy

### 17.1 Deployment Options

| Method | Command | Use Case |
|--------|---------|----------|
| **Single Binary** | `./omniclaw gateway` | Any Linux/macOS/Windows host |
| **Docker** | `docker run omniclaw:latest` | Cloud VMs, NAS devices |
| **Docker Compose** | `docker compose up` | Multi-service with reverse proxy |
| **systemd Service** | `systemctl start omniclaw` | Persistent Linux server |
| **Embedded** | Cross-compiled ARM binary | Raspberry Pi, RISC-V boards |

### 17.2 Configuration Hierarchy

```
  Environment Variables (highest priority)
         │
         ▼
  config.json (user configuration)
         │
         ▼
  Built-in Defaults (lowest priority)
```

### 17.3 Container Architecture

```dockerfile
# Stage 1: Build
FROM golang:1.22-alpine AS builder
WORKDIR /src
COPY . .
RUN CGO_ENABLED=0 go build -ldflags="-s -w" -o /omniclaw ./cmd/omniclaw

# Stage 2: Runtime
FROM scratch
COPY --from=builder /omniclaw /omniclaw
COPY --from=builder /etc/ssl/certs /etc/ssl/certs
ENTRYPOINT ["/omniclaw"]
CMD ["gateway"]
```

---

## 18. Fault Tolerance & Resilience

### 18.1 Failure Modes & Mitigations

| Failure Mode | Impact | Mitigation |
|-------------|--------|-----------|
| LLM provider 5xx/timeout | Agent cannot respond | FallbackChain retries with next candidate provider |
| LLM provider 429 rate-limit | Temporary unavailability | CooldownTracker backs off; routes to alternate provider |
| Crash during session save | Potential data loss | Atomic file writes (write-to-temp + rename) |
| MessageBus overflow | Dropped messages | Bounded channel with backpressure; publisher blocks |
| Channel adapter disconnect | Missing messages | Reconnect with exponential backoff; heartbeat probing |
| MCP server unreachable | MCP tools unavailable | Lazy connection with retry on next message |
| Disk full | Cannot persist sessions | Pre-flight disk check; warn via system event |
| OOM on constrained device | Process killed | Memory budgeting; aggressive context window limits |

### 18.2 Graceful Shutdown Sequence

```
  SIGTERM/SIGINT received
         │
         ▼
  1. Stop accepting new inbound messages
  2. Drain in-flight agent loops (30s timeout)
  3. Cancel active subagents
  4. Save all dirty sessions atomically
  5. Close MCP connections
  6. Close channel adapters
  7. Close HTTP/WS server
  8. Exit
```

---

## 19. Future Scope — Robotics Adoption

### 19.1 HAL Architecture

```
  ┌────────────────────────────────────────┐
  │           HAL Manager                  │
  │  ┌──────┐ ┌──────┐ ┌──────┐ ┌──────┐  │
  │  │ I2C  │ │ SPI  │ │ GPIO │ │ CAN  │  │
  │  │Driver│ │Driver│ │Driver│ │Driver│  │
  │  └──┬───┘ └──┬───┘ └──┬───┘ └──┬───┘  │
  │     │        │        │        │       │
  ├─────┼────────┼────────┼────────┼───────┤
  │     ▼        ▼        ▼        ▼       │
  │          Linux /dev/ Interfaces        │
  └────────────────────────────────────────┘
```

### 19.2 ROS2 Integration Bridge

| Component | Description |
|-----------|-------------|
| `ROS2Subscriber` | Listens on configurable ROS2 topics, translates sensor data to InboundMessages |
| `ROS2Publisher` | Converts agent tool-call outputs to ROS2 action goals or topic publications |
| `TFLookup` | Queries ROS2 TF tree for spatial awareness in agent context |
| `URDFParser` | Loads robot description for kinematic-aware tool definitions |

### 19.3 Realtime Priority Queue

Robotic sensor interrupts require sub-100ms response. The MessageBus will support a **priority lane** where HAL-originated messages bypass the standard FIFO queue and are processed immediately by a dedicated goroutine.

### 19.4 Safety Constraints for Physical Actuators

| Constraint | Implementation |
|-----------|---------------|
| Force limits | HAL tool validates torque/force before sending to actuator |
| Emergency stop | Global E-STOP channel cancels all pending HAL commands |
| Rate limiting | Max N actuator commands per second per joint |
| Simulation mode | HAL can run against a Gazebo/MuJoCo sim before physical execution |

---

## 20. Appendices

### Appendix A: Configuration Schema Reference

```json
{
  "agents": {
    "default": {
      "model": "gpt-4o",
      "system_prompt": "You are a helpful assistant.",
      "workspace": "./workspace",
      "max_iterations": 40,
      "max_tokens": 4096,
      "temperature": 0.1,
      "restrict_to_workspace": true
    }
  },
  "channels": {
    "telegram": { "token": "BOT_TOKEN", "enabled": true },
    "discord": { "token": "BOT_TOKEN", "enabled": false }
  },
  "gateway": {
    "port": 18789,
    "auth": { "mode": "token", "token": "SECRET" },
    "controlUi": { "enabled": true }
  },
  "tools": {
    "exec": { "timeout": 30, "restrict_to_workspace": true },
    "web": { "brave": { "api_key": "", "enabled": false } },
    "mcp": { "enabled": false, "servers": {} }
  },
  "memory": { "window": 100 },
  "cron": { "enabled": true },
  "hal": { "enabled": false, "devices": {} }
}
```

### Appendix B: API Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| POST | `/v1/chat/completions` | Token | OpenAI-compatible chat API |
| POST | `/v1/responses` | Token | OpenResponses API |
| GET | `/health` | None | Health check endpoint |
| WS | `/ws` | Token | WebSocket control plane |
| GET | `/ui/*` | Token | Control UI static assets |

### Appendix C: Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OMNICLAW_CONFIG_PATH` | `~/.omniclaw/config.json` | Config file location |
| `OMNICLAW_WORKSPACE` | `~/.omniclaw/workspace` | Default workspace |
| `OMNICLAW_GATEWAY_PORT` | `18789` | Gateway listen port |
| `OMNICLAW_LOG_LEVEL` | `info` | Log verbosity |
| `OPENAI_API_KEY` | — | OpenAI API key |
| `ANTHROPIC_API_KEY` | — | Anthropic API key |
| `GOOGLE_API_KEY` | — | Gemini API key |

### Appendix D: Glossary

| Term | Definition |
|------|-----------|
| **MCP** | Model Context Protocol — standardized interface for connecting LLMs to external tools |
| **HAL** | Hardware Abstraction Layer — unified interface for physical device I/O |
| **Skill** | Installable package containing prompt instructions and optional tool bindings |
| **FallbackChain** | Ordered list of LLM provider+model pairs tried sequentially on failure |
| **Cooldown** | Temporary back-off period after a provider returns rate-limit or server errors |
| **Consolidation** | LLM-driven summarization of old messages to reduce context window usage |

---

> **Document End** — OmniClaw Architecture v1.0
> Generated: March 2026
