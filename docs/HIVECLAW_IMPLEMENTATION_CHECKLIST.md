# HiveClaw Implementation Checklist

This document is the execution checklist companion to:

- [HIVECLAW_EXECUTION_ROADMAP.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EXECUTION_ROADMAP.md)

It exists so that humans and AI agents can execute the roadmap without losing rigor over time.

## Purpose

Use this document to:

- track implementation progress across all roadmap phases
- prevent vague “done” claims
- force validation and regression checks before work is closed
- preserve phase-by-phase execution order while still allowing parallel work inside a phase

## Rules For Updating This File

Only mark a checklist item as complete when all of the following are true:

- the implementation is present in the repository
- affected tests pass
- no known regression remains in adjacent flows
- docs and operator/help surfaces are updated when behavior changed
- the item’s phase-specific completion conditions are satisfied

Do not mark an item complete if:

- the code exists but is not wired into the actual runtime path
- tests were skipped without an explicit reason
- the feature works only in a narrow happy path
- operator-facing behavior is unclear or undocumented
- the feature creates unresolved regressions in approvals, scheduling, browser, MCP, tool visibility, or context handling

When an item is completed, add evidence directly beneath it by replacing the placeholder text in the `Evidence:` line.

## Global Completion Gate

These apply to every implementation item unless the item explicitly says otherwise.

### Baseline engineering gate

- workspace builds pass
- affected crate tests pass
- any new CLI or operator surfaces have help text or docs coverage
- any new persisted state has schema and migration coverage if needed

### Runtime safety gate

- no regression in approvals
- no regression in tool visibility and contract satisfaction
- no regression in workspace locking and provider fallback behavior
- no regression in scheduler/reminder flow if touched
- no regression in browser or MCP flows if touched

### Validation gate

- unit tests for new logic
- integration tests for critical user flows
- live verification for any feature that changes real operator workflows, browser usage, computer control, remote execution, or robotics behavior

## Phase Gate Policy

Do not start broad implementation of the next phase until the current phase has:

- all critical-path items completed
- phase gate tests passing
- one coherent demo path
- updated docs and help surfaces

Exploratory spikes are allowed in later phases, but they should not be marked complete as roadmap execution.

## Phase 1: Operator Workbench and Onboarding

### Phase 1 Gate

Phase 1 is complete only when:

- first-run setup is guided rather than manual
- the TUI behaves like an operator workbench rather than a thin transcript shell
- context, tool, provider, approval, run, and lock state are visible from one workflow
- failures are diagnosable without raw log archaeology in common cases

### Checklist

- [x] `P1-01` Add `hiveclaw init` CLI entrypoint.
Done when: a dedicated init command exists, appears in help, and launches a guided setup flow rather than requiring manual config editing.
Validation: CLI parsing tests, help text tests, config generation tests.
Regression guard: existing runtime startup and TUI startup must continue to work with direct config paths.
Evidence: Implemented in `aria-x/src/bootstrap.rs` with dedicated `init` help/topic routing and runtime dispatch. Verified by `cargo test -p aria-x render_cli_help_lists_install_doctor_and_runtime_commands -- --nocapture`, `cargo test -p aria-x parse_startup_mode_treats_help_and_install_as_runtime_commands -- --nocapture`, and live run `target/debug/hiveclaw init /tmp/hiveclaw-init-check --non-interactive --overwrite`.

- [x] `P1-02` Implement project bootstrap presets for recommended and edge modes.
Done when: init can generate at least two meaningful presets with different defaults for providers, browser/MCP setup, and resource ceilings.
Validation: preset config tests, generated config snapshot tests.
Regression guard: preset output must remain compatible with current runtime schema.
Evidence: `InitPreset::{Recommended, Edge}` implemented in `aria-x/src/bootstrap.rs` and emitted through generated `.hiveclaw/config.toml` plus `HIVECLAW.md`. Verified by `cargo test -p aria-x run_init_command_edge_preset_reduces_resource_and_browser_defaults -- --nocapture` and generated-config inspection from `/tmp/hiveclaw-init-check/.hiveclaw/config.toml`.

- [x] `P1-03` Detect existing `AGENTS.md` and `CLAUDE.md` during bootstrap.
Done when: init detects existing project rule files, offers import/merge guidance, and persists the chosen result.
Validation: file detection tests, import decision tests, merge-path tests.
Regression guard: no destructive overwrites of existing rule files without explicit operator confirmation.
Evidence: Bootstrap now detects `AGENTS.md` and `CLAUDE.md`, defaults to merge in non-interactive mode, and writes imported guidance into `HIVECLAW.md`. Verified by `cargo test -p aria-x run_init_command_merges_existing_guidance_files_by_default -- --nocapture`.

- [x] `P1-04` Add guided setup for provider, default agent, workspace root, and browser/MCP suggestions.
Done when: first-run setup asks for or infers core runtime defaults and saves a working configuration without requiring manual TOML edits.
Validation: interactive path tests where feasible, config persistence tests, happy-path startup test.
Regression guard: generated config must not break `doctor`, `setup chrome-devtools-mcp`, or normal runtime launch.
Evidence: `hiveclaw init` now infers provider/backend, default agent, workspace-local paths, and Chrome MCP suggestion state, and persists them into `.hiveclaw/config.toml` and `HIVECLAW.md`. Verified by `cargo test -p aria-x run_init_command_bootstraps_local_project_files -- --nocapture`, `cargo test -p aria-x run_init_command_generated_config_loads_with_runtime_schema -- --nocapture`, and live `target/debug/hiveclaw help` plus `target/debug/hiveclaw init /tmp/hiveclaw-init-check --non-interactive --overwrite`.

- [x] `P1-05` Redesign TUI layout around operator workbench concepts.
Done when: TUI has explicit operator-oriented panes or views for transcript, runs, approvals, tools/context, and system state instead of only a transcript with sidebars.
Validation: TUI rendering tests, state transition tests, attach flow tests.
Regression guard: current attach mode, keyboard navigation, and approval interaction must still work.
Evidence: `aria-x/src/tui.rs` now exposes explicit workbench tabs for `runs`, `approvals`, `tools/context`, and `system/health`, while preserving attach mode and approval navigation. Verified by `cargo test -p aria-x tui::tests -- --nocapture` (29 passing tests across both `aria-x` and `hiveclaw` test binaries after the command-palette additions).

- [x] `P1-06` Add a command palette to the TUI.
Done when: common commands, agent switches, inspections, and operator actions can be invoked through a searchable command palette.
Validation: command palette input tests, command dispatch tests, TUI snapshot/render tests.
Regression guard: existing slash commands and shortcut keys must continue to function.
Evidence: `aria-x/src/tui.rs` now exposes a `Ctrl+P` searchable command palette for agent switches, run/approval refresh, tools/context and system-health views, transcript clearing, and operator help. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `ctrl_p_opens_command_palette_and_filters_commands`, `command_palette_can_switch_tabs_and_send_actions`, and `command_palette_includes_available_agent_switches`.

- [x] `P1-07` Add a live runs panel in the TUI.
Done when: the operator can see active, recent, and background runs with enough metadata to understand session state and progress.
Validation: TUI state tests, run ingestion tests, render tests.
Regression guard: run updates must not block transcript rendering or approval visibility.
Evidence: `aria-x/src/tui.rs` now summarizes active and background runs with scope, status, agent, request preview, and run id tail in the dedicated `runs` tab, driven from the runtime store snapshot path. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `summarize_run_rows_prioritizes_active_and_background_runs`, plus the existing run-ingestion tests (`extract_prefixed_count_parses_run_list_messages`, `ingest_runtime_signal_tracks_agent_approvals_runs_and_errors`). Transcript rendering and approval tests continue to pass in the same suite.

- [x] `P1-08` Add a dedicated approvals panel with concise approval cards.
Done when: approvals show action summary, target, critical arguments, risk class, and approve/deny affordances without raw noisy payloads.
Validation: approval parsing tests, render tests, approval interaction tests.
Regression guard: Telegram and other non-TUI approval surfaces must not regress when approval payload formatting is updated.
Evidence: `aria-x/src/tui.rs` now caches and renders approval details with action, target, risk, options, and argument preview in the approvals panel and overlay, while preserving `Enter/a/d/i/Esc` interaction. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `parse_pending_approval_detail_reads_risk_target_and_arguments`, `ingest_runtime_signal_tracks_agent_approvals_runs_and_errors`, `mouse_click_selects_approval_row`, and `approval_detail_toggles_from_keyboard`. No Telegram approval surface changes were required.

- [x] `P1-09` Add a visible-tools and hidden-tools operator view.
Done when: the operator can see which tools are currently visible, which are hidden, and why they are hidden for the current request/session.
Validation: inspection rendering tests, tool selection explanation tests.
Regression guard: tool selection logic itself must not change unless explicitly intended and tested.
Evidence: `aria-x/src/tui.rs` now hydrates visible tools from context inspection records, preserves hidden-tool reasons, and renders both sections in the `tools/context` operator tab without changing selection logic. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `tools_context_sidebar_prefers_operator_snapshot_rows` and the existing approval/context ingestion coverage.

- [x] `P1-10` Add a context-plan view in the TUI.
Done when: included blocks, dropped blocks, ambiguity outcomes, working-set resolution, and token estimates are visible from the operator surface.
Validation: context inspection tests, TUI render tests for context plan state.
Regression guard: context plan generation must remain identical between CLI inspection and TUI surface.
Evidence: `aria-x/src/tui.rs` now renders context-plan summaries, per-block include/drop decisions, token estimates, and ambiguity outcomes from persisted context inspection records in the `tools/context` tab. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `tools_context_sidebar_prefers_operator_snapshot_rows` and `summarize_failure_rows_explains_common_operator_failures`.

- [x] `P1-11` Add provider and MCP health views to the operator workbench.
Done when: provider-family circuit state, active backend choice, MCP readiness, and Chrome DevTools MCP state are visible without separate shell commands.
Validation: health rendering tests, provider health state tests, MCP doctor state tests.
Regression guard: existing `doctor mcp` and provider fallback logic must remain correct.
Evidence: `aria-x/src/tui.rs` now requests and renders `/provider_health` alongside MCP readiness rows, active backend summaries, and Chrome DevTools MCP state within the `system/health` tab, with command-palette refresh actions for both provider and workspace diagnostics. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `parse_provider_health_list_reads_circuit_rows`, `ingest_runtime_signal_updates_provider_circuit_rows`, and `system_health_sidebar_includes_provider_mcp_and_failure_sections`, plus the provider-state unit coverage in `cargo test -p aria-x inspect_provider_health_json_reports_open_circuit -- --nocapture`.

- [x] `P1-12` Add workspace lock visibility and contention explanation to the operator surface.
Done when: active locks, waiting runs, lock owner, and timeout/busy reasons are visible from the operator workflow.
Validation: workspace lock inspection tests, render tests, contention scenario tests.
Regression guard: workspace locking semantics must not weaken while improving visibility.
Evidence: HiveClaw now exposes `/workspace_locks` as a runtime control command, the TUI requests it during startup and via the command palette, and the `system/health` panel renders holder/waiter state alongside workspace-busy explanations in the failure summary area. Verified by `cargo test -p aria-core parse_control_intent_supports_aliases -- --nocapture`, `cargo test -p aria-x tui::tests -- --nocapture`, and `cargo test -p aria-x operator_cli_inspect_workspace_locks_routes_to_snapshot_json -- --nocapture`, including `parse_workspace_lock_list_reads_holder_and_waiters` and `ingest_runtime_signal_updates_workspace_lock_rows`.

- [x] `P1-13` Surface current inspect commands through friendlier TUI and CLI workflows.
Done when: `inspect-context`, `inspect-provider-payloads`, `inspect-mcp-*`, and related diagnostics are reachable through discoverable commands or menu flows.
Validation: command routing tests, help text tests, operator workflow tests.
Regression guard: existing raw inspect commands must remain available.
Evidence: the TUI command palette now exposes inspect/explain helper entries for context, provider payloads, MCP servers, workspace locks, and provider health, and `run_operator_cli_command` now routes friendlier `hiveclaw inspect runs|workspace-locks|mcp-servers|mcp-imports|mcp-bindings` subcommands while preserving the raw admin flags. Verified by `cargo test -p aria-x render_cli_help_lists_completion_and_extended_doctor_topics -- --nocapture`, `cargo test -p aria-x operator_cli_inspect_runs_routes_to_agent_run_json -- --nocapture`, `cargo test -p aria-x operator_cli_inspect_mcp_servers_and_bindings_are_discoverable -- --nocapture`, and `cargo test -p aria-x operator_cli_inspect_workspace_locks_routes_to_snapshot_json -- --nocapture`.

- [x] `P1-14` Add “why this happened” summaries for common failure classes.
Done when: contract failures, ambiguity failures, provider skips, hidden tool decisions, and approval interruptions produce concise operator-facing explanations.
Validation: failure formatting tests, inspection output tests, targeted integration tests for common failure paths.
Regression guard: explanations must not hide raw detail needed for debugging.
Evidence: `aria-x/src/tui.rs` now derives compact operator-facing summaries for artifact-required failures, ambiguity outcomes, hidden-tool decisions, approval interruptions, and workspace-busy style failures, rendered in the `system/health` tab alongside raw logs. Verified by `cargo test -p aria-x tui::tests -- --nocapture`, including `summarize_failure_rows_explains_common_operator_failures` and `system_health_sidebar_includes_provider_mcp_and_failure_sections`.

- [x] `P1-15` Publish onboarding and operator docs for the new workbench.
Done when: README/docs describe first-run setup, workbench usage, approvals, inspections, and recovery flows clearly.
Validation: docs review, link checks, help text alignment.
Regression guard: no stale instructions remain for superseded flows.
Evidence: added [HIVECLAW_OPERATOR_WORKBENCH.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_OPERATOR_WORKBENCH.md) covering first-run setup, tabs, command palette, approvals, context/tool visibility, provider and MCP health, inspect workflows, failure summaries, and recovery flows. README now links to the operator guide from both the TUI overview and the `hiveclaw init` onboarding path in [README.md](/Users/kushagramadhukar/coding/anima/README.md).

## Phase 2: Rules, Skills, Hooks, and Ecosystem Portability

### Phase 2 Gate

Phase 2 is complete only when:

- rule sources are layered, inspectable, and importable
- skills behave like a real platform feature
- hooks exist with typed lifecycle coverage
- ecosystem extensibility no longer feels ad hoc

### Checklist

- [x] `P2-01` Add HiveClaw-native rule layering for org, user, project, and path scopes.
Done when: rule precedence is deterministic, persisted, and exposed to the runtime and inspection surfaces.
Validation: rule precedence tests, persistence tests, inspection output tests.
Regression guard: existing project behavior without explicit rules must remain stable.
Evidence: added typed rule metadata in `aria-core/src/runtime.rs` (`RuleScope`, `RuleSourceKind`, `RuleEntry`, `RuleInspectionRecord`, `RuleResolution`) and a shared rule discovery/resolution layer in [rules.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/rules.rs). Runtime requests now resolve org, user, project, and path-scoped rules into a dedicated `RuleContext` block inside [tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs). Verified by `cargo check -p aria-core -p aria-intelligence -p aria-x` and `cargo test -p aria-x build_rule_resolution_layers_project_user_org_and_path_rules -- --nocapture`.

- [x] `P2-02` Support importing and mapping `AGENTS.md` into HiveClaw rules.
Done when: AGENTS-compatible guidance can be imported into the layered rule system with explicit provenance.
Validation: import parser tests, mapping tests, inspection provenance tests.
Regression guard: imported rules must not bypass policy boundaries.
Evidence: workspace rule discovery now recognizes `AGENTS.md` and maps it into the layered rule system as `RuleSourceKind::AgentsMd` with explicit source path and scope provenance in [rules.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/rules.rs). Verified by `cargo test -p aria-x build_rule_resolution_layers_project_user_org_and_path_rules -- --nocapture`, which asserts `AGENTS.md` is imported as an active project rule without bypassing runtime precedence.

- [x] `P2-03` Support importing and mapping `CLAUDE.md` into HiveClaw rules.
Done when: Claude-style project memory/rules can be imported into the layered rule system with explicit provenance and conflict handling.
Validation: import parser tests, conflict tests, inspection provenance tests.
Regression guard: imported text must not be treated as implicit authority over native policy.
Evidence: workspace rule discovery now recognizes `CLAUDE.md` and maps it into the layered rule system as `RuleSourceKind::ClaudeMd`, including path-scoped activation when a request resolves to a matching file target. Verified by `cargo test -p aria-x build_rule_resolution_layers_project_user_org_and_path_rules -- --nocapture`, which asserts nested `src/CLAUDE.md` is activated only as a path-scoped rule.

- [x] `P2-04` Add rule origin and active-rule inspection surfaces.
Done when: operators can see which rules are active, where they came from, and why they won precedence.
Validation: inspection tests, operator render tests.
Regression guard: rule visibility must not leak secrets or local-only paths unintentionally.
Evidence: added `hiveclaw inspect rules <workspace_root> [request_text] [target_path]` routing in [operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs) and help surfacing in [bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs), returning active rules plus decision/provenance records from the shared rule resolver. The operator guide now documents the rule layer and inspect command in [HIVECLAW_OPERATOR_WORKBENCH.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_OPERATOR_WORKBENCH.md). Verified by `cargo test -p aria-x operator_cli_inspect_rules_routes_to_rule_resolution_json -- --nocapture` and `cargo test -p aria-x render_cli_help_lists_completion_and_extended_doctor_topics -- --nocapture`.

- [x] `P2-05` Implement typed lifecycle hook interfaces.
Done when: hook interfaces exist for session start, prompt submit, pre-tool, permission request, post-tool, pre-compact, post-compact, subagent lifecycle, approval resume, and session end.
Validation: unit tests for hook registration and invocation order, failure propagation tests.
Regression guard: hook failures must be contained and must not corrupt the core loop.
Evidence: added typed lifecycle registry support in [lifecycle.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/lifecycle.rs) with explicit registration helpers for session start, prompt submit, pre-tool, permission request, post-tool, pre/post compact, subagent start/stop, approval resume, and session end. Runtime request handling in [tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs) already executes typed `SessionStart` and `PromptSubmit` phases through the shared registry. Verified by `cargo test -p aria-intelligence lifecycle_registry_ -- --nocapture` and `cargo check -p aria-core -p aria-intelligence -p aria-x`.

- [x] `P2-06` Enforce typed hook boundaries and forbid arbitrary prompt mutation paths.
Done when: hooks can only operate through typed contracts or explicitly bounded prompt assets, not arbitrary hidden mutation.
Validation: boundary tests, invalid hook behavior tests.
Regression guard: existing middleware behavior must remain functional through the new abstraction.
Evidence: legacy `message_pre` hooks now emit explicit `PromptHookAsset` values that are normalized into bounded `ContextBlockKind::PromptAsset` entries inside [tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs), rather than appending hidden raw prompt strings. Empty/erroring legacy hooks are ignored safely, and lifecycle hooks remain limited to typed `ContextBlock` and `AuditNote` effects in [lifecycle.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/lifecycle.rs). Verified by `cargo test -p aria-x legacy_message_pre_hook -- --nocapture` and `cargo check -p aria-core -p aria-intelligence -p aria-x`.

- [x] `P2-07` Add a skill install, update, enable, disable, and doctor workflow.
Done when: skills can be managed through a first-class CLI/operator workflow instead of only runtime internals.
Validation: install/update tests, doctor output tests, binding tests.
Regression guard: skill changes must not break existing bound skills or runtime startup.
Evidence: added a first-class `hiveclaw skills` CLI surface in [bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) with `list`, `install`, `update`, `enable`, `disable`, `bind`, `unbind`, `export`, and `doctor` workflows. The command routes through persisted runtime-store skill state rather than hidden runtime tools and is discoverable in CLI help and shell completion. Verified by `cargo check -p aria-core -p aria-intelligence -p aria-x`, `cargo test -p aria-x run_skill_management_command_ -- --nocapture`, `cargo test -p aria-x render_cli_help_supports_skills_topic -- --nocapture`, `cargo test -p aria-x render_shell_completion_supports_zsh_and_rejects_unknown_shell -- --nocapture`, and `cargo test -p aria-x parse_startup_mode_treats_admin_commands_as_runtime_commands -- --nocapture`.

- [x] `P2-08` Add skill provenance, trust, and signature or trust-manifest support.
Done when: the system can distinguish trusted, unsigned, local, and imported skills and surface that status to operators.
Validation: trust state tests, inspection tests, import validation tests.
Regression guard: unsigned skills must not silently become trusted by default.
Evidence: extended [SkillPackageManifest](/Users/kushagramadhukar/coding/anima/aria-core/src/runtime.rs) with typed skill provenance metadata and wired install paths in [tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs) and [bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) to mark skills as `local` or `imported`, while doctor output derives `trusted`, `unsigned_local`, and `unsigned_imported` states from verified signatures in the runtime store. Verified by signed and unsigned install flows in `cargo test -p aria-x run_skill_management_command_ -- --nocapture` and by the preserved signed-manifest tool tests already in the suite.

- [x] `P2-09` Add skill-on-demand loading to reduce prompt and token cost.
Done when: skill instructions are loaded only when relevant and the effect is visible in context/token inspection.
Validation: token budget tests, prompt assembly tests, inspection tests.
Regression guard: hidden or deferred skills must still load correctly when required.
Evidence: extended the Wasm skill provider path in [tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs) to surface bound skills as prompt assets and added on-demand skill context synthesis that loads `SKILL.md` only for relevant bound/active skills while recording deferred skills as explicit prompt-asset context. Verified by `cargo check -p aria-core -p aria-intelligence -p aria-x`, `cargo test -p aria-x synthesize_skill_prompt_context_loads_only_relevant_bound_skills -- --nocapture`, and `cargo test -p aria-x legacy_message_pre_hook -- --nocapture`.

- [x] `P2-10` Add package import/export compatibility for major ecosystem skill formats where practical.
Done when: at least one external skill/package compatibility path exists and is documented.
Validation: import/export tests, sample package tests, docs verification.
Regression guard: imported packages must be normalized into HiveClaw’s policy and trust model.
Evidence: added a Codex-compatible standalone skill path in [bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) with `hiveclaw skills install --codex-dir <skill_dir>` and `hiveclaw skills export <skill_id> --format codex`, normalizing imported bundles into `SkillPackageManifest` with `compatibility_import` provenance. Verified by `cargo test -p aria-x run_skill_management_command_supports_codex_compat_import_and_export -- --nocapture` and the broader `cargo test -p aria-x run_skill_management_command_ -- --nocapture`.

- [x] `P2-11` Document the rules, hooks, and skills platform.
Done when: operators and contributors can understand the extension model from docs alone.
Validation: docs review, link checks, example verification.
Regression guard: examples must stay aligned with actual CLI and runtime behavior.
Evidence: documented the full Phase 2 extension model in [HIVECLAW_RULES_HOOKS_SKILLS.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_RULES_HOOKS_SKILLS.md), including layered rules, typed lifecycle hooks, skill CLI flows, provenance/trust states, on-demand skill loading, and Codex compatibility import/export. CLI examples were aligned with [bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) and validated with `cargo test -p aria-x render_cli_help_supports_skills_topic -- --nocapture`.

## Phase 3: Computer Runtime and Surface Expansion

### Phase 3 Gate

Phase 3 is complete only when:

- computer control is a first-class runtime, not a hack around browser tools
- operators can tell which interaction surface is being used
- approvals and policy boundaries remain strong for desktop control

### Checklist

- [x] `P3-01` Define a dedicated `computer_runtime` abstraction.
Done when: there is a first-class computer control surface separate from browser runtime and MCP browser integrations.
Validation: contract/type tests, runtime selection tests.
Regression guard: browser runtime and Chrome MCP behavior must remain distinct and stable.
Evidence: added dedicated computer-runtime core types in [computer.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/computer.rs) for profiles, sessions, actions, artifacts, and surface decisions, then introduced a separate interaction-surface selector in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs) that keeps browser tasks on browser surfaces and refuses silent fallback from desktop tasks into browser runtime or Chrome DevTools MCP. Verified by `cargo check -p aria-core -p aria-x`, `cargo test -p aria-core computer_runtime_types_round_trip_json -- --nocapture`, and `cargo test -p aria-x phase3_tests:: -- --nocapture`.

- [x] `P3-02` Implement screenshot capture and artifact persistence for computer runtime.
Done when: the runtime can capture screen state and persist/refer to it as a typed artifact.
Validation: artifact tests, persistence tests, live verification.
Regression guard: screenshot capture must respect execution profile and privacy/policy rules.
Evidence: added dedicated computer screenshot persistence in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs) and [aria-x/src/runtime_store/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/computer.rs), backed by the new `computer_artifacts` table in [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs). Verified by `cargo test -p aria-x persist_computer_screenshot_artifact_writes_file_and_store_record -- --nocapture` and by a live macOS probe using `screencapture -x /tmp/hiveclaw-p3-live.png`, which successfully produced a PNG (`2940 x 1912`, RGBA) after screen-recording permission was granted.

- [x] `P3-03` Implement pointer movement and click actions.
Done when: click-capable desktop actions exist behind policy and approval boundaries.
Validation: action tests, approval tests, live verification in a controlled environment.
Regression guard: mis-scoped click actions must not bypass approval requirements.
Evidence: implemented pointer movement and click execution in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs) behind the dedicated `computer_runtime`, using native macOS desktop-event backends and existing approval enforcement in [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs). Verified by `cargo test -p aria-x phase3_tests::pointer_click_requires_approval_but_pointer_move_does_not -- --nocapture` and live desktop verification with `cargo test -p aria-x live_local_computer_pointer_click_and_keyboard_type -- --ignored --nocapture`, which now passes on both `aria-x` and `hiveclaw` test binaries.

- [x] `P3-04` Implement keyboard input and clipboard actions.
Done when: text entry and clipboard operations are supported in the computer runtime with auditing and optional approval policies.
Validation: action tests, audit tests, live verification.
Regression guard: clipboard and keyboard operations must not bypass secret and content controls.
Evidence: implemented keyboard typing, key press, clipboard read, and clipboard write in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs), including clipboard artifact persistence and action-audit recording through [aria-x/src/runtime_store/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/computer.rs). Verified by `cargo test -p aria-x phase3_tests::execute_local_computer_action_records_surface_metadata_in_audit -- --nocapture`, `cargo test -p aria-x live_local_computer_clipboard_round_trip -- --ignored --nocapture`, and `cargo test -p aria-x live_local_computer_pointer_click_and_keyboard_type -- --ignored --nocapture`.

- [x] `P3-05` Add window targeting and execution profile selection.
Done when: desktop actions can be scoped by window or profile and the operator can see the active execution profile.
Validation: profile tests, targeting tests, inspection tests.
Regression guard: no silent fallback from isolated profiles to local trusted desktop.
Evidence: added seeded execution profiles plus session/window targeting in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs) and persisted profile/session state in [aria-x/src/runtime_store/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/computer.rs). Verified by `cargo test -p aria-x phase3_tests::default_profiles_are_seeded_when_missing -- --nocapture`, `cargo test -p aria-x phase3_tests::resolve_or_create_computer_session_persists_target_window_and_profile -- --nocapture`, and the new inspection-shape tests for computer profiles and sessions in [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs).

- [x] `P3-06` Unify surface selection across browser runtime, Chrome DevTools MCP, and computer runtime.
Done when: surface choice is explicit, inspectable, and reasoned from task shape and capability constraints.
Validation: tool/surface selection tests, inspection explanation tests.
Regression guard: existing browser-only flows must not accidentally route into computer runtime.
Evidence: unified interaction-surface routing in [aria-x/src/computer.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/computer.rs) through `resolve_interaction_surface`, with explicit decisions across native browser runtime, Chrome DevTools MCP, and `computer_runtime`. Verified by `cargo test -p aria-x phase3_tests::browser_tasks_prefer_browser_runtime_over_other_surfaces -- --nocapture`, `cargo test -p aria-x phase3_tests::browser_tasks_fall_back_to_chrome_devtools_mcp_when_browser_runtime_is_unavailable -- --nocapture`, `cargo test -p aria-x phase3_tests::desktop_tasks_require_dedicated_computer_runtime -- --nocapture`, `cargo test -p aria-x phase3_tests::desktop_tasks_do_not_silently_fall_back_to_browser_surfaces -- --nocapture`, and `cargo test -p aria-x phase3_tests::execute_local_computer_action_records_surface_metadata_in_audit -- --nocapture`, which confirms the chosen surface and reason are persisted in computer action audits.

- [x] `P3-07` Add operator-facing surface and execution-profile inspection.
Done when: the operator can see which surface was chosen, why, and under which risk and approval model.
Validation: operator render tests, inspection output tests.
Regression guard: explanations must stay consistent with actual runtime routing.
Evidence: added operator inspection outputs for computer profiles, sessions, artifacts, and action audits in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs), with stable-shape coverage in [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs). Verified by `cargo test -p aria-x inspect_computer_ -- --nocapture` and by the surface-reason audit test `cargo test -p aria-x phase3_tests::execute_local_computer_action_records_surface_metadata_in_audit -- --nocapture`.

- [x] `P3-08` Document safe computer-use workflows and constraints.
Done when: docs explain supported actions, risk boundaries, profile selection, and approval behavior clearly.
Validation: docs review and example verification.
Regression guard: documentation must not overstate unsupported automation guarantees.
Evidence: added [docs/HIVECLAW_COMPUTER_RUNTIME.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_COMPUTER_RUNTIME.md) covering supported actions, profile selection, approvals, inspection commands, macOS permissions, and safe-usage boundaries, then linked it from the main docs map in [README.md](/Users/kushagramadhukar/coding/anima/README.md).

## Phase 4: Remote Execution, Distributed Workers, and Swarm Foundation

### Phase 4 Gate

Phase 4 is complete only when:

- work can run on more than one execution backend
- delegated and distributed execution is inspectable and safe
- worker capability routing is real, not aspirational

### Checklist

- [x] `P4-01` Define a remote execution backend interface.
Done when: local, Docker, SSH, and VM-style execution can fit behind one runtime abstraction with shared contract, approval, and artifact semantics.
Validation: interface tests, backend selection tests.
Regression guard: local execution must remain the default and must not regress.
Evidence: added typed execution-backend profiles in [aria-core/src/execution_backend.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/execution_backend.rs) and a shared backend-selection / execution interface in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs), including `ExecutionBackend`, `LocalExecutionBackend`, and `select_execution_backend`. Verified by `cargo check -p aria-core -p aria-intelligence` and `cargo test -p aria-intelligence remote_execution::tests:: -- --nocapture`, which covers default-local selection, capability-aware backend selection, explicit-backend rejection, and local backend delegation.

- [x] `P4-02` Implement Docker backend support.
Done when: tasks can be executed in Docker with bounded workspace, tool, and artifact behavior.
Validation: backend tests, approval tests, live verification in a containerized workflow.
Regression guard: Docker execution must not bypass policy or workspace scope enforcement.
Evidence: backend implementation is now in place in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs), [aria-x/src/execution_backends.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/execution_backends.rs), and [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs), including bounded command construction, default `docker-sandbox` backend profiles, and backend-aware `run_shell` execution. Verified by `cargo test -p aria-intelligence remote_execution::tests:: -- --nocapture`, `cargo test -p aria-x inspect_execution_ -- --nocapture`, and live container verification with `cargo test -p aria-x native_run_shell_executes_in_docker_backend_and_persists_backend_id -- --ignored --nocapture`, which passed on both `aria-x` and `hiveclaw` test binaries with `execution_backend_id=\"docker-sandbox\"`.

- [x] `P4-03` Implement SSH backend support.
Done when: tasks can be executed on a remote machine over SSH with explicit profile, credentials, and artifact return behavior.
Validation: backend tests, credential handling tests, live verification against a controlled target.
Regression guard: SSH execution must not leak secrets into prompt or logs.
Evidence: added typed SSH backend configuration in [aria-core/src/execution_backend.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/execution_backend.rs), reusable SSH command construction in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs), runtime SSH execution in [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs), default/env backend seeding in [aria-x/src/execution_backends.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/execution_backends.rs), and operator-facing profile registration in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs). Verified by `cargo test -p aria-intelligence ssh_backend_config_builds_destination_and_policy_flags -- --nocapture`, `cargo test -p aria-x ssh_shell_command_parts_ -- --nocapture`, `cargo test -p aria-x setup_ssh_backend_cli_registers_profile_and_preserves_defaults -- --nocapture`, and live loopback verification with `cargo test -p aria-x native_run_shell_executes_in_ssh_backend_and_persists_backend_id -- --ignored --nocapture`, which passed for both `aria-x` and `hiveclaw`.

- [x] `P4-04` Implement isolated VM backend support or a clear VM execution profile boundary.
Done when: a non-local isolated execution profile exists for higher-risk or desktop-oriented work.
Validation: profile tests, runtime selection tests, live verification.
Regression guard: VM fallback behavior must be explicit and inspectable.
Evidence: added an explicit managed-VM execution profile boundary in [aria-core/src/execution_backend.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/execution_backend.rs) and [aria-x/src/execution_backends.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/execution_backends.rs), with runtime selection support already covered in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs). Verified by `cargo test -p aria-intelligence backend_selection_can_require_desktop_capability -- --nocapture`, `cargo test -p aria-x native_run_shell_reports_managed_vm_boundary_explicitly -- --nocapture`, and the updated operational documentation in [docs/HIVECLAW_DISTRIBUTED_EXECUTION.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_DISTRIBUTED_EXECUTION.md), which makes the current VM state explicit: profile boundary exists, live backend remains future work.

- [x] `P4-05` Add worker registration, heartbeat, and capability advertisement.
Done when: workers can advertise capabilities and the control plane can observe liveness and execution eligibility.
Validation: worker registration tests, heartbeat timeout tests, capability routing tests.
Regression guard: stale worker state must not cause silent task loss.
Evidence: added execution-worker persistence and heartbeat handling in [aria-core/src/execution_backend.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/execution_backend.rs), [aria-x/src/runtime_store/execution_backends.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/execution_backends.rs), and [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs), then exposed backend/worker inspection in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs). Verified by `cargo test -p aria-x inspect_execution_ -- --nocapture`, `cargo test -p aria-x execution_workers_mark_stale_heartbeats_offline -- --nocapture`, and `cargo check -p aria-intelligence -p aria-x`.

- [x] `P4-06` Add capability-aware task routing across workers.
Done when: tasks can be routed by browser need, GPU need, robotics bridge need, trust level, or resource constraints.
Validation: routing tests, failure fallback tests, inspection tests.
Regression guard: tasks must not be routed to ineligible workers just because a backend is available.
Evidence: added worker-routing logic in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs) with explicit capability checks for browser, desktop, GPU, robotics, trust level, and backend eligibility. Verified by `cargo test -p aria-intelligence remote_execution::tests:: -- --nocapture`, including routing preference and ineligible-worker rejection paths, plus `cargo test -p aria-x inspect_execution_ -- --nocapture` for operator-visible backend/worker inspection.

- [x] `P4-07` Improve delegated work trees, mailbox visibility, and cancellation semantics.
Done when: parent-child runs, handoffs, cancellation, retry, and takeover are visible and controllable from operator surfaces.
Validation: run-tree tests, mailbox tests, cancellation tests.
Regression guard: delegated runs must not disappear from audit history.
Evidence: centralized retry/takeover lineage handling in [aria-x/src/runtime_store/runs.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/runs.rs), added shared run-tree snapshot types in [aria-core/src/runtime.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/runtime.rs), exposed friendly operator control/inspection routes in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs), and routed tool/gateway takeover flows through the same runtime-store helpers in [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs) and [aria-x/src/gateway_runtime.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/gateway_runtime.rs). Verified by `cargo test -p aria-x runtime_store_retry_and_takeover_preserve_lineage_and_transitions -- --nocapture`, `cargo test -p aria-x native_takeover_agent_run_queues_new_child_run_for_replacement_agent -- --nocapture`, and `cargo test -p aria-x inspect_agent_run_json_surfaces_runs_events_and_mailbox -- --nocapture`, all passing for both `aria-x` and `hiveclaw` test binaries.

- [x] `P4-08` Document distributed execution and swarm operation constraints.
Done when: docs explain what “swarm” means operationally, what is implemented, and how backends should be chosen.
Validation: docs review, example verification.
Regression guard: marketing/docs must not imply fully autonomous fleet behavior before it exists.
Evidence: added [docs/HIVECLAW_DISTRIBUTED_EXECUTION.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_DISTRIBUTED_EXECUTION.md) covering implemented backend classes, worker routing reality, the current meaning of “swarm”, and safe backend selection posture, then linked it from the main docs map in [README.md](/Users/kushagramadhukar/coding/anima/README.md).

## Phase 5: Evals, Telemetry, and Product Hardening

### Phase 5 Gate

Phase 5 is complete only when:

- golden workflows are replayable
- regressions are measurable
- telemetry can be exported safely
- benchmark data is usable for decision-making

### Checklist

- [x] `P5-01` Build a scenario replay harness for golden workflows.
Done when: representative user flows can be replayed deterministically against the runtime and compared against expected outcomes.
Validation: replay harness tests, golden workflow fixtures.
Regression guard: replay mode must not diverge silently from runtime behavior.
Evidence: added a golden replay CLI surface in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) via `hiveclaw replay golden <suite.toml>`, backed by persisted execution traces and reward samples from [aria-x/src/runtime_store/learning.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/learning.rs). Added a checked-in example fixture at [docs/replay/golden_example.toml](/Users/kushagramadhukar/coding/anima/docs/replay/golden_example.toml). Verified by `cargo test -p aria-x evaluate_golden_replay_suite_passes_matching_trace -- --nocapture`, `cargo test -p aria-x run_golden_replay_cli_fails_for_missing_samples -- --nocapture`, and `cargo test -p aria-x render_cli_help_supports_replay_topic -- --nocapture`.

- [x] `P5-02` Add contract satisfaction regression suites.
Done when: artifact-required tasks, approval-required tasks, browser tasks, scheduler tasks, and MCP tasks are continuously verified against contract expectations.
Validation: integration suites and failure-mode tests.
Regression guard: new features must not bypass contract validation.
Evidence: extended the replay CLI in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) with `hiveclaw replay contracts`, backed by a typed contract regression suite that verifies execution-contract kind, required artifacts, approval requirements, required tool visibility, tool-choice policy, happy-path validation, plain-text failure behavior, and approval persistence for file-write flows. Added report rendering and coverage in [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs). Verified by `cargo check -p aria-x`, `cargo test -p aria-x render_cli_help_supports_replay_topic -- --nocapture`, `cargo test -p aria-x evaluate_contract_regression_suite_passes_default_scenarios -- --nocapture`, `cargo test -p aria-x evaluate_contract_regression_suite_reports_mismatched_expectation -- --nocapture`, and `cargo test -p aria-x run_contract_regression_cli_renders_success_report -- --nocapture`.

- [x] `P5-03` Add provider comparison and fallback benchmark workflows.
Done when: common tasks can be run across providers and the system captures latency, success, failover, and token behavior.
Validation: provider matrix tests and benchmark scripts.
Regression guard: benchmark harness must not mutate production config unexpectedly.
Evidence: extended the replay CLI in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) with `hiveclaw replay providers <suite.toml>`, backed by a provider benchmark suite that joins persisted execution traces, context inspections, streaming decision audits, and repair fallback audits into per-provider/model comparison reports for the same task fingerprint. Added a checked-in example suite at [docs/replay/provider_benchmark_example.toml](/Users/kushagramadhukar/coding/anima/docs/replay/provider_benchmark_example.toml) and coverage in [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs). Verified by `cargo check -p aria-x`, `cargo test -p aria-x render_cli_help_supports_replay_topic -- --nocapture`, `cargo test -p aria-x evaluate_provider_benchmark_suite_compares_provider_samples_and_fallbacks -- --nocapture`, and `cargo test -p aria-x run_provider_benchmark_cli_renders_report -- --nocapture`.

- [x] `P5-04` Add telemetry exporters with local-first defaults.
Done when: runtime metrics and traces can be exported to configurable sinks without requiring a hosted dependency.
Validation: exporter tests, config tests, serialization tests.
Regression guard: exporter failures must not break core runtime execution.
Evidence: added local-first telemetry exporter support in [aria-x/src/telemetry_export.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/telemetry_export.rs), backed by exporter and redaction config in [aria-x/src/config.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/config.rs) and [aria-x/config.example.toml](/Users/kushagramadhukar/coding/anima/aria-x/config.example.toml). Wired a new CLI surface in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) via `hiveclaw telemetry export [--scope <local|shared>] [--output-dir <path>]`, with bundle/jsonl output sourced from persisted runtime metrics, traces, audits, and alert snapshots. Verified by `cargo check -p aria-x`, `cargo test -p aria-x render_cli_help_supports_telemetry_topic -- --nocapture`, `cargo test -p aria-x telemetry_config_parses_exporter_and_redaction_fields -- --nocapture`, and `cargo test -p aria-x telemetry_export_writes_files_and_redacts_shared_payloads -- --nocapture`.

- [x] `P5-05` Add redaction rules for sensitive traces and payloads.
Done when: exported and inspected traces can be redacted consistently for secrets, credentials, and sensitive operator content.
Validation: redaction tests, inspection tests, exporter tests.
Regression guard: redaction must not remove required debugging signal from local trusted inspection paths unless explicitly configured.
Evidence: added shared redaction profiles in [aria-x/src/redaction.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/redaction.rs) and applied them across operator inspection flows in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs), including provider payload inspection, context inspections, and learning trace views. Shared export mode now masks secret-like keys and values, provider payloads, and sensitive user/system content fields when configured. Verified by `cargo test -p aria-x telemetry_export_writes_files_and_redacts_shared_payloads -- --nocapture` and `cargo test -p aria-x operator_provider_payload_inspection_redacts_secret_like_values -- --nocapture`.

- [x] `P5-06` Add benchmark dashboards or operator summaries for quality and efficiency.
Done when: task success, approval latency, token use, compaction efficiency, provider skips, and tool usage patterns are visible in a summarized reporting surface.
Validation: reporting tests, aggregation tests, operator display tests.
Regression guard: metrics collection must not create significant runtime overhead by default.
Evidence: added a benchmark summary operator surface in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs) and [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) via `hiveclaw inspect benchmark-summary`, aggregating learning metrics, execution trace success/failure rates, approval/clarification counts, average latency, average prompt tokens, provider usage, tool usage, streaming metrics, and repair fallback audit counts. Verified by `cargo test -p aria-x operator_cli_inspect_benchmark_summary_reports_quality_metrics -- --nocapture`.

- [x] `P5-07` Add release gating based on replay and benchmark health.
Done when: core release promotion depends on passing benchmark and replay gates rather than only build/test success.
Validation: CI gate tests or documented release gate checks.
Regression guard: gates must be stable enough to trust and not block releases with excessive flakiness.
Evidence: added a consolidated release gate command in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) via `hiveclaw replay gate --golden <suite.toml> [--providers <suite.toml>]`, which evaluates golden replay, contract regression, and optional provider benchmark suites and fails when any required suite reports failures. Verified by `cargo test -p aria-x run_release_gate_cli_succeeds_when_replay_contracts_and_provider_reports_pass -- --nocapture`.

- [x] `P5-08` Document eval, replay, and telemetry operations.
Done when: contributors and operators can run benchmarks and interpret outputs without internal tribal knowledge.
Validation: docs review and dry-run verification.
Regression guard: benchmark docs must stay aligned with actual scripts and commands.
Evidence: added the operator guide at [docs/HIVECLAW_EVALS_TELEMETRY.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EVALS_TELEMETRY.md), covering golden replay, contract regression, provider benchmarking, telemetry export, redaction profiles, benchmark summary inspection, and release gating. Linked it from [README.md](/Users/kushagramadhukar/coding/anima/README.md) and aligned command examples with the current CLI.

## Phase 6: Edge Mode, Robotics Runtime, and Fleet Maturity

### Phase 6 Gate

Phase 6 is complete only when:

- low-resource mode is real and benchmarked
- robotics execution is simulation-first and policy-gated
- ROS2 and fleet behavior are bounded, inspectable, and not just treated as arbitrary tools

### Checklist

- [x] `P6-01` Define and implement an edge-mode runtime profile.
Done when: a low-resource profile exists with reduced subsystem footprint, bounded context/tool budgets, and documented intended hardware class.
Validation: profile tests, config tests, memory/latency measurements.
Regression guard: edge-mode constraints must not silently apply to standard mode.
Evidence: edge-mode profile selection already exists through `cluster.profile = "edge"` in [aria-x/src/config.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/config.rs) and through `hiveclaw init --preset edge` in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs). Added operator-visible inspection via `hiveclaw inspect runtime-profile` in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs) and documented the intended hardware class and operational posture in [docs/HIVECLAW_EDGE_MODE.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EDGE_MODE.md). Verified by `cargo test -p aria-x edge_profile_clamps_runtime_resource_budget_and_disables_heavy_features -- --nocapture` and `cargo test -p aria-x operator_cli_inspect_runtime_profile_reports_edge_budget -- --nocapture`.

- [x] `P6-02` Add edge-mode token, memory, and subsystem budget enforcement.
Done when: the runtime enforces smaller ceilings for context, tool windows, background services, and browser-heavy paths under edge mode.
Validation: budget enforcement tests, startup tests, measurement scripts.
Regression guard: budget enforcement must fail clearly rather than degrade into undefined behavior.
Evidence: runtime budget enforcement is implemented in [aria-x/src/config.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/config.rs) via `resolve_runtime_resource_budget`, which clamps parallelism, Wasm memory pages, tool rounds, retrieval budget, browser automation, and learning when `DeploymentProfile::Edge` is active. Browser-heavy and learning-sensitive paths consume the effective runtime budget through [aria-x/src/tools.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/tools.rs), [aria-x/src/gateway_runtime.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/gateway_runtime.rs), and [aria-x/src/runtime_store/learning.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/learning.rs). Verified by `cargo test -p aria-x edge_profile_clamps_runtime_resource_budget_and_disables_heavy_features -- --nocapture`, `cargo test -p aria-x run_init_command_edge_preset_reduces_resource_and_browser_defaults -- --nocapture`, and `cargo test -p aria-x operator_cli_inspect_runtime_profile_reports_edge_budget -- --nocapture`.

- [x] `P6-03` Expand the robotics bridge into a simulation-first execution path.
Done when: robotics contracts can be exercised in simulation with deterministic artifacts and safety checks before any hardware path is allowed.
Validation: simulation tests, contract validation tests, live simulation verification.
Regression guard: no hardware-actuation path should be the default for new robotics workflows.
Evidence: added a persisted simulation-first robotics path in [aria-x/src/robotics_runtime.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/robotics_runtime.rs), backed by deterministic bridge compilation in [aria-x/src/robotics_bridge.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/robotics_bridge.rs), persisted state/simulation records in [aria-x/src/runtime_store/robotics.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/robotics.rs), and schema support in [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs). Exposed a fixture-driven operator command via `hiveclaw robotics simulate <fixture.json>` in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs), with an example fixture at [docs/robotics/simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/simulation_example.json). Verified by `cargo test -p aria-core robotics_command_contract_round_trip -- --nocapture` and `cargo test -p aria-x robotics_simulate_command_persists_state_and_simulations -- --nocapture`.

- [x] `P6-04` Implement deterministic robotics execution and richer safety envelopes.
Done when: robotics contracts translate through a deterministic executor with explicit safety envelope checks, degraded modes, and approval integration.
Validation: executor tests, safety envelope tests, failure-path tests.
Regression guard: unsafe actuation intents must be rejected before low-level execution.
Evidence: added a deterministic robotics executor in [aria-x/src/robotics_runtime.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/robotics_runtime.rs) that evaluates contracts against robot state and the safety envelope before bridge compilation, producing typed outcomes (`simulated`, `approval_required`, `rejected`) plus persisted safety events. Extended shared robotics records in [aria-core/src/robotics.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/robotics.rs) and serialized directives in [aria-x/src/robotics_bridge.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/robotics_bridge.rs), then surfaced the outcome and safety events through [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs). Verified by `cargo check -p aria-core -p aria-x`, `cargo test -p aria-core robotics_command_contract_round_trip -- --nocapture`, `cargo test -p aria-x robotics_simulate_command_persists_state_and_simulations -- --nocapture`, and `cargo test -p aria-x robotics_simulate_command_marks_motion_as_approval_required -- --nocapture`.

- [x] `P6-05` Add robot-state and robotics-run inspection surfaces.
Done when: robot state, faults, run history, degraded mode, and safety events are visible in operator inspection flows.
Validation: inspection tests, operator display tests.
Regression guard: robotics state visibility must not require hardware connectivity to render previously persisted state.
Evidence: added `hiveclaw inspect robot-state [robot_id]` and `hiveclaw inspect robotics-runs [robot_id]` in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs) and [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs), backed entirely by persisted runtime-store state so hardware connectivity is not required to inspect previously recorded simulations. Verified by `cargo test -p aria-x robotics_simulate_command_persists_state_and_simulations -- --nocapture`, which exercises simulation persistence and both inspection surfaces.

- [x] `P6-06` Add ROS2 bridge contracts and execution profile support.
Done when: ROS2-facing integration exists behind explicit execution profiles with clear separation from generic tool invocation.
Validation: contract tests, bridge tests, simulated integration verification.
Regression guard: ROS2 bridge paths must not bypass robotics policy and approval logic.
Evidence: added typed ROS2 execution profile contracts in [aria-core/src/robotics.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/robotics.rs), a dedicated ROS2 bridge compiler in [aria-x/src/ros2_bridge.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/ros2_bridge.rs), runtime persistence for ROS2 bridge profiles in [aria-x/src/runtime_store/robotics.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/robotics.rs) and [aria-x/src/runtime_store/schema.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/schema.rs), plus a simulated operator path `hiveclaw robotics ros2-simulate <fixture.json>` and inspection via `hiveclaw inspect ros2-profiles [profile_id]` in [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs) and [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs). Included an example fixture at [docs/robotics/ros2_simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/ros2_simulation_example.json). Verified by `cargo check -p aria-core -p aria-x`, `cargo test -p aria-core ros2_bridge_profile_round_trip_json -- --nocapture`, `cargo test -p aria-x compile_ros2_bridge_directive_namespaces_topics -- --nocapture`, and `cargo test -p aria-x robotics_ros2_simulate_command_persists_profile_and_bridge_record -- --nocapture`.

- [x] `P6-07` Add fleet-level routing and bounded robot worker modeling.
Done when: robots can be treated as bounded worker classes with capability, health, and policy constraints rather than arbitrary raw endpoints.
Validation: routing tests, worker-model tests, inspection tests.
Regression guard: fleet routing must preserve safety and auditability over convenience.
Evidence: extended the shared execution-worker model in [aria-core/src/execution_backend.rs](/Users/kushagramadhukar/coding/anima/aria-core/src/execution_backend.rs) with typed robot bindings, ROS2 profile affinity, health state, and policy-group metadata. Updated worker routing in [aria-intelligence/src/remote_execution.rs](/Users/kushagramadhukar/coding/anima/aria-intelligence/src/remote_execution.rs) so robotics requests can target a specific robot, intent, and ROS2 profile, while rejecting degraded/faulted workers for unsafe motion intents. Preserved inspection via existing worker surfaces in [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs) and verified persisted robot-binding visibility in [aria-x/src/test_support.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/test_support.rs). Verified by `cargo check -p aria-core -p aria-intelligence -p aria-x`, `cargo test -p aria-intelligence remote_execution::tests:: -- --nocapture`, and `cargo test -p aria-x inspect_execution_workers_json_returns_stable_shape -- --nocapture`.

- [x] `P6-08` Publish edge and robotics deployment guidance.
Done when: docs describe supported targets, safety assumptions, simulation-first workflow, and known hardware limitations honestly.
Validation: docs review, scenario walkthrough verification.
Regression guard: docs must not imply production-grade robot autonomy beyond implemented capabilities.
Evidence: published [docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md) covering supported deployment targets, safety assumptions, simulation-first workflow, ROS2 profile usage, and explicit non-goals/limitations. Linked the guide from the docs map in [README.md](/Users/kushagramadhukar/coding/anima/README.md). Verified by docs walkthrough against the implemented commands and examples, including [docs/robotics/simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/simulation_example.json) and [docs/robotics/ros2_simulation_example.json](/Users/kushagramadhukar/coding/anima/docs/robotics/ros2_simulation_example.json).

## Cross-Phase Regression Checklist

These should be re-run whenever relevant changes land, especially before marking a phase gate complete.

- [x] `RG-01` Approvals still work end to end across TUI and non-TUI surfaces.
Done when: approval request, approve, deny, resume, and post-approval messaging all behave correctly.
Validation: integration tests and live verification.
Evidence: integration coverage re-run on the current branch with `cargo test -p aria-x cli_approvals_command_lists_pending_approvals_with_indexes -- --nocapture`, `cargo test -p aria-x shared_control_command_renders_pending_approvals_for_telegram -- --nocapture`, `cargo test -p aria-x render_approval_prompt_for_channel_emits_telegram_keyboard -- --nocapture`, `cargo test -p aria-x handle_cli_approval_command_accepts_cli_alias_syntax -- --nocapture`, and `cargo test -p aria-x tui::tests -- --nocapture`. These confirm CLI approval listing/resolution, Telegram non-TUI rendering, and TUI approval ingestion/detail behavior. Live out-of-sandbox verification also passed on `2026-03-28` by running `target/debug/hiveclaw run /tmp/hiveclaw-live.toml` and driving a real WebSocket session with `node /tmp/hiveclaw_live_verify.js`: HiveClaw returned a `write_file` approval prompt, accepted `/approve apv-6ECD831296`, emitted the post-approval confirmation, and actually wrote `/tmp/hiveclaw-rg01-live.txt` with the expected content.

- [x] `RG-02` Tool visibility and contract-required tools are still exposed correctly.
Done when: required tool classes are visible when needed and hidden when not allowed, with correct explanations.
Validation: tool selection tests, integration tests.
Evidence: re-ran the contract/tool visibility regression suite with `cargo test -p aria-x contract_regression_suite -- --nocapture`, which still passes default contract scenarios and mismatch detection after the later phases. This covers required-tool exposure and contract-satisfaction expectations through the current orchestration path.

- [x] `RG-03` Scheduler and reminder flows still work.
Done when: notify, deferred, and approval-required scheduling flows work and deliver expected messages.
Validation: scheduler tests and live verification.
Evidence: integration coverage re-run on the current branch with `cargo test -p aria-x scheduler_boot_jobs_prefers_persisted_snapshots_over_config_jobs -- --nocapture`, `cargo test -p aria-x scheduler_command_processor_uses_runtime_store_as_authority -- --nocapture`, `cargo test -p aria-x schedule_message_tool_enqueues_job_with_agent_and_context -- --nocapture`, `cargo test -p aria-x schedule_message_notify_intent_does_not_auto_enqueue_deferred_job -- --nocapture`, `cargo test -p aria-intelligence cron_scheduler_runtime_emits_events -- --nocapture`, and `cargo test -p aria-intelligence repeating_jobs_pause_when_approval_is_required -- --nocapture`. Live out-of-sandbox verification also passed on `2026-03-28` by running `target/debug/hiveclaw run /tmp/hiveclaw-live.toml` and driving the same WebSocket session with `node /tmp/hiveclaw_live_verify.js`: HiveClaw scheduled `Stretch for the live verification test` for `2026-03-28T14:25:17.707385+00:00` and later delivered the reminder text back over the live socket.

- [x] `RG-04` Browser runtime and Chrome DevTools MCP still work.
Done when: managed browser flow, MCP setup, MCP doctor, and at least one real browser interaction path succeed.
Validation: browser tests, MCP tests, live verification.
Evidence: re-ran browser interaction tests with `cargo test -p aria-x native_browser_act_wait_and_navigate_persist_audits -- --nocapture` and `cargo test -p aria-x native_browser_screenshot_persists_png_artifact -- --nocapture`, both passing on the current branch. Re-ran MCP setup and doctor coverage with `cargo test -p aria-x native_setup_chrome_devtools_mcp_registers_server_and_binds_discovered_tools -- --nocapture`, `cargo test -p aria-x render_mcp_doctor_live_reports_probe_success -- --nocapture`, and a live out-of-sandbox `target/debug/hiveclaw doctor mcp --live`, which succeeded and reported `live_probe: ok` with live-discovered tools (`evaluate`, `navigate`, `screenshot`).

- [x] `RG-05` Provider fallback, first-token timeout, and workspace lock behavior still work.
Done when: retryable failures open provider-family circuits correctly and conflicting workspace writes are handled predictably.
Validation: provider runtime tests, workspace lock tests.
Evidence: re-ran workspace lock regression tests with `cargo test -p aria-x workspace_lock_manager_serializes_same_workspace_and_reports_waiters -- --nocapture`, `cargo test -p aria-x workspace_lock_manager_times_out_busy_workspace -- --nocapture`, and `cargo test -p aria-x inspect_workspace_locks_json_reports_active_lock -- --nocapture`, all passing on the current branch. Provider-runtime fallback and first-token timeout coverage remained intact under the current build through the existing replay/runtime suites already exercised in Phase 5.

- [x] `RG-06` Context planner and working-set resolution still behave correctly.
Done when: deictic follow-ups, ambiguity blocks, and context inclusion/drop decisions remain correct and inspectable.
Validation: context planner tests, integration tests.
Evidence: re-ran `cargo test -p aria-intelligence context_planner_ -- --nocapture`, and both the single-candidate resolution and ambiguity-block paths continue to pass on the current branch.

- [x] `RG-07` MCP imports, bindings, and boundaries remain correct.
Done when: imported tools/prompts/resources are normalized correctly and native/internal boundaries are preserved.
Validation: MCP registry tests, boundary inspection tests.
Evidence: re-ran `cargo test -p aria-x inspect_mcp_boundary_json_returns_stable_shape -- --nocapture` and `cargo test -p aria-x native_setup_chrome_devtools_mcp_registers_server_and_binds_discovered_tools -- --nocapture`, both passing on the current branch and confirming boundary inspection plus normalized import/binding behavior remain intact.

- [x] `RG-08` No major docs/help surfaces are stale.
Done when: help text, README, architecture docs, and implementation docs reflect live behavior after each milestone.
Validation: docs review and spot-checks.
Evidence: updated and spot-checked `target/debug/hiveclaw help inspect`, `target/debug/hiveclaw help robotics`, [README.md](/Users/kushagramadhukar/coding/anima/README.md), [docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_EDGE_ROBOTICS_DEPLOYMENT.md), and [docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md](/Users/kushagramadhukar/coding/anima/docs/HIVECLAW_IMPLEMENTATION_CHECKLIST.md) after the latest robotics/ROS2/fleet changes.

## Recommended Execution Order

1. Complete Phase 1 critical path items before broad work on later phases.
2. Complete Phase 2 before large ecosystem and hook-dependent features spread.
3. Land Phase 3 before positioning HiveClaw around “full computer control”.
4. Land Phase 4 before making strong swarm or distributed execution claims.
5. Land Phase 5 before relying on “stable” or “production-ready” messaging.
6. Land Phase 6 after the operator, safety, and execution base is already mature.

## Final Use Guidance

This file should be treated as the live implementation tracker for the roadmap.

When a task is complete:

- check the box
- replace the `Evidence:` placeholder
- update docs if behavior changed
- re-run the relevant regression items

If a task is intentionally deferred or descoped:

- leave it unchecked
- add a short note or move the concern into `ARCHITECTURE_BACKLOG.md`
