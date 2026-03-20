# Contributing to HiveClaw

Thanks for your interest in contributing.

## License

By contributing to this repository, you agree that your contributions are submitted under the same license as the project:

- GNU Affero General Public License v3.0 or later

See [LICENSE](LICENSE) for the full terms.

## Before opening a PR

1. Make sure the change is technically justified.
2. Keep the scope focused.
3. Run the relevant tests for the area you touched.
4. Do not commit local secrets, runtime state, or generated live config files.
5. Review [REVIEWER_CHECKLIST.md](REVIEWER_CHECKLIST.md) and [docs/ENGINEERING_PR_REVIEW_CHECKLIST.md](docs/ENGINEERING_PR_REVIEW_CHECKLIST.md) if your change touches architecture, security, MCP, agents, or tool execution.

## Contribution guidance

- Prefer small, reviewable changes over large mixed commits.
- Keep architecture boundaries explicit.
- Do not bypass policy, capability, approval, or scope enforcement in code.
- Avoid adding local-machine paths, tokens, generated runtime files, or session logs to Git.
- If you add a new provider, tool, skill, channel, or runtime surface, include tests and update the relevant docs.

## Development baseline

Typical commands:

```bash
cargo build --workspace
cargo test --workspace
```

For runtime-specific validation, also use the existing scripts in `scripts/` where relevant.

## Security

If you discover a security issue, do not open a public exploit-style issue with live secrets or reproduction data that exposes credentials. Share a minimal report and keep sensitive material out of the repository.
