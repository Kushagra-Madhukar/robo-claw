# HiveClaw Edge Mode

This document explains what `edge` mode means in HiveClaw today, what it is intended for, and what it does not claim yet.

## Purpose

Edge mode is the low-resource deployment profile for HiveClaw.

It is intended for:

- low-end CPU and memory nodes
- embedded Linux boxes and SBC-class devices
- robot-side support processes
- constrained local gateways where browser-heavy features would be a liability

It is not intended to imply full robot autonomy or full desktop-agent parity on weak hardware.

## What Edge Mode Changes

When `cluster.profile = "edge"`, HiveClaw clamps the active runtime budget:

- `max_parallel_requests` is capped at `2`
- `wasm_max_memory_pages` is capped at `96`
- `max_tool_rounds` is capped at `4`
- `retrieval_context_char_budget` is capped at `6000`
- `browser_automation_enabled` is forced `false`
- `learning_enabled` is forced `false`

These caps are applied at runtime even if the config file asks for larger values.

## Why These Limits Exist

Edge mode is designed to preserve predictable behavior under constrained conditions.

The current goals are:

- keep concurrency bounded
- keep prompt/context pressure bounded
- keep tool loops shorter
- disable heavy subsystems that can dominate CPU, RAM, or I/O
- reduce background write pressure from learning traces

## Intended Hardware Class

The current target class for edge mode is:

- SBCs and mini PCs with modest RAM
- robot companion nodes running bounded support logic
- small local control-plane processes on-device
- low-memory, single-tenant self-hosted nodes

A reasonable current mental model is:

- support node, not full workstation replacement
- bounded assistant runtime, not unconstrained multi-surface automation host

## Operational Expectations

Use edge mode when:

- you want predictable runtime ceilings
- browser automation is not required on-device
- learning traces are not critical on-device
- the device is acting as a bounded local executor or robotics-side support node

Avoid edge mode when:

- you need full browser/computer-use flows on the same machine
- you want wider retrieval context windows
- you want heavy multi-request parallelism
- you want long or tool-dense coding-agent sessions on-device

## Verification Surface

You can inspect the active runtime profile and effective budget with:

```bash
hiveclaw inspect runtime-profile
```

This reports:

- deployment profile
- effective runtime budget
- runtime store backend
- intended hardware class summary
- key edge-mode constraints

## Current Limits of the Feature

Edge mode today is a runtime-budget profile, not a fully separate product runtime.

That means:

- it does not yet measure or publish full device-footprint benchmarks automatically
- it does not yet enforce robotics-specific execution modes by itself
- it does not yet provide a dedicated edge-only UX surface
- it does not make unsupported heavy paths magically safe on weak hardware

## Relationship to Robotics

Edge mode is useful for robotics-adjacent deployments because it reduces overhead on robot-side nodes, but it is not the robotics safety system.

Robotics safety, simulation, deterministic execution, and ROS2 boundaries remain separate Phase 6 concerns.

## Related Docs

- [HIVECLAW_EXECUTION_ROADMAP.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EXECUTION_ROADMAP.md)
- [HIVECLAW_IMPLEMENTATION_CHECKLIST.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md)
- [HIVECLAW_DISTRIBUTED_EXECUTION.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_DISTRIBUTED_EXECUTION.md)
- [HIVECLAW_COMPUTER_RUNTIME.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_COMPUTER_RUNTIME.md)
