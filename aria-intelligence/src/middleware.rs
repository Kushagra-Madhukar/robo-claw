use super::*;

use aria_core::{
    ExecutionArtifactKind, ExecutionContextPack, WorkingSetEntry, WorkingSetEntryKind,
    WorkingSetStatus,
};

pub struct ToolLoopMiddlewareContext<'a> {
    pub context_pack: &'a mut ExecutionContextPack,
    pub available_tools: &'a [CachedTool],
    pub tool_runtime_policy: &'a ToolRuntimePolicy,
}

pub trait ToolLoopMiddleware: Send + Sync {
    fn before_tool_call(
        &self,
        _ctx: &mut ToolLoopMiddlewareContext<'_>,
        _call: &ToolCall,
    ) -> Result<(), OrchestratorError> {
        Ok(())
    }

    fn after_tool_call(
        &self,
        _ctx: &mut ToolLoopMiddlewareContext<'_>,
        _executed: &ExecutedToolCall,
    ) -> Result<(), OrchestratorError> {
        Ok(())
    }
}

#[derive(Default)]
pub struct StateTrackingMiddleware;

impl ToolLoopMiddleware for StateTrackingMiddleware {
    fn before_tool_call(
        &self,
        ctx: &mut ToolLoopMiddlewareContext<'_>,
        call: &ToolCall,
    ) -> Result<(), OrchestratorError> {
        let Some(contract) = &ctx.context_pack.execution_contract else {
            return Ok(());
        };
        if contract.allowed_tool_classes.is_empty() {
            return Ok(());
        }
        let tool_class = tool_class_for_name(&call.name);
        if contract
            .allowed_tool_classes
            .iter()
            .any(|class| class == tool_class)
        {
            return Ok(());
        }
        Err(OrchestratorError::ToolError(format!(
            "tool '{}' is not permitted by execution contract {:?}",
            call.name, contract.kind
        )))
    }

    fn after_tool_call(
        &self,
        ctx: &mut ToolLoopMiddlewareContext<'_>,
        executed: &ExecutedToolCall,
    ) -> Result<(), OrchestratorError> {
        let working_set = ctx.context_pack.working_set.get_or_insert_with(Default::default);
        working_set.entries.push(working_set_entry_from_executed_tool(executed));
        Ok(())
    }
}

fn tool_class_for_name(name: &str) -> &'static str {
    if matches!(name, "set_reminder" | "schedule_message" | "manage_cron") {
        "schedule"
    } else if name.starts_with("browser_") || name.starts_with("crawl_") || name.starts_with("watch_")
    {
        "browser"
    } else if matches!(name, "invoke_mcp_tool" | "render_mcp_prompt" | "read_mcp_resource") {
        "mcp"
    } else if name == "spawn_agent" {
        "subagent"
    } else {
        "native"
    }
}

fn artifact_kind_for_tool(name: &str) -> Option<ExecutionArtifactKind> {
    match name {
        "set_reminder" | "schedule_message" | "manage_cron" => Some(ExecutionArtifactKind::Schedule),
        "computer_profile_list" | "computer_session_start" | "computer_session_list"
        | "computer_capture" | "computer_act" => Some(ExecutionArtifactKind::Computer),
        "invoke_mcp_tool" | "render_mcp_prompt" | "read_mcp_resource" => {
            Some(ExecutionArtifactKind::Mcp)
        }
        "spawn_agent" => Some(ExecutionArtifactKind::SubAgent),
        "search_tool_registry" => Some(ExecutionArtifactKind::ToolSearch),
        "read_file" | "write_file" | "edit_file" | "execute_file" => Some(ExecutionArtifactKind::File),
        _ if name.starts_with("browser_") || name.starts_with("crawl_") || name.starts_with("watch_") => {
            Some(ExecutionArtifactKind::Browser)
        }
        _ => None,
    }
}

pub(crate) fn working_set_entry_from_executed_tool(executed: &ExecutedToolCall) -> WorkingSetEntry {
    let payload = executed.result.as_provider_payload();
    let locator = extract_locator_from_payload(&payload)
        .or_else(|| extract_locator_from_summary(executed.result.render_for_prompt()));
    WorkingSetEntry {
        entry_id: executed
            .call
            .invocation_id
            .clone()
            .unwrap_or_else(|| {
                format!(
                    "{}-{}",
                    executed.call.name,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|duration| duration.as_micros())
                        .unwrap_or_default()
                )
            }),
        kind: if artifact_kind_for_tool(&executed.call.name).is_some() {
            WorkingSetEntryKind::Artifact
        } else {
            WorkingSetEntryKind::ToolOutput
        },
        artifact_kind: artifact_kind_for_tool(&executed.call.name),
        locator,
        operation: Some(executed.call.name.clone()),
        origin_tool: Some(executed.call.name.clone()),
        channel: None,
        session_id: None,
        status: WorkingSetStatus::Completed,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
        updated_at_us: None,
        summary: executed.result.render_for_prompt().trim().to_string(),
        payload: Some(payload),
        approval_id: None,
    }
}

fn extract_locator_from_payload(payload: &serde_json::Value) -> Option<String> {
    let object = payload.as_object()?;
    for key in [
        "path",
        "file_path",
        "locator",
        "url",
        "resource_id",
        "job_id",
        "artifact_id",
        "browser_session_id",
        "profile_id",
        "id",
    ] {
        if let Some(value) = object.get(key).and_then(|value| value.as_str()) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn extract_locator_from_summary(summary: &str) -> Option<String> {
    summary
        .split_whitespace()
        .find(|token| token.starts_with('/') || token.starts_with("./") || token.contains('.'))
        .map(|token| token.trim_matches(|ch: char| "\"'`()[]{}<>,.;".contains(ch)).to_string())
        .filter(|token| !token.is_empty())
}
