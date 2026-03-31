#[cfg(test)]
fn approval_records_dir(sessions_dir: &Path) -> PathBuf {
    sessions_dir.join("approvals")
}

#[cfg(test)]
fn approval_record_path(sessions_dir: &Path, approval_id: &str) -> PathBuf {
    approval_records_dir(sessions_dir).join(format!("{}.json", approval_id))
}

fn approval_id_for(session_id: uuid::Uuid, tool_name: &str) -> String {
    format!("{}__{}", session_id, tool_name)
}

fn request_trace_id(req: &AgentRequest) -> String {
    uuid::Uuid::from_bytes(req.request_id).to_string()
}

const AGENT_ELEVATION_TOOL_NAME: &str = "__agent_elevation__";
const APPROVAL_HANDLE_TTL_US: u64 = 24 * 60 * 60 * 1_000_000;

fn write_elevation_grant(sessions_dir: &Path, grant: &aria_core::ElevationGrant) -> io::Result<()> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .upsert_elevation(grant)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

#[cfg(test)]
fn read_elevation_grant(
    sessions_dir: &Path,
    session_id: uuid::Uuid,
    agent_id: &str,
) -> io::Result<aria_core::ElevationGrant> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .read_elevation(session_id, agent_id)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

fn has_active_elevation_grant(
    sessions_dir: &Path,
    session_id: uuid::Uuid,
    user_id: &str,
    agent_id: &str,
    now_us: u64,
) -> bool {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .has_active_elevation(session_id, user_id, agent_id, now_us)
        .unwrap_or(false)
}

fn build_agent_elevation_message(agent_id: &str) -> String {
    format!(
        "Privilege elevation required for agent '{}'. Please approve this session before continuing.",
        agent_id
    )
}

fn build_agent_elevation_approval_record(
    req: &AgentRequest,
    agent_id: &str,
) -> aria_core::ApprovalRecord {
    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
    aria_core::ApprovalRecord {
        approval_id: approval_id_for(session_uuid, AGENT_ELEVATION_TOOL_NAME),
        session_id: req.session_id,
        user_id: req.user_id.clone(),
        channel: req.channel,
        agent_id: agent_id.to_string(),
        tool_name: AGENT_ELEVATION_TOOL_NAME.to_string(),
        arguments_json: serde_json::json!({
            "agent_id": agent_id,
            "_trace_id": request_trace_id(req),
        })
        .to_string(),
        pending_prompt: String::new(),
        original_request: request_text_from_content(&req.content),
        status: aria_core::ApprovalStatus::Pending,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
        resolved_at_us: None,
    }
}

fn build_tool_approval_record(
    req: &AgentRequest,
    call: &ToolCall,
    pending_prompt: String,
) -> aria_core::ApprovalRecord {
    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
    let trace_id = request_trace_id(req);
    let mut arguments_value = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .unwrap_or_else(|_| serde_json::json!({}));
    if let Some(map) = arguments_value.as_object_mut() {
        map.insert(
            "_trace_id".to_string(),
            serde_json::Value::String(trace_id),
        );
    }
    aria_core::ApprovalRecord {
        approval_id: approval_id_for(session_uuid, &call.name),
        session_id: req.session_id,
        user_id: req.user_id.clone(),
        channel: req.channel,
        agent_id: "pending".to_string(),
        tool_name: call.name.clone(),
        arguments_json: arguments_value.to_string(),
        pending_prompt,
        original_request: request_text_from_content(&req.content),
        status: aria_core::ApprovalStatus::Pending,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
        resolved_at_us: None,
    }
}

fn extract_domain_approval_request(
    req: &AgentRequest,
    error_message: &str,
) -> Option<(String, aria_core::WebActionFamily)> {
    let action_family = if error_message.contains("policy denied action 'web_domain_fetch'") {
        aria_core::WebActionFamily::Fetch
    } else if error_message.contains("policy denied action 'web_domain_crawl'") {
        aria_core::WebActionFamily::Crawl
    } else if error_message.contains("policy denied action 'web_domain_screenshot'") {
        aria_core::WebActionFamily::Screenshot
    } else if error_message.contains("policy denied action 'web_domain_interactive_read'") {
        aria_core::WebActionFamily::InteractiveRead
    } else if error_message.contains("policy denied action 'web_domain_interactive_write'") {
        aria_core::WebActionFamily::InteractiveWrite
    } else if error_message.contains("policy denied action 'web_domain_login'") {
        aria_core::WebActionFamily::Login
    } else if error_message.contains("policy denied action 'web_domain_download'") {
        aria_core::WebActionFamily::Download
    } else {
        return None;
    };

    let text = request_text_from_content(&req.content);
    let domain_from_text = text
        .split_whitespace()
        .find_map(|token| {
            let candidate = token
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | '.' | ')' | '('));
            reqwest::Url::parse(candidate)
                .ok()
                .and_then(|url| url.host_str().map(|host| host.to_ascii_lowercase()))
        });

    let domain = if let Some(domain) = domain_from_text {
        domain
    } else if let Some(resource) = error_message
        .split("resource '")
        .nth(1)
        .and_then(|rest| rest.strip_suffix('\''))
    {
        resource
            .strip_prefix("web_domain_")
            .unwrap_or(resource)
            .replace('_', ".")
    } else {
        return None;
    };

    Some((domain, action_family))
}

fn build_domain_access_approval_record(
    req: &AgentRequest,
    domain: &str,
    action_family: aria_core::WebActionFamily,
) -> aria_core::ApprovalRecord {
    let session_uuid = uuid::Uuid::from_bytes(req.session_id);
    aria_core::ApprovalRecord {
        approval_id: format!("{}__set_domain_access_decision__{}", session_uuid, domain),
        session_id: req.session_id,
        user_id: req.user_id.clone(),
        channel: req.channel,
        agent_id: "pending".to_string(),
        tool_name: "set_domain_access_decision".to_string(),
        arguments_json: serde_json::json!({
            "domain": domain,
            "decision": "allow_once",
            "action_family": match action_family {
                aria_core::WebActionFamily::Fetch => "fetch",
                aria_core::WebActionFamily::Crawl => "crawl",
                aria_core::WebActionFamily::Screenshot => "screenshot",
                aria_core::WebActionFamily::InteractiveRead => "interactive_read",
                aria_core::WebActionFamily::InteractiveWrite => "interactive_write",
                aria_core::WebActionFamily::Login => "login",
                aria_core::WebActionFamily::Download => "download",
            },
            "scope": "domain",
            "_trace_id": request_trace_id(req),
        })
        .to_string(),
        pending_prompt: String::new(),
        original_request: request_text_from_content(&req.content),
        status: aria_core::ApprovalStatus::Pending,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
        resolved_at_us: None,
    }
}

fn write_approval_record(
    sessions_dir: &Path,
    record: &aria_core::ApprovalRecord,
) -> io::Result<()> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .upsert_approval(record)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

pub(crate) fn ensure_approval_handle(
    sessions_dir: &Path,
    record: &aria_core::ApprovalRecord,
) -> io::Result<String> {
    let expires_at_us = record
        .created_at_us
        .saturating_add(APPROVAL_HANDLE_TTL_US);
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let _ = store.prune_expired_approval_handles(record.created_at_us);
    store
        .resolve_or_create_approval_handle(
            &record.approval_id,
            record.session_id,
            &record.user_id,
            record.created_at_us,
            expires_at_us,
        )
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

fn resolve_approval_selector(
    sessions_dir: &Path,
    session_id: [u8; 16],
    user_id: &str,
    selector: &str,
) -> Result<String, String> {
    let token = selector.trim();
    if token.is_empty() {
        return Err("Approval selector cannot be empty.".to_string());
    }
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    if let Some(approval_id) = store
        .resolve_approval_handle(token, session_id, user_id, now_us)
        .map_err(|err| format!("resolve approval selector failed: {}", err))?
    {
        return Ok(approval_id);
    }
    Ok(token.to_string())
}

fn read_approval_record(
    sessions_dir: &Path,
    approval_id: &str,
) -> io::Result<aria_core::ApprovalRecord> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .read_approval(approval_id)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

#[cfg(test)]
fn remove_approval_record(sessions_dir: &Path, approval_id: &str) -> io::Result<()> {
    RuntimeStore::for_sessions_dir(sessions_dir)
        .delete_approval(approval_id)
        .map_err(|err| io::Error::new(io::ErrorKind::Other, err))
}

fn resolve_approval_record(
    sessions_dir: &Path,
    approval_id: &str,
    decision: aria_core::ApprovalResolutionDecision,
) -> Result<aria_core::ApprovalRecord, String> {
    let mut record = read_approval_record(sessions_dir, approval_id)
        .map_err(|err| format!("Approval '{}' not found: {}", approval_id, err))?;
    if record.status != aria_core::ApprovalStatus::Pending {
        return Err(format!(
            "Approval '{}' is already {:?}.",
            approval_id, record.status
        ));
    }
    record.status = match decision {
        aria_core::ApprovalResolutionDecision::Approve => aria_core::ApprovalStatus::Approved,
        aria_core::ApprovalResolutionDecision::Deny => aria_core::ApprovalStatus::Denied,
    };
    record.resolved_at_us = Some(chrono::Utc::now().timestamp_micros() as u64);
    write_approval_record(sessions_dir, &record)
        .map_err(|err| format!("write approval record failed: {}", err))?;
    Ok(record)
}

#[derive(Debug, Clone, Serialize)]
struct ApprovalDisplayDescriptor {
    approval_id: String,
    tool_name: String,
    agent_id: String,
    action_summary: String,
    target_summary: Option<String>,
    risk_summary: String,
    arguments_preview: String,
    options: Vec<&'static str>,
}

fn approval_options_for_tool(tool_name: &str) -> Vec<&'static str> {
    match tool_name {
        "set_domain_access_decision" => vec![
            "allow once",
            "allow for session",
            "allow always",
            "deny once",
            "deny always",
        ],
        AGENT_ELEVATION_TOOL_NAME => vec!["elevate this session", "deny"],
        _ => vec!["approve once", "deny"],
    }
}

pub(crate) fn build_approval_descriptor(record: &aria_core::ApprovalRecord) -> ApprovalDisplayDescriptor {
    let args = serde_json::from_str::<serde_json::Value>(&record.arguments_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    let target_summary = args
        .get("url")
        .and_then(|v| v.as_str())
        .map(|v| format!("url={}", v))
        .or_else(|| {
            args.get("domain")
                .and_then(|v| v.as_str())
                .map(|v| format!("domain={}", v))
        })
        .or_else(|| {
            args.get("profile_id")
                .and_then(|v| v.as_str())
                .map(|v| format!("profile={}", v))
        })
        .or_else(|| {
            args.get("agent_id")
                .and_then(|v| v.as_str())
                .map(|v| format!("agent={}", v))
        })
        .or_else(|| {
            match (
                args.get("x").and_then(|v| v.as_i64()),
                args.get("y").and_then(|v| v.as_i64()),
            ) {
                (Some(x), Some(y)) => Some(format!("point={},{}", x, y)),
                _ => None,
            }
        })
        .or_else(|| {
            args.get("profile_id")
                .and_then(|v| v.as_str())
                .map(|v| format!("profile={}", v))
        });
    let action_summary = match record.tool_name.as_str() {
        AGENT_ELEVATION_TOOL_NAME => "elevate agent privileges".to_string(),
        "browser_download" => "download remote content".to_string(),
        "browser_act" => args
            .get("action")
            .and_then(|v| v.as_str())
            .map(|action| format!("browser action: {}", action))
            .unwrap_or_else(|| "browser action".to_string()),
        "computer_act" => args
            .get("action")
            .and_then(|v| v.as_str())
            .map(|action| format!("computer action: {}", action))
            .unwrap_or_else(|| "computer action".to_string()),
        "computer_capture" | "computer_screenshot" => "capture desktop screenshot".to_string(),
        "set_domain_access_decision" => "change stored domain access policy".to_string(),
        other => format!("execute tool '{}'", other),
    };
    let risk_summary = match record.tool_name.as_str() {
        AGENT_ELEVATION_TOOL_NAME => "high: privileged agent access".to_string(),
        "run_shell" | "write_file" | "browser_act" | "computer_act" => {
            "high: side-effecting action".to_string()
        }
        "computer_capture" | "computer_screenshot" => {
            "medium: desktop observation and artifact persistence".to_string()
        }
        "browser_download" => "medium: artifact persistence and content ingestion".to_string(),
        "set_domain_access_decision" => "medium: changes future trust decisions".to_string(),
        _ => "medium: explicit human approval required".to_string(),
    };
    let arguments_preview = approval_arguments_preview(&record.tool_name, &args);
    ApprovalDisplayDescriptor {
        approval_id: record.approval_id.clone(),
        tool_name: record.tool_name.clone(),
        agent_id: record.agent_id.clone(),
        action_summary,
        target_summary,
        risk_summary,
        arguments_preview,
        options: approval_options_for_tool(&record.tool_name),
    }
}

fn approval_arguments_preview(tool_name: &str, args: &serde_json::Value) -> String {
    let mut map = serde_json::Map::new();
    let keep = match tool_name {
        "write_file" => vec!["path", "content"],
        "run_shell" => vec!["command"],
        "manage_cron" => vec!["action", "id", "prompt", "schedule", "agent_id"],
        "set_reminder" | "schedule_message" => {
            vec!["task", "schedule", "mode", "deferred_prompt", "agent_id"]
        }
        "browser_act" => vec!["action", "url", "selector", "text", "value", "millis"],
        "computer_act" => vec![
            "action",
            "profile_id",
            "target_window_id",
            "x",
            "y",
            "button",
            "text",
            "key",
        ],
        "computer_capture" | "computer_screenshot" => {
            vec!["computer_session_id", "profile_id"]
        }
        "browser_download" | "browser_open" | "browser_snapshot" | "browser_extract" | "browser_screenshot" => {
            vec!["url", "filename", "browser_session_id", "profile_id"]
        }
        _ => Vec::new(),
    };
    if let Some(obj) = args.as_object() {
        if keep.is_empty() {
            map = obj.clone();
        } else {
            for key in keep {
                if let Some(value) = obj.get(key) {
                    if key == "content" {
                        let content = value.as_str().unwrap_or_default();
                        let preview = if content.len() > 240 {
                            format!("{}... ({} chars)", &content[..240], content.len())
                        } else {
                            content.to_string()
                        };
                        map.insert("content_preview".into(), serde_json::Value::String(preview));
                    } else {
                        map.insert(key.to_string(), value.clone());
                    }
                }
            }
        }
    }
    serde_json::to_string_pretty(&serde_json::Value::Object(map))
        .unwrap_or_else(|_| "{}".to_string())
}

fn format_approval_message(record: &aria_core::ApprovalRecord) -> String {
    let descriptor = build_approval_descriptor(record);
    let target = descriptor
        .target_summary
        .as_ref()
        .map(|value| format!("\nTarget: {}", value))
        .unwrap_or_default();
    let options = descriptor
        .options
        .iter()
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Approval required\n\nAction: {}\nAgent: {}{}\nRisk: {}\n\nArguments:\n{}\n\nOptions: {}",
        descriptor.action_summary,
        descriptor.agent_id,
        target,
        descriptor.risk_summary,
        descriptor.arguments_preview,
        options
    )
}

#[derive(Debug, Clone)]
struct ApprovalRenderOutput {
    text: String,
    parse_mode: Option<&'static str>,
    reply_markup: Option<serde_json::Value>,
}

fn render_approval_prompt_for_channel(
    record: &aria_core::ApprovalRecord,
    handle_id: Option<&str>,
) -> ApprovalRenderOutput {
    let base_text = format_approval_message(record);
    match record.channel {
        aria_core::GatewayChannel::Telegram => {
            let target = handle_id.unwrap_or(record.approval_id.as_str());
            let approve_cb = format!("/approve {}", target);
            let deny_cb = format!("/deny {}", target);
            let approve_text = if record.tool_name == AGENT_ELEVATION_TOOL_NAME {
                "✅ Elevate"
            } else {
                "✅ Approve"
            };
            let keyboard = vec![vec![
                serde_json::json!({
                    "text": approve_text,
                    "callback_data": &approve_cb[..std::cmp::min(approve_cb.len(), 64)]
                }),
                serde_json::json!({
                    "text": "❌ Deny",
                    "callback_data": &deny_cb[..std::cmp::min(deny_cb.len(), 64)]
                }),
            ]];
            ApprovalRenderOutput {
                text: base_text,
                parse_mode: None,
                reply_markup: Some(serde_json::json!({ "inline_keyboard": keyboard })),
            }
        }
        _ => ApprovalRenderOutput {
            text: base_text,
            parse_mode: None,
            reply_markup: None,
        },
    }
}

fn persist_pending_approval_working_set_entry(
    sessions_dir: &Path,
    record: &aria_core::ApprovalRecord,
) {
    let locator = serde_json::from_str::<serde_json::Value>(&record.arguments_json)
        .ok()
        .and_then(|payload| extract_locator_from_tool_payload(&payload));
    let entry = aria_core::WorkingSetEntry {
        entry_id: format!("approval-{}", record.approval_id),
        kind: aria_core::WorkingSetEntryKind::PendingApproval,
        artifact_kind: execution_artifact_kind_for_tool(&record.tool_name),
        locator,
        operation: Some(record.tool_name.clone()),
        origin_tool: Some(record.tool_name.clone()),
        channel: Some(record.channel),
        session_id: Some(record.session_id),
        status: aria_core::WorkingSetStatus::Pending,
        created_at_us: record.created_at_us,
        updated_at_us: None,
        summary: build_approval_descriptor(record).action_summary,
        payload: serde_json::from_str(&record.arguments_json).ok(),
        approval_id: Some(record.approval_id.clone()),
    };
    let _ = RuntimeStore::for_sessions_dir(sessions_dir).append_working_set_entry(&entry);
}

fn persist_pending_approval_for_result(
    sessions_dir: &Path,
    req: &AgentRequest,
    result: &aria_intelligence::OrchestratorResult,
) -> Result<(aria_core::ApprovalRecord, String), String> {
    match result {
        aria_intelligence::OrchestratorResult::AgentElevationRequired { agent_id, message } => {
            let record = build_agent_elevation_approval_record(req, agent_id);
            write_approval_record(sessions_dir, &record)
                .map_err(|e| format!("write approval record failed: {}", e))?;
            persist_pending_approval_working_set_entry(sessions_dir, &record);
            let handle = ensure_approval_handle(sessions_dir, &record)
                .map_err(|e| format!("approval handle allocation failed: {}", e))?;
            let text = format!(
                "{}\n\n{}\n\nStored pending approval '{}' (handle: `{}`). Inspect with `--inspect-approvals {} {}`.",
                format_approval_message(&record),
                message,
                record.approval_id,
                handle,
                uuid::Uuid::from_bytes(req.session_id),
                req.user_id
            );
            Ok((record, text))
        }
        aria_intelligence::OrchestratorResult::ToolApprovalRequired {
            call,
            pending_prompt,
        } => {
            let record = build_tool_approval_record(req, call, pending_prompt.clone());
            write_approval_record(sessions_dir, &record)
                .map_err(|e| format!("write approval record failed: {}", e))?;
            persist_pending_approval_working_set_entry(sessions_dir, &record);
            let handle = ensure_approval_handle(sessions_dir, &record)
                .map_err(|e| format!("approval handle allocation failed: {}", e))?;
            let text = format!(
                "{}\n\nStored pending approval '{}' (handle: `{}`). Inspect with `--inspect-approvals {} {}`.",
                format_approval_message(&record),
                record.approval_id,
                handle,
                uuid::Uuid::from_bytes(req.session_id),
                req.user_id
            );
            Ok((record, text))
        }
        _ => Err("result does not require approval".into()),
    }
}

fn persist_pending_approval_for_error(
    sessions_dir: &Path,
    req: &AgentRequest,
    error_message: &str,
) -> Result<(aria_core::ApprovalRecord, String), String> {
    let Some((domain, action_family)) = extract_domain_approval_request(req, error_message) else {
        return Err("error does not map to a pending approval".into());
    };
    let record = build_domain_access_approval_record(req, &domain, action_family);
    write_approval_record(sessions_dir, &record)
        .map_err(|e| format!("write approval record failed: {}", e))?;
    persist_pending_approval_working_set_entry(sessions_dir, &record);
    let handle = ensure_approval_handle(sessions_dir, &record)
        .map_err(|e| format!("approval handle allocation failed: {}", e))?;
    let text = format!(
        "{}\n\nStored pending approval '{}' (handle: `{}`). Inspect with `--inspect-approvals {} {}`.",
        format_approval_message(&record),
        record.approval_id,
        handle,
        uuid::Uuid::from_bytes(req.session_id),
        req.user_id
    );
    Ok((record, text))
}

fn persist_pending_approval_for_tool_error(
    sessions_dir: &Path,
    req: &AgentRequest,
    call: &ToolCall,
    error_message: &str,
) -> Result<(aria_core::ApprovalRecord, String), String> {
    if error_message.starts_with("APPROVAL_REQUIRED::") {
        let result = aria_intelligence::OrchestratorResult::ToolApprovalRequired {
            call: call.clone(),
            pending_prompt: format_orchestrator_error_for_user(&format!(
                "tool error: {}",
                error_message
            )),
        };
        return persist_pending_approval_for_result(sessions_dir, req, &result);
    }
    persist_pending_approval_for_error(sessions_dir, req, error_message)
}

#[derive(Debug, Deserialize)]
struct ControlDocumentQuery {
    workspace_root: String,
}

#[derive(Debug, Deserialize)]
struct RetrievalTraceQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentQuery {
    agent_id: String,
}

#[derive(Debug, Deserialize)]
struct FingerprintQuery {
    fingerprint: String,
}

#[derive(Debug, Deserialize)]
struct ScopeDenialQuery {
    agent_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillSignatureQuery {
    skill_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ShellExecAuditQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApprovalInspectQuery {
    session_id: Option<String>,
    user_id: Option<String>,
    status: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequestPolicyAuditQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SecretUsageAuditQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RepairFallbackAuditQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingDecisionAuditQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingActivityQuery {
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StreamingMetricQuery {
    provider_id: Option<String>,
    model_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DomainDecisionQuery {
    domain: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebsiteMemoryQuery {
    domain: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserProfileBindingQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserSessionQuery {
    session_id: Option<String>,
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserArtifactQuery {
    session_id: Option<String>,
    browser_session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WatchJobQuery {
    agent_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelCapabilityQuery {
    provider_id: String,
    model_id: Option<String>,
}

fn matching_local_capability_override<'a>(
    config: &'a Config,
    provider_id: &str,
    model_id: &str,
) -> Option<&'a ModelCapabilityOverrideConfig> {
    config
        .llm
        .capability_overrides
        .iter()
        .find(|entry| entry.provider_id == provider_id && entry.model_id == model_id)
}
