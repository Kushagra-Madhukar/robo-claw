## HiveClaw Node Layout

This folder contains example configurations for running HiveClaw in a multi-node
topology using the existing `aria-x` orchestrator binary.

Each node is started by pointing `aria-x` at the appropriate config file:

```bash
cargo run -p aria-x -- nodes/orchestrator.toml
cargo run -p aria-x -- nodes/relay.toml
cargo run -p aria-x -- nodes/companion.toml
cargo run -p aria-x -- nodes/micro.toml
```

- `orchestrator` node: full ReAct loop, policy checks, SSMU, and LLM backends.
- `relay` node: edge execution + sensor routing.
- `companion` node: user-facing device (e.g. phone / laptop) with ROS2 bridge.
- `micro` node: embedded MCU-style node running distilled Wasm skills only.

