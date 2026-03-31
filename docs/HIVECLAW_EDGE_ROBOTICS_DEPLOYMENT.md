# HiveClaw Edge and Robotics Deployment Guide

This guide describes the current supported deployment shape for HiveClaw on edge nodes and robotics-adjacent systems.

The key design rule is simple:

- simulation first
- deterministic bridge second
- explicit approval and policy gates before any hardware path

HiveClaw is not yet a production robot-autonomy stack. It is a bounded agent runtime with an emerging robotics control plane.

## Supported targets today

### Edge support

HiveClaw currently supports an edge-oriented runtime profile intended for:

- low-end CPU and memory nodes
- robot-side support nodes
- gateway-side operator stations with tighter budgets
- bounded local execution where browser and desktop-heavy features should be reduced

The edge profile currently enforces:

- reduced parallelism
- reduced Wasm memory budget
- reduced retrieval and runtime overhead
- browser automation disabled by default

Inspect the active profile with:

```bash
hiveclaw inspect runtime-profile
```

See also:

- [HIVECLAW_EDGE_MODE.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EDGE_MODE.md)

### Robotics support

HiveClaw currently supports:

- deterministic robotics contracts
- simulation-first robotics execution
- safety-envelope checks
- approval-required motion handling
- persisted robot state and robotics run inspection
- ROS2 bridge profiles compiled through explicit execution profiles

Available commands:

```bash
hiveclaw robotics simulate <fixture.json>
hiveclaw robotics ros2-simulate <fixture.json>
hiveclaw inspect robot-state [robot_id]
hiveclaw inspect ros2-profiles [profile_id]
hiveclaw inspect robotics-runs [robot_id]
```

Example fixtures:

- [simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/simulation_example.json)
- [ros2_simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/ros2_simulation_example.json)

## Safety assumptions

These assumptions are required for the current implementation:

- hardware actuation is not the default path
- motion is bounded by a deterministic safety envelope
- approval-required motion must remain approval-gated
- degraded local mode and active faults block motion
- ROS2 integration is routed through explicit bridge profiles, not arbitrary freeform tool calls
- fleet routing must prefer healthy, policy-eligible robot workers over convenience

## What is implemented versus what is not

### Implemented now

- simulation-first robotics flow
- deterministic contract compilation
- rejection and approval-required outcomes
- persisted safety events
- operator inspection for robot state and robotics runs
- ROS2 profile persistence and ROS2 bridge directive compilation
- bounded robot worker routing in the execution-worker model

### Not implemented yet

- real production ROS2 transport/session management
- autonomous multi-robot coordination
- production-grade fleet orchestration
- hardware certification or safety-case tooling
- robot-side secure update and lifecycle management
- guaranteed real-time control semantics

## Recommended workflow

Use this sequence for any new robotics workflow:

1. define the robotics contract and safety envelope
2. run `hiveclaw robotics simulate`
3. if the target path is ROS2-facing, run `hiveclaw robotics ros2-simulate`
4. inspect the resulting robot state, ROS2 profile, and robotics run output
5. only then consider any future hardware or live bridge rollout

## Deployment patterns

### Operator workstation

Use when:

- a human operator is supervising
- TUI/operator inspection is needed
- browser or desktop control may coexist with robotics diagnostics

Recommended:

- standard runtime profile or cluster profile
- full inspection enabled
- Chrome DevTools MCP optional

### Edge support node

Use when:

- the node is near the robot
- resource budget is limited
- the node is acting as a bounded executor or telemetry sidecar

Recommended:

- edge runtime profile
- no heavy browser features
- no assumption of desktop tooling

### Lab ROS2 simulation lane

Use when:

- you need ROS2 topic/service compilation without touching hardware
- you are validating bridge profiles and routing
- you are exercising policy and approval behavior

Recommended:

- `hiveclaw robotics ros2-simulate`
- explicit ROS2 bridge profiles
- robot workers with declared ROS2 profile bindings

## Known limitations

- the ROS2 path currently validates bridge profiles and compiles deterministic directives, but does not claim a finished live ROS2 transport layer
- robot workers are modeled as bounded execution workers; they are not yet a complete fleet-control subsystem
- “robotics” in HiveClaw currently means bounded contract execution, inspection, and routing discipline, not full autonomy

That honesty matters. The current platform is strong because it is explicit about boundaries instead of pretending unfinished control paths are production-ready.
