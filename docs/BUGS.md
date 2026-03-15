# Bugs

## 1. Artifact-bearing requests can get pinned to a text-only session model

- Symptoms:
  - reminder and cron-style requests fail even though scheduling tools are registered
  - file creation requests return prose instead of a tool call
- Root cause:
  - a session-level model override can force the request onto a backend that exposes `tools=[]`
  - the runtime then attempts an artifact-bearing request on a text-only path
- Required fix:
  - if the execution contract requires a durable artifact, do not honor a text-only session override
  - fall back to a tool-capable backend, or fail explicitly before inference if no such backend exists

## 2. File creation requests do not resolve to a file artifact contract

- Symptoms:
  - requests like `create a hello.js file` can complete with plain text instead of a file tool call
- Root cause:
  - file-write intents currently fall through to the generic `ToolAssisted` contract
  - the generic contract does not require a `File` artifact
- Required fix:
  - classify file create, write, save, and edit requests as `ArtifactCreate`
  - require a `File` artifact and forbid `plain_text_only` completion

## 3. Scheduling tool schemas are too strict

- Symptoms:
  - valid schedule tool calls can be rejected because optional fields are treated as required
- Root cause:
  - the tool schema normalizer marks all properties as required unless the schema explicitly declares `required`
  - current scheduling schemas are property maps, not full object schemas
- Required fix:
  - register `manage_cron`, `schedule_message`, and `set_reminder` with explicit object schemas
  - keep only the truly required fields in `required`
  - leave action-specific validation to the tool implementation

## 4. `search_tool_registry` has an invalid dynamic-cache schema

- Symptoms:
  - the tool can be hidden from the model as schema-invalid in visibility filtering
- Root cause:
  - the dynamic cache inserts a non-schema JSON object instead of a valid JSON Schema object
- Required fix:
  - replace the cache-time schema with a valid object schema for `{ query: string }`

