# ARIA-X — Autonomous Reasoning and Intelligence Architecture

A modular, multi-crate Rust agent runtime implementing the ReAct (Reasoning and Acting) pattern with zero-trust authorization, sandboxed Wasm execution, and mesh transport.

## Architecture

```
┌─────────────┐     ┌──────────────────┐     ┌───────────────┐
│ aria-gateway │────▶│ aria-intelligence │────▶│  aria-policy  │
│  (L1 Norm)  │     │ (L2 Router/Orch) │     │ (Cedar Auth)  │
└─────────────┘     └───────┬──────────┘     └───────────────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
      ┌────────────┐ ┌───────────┐ ┌───────────┐
      │ aria-mesh  │ │ aria-ssmu │ │aria-skill- │
      │ (L4 Zenoh) │ │  (RAG)   │ │  runtime   │
      └────────────┘ └───────────┘ │  (Wasm)   │
                                   └───────────┘
```

## Crates

| Crate | Purpose |
|-------|---------|
| `aria-core` | `#![no_std]` types: `AgentRequest`, `AgentResponse`, `ToolDefinition`, `HardwareIntent` |
| `aria-mesh` | L4 Zenoh mesh pub/sub transport |
| `aria-policy` | Cedar zero-trust policy engine |
| `aria-skill-runtime` | Extism Wasm executor with strict sandboxing |
| `aria-ssmu` | PageIndex tree + session memory (RAG engine) |
| `aria-intelligence` | Semantic router, dynamic tool cache, ReAct orchestrator |
| `aria-gateway` | Gateway adapters (Telegram, CLI) for signal normalization |
| `aria-x` | Final orchestrator binary |

## Quick Start

```bash
# Build the entire workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Run the orchestrator
cargo run -p aria-x -- aria-x/config.toml
```

## Multi-Node Deployment (Zenoh)

ARIA-X nodes discover each other automatically via Zenoh peer-to-peer:

```bash
# Terminal 1: Start orchestrator node
cargo run -p aria-x -- aria-x/config.toml

# Terminal 2: Start a relay node (uses same Zenoh mesh)
# Nodes auto-discover on the local network via multicast.
# For remote nodes, configure endpoints in config.toml:
#   [mesh]
#   endpoints = ["tcp/192.168.1.100:7447"]
```

## Configuration

See [`aria-x/config.toml`](aria-x/config.toml) for all options:

- **LLM backend**: `mock`, `ollama`, `claude`
- **Policy**: Path to Cedar `.cedar` policy file
- **Gateway**: `cli` or `telegram`
- **Mesh**: Zenoh `peer` or `client` mode

## Development

```bash
# Format
cargo fmt --all

# Lint
cargo clippy --workspace -- -D warnings

# Test a specific crate
cargo test -p aria-policy
```

## License

Private — All rights reserved.
