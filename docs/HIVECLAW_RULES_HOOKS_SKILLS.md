# HiveClaw Rules, Hooks, and Skills

This document explains the extension model that Phase 2 formalized in HiveClaw:

- layered rules and guidance import
- typed lifecycle hooks
- skill install/bind/doctor workflows
- trust and provenance surfaces
- on-demand skill loading for prompt efficiency
- Codex-compatible skill import/export

## Rules

HiveClaw resolves rules in a layered order rather than treating project guidance as one flat prompt blob.

Resolution layers:

1. org rules
2. user rules
3. project rules
4. path-scoped rules

Imported guidance sources:

- `AGENTS.md`
- `CLAUDE.md`
- `HIVECLAW.md`

The runtime keeps rule origin metadata so operators can inspect where a rule came from and which rules were active for a request.

Operator commands:

```bash
hiveclaw inspect rules /path/to/workspace "request text" optional/target/path
```

## Hooks

HiveClaw exposes typed lifecycle hooks instead of allowing arbitrary hidden prompt mutation.

Supported lifecycle phases:

- `session_start`
- `prompt_submit`
- `pre_tool`
- `permission_request`
- `post_tool`
- `pre_compact`
- `post_compact`
- `subagent_start`
- `subagent_stop`
- `approval_resume`
- `session_end`

Hook effects are normalized into typed assets and context blocks. Legacy prompt hooks are still supported as a compatibility path, but their output is wrapped into bounded `PromptAsset` blocks rather than being spliced directly into the prompt.

## Skills

Skills are persisted in the runtime store as normalized `SkillPackageManifest` records plus bindings, activations, and optional signatures.

Core lifecycle:

1. install a skill
2. bind it to an agent
3. choose an activation policy
4. inspect it through doctor/operator surfaces
5. load its instruction document on demand when relevant

### Skill CLI

```bash
hiveclaw skills list
hiveclaw skills doctor [skill_id]
hiveclaw skills install --dir <skill_dir>
hiveclaw skills install --signed-dir <skill_dir> [--public-key <hex>]
hiveclaw skills install --manifest <skill.toml>
hiveclaw skills install --codex-dir <skill_dir>
hiveclaw skills update --dir <skill_dir>
hiveclaw skills enable <skill_id>
hiveclaw skills disable <skill_id>
hiveclaw skills bind <skill_id> [--agent <agent_id>] [--policy <manual|auto_suggest|auto_load_low_risk|approval_required>] [--version <requirement>]
hiveclaw skills unbind <skill_id> [--agent <agent_id>]
hiveclaw skills export <skill_id> [--output-dir <path>] [--signing-key-hex <hex>] [--format <native|codex>]
```

### Trust and provenance

HiveClaw distinguishes between:

- `local`
- `imported`
- `generated`
- `compatibility_import`

Doctor output also derives trust state from persisted signatures:

- `trusted`
- `unsigned_local`
- `unsigned_imported`

The current Phase 2 implementation surfaces trust clearly to operators and preserves signature verification state in the runtime store.

### Activation policies

Bindings support these activation policies:

- `manual`
- `auto_suggest`
- `auto_load_low_risk`
- `approval_required`

`auto_load_low_risk` is the main path used for prompt-efficient skill loading.

## On-demand skill loading

HiveClaw does not inject every bound skill document into every request.

Instead, the runtime:

1. enumerates enabled skills bound to the active agent
2. scores them against request text, visible tools, bindings, and active activations
3. loads only relevant `SKILL.md` instruction documents into prompt context
4. records included vs deferred skill assets in context inspection

This keeps token cost bounded while preserving the ability to surface the right instructions when needed.

In practice, a skill will load when:

- it is actively activated for the agent, or
- it matches request text/tool context strongly enough, or
- its activation policy permits low-risk auto-loading and the request is relevant

## Codex compatibility

HiveClaw now supports a practical compatibility bridge for Codex-style standalone skills.

Supported import shape:

- a directory containing `SKILL.md`
- optional YAML frontmatter with `name` and `description`
- optional companion folders such as `scripts/`, `references/`, and `assets/`

Import path:

```bash
hiveclaw skills install --codex-dir /path/to/skill
```

Export path:

```bash
hiveclaw skills export my_skill --output-dir ./skills --format codex
```

Normalization behavior:

- `SKILL.md` is mapped into HiveClaw's `entry_document`
- the skill directory name becomes the normalized `skill_id`
- frontmatter fields map into `name` and `description`
- provenance is stored as `compatibility_import`

This is intentionally scoped to the actual reusable skill contract rather than the broader plugin/package ecosystem.

## Inspection and operator visibility

Relevant operator surfaces:

- TUI operator workbench
- `hiveclaw inspect rules ...`
- `hiveclaw skills doctor ...`
- context inspection records and context-plan block inclusion/drop reasons

When a skill is not loaded, the context plan will still show deferred skill asset information so operators can understand why a skill was not injected.

## Recommended usage

Use this order for teams adopting the extension model:

1. initialize project guidance with `hiveclaw init`
2. import existing `AGENTS.md` / `CLAUDE.md` guidance
3. install and bind a small number of skills
4. keep most skills on `manual` or `auto_suggest`
5. use `auto_load_low_risk` only for trusted, high-signal skills
6. inspect rule/context decisions before adding more hidden automation

## Related docs

- [HIVECLAW_OPERATOR_WORKBENCH.md](HIVECLAW_OPERATOR_WORKBENCH.md)
- [HIVECLAW_EXECUTION_ROADMAP.md](HIVECLAW_EXECUTION_ROADMAP.md)
- [HIVECLAW_IMPLEMENTATION_CHECKLIST.md](HIVECLAW_IMPLEMENTATION_CHECKLIST.md)
- [CHROME_DEVTOOLS_MCP.md](CHROME_DEVTOOLS_MCP.md)
