# HiveClaw Computer Runtime

HiveClaw now treats desktop control as a first-class runtime surface, separate from both the native browser runtime and Chrome DevTools MCP.

## Why this exists

Desktop control has different risk, approval, and policy requirements from browser automation.

HiveClaw therefore separates:

- `browser_runtime`
  - managed browser profiles and browser-native interaction
- `chrome_devtools`
  - MCP-backed access to Chrome DevTools sessions
- `computer_runtime`
  - desktop screenshot, pointer, keyboard, clipboard, and window-scoped actions

This separation is intentional. Browser tasks must not silently escalate into desktop control, and desktop tasks must not silently fall back into browser-only surfaces.

## Supported runtime model

Current `computer_runtime` support is focused on local desktop execution with explicit profiles, sessions, approvals, and audits.

Current action families:

- screenshot capture
- pointer movement
- pointer click
- keyboard typing
- key press
- clipboard read
- clipboard write
- window focus

Current artifact families:

- screenshot
- window snapshot
- clipboard snapshot

## Execution profiles

HiveClaw seeds default local profiles when none exist.

Default profiles currently include:

- `desktop-safe`
  - local trusted desktop profile with pointer, keyboard, and clipboard enabled
- `desktop-observe`
  - observation-oriented profile with tighter interaction capability

Execution profiles define:

- runtime kind
- isolation/headless intent
- pointer allowance
- keyboard allowance
- clipboard allowance
- allowed windows

Operators should treat profiles as the main safety boundary for computer actions. The runtime must not silently downgrade from isolated or restricted profiles into a broader trusted desktop profile.

## Sessions and window targeting

Computer actions execute against typed desktop sessions.

A session records:

- owning HiveClaw session
- agent id
- active execution profile
- runtime kind
- selected window id if applicable

Window targeting is explicit. Desktop actions can be scoped to a target window, and the selected window is persisted for operator inspection.

This is important for:

- limiting accidental cross-window input
- making follow-up actions explainable
- keeping approval summaries specific

## Approval and risk model

HiveClaw treats computer control as a high-trust surface.

General rule of thumb:

- observation-only actions are lower risk
- pointer clicks and keyboard input are higher risk
- clipboard reads and writes require careful review because they may expose or overwrite sensitive data

Examples:

- `computer_capture`
  - read-oriented and lower risk than direct desktop mutation
- `computer_act` with `pointer_click`
  - approval-gated by default
- `computer_act` with `keyboard_type`
  - approval/policy gated and capability-scoped

Operators should expect desktop actions to be auditable and, when appropriate, approval-gated before execution.

## Audit and inspection surfaces

HiveClaw persists computer-runtime state in the runtime store.

Inspectable records include:

- computer profiles
- computer sessions
- computer artifacts
- computer action audits

Operator inspection commands:

```bash
hiveclaw inspect --inspect-computer-profiles
hiveclaw inspect --inspect-computer-sessions
hiveclaw inspect --inspect-computer-artifacts
hiveclaw inspect --inspect-computer-action-audits
```

These inspection surfaces are intended to answer:

- which computer surface was used
- which profile was active
- which session/window the action targeted
- which artifacts were produced
- which actions were executed and when

## macOS requirements

On macOS, live desktop control requires system permissions.

### Screen capture

For screenshot capture, the host process must have:

- Screen Recording permission

### Pointer and keyboard control

For pointer movement, clicks, window focus through accessibility APIs, and keyboard typing, the host process must have:

- Accessibility / Assistive Access permission

Without these permissions, HiveClaw can still compile and pass non-live tests, but real desktop actions will fail at runtime.

## Safe usage guidance

Use desktop control only when browser surfaces are not the right fit.

Prefer:

- `browser_runtime` for browser-native flows
- `chrome_devtools` for Chrome session debugging and browser-backed MCP access
- `computer_runtime` for true desktop tasks requiring screen, pointer, keyboard, clipboard, or window control

Recommended operator habits:

- start with screenshot capture before mutation
- use the narrowest execution profile available
- scope actions to a target window where possible
- inspect audits after risky actions
- avoid broad clipboard reads in mixed-trust environments

## Current boundaries

Current desktop support should be considered an operator-grade alpha surface.

What is supported now:

- typed desktop actions
- typed desktop profiles and sessions
- persisted artifacts and audits
- explicit separation from browser surfaces

What is intentionally still constrained:

- no claim of full unattended desktop autonomy
- no silent bypass of approval for risky actions
- no claim that every OS/backend has feature parity

## Related docs

- [Execution Roadmap](./HIVECLAW_EXECUTION_ROADMAP.md)
- [Implementation Checklist](./HIVECLAW_IMPLEMENTATION_CHECKLIST.md)
- [Operator Workbench](./HIVECLAW_OPERATOR_WORKBENCH.md)
- [Chrome DevTools MCP](./CHROME_DEVTOOLS_MCP.md)
