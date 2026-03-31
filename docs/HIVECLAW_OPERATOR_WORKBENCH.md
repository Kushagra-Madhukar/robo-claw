# HiveClaw Operator Workbench

This guide covers the new Phase 1 operator workflow:

- first-run setup
- TUI workbench navigation
- approvals
- context and tool inspection
- provider and MCP health
- workspace lock visibility
- common recovery flows

## First Run

Bootstrap a project-local HiveClaw workspace:

```bash
hiveclaw init
```

Useful variants:

```bash
hiveclaw init --preset recommended
hiveclaw init --preset edge
hiveclaw init --non-interactive --overwrite
```

Bootstrap writes:

- `.hiveclaw/config.toml`
- `.hiveclaw/policies/default.cedar`
- `.hiveclaw/agents/README.md`
- `HIVECLAW.md`

If `AGENTS.md` or `CLAUDE.md` already exist, bootstrap imports or references them into `HIVECLAW.md` based on the selected mode.

## Workbench Layout

Launch the TUI:

```bash
hiveclaw tui
```

The workbench is organized around operator tasks rather than a plain transcript shell.

Tabs:

- `runs`
- `approvals`
- `tools/context`
- `system/health`

The transcript remains visible, but the workbench now keeps the operational state nearby:

- current runs and background work
- pending approvals
- visible and hidden tool state
- context-plan decisions
- provider and MCP health
- workspace lock contention
- concise failure explanations

## Command Palette

Open the command palette with `Ctrl+P`.

Use it to:

- switch tabs
- switch agents
- refresh runs
- refresh approvals
- refresh workspace locks
- refresh provider health
- jump to inspect/explain flows

The command palette is the main discovery path for operator actions.

## Approvals

The approvals panel is designed for fast review instead of raw JSON inspection.

Each approval card shows:

- action summary
- target
- risk
- argument preview
- available actions

Keyboard affordances remain available:

- `Enter`
- `a`
- `d`
- `i`
- `Esc`

Non-TUI approval surfaces continue to work, including Telegram.

## Context And Tool Visibility

The `tools/context` tab shows:

- visible tools
- hidden tools
- why hidden
- context-plan include decisions
- context-plan drop decisions
- token estimates
- ambiguity and reference-resolution outcomes

This is the main operator surface for answering:

- why a tool was visible
- why a tool was hidden
- why a context block was included
- why a context block was dropped
- whether the runtime found ambiguity in follow-up references
- which active rules were injected for the current request

## Rules

HiveClaw now resolves rule layers from:

- org rules
- user rules
- project rules
- path-scoped rules

Workspace discovery recognizes:

- `HIVECLAW.md`
- `AGENTS.md`
- `CLAUDE.md`

Global rule files can be provided through:

- `HIVECLAW_ORG_RULES_PATH`
- `HIVECLAW_USER_RULES_PATH`

or the default config-area files:

- `rules/org.md`
- `rules/user.md`

Path-scoped rules are activated only when the current request resolves to a matching path target.

## Runs

The `runs` tab surfaces active and recent agent activity:

- run scope
- status
- target agent
- request preview
- run id tail

This is the fastest way to understand whether work is active, backgrounded, or blocked.

## Provider And MCP Health

The `system/health` tab includes:

- active backend summary
- provider-family circuit rows
- tool-provider readiness
- MCP server rows
- workspace lock rows
- failure summaries

The TUI refresh path uses:

- `/provider_health`
- `/workspace_locks`

These are safe runtime control commands and are also available over the runtime path outside the TUI.

For shell-based checks:

```bash
hiveclaw doctor mcp
hiveclaw doctor mcp --live
hiveclaw doctor mcp --live --mode auto_connect
```

## Inspect Workflows

The command palette points to the main inspect workflows.

Useful shell commands:

```bash
hiveclaw inspect context [session_id] [agent_id]
hiveclaw explain context [session_id] [agent_id]
hiveclaw inspect rules <workspace_root> [request_text] [target_path]
hiveclaw inspect provider-payloads [session_id] [agent_id]
hiveclaw explain provider-payloads [session_id] [agent_id]
hiveclaw inspect runs <session_id>
hiveclaw inspect workspace-locks
hiveclaw inspect mcp-servers
hiveclaw inspect mcp-imports <server_id>
hiveclaw inspect mcp-bindings <agent_id>
```

These are intended for operator diagnosis without raw log archaeology.

## Failure Summaries

The `system/health` tab now summarizes common classes of issues:

- missing required artifact
- reference ambiguity
- hidden-tool mismatch
- approval interruption
- workspace busy / lock contention

These summaries are intentionally concise. They do not replace raw logs, but they should tell the operator what to inspect next.

## Recovery Flows

Recommended recovery sequence:

1. Check `approvals` for blocked actions.
2. Check `runs` for backgrounded or stalled work.
3. Check `tools/context` for hidden-tool or context-plan issues.
4. Check `system/health` for provider circuit, MCP readiness, or workspace lock contention.
5. Use `inspect` or `explain` commands for deeper detail.

## Phase 1 Outcome

Phase 1 is considered complete when the operator can:

- bootstrap a project without manual TOML editing
- run HiveClaw from a first-class `hiveclaw` command
- diagnose common failures from the TUI
- inspect context, tool, run, lock, and MCP state from discoverable surfaces
- recover from common approval and contention issues without digging through raw logs first
