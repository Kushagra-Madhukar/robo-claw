# Chrome DevTools MCP

HiveClaw can integrate the Chrome DevTools MCP server as an external MCP-backed browser provider.

This is separate from the native HiveClaw browser runtime:

- native `browser_runtime` remains the local trust-boundary system for managed profiles, session persistence, challenge handling, and approval-aware browser workflows
- `chrome_devtools` is treated as a leaf external MCP integration attached to a live Chrome/Chromium DevTools session

That separation is intentional.

## When to Use It

Use Chrome DevTools MCP when you want:

- browser access through a Chrome-backed MCP provider
- Chrome-native debugging and inspection tools exposed through MCP
- either a HiveClaw-launched isolated browser session or an attached existing Chrome session

Keep using the native browser runtime when you need:

- managed browser profiles
- encrypted browser session state persistence
- manual login checkpoints and challenge handling
- HiveClaw-owned browser audit and state workflows

## Chrome Requirements

The Chrome DevTools MCP flow depends on the Chrome-side feature described by Google:

- Chrome 144 or newer
- the MCP server process available via `npx chrome-devtools-mcp@latest`
- remote debugging approval enabled in `chrome://inspect/#remote-debugging` only when using attach mode (`mode=auto_connect`)

## HiveClaw Integration Path

HiveClaw now supports two MCP tools for this flow:

- `setup_chrome_devtools_mcp`
  - registers a `chrome_devtools` MCP server profile
  - defaults to `npx -y chrome-devtools-mcp@latest --headless --isolated --slim`
  - supports `mode=launch_managed` by default for reliable browser access
  - supports `mode=auto_connect` to attach to an already-running Chrome session
  - optionally appends `--channel=beta|dev|canary`
  - discovers/imports the live MCP catalog
  - optionally binds discovered tools/prompts/resources to an agent

- `sync_mcp_server_catalog`
  - generic MCP catalog refresh for any registered MCP server
  - replaces stale imports with the currently discovered tool/prompt/resource set
  - can optionally bind the discovered entries to an agent

HiveClaw also exposes operator CLI entrypoints:

- `hiveclaw doctor mcp`
  - shows MCP runtime readiness, Chrome binary detection, and current `chrome_devtools` registration/import state
  - supports `--live` to perform a real Chrome DevTools MCP handshake probe
  - supports `--mode auto_connect` with `--live` to probe the active Chrome session instead of a managed launched session
- `hiveclaw setup chrome-devtools-mcp`
  - registers Chrome DevTools MCP, discovers the live catalog, and binds tools to an agent

The legacy `aria-x` command remains available for compatibility.

## Recommended Setup

For the default developer agent flow, configure Chrome DevTools MCP like this:

1. Ask HiveClaw to run `setup_chrome_devtools_mcp` for the developer or omni agent.
2. Let the MCP server launch an isolated Chrome session for the agent.

Equivalent CLI command:

```bash
hiveclaw setup chrome-devtools-mcp --agent developer
```

Conceptually, that setup request maps to:

```json
{
  "server_id": "chrome_devtools",
  "display_name": "Chrome DevTools MCP",
  "mode": "launch_managed",
  "channel": "stable",
  "bind_tools": true
}
```

If you want Chrome Beta instead:

```json
{
  "server_id": "chrome_devtools",
  "display_name": "Chrome DevTools MCP",
  "mode": "launch_managed",
  "channel": "beta",
  "bind_tools": true
}
```

## Attach to an Existing Chrome Session

Use attach mode when you explicitly want DevTools access to a Chrome session you already started yourself.

1. Enable remote debugging approval in `chrome://inspect/#remote-debugging`.
2. Start Chrome normally.
3. Ask HiveClaw to run `setup_chrome_devtools_mcp` with `mode=auto_connect`.

Equivalent CLI command:

```bash
hiveclaw setup chrome-devtools-mcp --mode auto_connect --agent developer
```

Conceptually:

```json
{
  "server_id": "chrome_devtools",
  "display_name": "Chrome DevTools MCP",
  "mode": "auto_connect",
  "channel": "stable",
  "bind_tools": true
}
```

## Advanced Setup

If you already manage the server process yourself, you can override the generated endpoint:

```json
{
  "server_id": "chrome_devtools",
  "endpoint_override": "npx -y chrome-devtools-mcp@latest --autoConnect --channel=beta",
  "bind_tools": true
}
```

That same override hook is also useful for tests and controlled local wrappers.

## Observability

After setup, the runtime persists:

- MCP server registration
- imported MCP tools/prompts/resources
- agent bindings for discovered MCP tools
- import cache counts in the runtime store

Use the existing MCP inspection surfaces to confirm:

- registered servers
- imports for `chrome_devtools`
- bindings for the target agent
- cache counts

For a quick CLI summary:

```bash
hiveclaw doctor mcp
hiveclaw doctor mcp --live
hiveclaw doctor mcp --live --mode auto_connect
```

## First Run

Recommended first operator flow:

1. Run `hiveclaw doctor mcp` to confirm local Chrome and `npx` detection.
2. Run `hiveclaw doctor mcp --live` to verify the managed-launch probe path.
3. If you want to work against your already-open Chrome session, run `hiveclaw doctor mcp --live --mode auto_connect`.
4. Register the integration for an agent with `hiveclaw setup chrome-devtools-mcp --agent developer`.

## Notes

- HiveClaw does not collapse Chrome DevTools MCP into the native browser runtime.
- That boundary is deliberate and matches the MCP policy rules in `aria-mcp`.
- If the Chrome DevTools MCP tool set changes across versions, rerun `sync_mcp_server_catalog` to refresh imports.
- In live verification on this machine, the managed launched mode (`--headless --isolated --slim`) successfully exposed the Chrome MCP tool catalog over stdio.
