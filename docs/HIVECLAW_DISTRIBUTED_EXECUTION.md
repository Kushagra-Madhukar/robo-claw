# HiveClaw Distributed Execution and Swarm Constraints

This document explains what HiveClaw means today by:

- distributed execution
- worker routing
- swarm operation
- execution backends

It also makes clear what is implemented now versus what remains planned.

## Current Reality

HiveClaw is already able to reason about execution beyond one local process, but it is not yet a fully autonomous swarm runtime.

What is implemented now:

- a typed execution backend abstraction in `aria-core` and `aria-intelligence`
- local backend selection as the default path
- Docker backend execution for bounded shell workloads
- an explicit managed-VM execution profile boundary for higher-risk desktop-oriented work
- persisted execution backend profiles in the runtime store
- worker registration, heartbeat, and capability advertisement
- capability-aware worker routing for browser, desktop, GPU, robotics, and trust constraints
- parent/child delegated runs with mailbox delivery and operator inspection
- run-tree inspection for delegated and continued work

What is not yet implemented as a finished product surface:

- SSH backend live execution
- a live isolated VM desktop execution backend behind the managed-VM profile boundary
- true distributed mailbox delivery across multiple nodes
- fleet-wide work stealing or consensus scheduling
- full autonomous swarm planning across many machines
- robotics actuation across a production ROS2 fleet

## Execution Backend Model

HiveClaw separates execution concerns into explicit backend profiles.

Current backend classes:

- `local`
- `docker`
- `ssh` profile-driven setup exists; live execution still depends on a reachable target
- `vm` profile boundary exists; live backend execution is still planned

Backends are chosen based on:

- requested backend id, when explicit
- capability requirements
- trust level requirements
- worker availability
- operator policy

Important rule:

- backend availability is not enough to make a backend eligible
- the backend or worker must satisfy the task capability and trust constraints

## Worker Routing Model

Workers are not treated as generic compute slots.

Each worker advertises:

- backend id
- backend kind
- trust level
- availability status
- capability flags
- heartbeat freshness

Routing currently considers:

- browser requirement
- desktop/computer-control requirement
- GPU requirement
- robotics bridge requirement
- trust level
- backend compatibility

Workers that are stale, paused, degraded, or missing required capability are not eligible.

## What “Swarm” Means in HiveClaw

In HiveClaw, `swarm` should be understood in a disciplined way.

Today, swarm means:

- one runtime can manage multiple delegated runs
- delegated work can be inspected as a run tree
- workers can advertise capabilities and receive eligible work
- the system can grow toward distributed execution without replacing the runtime core

Today, swarm does not yet mean:

- arbitrary autonomous replication of tasks across a fleet
- unsupervised multi-node takeover logic
- shared global memory consistency across many agents
- reliable distributed coordination for robotics actuation

The project goal is to evolve from:

- local-first delegated execution

to:

- capability-routed multi-worker execution

and later to:

- supervised swarm and fleet orchestration

## Safe Backend Selection Guidance

Use `local` when:

- the task is low-risk
- the workspace is local
- the operator wants the fastest feedback loop

Use `docker` when:

- the task needs containment
- shell execution should stay bounded
- the workload can operate inside a containerized workspace view

Use planned `ssh` when it lands for:

- controlled remote machine execution
- machine-specific tasks that should not run locally

Use the current managed-VM profile boundary when:

- desktop-oriented work should not silently fall back to `local`
- the operator wants a visible boundary for future isolated desktop execution
- routing and approvals should already reflect a higher-risk execution class

Use the future live `vm` backend when it lands for:

- higher-risk computer-use tasks
- isolated desktop automation
- stronger separation between operator machine and agent actions

## Current Constraints

Operators should assume these constraints today:

- Docker support is real, but higher-level distributed orchestration is still maturing
- worker routing exists, but takeover/cancellation/retry should still be treated as supervised operations
- browser and computer-control tasks should stay on explicitly capable workers or the local trusted host
- robotics and fleet language in the roadmap describes direction, not finished production capability

## Recommended Operational Posture

For current use, the safest posture is:

1. keep `local` as the default backend
2. use `docker` for bounded remote-style execution needs
3. treat worker routing as capability-aware scheduling, not full swarm autonomy
4. keep a human in the loop for takeover, cancellation, and high-risk backend changes
5. use operator inspection before claiming a task is truly distributed

## Near-Term Roadmap Link

The next execution milestones after the current backend foundation are:

- SSH backend support
- VM execution profile boundary
- richer delegated work-tree and mailbox surfaces
- clearer distributed execution operator workflows
- eventually, robotics and fleet runtime maturation

See also:

- [HIVECLAW_EXECUTION_ROADMAP.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EXECUTION_ROADMAP.md)
- [HIVECLAW_IMPLEMENTATION_CHECKLIST.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md)
- [HIVECLAW_COMPUTER_RUNTIME.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_COMPUTER_RUNTIME.md)
