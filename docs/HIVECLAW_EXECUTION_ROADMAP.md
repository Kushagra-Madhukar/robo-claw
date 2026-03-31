# HiveClaw Execution Roadmap

This document turns the current architecture review and competitive analysis into a concrete execution roadmap for HiveClaw.

It is not a speculative vision note. It is the working program plan for making HiveClaw competitive with the strongest agent runtimes while preserving the parts of the architecture that are already better than the field.

Execution tracker:

- [HIVECLAW_IMPLEMENTATION_CHECKLIST.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md)

## Document Purpose

Use this document for:

- prioritizing work across quarters and sprint cycles
- deciding what to copy, what not to copy, and why
- aligning technical work with product impact
- preserving roadmap intent so we can resume planning later without losing context
- measuring whether HiveClaw is getting more useful, more usable, and more defensible

## Planning Principles

HiveClaw should not try to win by becoming a thinner clone of OpenClaw, Claude Code, Codex, or NeMo.

HiveClaw should win by combining:

- Rust-native runtime ownership
- stronger policy and trust boundaries
- typed tool and context orchestration
- better operator workflows
- better onboarding and ecosystem ergonomics
- computer, browser, multi-channel, and robotics continuity
- clear edge and fleet evolution paths

This means:

- preserve the core runtime architecture
- improve the product layer first
- add ecosystem portability second
- add broader execution surfaces third
- harden with evals and low-resource modes continuously

## Current Position

### Core strengths already in the codebase

- typed execution contracts and artifacts
- typed working set and context planning
- provider-aware fallback and circuit breaking
- workspace-level coordination
- policy-gated tools, secrets, browser state, and MCP boundaries
- operator inspection and audit surfaces
- browser and Chrome DevTools MCP support
- scheduler and background run support
- robotics-aware typed contracts and power hooks

### Core weaknesses relative to the strongest competitors

- onboarding is still too manual
- TUI and operator UX are functional but not premium
- rules and skills ergonomics are weaker than Claude Code and Codex
- ecosystem portability is not yet strong enough
- full computer control is missing
- remote execution backends are incomplete
- evals and regressions are not yet productized
- edge mode and robotics execution are early relative to the long-term positioning

## Competitive Benchmark Summary

This summary is the working benchmark lens for roadmap decisions.

| System | Main lesson to copy | Main thing not to copy |
|---|---|---|
| OpenClaw | Gateway/session ownership, context visibility, plugin ergonomics | Markdown-heavy runtime truth and prompt-bloat memory |
| Claude Code | Init flow, rule hierarchy, hooks, daily-use UX | Prompt-only governance without hard policy boundaries |
| Codex | Skills portability, tooling/docs polish, open standard posture | Loosening native trust boundaries for convenience |
| NeMo Agent Toolkit | Eval, observability, exporter model, A2A/fleet thinking | Heavy enterprise abstraction without product focus |
| Hermes Agent | Always-on execution model and cross-surface continuity | Autonomous skill growth without trust and review controls |
| Nanobot | Small-footprint thinking and ease of install | Oversimplifying away safety and typed orchestration |

## Copy / Do Not Copy Decisions

### Things HiveClaw should copy

- first-run initialization and project bootstrap
- rules hierarchy and compatibility imports
- lifecycle hook SDK
- skill portability and package install/update flows
- context visibility and token-budget inspection
- desktop and browser continuity
- clearer operator workbench flows
- evaluator and telemetry exporter abstractions
- low-resource deployment profiles

### Things HiveClaw should not copy

- markdown files as the primary runtime truth
- heuristic prompt rescue in place of typed execution
- brittle plugin mutation of prompt state without typed contracts
- unsafe self-modifying skill generation
- generic “agent OS” marketing without operational depth
- enterprise-weight abstractions before the user workflow is excellent

## Success Metrics

These are the metrics that matter for market impact, not just internal elegance.

### Adoption and first-run metrics

| Metric | Current intent | Target |
|---|---|---|
| Time to first productive run | Too manual | under 10 minutes |
| Time to bootstrap a new project | Too manual | under 5 minutes |
| Chrome MCP onboarding | Good but CLI-oriented | one guided setup flow |
| Skill install and bind flow | Internal-first | under 2 minutes |

### Operator experience metrics

| Metric | Target |
|---|---|
| Approval to post-approval response latency | under 2 seconds |
| Context inspection access | one command/panel away |
| Tool visibility explanation | always visible in operator UI |
| Run diagnosis without log digging | achievable from operator surface alone |

### Runtime quality metrics

| Metric | Target |
|---|---|
| Artifact-required task success rate | above 95 percent on golden scenarios |
| Tool execution success rate on approved actions | above 97 percent |
| Provider failover response after first-token hang | under 30 seconds |
| Workspace contention behavior | deterministic and explainable |

### Token and efficiency metrics

| Metric | Target |
|---|---|
| Context waste from duplicate blocks | near zero after planner inclusion |
| Token savings from schema and skill-on-demand loading | 25 to 40 percent |
| Edge-mode idle RSS for runtime without browser | under 120 MB |
| Edge-mode cold start | under 2 seconds to CLI readiness |

### Strategic differentiation metrics

| Metric | Target |
|---|---|
| Supported surfaces | TUI, WebSocket, Telegram, browser, computer runtime, remote backends |
| Fleet and robotics readiness | simulation-first bridge and ROS2 execution profile |
| Skill portability | import and package compatibility for major ecosystem standards |

## Delivery Model

This roadmap is organized into six phases.

Each phase is a market-facing milestone, not just an engineering milestone.

The order is ranked by combined product impact, differentiation, and enabling leverage.

## Phase 1: Operator Workbench and Onboarding

### Why this goes first

This phase has the highest immediate product impact.

The core runtime is already stronger than it feels. The fastest way to improve adoption, retention, trust, and demos is to make the operator surface excellent and make setup obvious.

### Product outcome

HiveClaw becomes usable and impressive on day one, not just architecturally strong on inspection.

### Sprint themes

#### Sprint 1A: Guided bootstrap

- add `hiveclaw init`
- create project bootstrap for rules, providers, default agent, workspace settings, MCP suggestions, browser setup, and optional Chrome MCP setup
- detect existing `AGENTS.md` and `CLAUDE.md` and offer import or merge
- add “recommended setup” and “edge setup” presets

#### Sprint 1B: Operator workbench core

- redesign TUI around workbench concepts instead of transcript-first shell
- add panes for:
  - live runs
  - approvals
  - active tools
  - visible and hidden tools
  - context plan
  - provider and MCP health
  - workspace lock state
  - scheduled jobs
- add command palette
- add richer approval cards with concise action summaries

#### Sprint 1C: Inspection surfaced as product

- expose current inspection primitives directly in TUI and CLI
- make `inspect-context`, `inspect-provider-payloads`, `inspect-mcp-*`, `inspect-scope-denials`, and run diagnostics visible from UI
- add “why this happened” summaries for:
  - tool hidden
  - provider skipped
  - ambiguity triggered
  - contract failure

### Crates and modules impacted

- `aria-x/src/tui.rs`
- `aria-x/src/bootstrap.rs`
- `aria-x/src/operator.rs`
- `aria-x/src/approvals.rs`
- `aria-x/src/gateway_runtime.rs`
- `aria-intelligence/src/context_planner.rs`
- `aria-intelligence/src/prompting.rs`
- docs and onboarding files

### Exit criteria

- first-run guided setup exists
- operator can diagnose a failed run without raw log archaeology
- approvals feel trustworthy and concise
- agent, tool, context, and provider state are inspectable from a single surface

### Impact

| Dimension | Rating |
|---|---|
| Market impact | Very high |
| Engineering leverage | High |
| Risk | Low to medium |
| Differentiation | High |

## Phase 2: Rules, Skills, Hooks, and Ecosystem Portability

### Why this goes second

Once the operator surface is credible, the next thing users expect is an ecosystem that is easy to extend and easy to carry across projects.

This phase turns HiveClaw from a powerful runtime into a platform.

### Product outcome

HiveClaw becomes easier to adopt, easier to customize, and easier to integrate into existing agent workflows.

### Sprint themes

#### Sprint 2A: Rules hierarchy

- add HiveClaw-native rules with layered precedence:
  - org
  - user
  - project
  - path-scoped
- support import compatibility for:
  - `AGENTS.md`
  - `CLAUDE.md`
- expose rule origin in inspections

#### Sprint 2B: Hook SDK

- add typed lifecycle hook bus
- support:
  - session start
  - prompt submit
  - pre-tool
  - permission request
  - post-tool
  - pre-compact
  - post-compact
  - subagent start and end
  - approval resume
  - session end
- forbid untyped prompt mutation and enforce hook contracts

#### Sprint 2C: Skill platform

- add skill registry UX
- add install, update, enable, disable, bind, inspect, and doctor flows
- support package signing or trust manifests
- align skill packaging with open-standard skill formats where practical
- add “skill on demand” loading to reduce token waste

### Crates and modules impacted

- `aria-core`
- `aria-intelligence/src/middleware.rs`
- `aria-intelligence/src/tools.rs`
- `aria-intelligence/src/prompting.rs`
- `aria-x/src/runtime_store/skills.rs`
- `aria-x/src/bootstrap.rs`
- new hook and rules modules if needed

### Exit criteria

- rules can be initialized, imported, layered, and inspected
- hooks exist with strong lifecycle coverage
- skills can be installed and managed like a product feature rather than a codepath

### Impact

| Dimension | Rating |
|---|---|
| Market impact | High |
| Engineering leverage | Very high |
| Risk | Medium |
| Differentiation | High |

## Phase 3: Computer Runtime and Surface Expansion

### Why this goes third

Browser access is useful. Full operator-grade computer use is what will materially change the perception of HiveClaw in the market.

This is the first major expansion phase that can create a standout feature set rather than just polish.

### Product outcome

HiveClaw gains a safe and inspectable computer-control layer that complements browser runtime and MCP browser integrations.

### Sprint themes

#### Sprint 3A: Computer runtime core

- create a dedicated `computer_runtime`
- support:
  - screenshot
  - cursor move and click
  - keyboard input
  - clipboard
  - window targeting
  - element-less screen interaction
- add policy gates, approvals, and audit trails

#### Sprint 3B: Unified surface model

- unify browser runtime, Chrome DevTools MCP, and computer runtime into one surface-selection model
- operator can see whether a task is using:
  - native browser runtime
  - Chrome DevTools MCP
  - full computer runtime
- tool selection explains why a surface was chosen

#### Sprint 3C: Safe desktop execution profiles

- add execution profiles for:
  - local trusted desktop
  - isolated VM desktop
  - headless remote browser
- default risky computer actions to constrained profiles

### Crates and modules impacted

- new `computer_runtime` module or crate
- `aria-core` surface and action contracts
- `aria-policy`
- `aria-x/src/approvals.rs`
- `aria-x/src/runtime_store`
- `aria-intelligence/src/tools.rs`

### Exit criteria

- computer runtime exists as a first-class capability
- browser and desktop actions are clearly separated and inspectable
- operator can constrain execution profile and approve high-risk actions

### Impact

| Dimension | Rating |
|---|---|
| Market impact | Very high |
| Engineering leverage | High |
| Risk | Medium to high |
| Differentiation | Very high |

## Phase 4: Remote Execution, Distributed Workers, and Swarm Foundation

### Why this goes fourth

This is where HiveClaw begins to become the “hive mind” platform rather than a single-node runtime with multiple adapters.

### Product outcome

HiveClaw can run tasks beyond one local machine and can reason about execution profiles in a way that sets up swarm and fleet behavior.

### Sprint themes

#### Sprint 4A: Remote execution backends

- add backend abstraction for:
  - local
  - Docker
  - SSH
  - isolated VM
- ensure all backends preserve approvals, policy checks, and artifact reporting

#### Sprint 4B: Worker plane

- add worker registration and heartbeat
- add capability advertisements per worker
- route work by constraints:
  - browser needed
  - GPU needed
  - robotics bridge needed
  - low-trust environment

#### Sprint 4C: Swarm task coordination

- explicit delegated work plans
- mailbox and run handoff improvements
- cancellation, retry, and takeover flows
- visibility into parent-child work trees

### Crates and modules impacted

- `aria-intelligence/src/runtime.rs`
- `aria-intelligence/src/router.rs`
- `aria-x/src/runtime_store/runs.rs`
- `aria-x/src/runtime_store/queues.rs`
- mesh and scheduler layers
- possible new worker/execution crate

### Exit criteria

- at least two non-local execution backends exist
- worker capability matching exists
- delegated work is visible, interruptible, and auditable

### Impact

| Dimension | Rating |
|---|---|
| Market impact | High |
| Engineering leverage | High |
| Risk | High |
| Differentiation | Very high |

## Phase 5: Evals, Telemetry, and Product Hardening

### Why this is essential

This phase is what turns a broad platform into a dependable one.

The market does not reward architectures forever. It rewards systems that can prove they keep working.

### Product outcome

HiveClaw gets regression resistance, measurable quality, and enterprise-facing credibility without becoming bloated.

### Sprint themes

#### Sprint 5A: Eval runner

- add scenario replay harness
- add contract satisfaction regression suites
- add tool-use and approval regression suites
- compare providers and fallback behavior over known tasks

#### Sprint 5B: Telemetry exporters and redaction

- add structured exporters
- add redaction rules for sensitive context, secrets, and user data
- support local-first defaults with optional external sinks

#### Sprint 5C: Benchmark dashboard

- measure:
  - task success
  - approval latency
  - token cost
  - compaction efficiency
  - tool usage patterns
  - provider skip and failover behavior
- turn inspections and traces into comparative dashboards

### Crates and modules impacted

- `aria-intelligence/src/telemetry.rs`
- learning and audit layers
- runtime store metrics and traces
- new eval crate or test harness modules
- docs and benchmark suites

### Exit criteria

- golden workflows are replayable
- regressions are measurable
- telemetry can be exported safely
- benchmark data influences roadmap decisions

### Impact

| Dimension | Rating |
|---|---|
| Market impact | Medium to high |
| Engineering leverage | Very high |
| Risk | Medium |
| Differentiation | High |

## Phase 6: Edge Mode, Robotics Runtime, and Fleet Maturity

### Why this comes last

This is the strategic moat, but it should land on a stable product base.

Trying to do this before the operator, ecosystem, and execution surfaces are mature would spread effort too thin.

### Product outcome

HiveClaw becomes the rare agent platform that can plausibly span laptop, node, edge device, and robot-adjacent execution.

### Sprint themes

#### Sprint 6A: Edge mode

- add low-resource execution profile
- tighten memory and token ceilings
- disable heavyweight subsystems by profile
- publish supported low-end targets

#### Sprint 6B: Robotics bridge maturity

- expand simulation-first robotics bridge
- add deterministic executor for robotics contracts
- add richer safety envelopes and degraded modes
- add robot-state inspection and runbook surfaces

#### Sprint 6C: Fleet and ROS2 integration

- add ROS2 bridge contracts
- add fleet health and routing model
- add policy-gated actuation workflows
- treat robots as bounded worker classes, not freeform tool endpoints

### Crates and modules impacted

- `aria-core/src/robotics.rs`
- `aria-intelligence/src/hardware.rs`
- `aria-x/src/robotics_bridge.rs`
- scheduler, worker, and mesh layers
- possible ROS2-specific bridge crate

### Exit criteria

- edge mode is documented and benchmarked
- robotics simulation workflows are validated
- ROS2 and fleet coordination are available behind explicit execution profiles

### Impact

| Dimension | Rating |
|---|---|
| Market impact | Medium initially, very high strategically |
| Engineering leverage | High |
| Risk | High |
| Differentiation | Extremely high |

## Cross-Cutting Delivery Lanes

These lanes should run across all phases rather than wait for a dedicated phase.

### Documentation lane

- keep README aligned to the live product
- keep architecture and roadmap documents current
- publish setup and migration docs per major feature

### Quality lane

- acceptance checks for new operator flows
- live validation for browser and computer integrations
- contract failure scenarios covered before feature promotion

### Security lane

- maintain native trust boundaries
- do not weaken vault, policy, scheduler, or runtime-store ownership
- keep human-in-the-loop guarantees strong as surfaces expand

### Token and context lane

- track token cost continuously
- measure tool schema cost
- prune prompt inflation
- keep retrieval and working-set inclusion explainable

## Short-Term Plan: Next 90 Days

If execution bandwidth is limited, this is the highest-value 90-day cut.

### Priority A

- `hiveclaw init`
- TUI operator workbench redesign
- inspect context/tool/provider state from UI
- concise approval redesign

### Priority B

- rule hierarchy
- `AGENTS.md` and `CLAUDE.md` import
- hook SDK v1
- skill install and bind flows

### Priority C

- computer runtime architecture spike
- first local screenshot and pointer control proof
- execution profile design for local versus isolated desktop control

## Long-Term Goal: 12-Month Product State

At the end of this roadmap, HiveClaw should be able to credibly present itself as:

- a local-first agent runtime with product-grade operator UX
- an ecosystem platform with rules, hooks, skills, and MCP portability
- a safe browser and computer control runtime
- a distributed worker and swarm-capable orchestration layer
- an eval-driven and inspectable system
- an edge- and robotics-aware platform with a real fleet path

## Sequencing Notes

### Why onboarding and operator UX come before computer control

Because current architecture is already strong enough that usability is the main blocker to market impact.

### Why evals are not phase one

Because they matter most after the product workflows are solid enough to benchmark meaningfully. Some quality work should still happen continuously.

### Why robotics is not phase one

Because robotics is a differentiator only if the operator, policy, execution, and observability layers are already credible.

## Risks and Anti-Patterns

Avoid these failure modes while executing the roadmap.

- adding impressive-looking features without making the operator surface better
- copying prompt-heavy memory models from weaker architectures
- merging native trust boundaries into generic MCP or plugin surfaces
- turning skills and hooks into unbounded code injection points
- building remote execution before there is good visibility into runs and approvals
- treating robotics as just another tool call instead of a bounded execution domain

## Project Management Guidance

Treat each phase as a product milestone with:

- one clear headline outcome
- one demo narrative
- one acceptance checklist
- one internal benchmark pack
- one docs update

Within each phase:

- reserve at least 20 percent of capacity for bug fixing and hardening
- reserve explicit time for live validation
- do not start the next phase before the current one has a demonstrable operator story

## Recommended Working Order

1. Phase 1: Operator Workbench and Onboarding
2. Phase 2: Rules, Skills, Hooks, and Ecosystem Portability
3. Phase 3: Computer Runtime and Surface Expansion
4. Phase 4: Remote Execution, Distributed Workers, and Swarm Foundation
5. Phase 5: Evals, Telemetry, and Product Hardening
6. Phase 6: Edge Mode, Robotics Runtime, and Fleet Maturity

## Final Steering Statement

HiveClaw does not need a backend rewrite to win.

It needs:

- a better first-run experience
- a better operator surface
- a stronger ecosystem layer
- broader execution surfaces
- measurable reliability
- a disciplined path into edge, swarm, and robotics

If we execute in that order, HiveClaw can stand out as the rare agent platform that is both technically serious and product-legible.
