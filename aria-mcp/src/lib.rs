use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use aria_core::{
    AgentCapabilityProfile, McpImportedPrompt, McpImportedResource, McpImportedTool,
    McpServerProfile,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpRegistryError {
    UnknownServer(String),
    DisabledServer(String),
    DuplicateImport(String),
    SessionError(String),
}

impl std::fmt::Display for McpRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpRegistryError::UnknownServer(server_id) => {
                write!(f, "unknown MCP server '{}'", server_id)
            }
            McpRegistryError::DisabledServer(server_id) => {
                write!(f, "disabled MCP server '{}'", server_id)
            }
            McpRegistryError::DuplicateImport(import_id) => {
                write!(f, "duplicate MCP import '{}'", import_id)
            }
            McpRegistryError::SessionError(message) => write!(f, "mcp session error: {}", message),
        }
    }
}

impl std::error::Error for McpRegistryError {}

#[derive(Debug, Default, Clone)]
pub struct McpRegistry {
    servers: BTreeMap<String, McpServerProfile>,
    tools: BTreeMap<String, Vec<McpImportedTool>>,
    prompts: BTreeMap<String, Vec<McpImportedPrompt>>,
    resources: BTreeMap<String, Vec<McpImportedResource>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpSession {
    pub server_id: String,
    pub transport: String,
    pub endpoint: String,
    pub protocol_version: Option<String>,
    pub capabilities_json: Option<String>,
    pub last_activity_us: u64,
    pub initialized: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpToolCallResult {
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpPromptRenderResult {
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct McpResourceReadResult {
    pub payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpBoundaryKind {
    LeafExternal,
    NativeInternal,
    ReviewRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpBoundaryRule {
    pub target: String,
    pub classification: McpBoundaryKind,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpBoundaryPolicySnapshot {
    pub rule: String,
    pub leaf_external: Vec<McpBoundaryRule>,
    pub native_internal: Vec<McpBoundaryRule>,
}

fn normalized_mcp_boundary_target(target: &str) -> String {
    target.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn native_internal_targets() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "browser_runtime",
            "Browser runtime owns local profile/session control, challenge handling, and transport security boundaries.",
        ),
        (
            "crawl_runtime",
            "Crawl runtime owns local traversal, watch scheduling, and content-scoping enforcement.",
        ),
        (
            "approval_engine",
            "Approvals are a local human-in-the-loop trust boundary and must remain native to the app.",
        ),
        (
            "vault",
            "Credential storage and secret material must remain inside the native vault boundary, not an external MCP plugin.",
        ),
        (
            "policy_engine",
            "Capability, filesystem, retrieval, and execution policy checks are core local authorization concerns.",
        ),
        (
            "scheduler_core",
            "Scheduling is core runtime orchestration and job ownership, not a leaf integration.",
        ),
        (
            "runtime_store",
            "Runtime persistence is the app's durable systems boundary and should not be delegated to MCP.",
        ),
    ]
}

fn leaf_external_targets() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "github",
            "External developer platforms are strong MCP candidates because they are leaf integrations behind a stable remote API.",
        ),
        (
            "jira",
            "Issue trackers are external service integrations and fit the MCP leaf-integration model.",
        ),
        (
            "linear",
            "Project-management SaaS integrations are leaf capabilities and can be imported cleanly over MCP.",
        ),
        (
            "notion",
            "Knowledge-base SaaS integrations are external content providers, not local trust-boundary systems.",
        ),
        (
            "slack",
            "Messaging SaaS is a leaf external integration and should not be implemented as core runtime logic.",
        ),
        (
            "google_drive",
            "Remote document/storage APIs are good MCP leaf integrations with clear remote ownership.",
        ),
        (
            "s3",
            "Object-store API access is a leaf remote integration rather than a native subsystem boundary.",
        ),
        (
            "docs_saas",
            "Third-party documentation/search SaaS endpoints are leaf external integrations.",
        ),
    ]
}

pub fn mcp_boundary_policy_snapshot() -> McpBoundaryPolicySnapshot {
    McpBoundaryPolicySnapshot {
        rule: "Use MCP for leaf external integrations. Keep trust-boundary subsystems native/internal.".into(),
        leaf_external: leaf_external_targets()
            .iter()
            .map(|(target, rationale)| McpBoundaryRule {
                target: (*target).into(),
                classification: McpBoundaryKind::LeafExternal,
                rationale: (*rationale).into(),
            })
            .collect(),
        native_internal: native_internal_targets()
            .iter()
            .map(|(target, rationale)| McpBoundaryRule {
                target: (*target).into(),
                classification: McpBoundaryKind::NativeInternal,
                rationale: (*rationale).into(),
            })
            .collect(),
    }
}

pub fn classify_mcp_boundary_target(target: &str) -> McpBoundaryRule {
    let normalized = normalized_mcp_boundary_target(target);
    if let Some((canonical, rationale)) = native_internal_targets()
        .iter()
        .find(|(canonical, _)| *canonical == normalized)
    {
        return McpBoundaryRule {
            target: (*canonical).into(),
            classification: McpBoundaryKind::NativeInternal,
            rationale: (*rationale).into(),
        };
    }
    if let Some((canonical, rationale)) = leaf_external_targets()
        .iter()
        .find(|(canonical, _)| *canonical == normalized)
    {
        return McpBoundaryRule {
            target: (*canonical).into(),
            classification: McpBoundaryKind::LeafExternal,
            rationale: (*rationale).into(),
        };
    }
    McpBoundaryRule {
        target: normalized,
        classification: McpBoundaryKind::ReviewRequired,
        rationale:
            "Unclassified target. Use MCP only if this is a leaf external integration and keep local trust-boundary systems native."
                .into(),
    }
}

pub fn reserved_native_mcp_target(target: &str) -> bool {
    classify_mcp_boundary_target(target).classification == McpBoundaryKind::NativeInternal
}

pub trait McpTransport {
    fn open_session(&self, server: &McpServerProfile) -> Result<McpSession, McpRegistryError>;
    fn call_tool(
        &self,
        session: &McpSession,
        tool: &McpImportedTool,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError>;
    fn render_prompt(
        &self,
        session: &McpSession,
        prompt: &McpImportedPrompt,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError>;
    fn read_resource(
        &self,
        session: &McpSession,
        resource: &McpImportedResource,
    ) -> Result<McpResourceReadResult, McpRegistryError>;
}

#[derive(Debug, Default, Clone)]
pub struct LocalStubTransport;

impl McpTransport for LocalStubTransport {
    fn open_session(&self, server: &McpServerProfile) -> Result<McpSession, McpRegistryError> {
        Ok(McpSession {
            server_id: server.server_id.clone(),
            transport: server.transport.clone(),
            endpoint: server.endpoint.clone(),
            protocol_version: None,
            capabilities_json: None,
            last_activity_us: now_us(),
            initialized: false,
            connected: true,
        })
    }

    fn call_tool(
        &self,
        session: &McpSession,
        tool: &McpImportedTool,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError> {
        Ok(McpToolCallResult {
            payload: serde_json::json!({
                "server_id": session.server_id,
                "tool_name": tool.tool_name,
                "input": input,
                "transport": session.transport,
                "endpoint": session.endpoint,
                "mode": "client_stub_v1",
            }),
        })
    }

    fn render_prompt(
        &self,
        session: &McpSession,
        prompt: &McpImportedPrompt,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError> {
        Ok(McpPromptRenderResult {
            payload: serde_json::json!({
                "server_id": session.server_id,
                "prompt_name": prompt.prompt_name,
                "arguments": arguments,
                "transport": session.transport,
                "endpoint": session.endpoint,
                "rendered_prompt": format!("MCP prompt {}::{} ({})", session.server_id, prompt.prompt_name, prompt.description),
                "mode": "client_stub_v1",
            }),
        })
    }

    fn read_resource(
        &self,
        session: &McpSession,
        resource: &McpImportedResource,
    ) -> Result<McpResourceReadResult, McpRegistryError> {
        Ok(McpResourceReadResult {
            payload: serde_json::json!({
                "server_id": session.server_id,
                "resource_uri": resource.resource_uri,
                "transport": session.transport,
                "endpoint": session.endpoint,
                "mime_type": resource.mime_type,
                "content": format!("MCP resource {}::{} ({})", session.server_id, resource.resource_uri, resource.description),
                "mode": "client_stub_v1",
            }),
        })
    }
}

pub struct McpClient<T: McpTransport> {
    registry: McpRegistry,
    transport: T,
    sessions: BTreeMap<String, McpSession>,
}

#[derive(Clone, Default)]
pub struct TransportSelector {
    persistent_stdio: PersistentSubprocessStdioTransport,
    oneshot_stdio: SubprocessStdioTransport,
}

impl McpTransport for TransportSelector {
    fn open_session(&self, server: &McpServerProfile) -> Result<McpSession, McpRegistryError> {
        match server.transport.as_str() {
            "stdio" => self.persistent_stdio.open_session(server),
            "stdio_once" => self.oneshot_stdio.open_session(server),
            _ => LocalStubTransport.open_session(server),
        }
    }

    fn call_tool(
        &self,
        session: &McpSession,
        tool: &McpImportedTool,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError> {
        match session.transport.as_str() {
            "stdio" => self.persistent_stdio.call_tool(session, tool, input),
            "stdio_once" => self.oneshot_stdio.call_tool(session, tool, input),
            _ => LocalStubTransport.call_tool(session, tool, input),
        }
    }

    fn render_prompt(
        &self,
        session: &McpSession,
        prompt: &McpImportedPrompt,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError> {
        match session.transport.as_str() {
            "stdio" => self
                .persistent_stdio
                .render_prompt(session, prompt, arguments),
            "stdio_once" => self.oneshot_stdio.render_prompt(session, prompt, arguments),
            _ => LocalStubTransport.render_prompt(session, prompt, arguments),
        }
    }

    fn read_resource(
        &self,
        session: &McpSession,
        resource: &McpImportedResource,
    ) -> Result<McpResourceReadResult, McpRegistryError> {
        match session.transport.as_str() {
            "stdio" => self.persistent_stdio.read_resource(session, resource),
            "stdio_once" => self.oneshot_stdio.read_resource(session, resource),
            _ => LocalStubTransport.read_resource(session, resource),
        }
    }
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_micros() as u64)
        .unwrap_or(0)
}

struct PersistentChild {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

#[derive(Clone)]
pub struct PersistentSubprocessStdioTransport {
    processes: Arc<Mutex<BTreeMap<String, PersistentChild>>>,
    next_request_id: Arc<AtomicU64>,
}

impl Default for PersistentSubprocessStdioTransport {
    fn default() -> Self {
        Self {
            processes: Arc::new(Mutex::new(BTreeMap::new())),
            next_request_id: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl PersistentSubprocessStdioTransport {
    fn spawn_process(
        &self,
        server_id: &str,
        endpoint: &str,
    ) -> Result<PersistentChild, McpRegistryError> {
        let mut parts = endpoint.split_whitespace();
        let program = parts.next().ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "empty stdio endpoint for server '{}'",
                server_id
            ))
        })?;
        let args: Vec<String> = parts.map(|part| part.to_string()).collect();
        let mut child = Command::new(program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                McpRegistryError::SessionError(format!(
                    "spawn stdio transport '{}' failed: {}",
                    endpoint, e
                ))
            })?;
        let stdin = child.stdin.take().ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "failed to capture stdin for server '{}'",
                server_id
            ))
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "failed to capture stdout for server '{}'",
                server_id
            ))
        })?;
        Ok(PersistentChild {
            _child: child,
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    fn ensure_process(&self, session: &McpSession) -> Result<(), McpRegistryError> {
        let mut guard = self.processes.lock().map_err(|_| {
            McpRegistryError::SessionError("persistent stdio process lock poisoned".into())
        })?;
        if !guard.contains_key(&session.server_id) {
            let child = self.spawn_process(&session.server_id, &session.endpoint)?;
            guard.insert(session.server_id.clone(), child);
        }
        Ok(())
    }

    fn next_request_id(&self) -> u64 {
        self.next_request_id.fetch_add(1, Ordering::Relaxed)
    }

    fn remove_process(&self, server_id: &str) -> Result<(), McpRegistryError> {
        let mut guard = self.processes.lock().map_err(|_| {
            McpRegistryError::SessionError("persistent stdio process lock poisoned".into())
        })?;
        guard.remove(server_id);
        Ok(())
    }

    fn initialize_session(&self, session: &McpSession) -> Result<McpSession, McpRegistryError> {
        let initialize_id = self.next_request_id();
        let response = self.invoke_raw(
            session,
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": initialize_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-05",
                    "capabilities": {
                        "roots": { "listChanged": false },
                        "sampling": {}
                    },
                    "clientInfo": {
                        "name": "aria-x",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            initialize_id,
        )?;
        let result = response
            .get("result")
            .ok_or_else(|| McpRegistryError::SessionError("missing initialize result".into()))?;
        let protocol_version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
        let capabilities_json = result
            .get("capabilities")
            .and_then(|cap| serde_json::to_string(cap).ok());
        let _ = self.notify_raw(
            session,
            serde_json::json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }),
        );
        Ok(McpSession {
            initialized: true,
            protocol_version,
            capabilities_json,
            last_activity_us: now_us(),
            ..session.clone()
        })
    }

    fn invoke_raw(
        &self,
        session: &McpSession,
        request: Value,
        expected_response_id: u64,
    ) -> Result<Value, McpRegistryError> {
        self.ensure_process(session)?;
        let request_line = format!(
            "{}\n",
            serde_json::to_string(&request).map_err(|e| {
                McpRegistryError::SessionError(format!("serialize stdio request failed: {}", e))
            })?
        );
        let mut guard = self.processes.lock().map_err(|_| {
            McpRegistryError::SessionError("persistent stdio process lock poisoned".into())
        })?;
        let child = guard.get_mut(&session.server_id).ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "missing persistent stdio process for '{}'",
                session.server_id
            ))
        })?;
        child
            .stdin
            .write_all(request_line.as_bytes())
            .map_err(|e| {
                McpRegistryError::SessionError(format!("write stdio request failed: {}", e))
            })?;
        child.stdin.flush().map_err(|e| {
            McpRegistryError::SessionError(format!("flush stdio request failed: {}", e))
        })?;
        let mut lines_read: usize = 0;
        loop {
            let mut line = String::new();
            let bytes = child.stdout.read_line(&mut line).map_err(|e| {
                McpRegistryError::SessionError(format!("read stdio response failed: {}", e))
            })?;
            if bytes == 0 {
                guard.remove(&session.server_id);
                return Err(McpRegistryError::SessionError(format!(
                    "stdio transport '{}' closed the session",
                    session.endpoint
                )));
            }
            lines_read += 1;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parsed = match serde_json::from_str::<Value>(trimmed) {
                Ok(value) => value,
                Err(_) => {
                    // Some third-party MCP servers write plain-text log lines to stdout.
                    if lines_read >= 64 {
                        return Err(McpRegistryError::SessionError(format!(
                            "parse stdio response failed after {} lines (last stdout={})",
                            lines_read, trimmed
                        )));
                    }
                    continue;
                }
            };
            if let Some(id) = parsed.get("id") {
                let has_result = parsed.get("result").is_some();
                let has_error = parsed.get("error").is_some();
                let matches = id.as_u64() == Some(expected_response_id)
                    || id.as_str() == Some(&expected_response_id.to_string());
                if matches && (has_result || has_error) {
                    return Ok(parsed);
                }
                if !matches && (has_result || has_error) {
                    // Compatibility fallback for non-compliant MCP servers that do not echo the request id.
                    return Ok(parsed);
                }
                if lines_read >= 64 {
                    return Err(McpRegistryError::SessionError(format!(
                        "MCP response id mismatch after {} lines for server '{}'",
                        lines_read, session.server_id
                    )));
                }
                continue;
            }
            if parsed.get("method").is_some() {
                // Notification/event frame.
                if lines_read >= 64 {
                    return Err(McpRegistryError::SessionError(format!(
                        "MCP response missing id after {} lines for server '{}'",
                        lines_read, session.server_id
                    )));
                }
                continue;
            }
            if parsed.get("result").is_some() || parsed.get("error").is_some() {
                return Ok(parsed);
            }
            if lines_read >= 64 {
                return Err(McpRegistryError::SessionError(format!(
                    "MCP response missing result/error after {} lines for server '{}'",
                    lines_read, session.server_id
                )));
            }
        }
    }

    fn notify_raw(&self, session: &McpSession, request: Value) -> Result<(), McpRegistryError> {
        self.ensure_process(session)?;
        let request_line = format!(
            "{}\n",
            serde_json::to_string(&request).map_err(|e| {
                McpRegistryError::SessionError(format!("serialize stdio request failed: {}", e))
            })?
        );
        let mut guard = self.processes.lock().map_err(|_| {
            McpRegistryError::SessionError("persistent stdio process lock poisoned".into())
        })?;
        let child = guard.get_mut(&session.server_id).ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "missing persistent stdio process for '{}'",
                session.server_id
            ))
        })?;
        child
            .stdin
            .write_all(request_line.as_bytes())
            .map_err(|e| {
                McpRegistryError::SessionError(format!("write stdio request failed: {}", e))
            })?;
        child.stdin.flush().map_err(|e| {
            McpRegistryError::SessionError(format!("flush stdio request failed: {}", e))
        })?;
        Ok(())
    }

    fn invoke_jsonrpc(
        &self,
        session: &McpSession,
        method: &str,
        params: Value,
    ) -> Result<Value, McpRegistryError> {
        let request_id = self.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params,
        });
        let mut last_error = None;
        for attempt in 0..2 {
            match self.invoke_raw(session, request.clone(), request_id) {
                Ok(response) => {
                    if let Some(error) = response.get("error") {
                        return Err(McpRegistryError::SessionError(format!(
                            "MCP {} failed: {}",
                            method, error
                        )));
                    }
                    return response.get("result").cloned().ok_or_else(|| {
                        McpRegistryError::SessionError(format!(
                            "MCP {} response missing result",
                            method
                        ))
                    });
                }
                Err(err) => {
                    last_error = Some(err);
                    if attempt == 0 {
                        let _ = self.remove_process(&session.server_id);
                        let _ = self.initialize_session(session);
                        continue;
                    }
                }
            }
        }
        Err(last_error.unwrap_or_else(|| {
            McpRegistryError::SessionError(format!("MCP {} failed without error", method))
        }))
    }
}

impl McpTransport for PersistentSubprocessStdioTransport {
    fn open_session(&self, server: &McpServerProfile) -> Result<McpSession, McpRegistryError> {
        if server.endpoint.trim().is_empty() {
            return Err(McpRegistryError::SessionError(format!(
                "empty stdio endpoint for server '{}'",
                server.server_id
            )));
        }
        let session = McpSession {
            server_id: server.server_id.clone(),
            transport: server.transport.clone(),
            endpoint: server.endpoint.clone(),
            protocol_version: None,
            capabilities_json: None,
            last_activity_us: now_us(),
            initialized: false,
            connected: true,
        };
        self.ensure_process(&session)?;
        self.initialize_session(&session)
    }

    fn call_tool(
        &self,
        session: &McpSession,
        tool: &McpImportedTool,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError> {
        let payload = self.invoke_jsonrpc(
            session,
            "tools/call",
            serde_json::json!({
                "name": tool.tool_name,
                "arguments": input,
            }),
        )?;
        Ok(McpToolCallResult { payload })
    }

    fn render_prompt(
        &self,
        session: &McpSession,
        prompt: &McpImportedPrompt,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError> {
        let payload = self.invoke_jsonrpc(
            session,
            "prompts/get",
            serde_json::json!({
                "name": prompt.prompt_name,
                "arguments": arguments,
            }),
        )?;
        Ok(McpPromptRenderResult { payload })
    }

    fn read_resource(
        &self,
        session: &McpSession,
        resource: &McpImportedResource,
    ) -> Result<McpResourceReadResult, McpRegistryError> {
        let payload = self.invoke_jsonrpc(
            session,
            "resources/read",
            serde_json::json!({
                "uri": resource.resource_uri,
            }),
        )?;
        Ok(McpResourceReadResult { payload })
    }
}

#[derive(Debug, Default, Clone)]
pub struct SubprocessStdioTransport;

impl SubprocessStdioTransport {
    fn invoke(&self, session: &McpSession, request: Value) -> Result<Value, McpRegistryError> {
        let mut parts = session.endpoint.split_whitespace();
        let program = parts.next().ok_or_else(|| {
            McpRegistryError::SessionError(format!(
                "empty stdio endpoint for server '{}'",
                session.server_id
            ))
        })?;
        let args: Vec<String> = parts.map(|part| part.to_string()).collect();
        let request_bytes = serde_json::to_vec(&request).map_err(|e| {
            McpRegistryError::SessionError(format!("serialize stdio request failed: {}", e))
        })?;

        let mut child = Command::new(program)
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                McpRegistryError::SessionError(format!(
                    "spawn stdio transport '{}' failed: {}",
                    session.endpoint, e
                ))
            })?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(&request_bytes).map_err(|e| {
                McpRegistryError::SessionError(format!("write stdio request failed: {}", e))
            })?;
        }
        let output = child.wait_with_output().map_err(|e| {
            McpRegistryError::SessionError(format!("wait stdio transport failed: {}", e))
        })?;
        if !output.status.success() {
            return Err(McpRegistryError::SessionError(format!(
                "stdio transport exited with status {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        serde_json::from_slice::<Value>(&output.stdout).map_err(|e| {
            McpRegistryError::SessionError(format!(
                "parse stdio response failed: {} (stdout={})",
                e,
                String::from_utf8_lossy(&output.stdout)
            ))
        })
    }
}

impl McpTransport for SubprocessStdioTransport {
    fn open_session(&self, server: &McpServerProfile) -> Result<McpSession, McpRegistryError> {
        if server.endpoint.trim().is_empty() {
            return Err(McpRegistryError::SessionError(format!(
                "empty stdio endpoint for server '{}'",
                server.server_id
            )));
        }
        Ok(McpSession {
            server_id: server.server_id.clone(),
            transport: server.transport.clone(),
            endpoint: server.endpoint.clone(),
            protocol_version: None,
            capabilities_json: None,
            last_activity_us: now_us(),
            initialized: false,
            connected: true,
        })
    }

    fn call_tool(
        &self,
        session: &McpSession,
        tool: &McpImportedTool,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError> {
        let payload = self.invoke(
            session,
            serde_json::json!({
                "kind": "tool",
                "server_id": session.server_id,
                "tool_name": tool.tool_name,
                "input": input,
            }),
        )?;
        Ok(McpToolCallResult { payload })
    }

    fn render_prompt(
        &self,
        session: &McpSession,
        prompt: &McpImportedPrompt,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError> {
        let payload = self.invoke(
            session,
            serde_json::json!({
                "kind": "prompt",
                "server_id": session.server_id,
                "prompt_name": prompt.prompt_name,
                "arguments": arguments,
            }),
        )?;
        Ok(McpPromptRenderResult { payload })
    }

    fn read_resource(
        &self,
        session: &McpSession,
        resource: &McpImportedResource,
    ) -> Result<McpResourceReadResult, McpRegistryError> {
        let payload = self.invoke(
            session,
            serde_json::json!({
                "kind": "resource",
                "server_id": session.server_id,
                "resource_uri": resource.resource_uri,
            }),
        )?;
        Ok(McpResourceReadResult { payload })
    }
}

impl<T: McpTransport> McpClient<T> {
    pub fn new(registry: McpRegistry, transport: T) -> Self {
        Self {
            registry,
            transport,
            sessions: BTreeMap::new(),
        }
    }

    pub fn ensure_session(&mut self, server_id: &str) -> Result<&McpSession, McpRegistryError> {
        if !self.sessions.contains_key(server_id) {
            let server = self
                .registry
                .servers
                .get(server_id)
                .cloned()
                .ok_or_else(|| McpRegistryError::UnknownServer(server_id.into()))?;
            if !server.enabled {
                return Err(McpRegistryError::DisabledServer(server_id.into()));
            }
            let session = self.transport.open_session(&server)?;
            self.sessions.insert(server_id.to_string(), session);
        }
        if let Some(session) = self.sessions.get_mut(server_id) {
            session.last_activity_us = now_us();
        }
        self.sessions.get(server_id).ok_or_else(|| {
            McpRegistryError::SessionError(format!("missing session for '{}'", server_id))
        })
    }

    fn session_supports_primitive(session: &McpSession, primitive: &str) -> bool {
        if !session.initialized {
            return true;
        }
        let Some(capabilities_json) = &session.capabilities_json else {
            return true;
        };
        let parsed: Value = match serde_json::from_str(capabilities_json) {
            Ok(value) => value,
            Err(_) => return false,
        };
        parsed.get(primitive).is_some()
    }

    pub fn call_tool_for_agent(
        &mut self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        tool_name: &str,
        input: Value,
    ) -> Result<McpToolCallResult, McpRegistryError> {
        if !self
            .registry
            .tool_allowed_for_agent(profile, server_id, tool_name)
        {
            return Err(McpRegistryError::SessionError(format!(
                "tool '{}::{}' is not allowed",
                server_id, tool_name
            )));
        }
        let tool = self
            .registry
            .tools
            .get(server_id)
            .and_then(|tools| tools.iter().find(|tool| tool.tool_name == tool_name))
            .cloned()
            .ok_or_else(|| McpRegistryError::UnknownServer(server_id.into()))?;
        let session = self.ensure_session(server_id)?.clone();
        if !Self::session_supports_primitive(&session, "tools") {
            return Err(McpRegistryError::SessionError(format!(
                "MCP server '{}' does not advertise tool capability",
                server_id
            )));
        }
        self.transport.call_tool(&session, &tool, input)
    }

    pub fn render_prompt_for_agent(
        &mut self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        prompt_name: &str,
        arguments: Value,
    ) -> Result<McpPromptRenderResult, McpRegistryError> {
        if !self
            .registry
            .prompt_allowed_for_agent(profile, server_id, prompt_name)
        {
            return Err(McpRegistryError::SessionError(format!(
                "prompt '{}::{}' is not allowed",
                server_id, prompt_name
            )));
        }
        let prompt = self
            .registry
            .prompts
            .get(server_id)
            .and_then(|prompts| {
                prompts
                    .iter()
                    .find(|prompt| prompt.prompt_name == prompt_name)
            })
            .cloned()
            .ok_or_else(|| McpRegistryError::UnknownServer(server_id.into()))?;
        let session = self.ensure_session(server_id)?.clone();
        if !Self::session_supports_primitive(&session, "prompts") {
            return Err(McpRegistryError::SessionError(format!(
                "MCP server '{}' does not advertise prompt capability",
                server_id
            )));
        }
        self.transport.render_prompt(&session, &prompt, arguments)
    }

    pub fn read_resource_for_agent(
        &mut self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        resource_uri: &str,
    ) -> Result<McpResourceReadResult, McpRegistryError> {
        if !self
            .registry
            .resource_allowed_for_agent(profile, server_id, resource_uri)
        {
            return Err(McpRegistryError::SessionError(format!(
                "resource '{}::{}' is not allowed",
                server_id, resource_uri
            )));
        }
        let resource = self
            .registry
            .resources
            .get(server_id)
            .and_then(|resources| {
                resources
                    .iter()
                    .find(|resource| resource.resource_uri == resource_uri)
            })
            .cloned()
            .ok_or_else(|| McpRegistryError::UnknownServer(server_id.into()))?;
        let session = self.ensure_session(server_id)?.clone();
        if !Self::session_supports_primitive(&session, "resources") {
            return Err(McpRegistryError::SessionError(format!(
                "MCP server '{}' does not advertise resource capability",
                server_id
            )));
        }
        self.transport.read_resource(&session, &resource)
    }

    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    pub fn session_health(&self) -> Vec<Value> {
        self.sessions
            .values()
            .map(|session| {
                serde_json::json!({
                    "server_id": session.server_id,
                    "transport": session.transport,
                    "endpoint": session.endpoint,
                    "connected": session.connected,
                    "initialized": session.initialized,
                    "protocol_version": session.protocol_version,
                    "capabilities_json": session.capabilities_json,
                    "last_activity_us": session.last_activity_us,
                })
            })
            .collect()
    }

    pub fn probe_sessions(&mut self) -> Vec<Value> {
        let now = now_us();
        self.sessions
            .iter_mut()
            .map(|(server_id, session)| {
                let idle_us = now.saturating_sub(session.last_activity_us);
                serde_json::json!({
                    "server_id": server_id,
                    "connected": session.connected,
                    "initialized": session.initialized,
                    "idle_seconds": idle_us / 1_000_000,
                })
            })
            .collect()
    }

    pub fn evict_stale_sessions(&mut self, max_idle_seconds: u64, now_us_override: u64) -> usize {
        let before = self.sessions.len();
        let max_idle_us = max_idle_seconds.saturating_mul(1_000_000);
        self.sessions.retain(|_, session| {
            now_us_override.saturating_sub(session.last_activity_us) <= max_idle_us
        });
        before.saturating_sub(self.sessions.len())
    }
}

impl McpRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_server(&mut self, profile: McpServerProfile) {
        self.servers.insert(profile.server_id.clone(), profile);
    }

    pub fn list_servers(&self) -> Vec<McpServerProfile> {
        self.servers.values().cloned().collect()
    }

    pub fn import_tool(&mut self, tool: McpImportedTool) -> Result<(), McpRegistryError> {
        self.ensure_server_enabled(&tool.server_id)?;
        let entries = self.tools.entry(tool.server_id.clone()).or_default();
        if entries
            .iter()
            .any(|entry| entry.import_id == tool.import_id)
        {
            return Err(McpRegistryError::DuplicateImport(tool.import_id));
        }
        entries.push(tool);
        Ok(())
    }

    pub fn import_prompt(&mut self, prompt: McpImportedPrompt) -> Result<(), McpRegistryError> {
        self.ensure_server_enabled(&prompt.server_id)?;
        let entries = self.prompts.entry(prompt.server_id.clone()).or_default();
        if entries
            .iter()
            .any(|entry| entry.import_id == prompt.import_id)
        {
            return Err(McpRegistryError::DuplicateImport(prompt.import_id));
        }
        entries.push(prompt);
        Ok(())
    }

    pub fn import_resource(
        &mut self,
        resource: McpImportedResource,
    ) -> Result<(), McpRegistryError> {
        self.ensure_server_enabled(&resource.server_id)?;
        let entries = self
            .resources
            .entry(resource.server_id.clone())
            .or_default();
        if entries
            .iter()
            .any(|entry| entry.import_id == resource.import_id)
        {
            return Err(McpRegistryError::DuplicateImport(resource.import_id));
        }
        entries.push(resource);
        Ok(())
    }

    pub fn list_imported_tools(&self, server_id: &str) -> Vec<McpImportedTool> {
        self.tools.get(server_id).cloned().unwrap_or_default()
    }

    pub fn list_imported_prompts(&self, server_id: &str) -> Vec<McpImportedPrompt> {
        self.prompts.get(server_id).cloned().unwrap_or_default()
    }

    pub fn list_imported_resources(&self, server_id: &str) -> Vec<McpImportedResource> {
        self.resources.get(server_id).cloned().unwrap_or_default()
    }

    pub fn list_tool_catalog_entries(&self, server_id: &str) -> Vec<aria_core::ToolCatalogEntry> {
        self.list_imported_tools(server_id)
            .into_iter()
            .map(|tool| aria_core::ToolCatalogEntry {
                tool_id: format!("mcp.{}.{}", tool.server_id, tool.tool_name),
                public_name: tool.tool_name.clone(),
                description: tool.description.clone(),
                parameters_json_schema: tool.parameters_schema.clone(),
                execution_kind: aria_core::ToolExecutionKind::McpImported,
                provider_kind: aria_core::ToolProviderKind::Mcp,
                runner_class: aria_core::ToolRunnerClass::Mcp,
                origin: aria_core::ToolOrigin {
                    provider_kind: aria_core::ToolProviderKind::Mcp,
                    provider_id: tool.server_id.clone(),
                    origin_id: Some(tool.import_id.clone()),
                    display_name: self
                        .servers
                        .get(&tool.server_id)
                        .map(|server| server.display_name.clone()),
                },
                artifact_kind: Some("mcp".into()),
                requires_approval: aria_core::ToolApprovalClass::None,
                side_effect_level: aria_core::ToolSideEffectLevel::ReadOnly,
                streaming_safe: false,
                parallel_safe: true,
                modalities: vec![aria_core::ToolModality::Text],
                capability_requirements: vec![
                    format!("mcp_server_allowlist:{}", tool.server_id),
                    format!("mcp_tool_allowlist:{}", tool.tool_name),
                ],
            })
            .collect()
    }

    pub fn list_prompt_assets(&self, server_id: &str) -> Vec<aria_core::PromptAssetEntry> {
        self.list_imported_prompts(server_id)
            .into_iter()
            .map(|prompt| aria_core::PromptAssetEntry {
                asset_id: format!("mcp.{}.{}", prompt.server_id, prompt.prompt_name),
                public_name: prompt.prompt_name.clone(),
                description: prompt.description.clone(),
                origin: aria_core::ToolOrigin {
                    provider_kind: aria_core::ToolProviderKind::Mcp,
                    provider_id: prompt.server_id.clone(),
                    origin_id: Some(prompt.import_id.clone()),
                    display_name: self
                        .servers
                        .get(&prompt.server_id)
                        .map(|server| server.display_name.clone()),
                },
                arguments_json_schema: prompt.arguments_schema.clone(),
            })
            .collect()
    }

    pub fn list_resource_context_entries(
        &self,
        server_id: &str,
    ) -> Vec<aria_core::ResourceContextEntry> {
        self.list_imported_resources(server_id)
            .into_iter()
            .map(|resource| aria_core::ResourceContextEntry {
                resource_id: format!("mcp.{}.{}", resource.server_id, resource.resource_uri),
                public_name: resource.resource_uri.clone(),
                description: resource.description.clone(),
                origin: aria_core::ToolOrigin {
                    provider_kind: aria_core::ToolProviderKind::Mcp,
                    provider_id: resource.server_id.clone(),
                    origin_id: Some(resource.import_id.clone()),
                    display_name: self
                        .servers
                        .get(&resource.server_id)
                        .map(|server| server.display_name.clone()),
                },
                mime_type: resource.mime_type.clone(),
            })
            .collect()
    }

    pub fn tool_allowed_for_agent(
        &self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        tool_name: &str,
    ) -> bool {
        if !self.server_enabled(server_id) {
            return false;
        }
        if !profile
            .mcp_server_allowlist
            .iter()
            .any(|server| server == server_id)
        {
            return false;
        }
        if !profile
            .mcp_tool_allowlist
            .iter()
            .any(|tool| tool == tool_name)
        {
            return false;
        }
        self.tools
            .get(server_id)
            .map(|tools| tools.iter().any(|tool| tool.tool_name == tool_name))
            .unwrap_or(false)
    }

    pub fn prompt_allowed_for_agent(
        &self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        prompt_name: &str,
    ) -> bool {
        if !self.server_enabled(server_id) {
            return false;
        }
        if !profile
            .mcp_server_allowlist
            .iter()
            .any(|server| server == server_id)
        {
            return false;
        }
        if !profile
            .mcp_prompt_allowlist
            .iter()
            .any(|prompt| prompt == prompt_name)
        {
            return false;
        }
        self.prompts
            .get(server_id)
            .map(|prompts| {
                prompts
                    .iter()
                    .any(|prompt| prompt.prompt_name == prompt_name)
            })
            .unwrap_or(false)
    }

    pub fn resource_allowed_for_agent(
        &self,
        profile: &AgentCapabilityProfile,
        server_id: &str,
        resource_uri: &str,
    ) -> bool {
        if !self.server_enabled(server_id) {
            return false;
        }
        if !profile
            .mcp_server_allowlist
            .iter()
            .any(|server| server == server_id)
        {
            return false;
        }
        if !profile
            .mcp_resource_allowlist
            .iter()
            .any(|resource| resource == resource_uri)
        {
            return false;
        }
        self.resources
            .get(server_id)
            .map(|resources| {
                resources
                    .iter()
                    .any(|resource| resource.resource_uri == resource_uri)
            })
            .unwrap_or(false)
    }

    fn ensure_server_enabled(&self, server_id: &str) -> Result<(), McpRegistryError> {
        match self.servers.get(server_id) {
            Some(profile) if profile.enabled => Ok(()),
            Some(_) => Err(McpRegistryError::DisabledServer(server_id.into())),
            None => Err(McpRegistryError::UnknownServer(server_id.into())),
        }
    }

    fn server_enabled(&self, server_id: &str) -> bool {
        self.servers
            .get(server_id)
            .map(|profile| profile.enabled)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{AgentClass, SideEffectLevel};

    fn profile() -> AgentCapabilityProfile {
        AgentCapabilityProfile {
            agent_id: "developer".into(),
            class: AgentClass::Generalist,
            tool_allowlist: vec![],
            skill_allowlist: vec![],
            mcp_server_allowlist: vec!["github".into()],
            mcp_tool_allowlist: vec!["create_issue".into()],
            mcp_prompt_allowlist: vec!["review_pr".into()],
            mcp_resource_allowlist: vec!["repo://issues".into()],
            filesystem_scopes: vec![],
            retrieval_scopes: vec![],
            delegation_scope: None,
            web_domain_allowlist: vec![],
            web_domain_blocklist: vec![],
            browser_profile_allowlist: vec![],
            browser_action_scope: None,
            browser_session_scope: None,
            crawl_scope: None,
            web_approval_policy: None,
            web_transport_allowlist: vec![],
            requires_elevation: false,
            side_effect_level: SideEffectLevel::ReadOnly,
            trust_profile: None,
        }
    }

    #[test]
    fn registry_round_trips_server_and_imports() {
        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stdio".into(),
            endpoint: "npx server-github".into(),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "github".into(),
                tool_name: "create_issue".into(),
                description: "Create issue".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");
        registry
            .import_prompt(McpImportedPrompt {
                import_id: "prompt-1".into(),
                server_id: "github".into(),
                prompt_name: "review_pr".into(),
                description: "Review PR".into(),
                arguments_schema: None,
            })
            .expect("import prompt");
        registry
            .import_resource(McpImportedResource {
                import_id: "resource-1".into(),
                server_id: "github".into(),
                resource_uri: "repo://issues".into(),
                description: "Issues".into(),
                mime_type: Some("application/json".into()),
            })
            .expect("import resource");

        assert_eq!(registry.list_servers().len(), 1);
        assert_eq!(registry.list_imported_tools("github").len(), 1);
        assert_eq!(registry.list_imported_prompts("github").len(), 1);
        assert_eq!(registry.list_imported_resources("github").len(), 1);
    }

    #[test]
    fn registry_enforces_explicit_allowlists_for_tools_prompts_and_resources() {
        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stdio".into(),
            endpoint: "npx server-github".into(),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "github".into(),
                tool_name: "create_issue".into(),
                description: "Create issue".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");
        registry
            .import_prompt(McpImportedPrompt {
                import_id: "prompt-1".into(),
                server_id: "github".into(),
                prompt_name: "review_pr".into(),
                description: "Review PR".into(),
                arguments_schema: None,
            })
            .expect("import prompt");
        registry
            .import_resource(McpImportedResource {
                import_id: "resource-1".into(),
                server_id: "github".into(),
                resource_uri: "repo://issues".into(),
                description: "Issues".into(),
                mime_type: None,
            })
            .expect("import resource");

        assert!(registry.tool_allowed_for_agent(&profile(), "github", "create_issue"));
        assert!(registry.prompt_allowed_for_agent(&profile(), "github", "review_pr"));
        assert!(registry.resource_allowed_for_agent(&profile(), "github", "repo://issues"));

        let mut deny_profile = profile();
        deny_profile.mcp_tool_allowlist.clear();
        deny_profile.mcp_prompt_allowlist.clear();
        deny_profile.mcp_resource_allowlist.clear();

        assert!(!registry.tool_allowed_for_agent(&deny_profile, "github", "create_issue"));
        assert!(!registry.prompt_allowed_for_agent(&deny_profile, "github", "review_pr"));
        assert!(!registry.resource_allowed_for_agent(&deny_profile, "github", "repo://issues"));
    }

    #[test]
    fn client_opens_and_reuses_sessions_for_allowed_calls() {
        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stdio".into(),
            endpoint: "npx server-github".into(),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "github".into(),
                tool_name: "create_issue".into(),
                description: "Create issue".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");
        let mut client = McpClient::new(registry, LocalStubTransport);

        let first = client
            .call_tool_for_agent(
                &profile(),
                "github",
                "create_issue",
                serde_json::json!({"title":"Bug"}),
            )
            .expect("first tool call");
        let second = client
            .call_tool_for_agent(
                &profile(),
                "github",
                "create_issue",
                serde_json::json!({"title":"Bug 2"}),
            )
            .expect("second tool call");

        assert_eq!(client.session_count(), 1);
        assert_eq!(first.payload["mode"].as_str(), Some("client_stub_v1"));
        assert_eq!(second.payload["transport"].as_str(), Some("stdio"));
        let health = client.session_health();
        assert_eq!(health.len(), 1);
        assert_eq!(health[0]["server_id"], "github");
        assert_eq!(health[0]["initialized"], false);
    }

    #[test]
    fn transport_selector_reuses_persistent_stdio_process() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-echo.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\ncount=0\nwhile IFS= read -r line; do\n  if printf '%s' \"$line\" | grep -q '\"method\":\"initialize\"'; then\n    printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-11-05\",\"capabilities\":{\"tools\":{},\"prompts\":{},\"resources\":{}}}}\\n'\n    continue\n  fi\n  if printf '%s' \"$line\" | grep -q '\"method\":\"notifications/initialized\"'; then\n    continue\n  fi\n  count=$((count + 1))\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"mode\":\"stdio_persistent_v2\",\"count\":%d}}\\n' \"$count\"\ndone\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let first = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":1}))
            .expect("first stdio call");
        let second = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":2}))
            .expect("second stdio call");

        assert_eq!(first.payload["mode"].as_str(), Some("stdio_persistent_v2"));
        assert_eq!(first.payload["count"].as_i64(), Some(1));
        assert_eq!(second.payload["count"].as_i64(), Some(2));
        assert_eq!(client.session_count(), 1);
        let health = client.session_health();
        assert_eq!(health.len(), 1);
        assert_eq!(health[0]["initialized"], true);
        assert!(health[0]["capabilities_json"].as_str().is_some());
    }

    #[test]
    fn transport_selector_ignores_notification_frames_before_response() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-notify-first.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\nwhile IFS= read -r line; do\n  if printf '%s' \"$line\" | grep -q '\"method\":\"initialize\"'; then\n    printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-11-05\",\"capabilities\":{\"tools\":{}}}}\\n'\n    continue\n  fi\n  if printf '%s' \"$line\" | grep -q '\"method\":\"notifications/initialized\"'; then\n    continue\n  fi\n  printf '{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{\"level\":\"info\",\"data\":\"ping\"}}\\n'\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"mode\":\"stdio_notify_first\",\"ok\":true}}\\n'\ndone\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let result = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":1}))
            .expect("stdio call with notify frame");
        assert_eq!(result.payload["mode"].as_str(), Some("stdio_notify_first"));
        assert_eq!(result.payload["ok"].as_bool(), Some(true));
    }

    #[test]
    fn transport_selector_skips_id_matched_frames_without_result_or_error() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-empty-id-first.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\nwhile IFS= read -r line; do\n  if printf '%s' \"$line\" | grep -q '\"method\":\"initialize\"'; then\n    printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-11-05\",\"capabilities\":{\"tools\":{}}}}\\n'\n    continue\n  fi\n  if printf '%s' \"$line\" | grep -q '\"method\":\"notifications/initialized\"'; then\n    continue\n  fi\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2}\\n'\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"mode\":\"stdio_id_empty_first\",\"ok\":true}}\\n'\ndone\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let result = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":1}))
            .expect("stdio call with id-only frame");
        assert_eq!(
            result.payload["mode"].as_str(),
            Some("stdio_id_empty_first")
        );
        assert_eq!(result.payload["ok"].as_bool(), Some(true));
    }

    #[test]
    fn transport_selector_reconnects_after_stdio_process_exit() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-restart.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\nseen=0\nwhile IFS= read -r line; do\n  if printf '%s' \"$line\" | grep -q '\"method\":\"initialize\"'; then\n    printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-11-05\",\"capabilities\":{\"tools\":{}}}}\\n'\n    continue\n  fi\n  if printf '%s' \"$line\" | grep -q '\"method\":\"notifications/initialized\"'; then\n    continue\n  fi\n  seen=$((seen + 1))\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"run\":%d}}\\n' \"$seen\"\n  exit 0\ndone\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let first = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":1}))
            .expect("first stdio call");
        let second = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":2}))
            .expect("reconnected stdio call");

        assert_eq!(first.payload["run"].as_i64(), Some(1));
        assert_eq!(second.payload["run"].as_i64(), Some(1));
        assert_eq!(client.session_count(), 1);
    }

    #[test]
    fn client_denies_calls_when_server_capability_is_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-no-tools.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\nwhile IFS= read -r line; do\n  if printf '%s' \"$line\" | grep -q '\"method\":\"initialize\"'; then\n    printf '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-11-05\",\"capabilities\":{\"prompts\":{}}}}\\n'\n    continue\n  fi\n  if printf '%s' \"$line\" | grep -q '\"method\":\"notifications/initialized\"'; then\n    continue\n  fi\n  printf '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\\n'\ndone\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let err = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({}))
            .expect_err("call should fail without tools capability");
        assert!(err
            .to_string()
            .contains("does not advertise tool capability"));
    }

    #[test]
    fn client_evicts_stale_sessions() {
        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stub".into(),
            endpoint: "stub://github".into(),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "github".into(),
                tool_name: "create_issue".into(),
                description: "Create issue".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");
        let mut client = McpClient::new(registry, TransportSelector::default());
        client
            .call_tool_for_agent(
                &profile(),
                "github",
                "create_issue",
                serde_json::json!({"title":"Bug"}),
            )
            .expect("seed session");
        assert_eq!(client.session_count(), 1);
        let evicted = client.evict_stale_sessions(0, now_us().saturating_add(1_000_000));
        assert_eq!(evicted, 1);
        assert_eq!(client.session_count(), 0);
    }

    #[test]
    fn transport_selector_supports_stdio_once_mode() {
        let temp = tempfile::tempdir().expect("tempdir");
        let script_path = temp.path().join("mcp-once.sh");
        std::fs::write(
            &script_path,
            "#!/bin/sh\ncat >/dev/null\nprintf '{\"mode\":\"stdio_once\"}'\n",
        )
        .expect("write script");

        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "echo".into(),
            display_name: "Echo".into(),
            transport: "stdio_once".into(),
            endpoint: format!("sh {}", script_path.display()),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "echo".into(),
                tool_name: "ping".into(),
                description: "Ping".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");

        let mut allow = profile();
        allow.mcp_server_allowlist = vec!["echo".into()];
        allow.mcp_tool_allowlist = vec!["ping".into()];
        allow.mcp_prompt_allowlist.clear();
        allow.mcp_resource_allowlist.clear();

        let mut client = McpClient::new(registry, TransportSelector::default());
        let result = client
            .call_tool_for_agent(&allow, "echo", "ping", serde_json::json!({"x":1}))
            .expect("stdio_once tool call");
        assert_eq!(result.payload["mode"].as_str(), Some("stdio_once"));
    }

    #[test]
    fn mcp_boundary_policy_snapshot_lists_native_and_leaf_targets() {
        let snapshot = mcp_boundary_policy_snapshot();
        assert!(snapshot
            .native_internal
            .iter()
            .any(|rule| rule.target == "browser_runtime"));
        assert!(snapshot
            .native_internal
            .iter()
            .any(|rule| rule.target == "runtime_store"));
        assert!(snapshot
            .leaf_external
            .iter()
            .any(|rule| rule.target == "github"));
        assert!(snapshot
            .leaf_external
            .iter()
            .any(|rule| rule.target == "notion"));
    }

    #[test]
    fn classify_mcp_boundary_target_marks_unknowns_for_review() {
        assert_eq!(
            classify_mcp_boundary_target("browser_runtime").classification,
            McpBoundaryKind::NativeInternal
        );
        assert_eq!(
            classify_mcp_boundary_target("github").classification,
            McpBoundaryKind::LeafExternal
        );
        assert_eq!(
            classify_mcp_boundary_target("custom_vendor_x").classification,
            McpBoundaryKind::ReviewRequired
        );
        assert!(reserved_native_mcp_target("scheduler_core"));
        assert!(!reserved_native_mcp_target("linear"));
    }

    #[test]
    fn registry_projects_mcp_imports_into_normalized_catalog_and_assets() {
        let mut registry = McpRegistry::new();
        registry.register_server(McpServerProfile {
            server_id: "github".into(),
            display_name: "GitHub".into(),
            transport: "stub".into(),
            endpoint: "stub://github".into(),
            auth_ref: None,
            enabled: true,
        });
        registry
            .import_tool(McpImportedTool {
                import_id: "tool-1".into(),
                server_id: "github".into(),
                tool_name: "create_issue".into(),
                description: "Create issue".into(),
                parameters_schema: "{}".into(),
            })
            .expect("import tool");
        registry
            .import_prompt(McpImportedPrompt {
                import_id: "prompt-1".into(),
                server_id: "github".into(),
                prompt_name: "review_pr".into(),
                description: "Review PR".into(),
                arguments_schema: Some("{}".into()),
            })
            .expect("import prompt");
        registry
            .import_resource(McpImportedResource {
                import_id: "resource-1".into(),
                server_id: "github".into(),
                resource_uri: "repo://issues".into(),
                description: "Issue feed".into(),
                mime_type: Some("application/json".into()),
            })
            .expect("import resource");

        let tools = registry.list_tool_catalog_entries("github");
        let prompts = registry.list_prompt_assets("github");
        let resources = registry.list_resource_context_entries("github");

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].provider_kind, aria_core::ToolProviderKind::Mcp);
        assert_eq!(tools[0].runner_class, aria_core::ToolRunnerClass::Mcp);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].public_name, "review_pr");
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].public_name, "repo://issues");
    }
}
