# HiveClaw Evals, Replay, and Telemetry

This guide covers the Phase 5 operator workflows for replay, contract regression, provider benchmarking, telemetry export, summary inspection, and release gating.

## Goals

Phase 5 gives HiveClaw a safer product loop:

- replay important workflows deterministically
- catch contract regressions before release
- compare provider behavior on the same task fingerprints
- export telemetry without requiring hosted infrastructure
- redact sensitive traces before sharing
- gate releases on runtime health instead of build success alone

## Command Surface

### Replay

Golden replay:

```bash
hiveclaw replay golden docs/replay/golden_example.toml
```

Contract regression suite:

```bash
hiveclaw replay contracts
```

Provider benchmark suite:

```bash
hiveclaw replay providers docs/replay/provider_benchmark_example.toml
```

Release gate:

```bash
hiveclaw replay gate --golden docs/replay/golden_example.toml --providers docs/replay/provider_benchmark_example.toml
```

## Golden Replay

Use golden replay when you want to verify that a known workflow still produces the same outcome profile.

What it checks:

- replay sample exists for the requested workflow
- runtime behavior can be compared against expected outcome fields
- deviations surface as report failures instead of being silently ignored

Best use cases:

- regression testing before release
- validating prompt/context changes
- confirming that tooling refactors did not alter stable flows

## Contract Regression Suite

The contract regression suite is the architectural guardrail for artifact-driven runtime behavior.

It verifies representative scenarios for:

- file creation flows
- schedule/reminder flows
- browser read flows
- browser act flows
- MCP invocation flows
- approval-required tool behavior

What it protects:

- execution-contract kind
- required artifacts
- required tool visibility
- tool-choice policy
- approval persistence
- happy-path contract satisfaction
- plain-text failure behavior when artifacts are missing

## Provider Benchmarking

Provider benchmark reports join persisted traces and inspections to compare providers on the same task fingerprint.

Current report dimensions include:

- success and failure counts
- average latency
- average prompt tokens
- approval and clarification frequency
- fallback outcomes
- repair fallback usage

Use provider benchmarking when deciding:

- default provider/model routing
- fallback ordering
- whether a degraded compat path is still worth carrying

## Telemetry Export

Export telemetry locally:

```bash
hiveclaw telemetry export
```

Export a shared bundle with stronger redaction:

```bash
hiveclaw telemetry export --scope shared
```

Export to a specific directory:

```bash
hiveclaw telemetry export --scope shared --output-dir /tmp/hiveclaw-telemetry
```

The exporter writes local-first artifacts without requiring a hosted dependency. Depending on config, it can emit:

- a structured JSON bundle
- a JSONL event stream

The export currently includes:

- learning metrics
- execution traces
- reward events
- retrieval traces
- context inspections
- streaming decision audits
- repair fallback audits
- channel health snapshots
- operational alert snapshots

## Redaction Profiles

HiveClaw now distinguishes between trusted local inspection and shared export.

Profiles:

- `TrustedLocalInspect`: keeps more debugging signal while still masking obvious secrets
- `LocalExport`: local-first export with secret masking
- `SharedExport`: stronger redaction for provider payloads and user/operator content

Redaction covers:

- secret-like keys such as tokens, passwords, cookies, keys, and authorization headers
- secret-like inline values
- provider request payloads in shared export mode
- user/system content fields in shared export mode when configured

Config lives in:

- [aria-x/src/config.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/config.rs)
- [aria-x/config.example.toml](/Users/kushagramadhukar/coding/anima/aria-x/config.example.toml)

Relevant config sections:

- `[telemetry.exporters]`
- `[telemetry.redaction]`

## Benchmark Summary Inspection

Inspect an aggregated benchmark and efficiency summary:

```bash
hiveclaw inspect benchmark-summary
```

The summary currently includes:

- task trace counts
- success and failure totals
- approval-required totals
- clarification-required totals
- average latency
- average prompt token usage
- provider usage
- tool usage
- streaming metrics
- repair fallback audit counts

Use this as the first operator checkpoint when:

- comparing runtime quality between branches
- evaluating whether a provider change helped or hurt
- checking whether approvals or clarifications are trending upward

## Release Gating

Release gating combines replay and regression surfaces into one operator command:

```bash
hiveclaw replay gate --golden docs/replay/golden_example.toml --providers docs/replay/provider_benchmark_example.toml
```

The gate currently evaluates:

- golden replay suite
- contract regression suite
- optional provider benchmark suite

The gate fails when any required suite reports failures.

Use it before:

- release promotion
- large tool/runtime refactors
- provider-routing changes
- prompt/context changes that affect artifact workflows

## Recommended Operator Flow

For a normal release-confidence pass:

1. Run `hiveclaw replay contracts`.
2. Run `hiveclaw replay golden docs/replay/golden_example.toml`.
3. Run `hiveclaw replay providers docs/replay/provider_benchmark_example.toml` when provider behavior changed.
4. Run `hiveclaw inspect benchmark-summary`.
5. Run `hiveclaw telemetry export --scope shared` if you need a portable bundle for review.
6. Run `hiveclaw replay gate --golden ... --providers ...` as the final consolidated check.

## Files and Modules

Main implementation points:

- [aria-x/src/bootstrap.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/bootstrap.rs)
- [aria-x/src/operator.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/operator.rs)
- [aria-x/src/redaction.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/redaction.rs)
- [aria-x/src/telemetry_export.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/telemetry_export.rs)
- [aria-x/src/runtime_store/learning.rs](/Users/kushagramadhukar/coding/anima/aria-x/src/runtime_store/learning.rs)
- [docs/replay/golden_example.toml](/Users/kushagramadhukar/coding/anima/docs/replay/golden_example.toml)
- [docs/replay/provider_benchmark_example.toml](/Users/kushagramadhukar/coding/anima/docs/replay/provider_benchmark_example.toml)

## Notes

- Telemetry export is designed to fail safely. Exporter problems must not break the core runtime path.
- Shared exports are for collaboration and bug reports, not for raw internal dumps.
- Contract regression is the most important guardrail for HiveClaw’s typed execution model. If that suite starts failing, treat it as an architectural regression, not a cosmetic issue.
