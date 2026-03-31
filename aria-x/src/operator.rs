pub(crate) fn run_admin_inspect_command(
    config: &Config,
    args: &[String],
) -> Result<Option<serde_json::Value>, String> {
    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let command = |flag: &str| args.iter().position(|a| a == flag);
    let required = |flag: &str, pos: usize| {
        args.get(pos + 1)
            .cloned()
            .ok_or_else(|| format!("Usage: {} <value>", flag))
    };

    if let Some(pos) = command("--inspect-compaction") {
        let session_id = required("--inspect-compaction", pos)?;
        return inspect_compaction_state_json(sessions_dir, &session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-control-docs") {
        let workspace_root = required("--inspect-control-docs", pos)?;
        return inspect_control_documents_json(sessions_dir, &workspace_root)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-rules") {
        let workspace_root = required("--inspect-rules", pos)?;
        let request_text = args.get(pos + 2).map(String::as_str);
        let target_path = args.get(pos + 3).map(String::as_str);
        return inspect_rule_resolution_json(&workspace_root, request_text, target_path)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-retrieval-traces") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_retrieval_traces_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-context") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_context_inspections_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-provider-payloads") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_provider_payloads_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-runs") {
        let session_id = required("--inspect-runs", pos)?;
        return inspect_agent_runs_json(sessions_dir, &session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-run-tree") {
        let session_id = required("--inspect-run-tree", pos)?;
        let root_run_id = args.get(pos + 2).map(String::as_str);
        return inspect_agent_run_tree_json(sessions_dir, &session_id, root_run_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-workspace-locks").is_some() {
        return inspect_workspace_locks_json().map(Some).map_err(|(_, e)| e);
    }
    if command("--inspect-agent-presence").is_some() {
        return inspect_agent_presence_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-durable-queue") {
        let queue_name = required("--inspect-durable-queue", pos)?;
        let tenant_id = required("--inspect-durable-queue", pos + 1)?;
        let workspace_scope = required("--inspect-durable-queue", pos + 2)?;
        return inspect_durable_queue_json(sessions_dir, &queue_name, &tenant_id, &workspace_scope)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-durable-dlq") {
        let queue_name = required("--inspect-durable-dlq", pos)?;
        let tenant_id = required("--inspect-durable-dlq", pos + 1)?;
        let workspace_scope = required("--inspect-durable-dlq", pos + 2)?;
        return inspect_durable_dlq_json(sessions_dir, &queue_name, &tenant_id, &workspace_scope)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--replay-dlq") {
        let dlq_id = required("--replay-dlq", pos)?;
        return replay_durable_dlq_json(sessions_dir, &dlq_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-session-overview") {
        let session_id = required("--inspect-session-overview", pos)?;
        return inspect_session_overview_json(sessions_dir, &session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-run-events") {
        let run_id = required("--inspect-run-events", pos)?;
        return inspect_agent_run_events_json(sessions_dir, &run_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-mailbox") {
        let run_id = required("--inspect-mailbox", pos)?;
        return inspect_agent_mailbox_json(sessions_dir, &run_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-skills").is_some() {
        return inspect_skill_packages_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-skill-bindings") {
        let agent_id = required("--inspect-skill-bindings", pos)?;
        return inspect_skill_bindings_json(sessions_dir, &agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-skill-activations") {
        let agent_id = required("--inspect-skill-activations", pos)?;
        return inspect_skill_activations_json(sessions_dir, &agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-skill-signatures") {
        let skill_id = args.get(pos + 1).map(String::as_str);
        return inspect_skill_signatures_json(sessions_dir, skill_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-mcp-servers").is_some() {
        return inspect_mcp_servers_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-mcp-imports") {
        let server_id = required("--inspect-mcp-imports", pos)?;
        return inspect_mcp_imports_json(sessions_dir, &server_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-mcp-bindings") {
        let agent_id = required("--inspect-mcp-bindings", pos)?;
        return inspect_mcp_bindings_json(sessions_dir, &agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-mcp-cache") {
        let server_id = required("--inspect-mcp-cache", pos)?;
        return inspect_mcp_cache_json(sessions_dir, &server_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-mcp-boundary").is_some() {
        return inspect_mcp_boundary_json().map(Some).map_err(|(_, e)| e);
    }
    if command("--inspect-learning-metrics").is_some() {
        return inspect_learning_metrics_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-benchmark-summary").is_some() {
        return inspect_benchmark_summary_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-runtime-profile").is_some() {
        return inspect_runtime_profile_json(config)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-learning-derivatives") {
        let fingerprint = required("--inspect-learning-derivatives", pos)?;
        return inspect_learning_derivatives_json(sessions_dir, &fingerprint)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-learning-traces") {
        let session_id = required("--inspect-learning-traces", pos)?;
        return inspect_learning_traces_json(sessions_dir, &session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-scope-denials") {
        let agent_id = args.get(pos + 1).map(String::as_str);
        let session_id = args.get(pos + 2).map(String::as_str);
        return inspect_scope_denials_json(sessions_dir, agent_id, session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-shell-exec-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_shell_exec_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-request-policy-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_request_policy_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-secret-usage-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_secret_usage_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-approvals") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let user_id = args.get(pos + 2).map(String::as_str);
        let status = args.get(pos + 3).map(String::as_str);
        return inspect_approvals_json(sessions_dir, session_id, user_id, status)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-browser-profiles").is_some() {
        return inspect_browser_profiles_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-domain-access-decisions") {
        let domain = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_domain_access_decisions_json(sessions_dir, domain, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-crawl-jobs").is_some() {
        return inspect_crawl_jobs_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-website-memory") {
        let domain = args.get(pos + 1).map(String::as_str);
        return inspect_website_memory_json(sessions_dir, domain)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-browser-profile-bindings") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_browser_profile_bindings_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-browser-sessions") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_browser_sessions_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-browser-artifacts") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let browser_session_id = args.get(pos + 2).map(String::as_str);
        return inspect_browser_artifacts_json(sessions_dir, session_id, browser_session_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-browser-action-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_browser_action_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-computer-profiles").is_some() {
        return inspect_computer_profiles_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-robot-state") {
        let robot_id = args.get(pos + 1).map(String::as_str);
        return inspect_robot_runtime_states_json(sessions_dir, robot_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-ros2-profiles") {
        let profile_id = args.get(pos + 1).map(String::as_str);
        return inspect_ros2_bridge_profiles_json(sessions_dir, profile_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-robotics-runs") {
        let robot_id = args.get(pos + 1).map(String::as_str);
        return inspect_robotics_simulations_json(sessions_dir, robot_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-computer-sessions") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_computer_sessions_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-computer-artifacts") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_computer_artifacts_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-computer-action-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_computer_action_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-execution-backends").is_some() {
        return inspect_execution_backends_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-execution-workers") {
        let backend_id = args.get(pos + 1).map(String::as_str);
        return inspect_execution_workers_json(sessions_dir, backend_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-browser-challenge-events") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_browser_challenge_events_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-watch-jobs") {
        let agent_id = args.get(pos + 1).map(String::as_str);
        return inspect_watch_jobs_json(sessions_dir, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-browser-bridge").is_some() {
        return inspect_browser_bridge_json().map(Some).map_err(|(_, e)| e);
    }
    if command("--inspect-browser-runtime-health").is_some() {
        return inspect_browser_runtime_health_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-web-storage-policy").is_some() {
        return inspect_web_storage_policy_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-repair-fallback-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_repair_fallback_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-streaming-decision-audits") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return inspect_streaming_decision_audits_json(sessions_dir, session_id, agent_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-streaming-activity") {
        let session_id = required("--inspect-streaming-activity", pos)?;
        let request_id = args.get(pos + 2).map(String::as_str);
        return inspect_streaming_activity_json(sessions_dir, &session_id, request_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-streaming-metrics") {
        let provider_id = args.get(pos + 1).map(String::as_str);
        let model_ref = args.get(pos + 2).map(String::as_str);
        return inspect_streaming_metrics_json(sessions_dir, provider_id, model_ref)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-provider-capabilities").is_some() {
        return inspect_provider_capabilities_json(sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-model-capabilities") {
        let provider_id = required("--inspect-model-capabilities", pos)?;
        let model_id = args.get(pos + 2).map(String::as_str);
        return inspect_model_capabilities_json(sessions_dir, &provider_id, model_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-model-probes") {
        let provider_id = required("--inspect-model-probes", pos)?;
        let model_id = required("--inspect-model-probes", pos + 1)?;
        return inspect_model_capability_probes_json(sessions_dir, &provider_id, &model_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if let Some(pos) = command("--inspect-model-capability-decision") {
        let provider_id = required("--inspect-model-capability-decision", pos)?;
        let model_id = required("--inspect-model-capability-decision", pos + 1)?;
        return inspect_model_capability_decision_json(config, sessions_dir, &provider_id, &model_id)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-channel-health").is_some() {
        return inspect_channel_health_json().map(Some).map_err(|(_, e)| e);
    }
    if command("--inspect-channel-transports").is_some() {
        return inspect_channel_transports_json(config)
            .map(Some)
            .map_err(|(_, e)| e);
    }
    if command("--inspect-operational-alerts").is_some() {
        return inspect_operational_alerts_json(config, sessions_dir)
            .map(Some)
            .map_err(|(_, e)| e);
    }

    Ok(None)
}

fn inspect_runtime_profile_json(
    config: &Config,
) -> Result<serde_json::Value, (u16, String)> {
    let budget = runtime_resource_budget();
    let deployment_profile = runtime_deployment_profile();
    let intended_hardware = match deployment_profile {
        DeploymentProfile::Edge => {
            "low-end CPU / memory nodes, embedded controllers, robot-side support processes"
        }
        DeploymentProfile::Node => {
            "single workstation, developer machine, or modest self-hosted node"
        }
        DeploymentProfile::Cluster => {
            "multi-node deployment with shared scheduling and runtime services"
        }
    };
    Ok(serde_json::json!({
        "deployment_profile": deployment_profile,
        "runtime_store_backend": config.cluster.runtime_store_backend,
        "resource_budget": {
            "max_parallel_requests": budget.max_parallel_requests,
            "wasm_max_memory_pages": budget.wasm_max_memory_pages,
            "max_tool_rounds": budget.max_tool_rounds,
            "retrieval_context_char_budget": budget.retrieval_context_char_budget,
            "browser_automation_enabled": budget.browser_automation_enabled,
            "learning_enabled": budget.learning_enabled,
        },
        "intended_hardware_class": intended_hardware,
        "constraints": match deployment_profile {
            DeploymentProfile::Edge => serde_json::json!([
                "browser automation disabled by default",
                "learning trace persistence disabled by default",
                "parallel request budget clamped",
                "retrieval and tool-round budgets clamped",
            ]),
            DeploymentProfile::Node => serde_json::json!([
                "balanced defaults for local-first workstation operation"
            ]),
            DeploymentProfile::Cluster => serde_json::json!([
                "shared runtime-store and scheduling concerns take precedence over single-node caps"
            ]),
        }
    }))
}

pub(crate) fn run_admin_explain_command(
    config: &Config,
    args: &[String],
) -> Result<Option<String>, String> {
    let sessions_dir = Path::new(&config.ssmu.sessions_dir);
    let command = |flag: &str| args.iter().position(|a| a == flag);

    if let Some(pos) = command("--explain-context") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return explain_context_inspections(sessions_dir, session_id, agent_id).map(Some);
    }
    if let Some(pos) = command("--explain-provider-payloads") {
        let session_id = args.get(pos + 1).map(String::as_str);
        let agent_id = args.get(pos + 2).map(String::as_str);
        return explain_provider_payloads(sessions_dir, session_id, agent_id).map(Some);
    }
    Ok(None)
}

pub(crate) fn run_operator_cli_command(
    config: &Config,
    args: &[String],
) -> Option<Result<String, String>> {
    match (args.get(1).map(String::as_str), args.get(2).map(String::as_str)) {
        (Some("inspect"), Some("context")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-context".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            if let Some(agent_id) = args.get(4) {
                forwarded.push(agent_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded)
            .and_then(|json| {
                json.ok_or_else(|| "no context inspections found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("rules")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-rules".into()];
            if let Some(workspace_root) = args.get(3) {
                forwarded.push(workspace_root.clone());
            }
            if let Some(request_text) = args.get(4) {
                forwarded.push(request_text.clone());
            }
            if let Some(target_path) = args.get(5) {
                forwarded.push(target_path.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no rules found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("provider-payloads" | "provider-payload")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-provider-payloads".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            if let Some(agent_id) = args.get(4) {
                forwarded.push(agent_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded)
            .and_then(|json| {
                json.ok_or_else(|| "no provider payload inspections found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("runs")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-runs".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no agent runs found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("run-tree")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-run-tree".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            if let Some(root_run_id) = args.get(4) {
                forwarded.push(root_run_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no agent run tree found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("run-events")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-run-events".into()];
            if let Some(run_id) = args.get(3) {
                forwarded.push(run_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no agent run events found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("mailbox")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-mailbox".into()];
            if let Some(run_id) = args.get(3) {
                forwarded.push(run_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no agent mailbox messages found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("workspace-locks")) => Some(
            run_admin_inspect_command(config, &[args[0].clone(), "--inspect-workspace-locks".into()])
                .and_then(|json| {
                    json.ok_or_else(|| "no workspace lock state found".to_string())
                        .and_then(|value| {
                            serde_json::to_string_pretty(&value)
                                .map_err(|e| format!("serialize failed: {}", e))
                        })
                }),
        ),
        (Some("inspect"), Some("benchmark-summary")) => Some(
            run_admin_inspect_command(config, &[args[0].clone(), "--inspect-benchmark-summary".into()])
                .and_then(|json| {
                    json.ok_or_else(|| "no benchmark summary found".to_string())
                        .and_then(|value| {
                            serde_json::to_string_pretty(&value)
                                .map_err(|e| format!("serialize failed: {}", e))
                        })
                }),
        ),
        (Some("inspect"), Some("runtime-profile")) => Some(
            run_admin_inspect_command(config, &[args[0].clone(), "--inspect-runtime-profile".into()])
                .and_then(|json| {
                    json.ok_or_else(|| "no runtime profile found".to_string())
                        .and_then(|value| {
                            serde_json::to_string_pretty(&value)
                                .map_err(|e| format!("serialize failed: {}", e))
                        })
                }),
        ),
        (Some("inspect"), Some("robot-state")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-robot-state".into()];
            if let Some(robot_id) = args.get(3) {
                forwarded.push(robot_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no robot runtime state found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("ros2-profiles")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-ros2-profiles".into()];
            if let Some(profile_id) = args.get(3) {
                forwarded.push(profile_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no ros2 bridge profiles found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("robotics-runs")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-robotics-runs".into()];
            if let Some(robot_id) = args.get(3) {
                forwarded.push(robot_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no robotics simulations found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("mcp-servers")) => Some(
            run_admin_inspect_command(config, &[args[0].clone(), "--inspect-mcp-servers".into()])
                .and_then(|json| {
                    json.ok_or_else(|| "no mcp server state found".to_string())
                        .and_then(|value| {
                            serde_json::to_string_pretty(&value)
                                .map_err(|e| format!("serialize failed: {}", e))
                        })
                }),
        ),
        (Some("inspect"), Some("mcp-imports")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-mcp-imports".into()];
            if let Some(server_id) = args.get(3) {
                forwarded.push(server_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no mcp imports found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("inspect"), Some("mcp-bindings")) => {
            let mut forwarded = vec![args[0].clone(), "--inspect-mcp-bindings".into()];
            if let Some(agent_id) = args.get(3) {
                forwarded.push(agent_id.clone());
            }
            Some(run_admin_inspect_command(config, &forwarded).and_then(|json| {
                json.ok_or_else(|| "no mcp bindings found".to_string())
                    .and_then(|value| {
                        serde_json::to_string_pretty(&value)
                            .map_err(|e| format!("serialize failed: {}", e))
                    })
            }))
        }
        (Some("explain"), Some("context")) => {
            let mut forwarded = vec![args[0].clone(), "--explain-context".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            if let Some(agent_id) = args.get(4) {
                forwarded.push(agent_id.clone());
            }
            Some(run_admin_explain_command(config, &forwarded)
            .and_then(|text| text.ok_or_else(|| "no context inspections found".to_string())))
        }
        (Some("explain"), Some("provider-payloads" | "provider-payload")) => {
            let mut forwarded = vec![args[0].clone(), "--explain-provider-payloads".into()];
            if let Some(session_id) = args.get(3) {
                forwarded.push(session_id.clone());
            }
            if let Some(agent_id) = args.get(4) {
                forwarded.push(agent_id.clone());
            }
            Some(run_admin_explain_command(config, &forwarded)
            .and_then(|text| {
                text.ok_or_else(|| "no provider payload inspections found".to_string())
            }))
        }
        _ => None,
    }
}

async fn run_live_admin_inspect_command(
    _config: &Config,
    args: &[String],
    provider_registry: &Arc<tokio::sync::Mutex<ProviderRegistry>>,
) -> Result<Option<serde_json::Value>, String> {
    let command = |flag: &str| args.iter().position(|a| a == flag);
    if command("--inspect-registered-providers").is_some() {
        return inspect_registered_providers_json(provider_registry)
            .await
            .map(Some)
            .map_err(|(_, e)| e);
    }
    Ok(None)
}

fn inspect_compaction_state_json(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed = uuid::Uuid::parse_str(session_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid session_id: {}", e),
        )
    })?;
    let state = RuntimeStore::for_sessions_dir(sessions_dir)
        .read_compaction_state(parsed)
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    serde_json::to_value(state).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_control_documents_json(
    sessions_dir: &Path,
    workspace_root: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let docs = store
        .list_control_documents(workspace_root)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let conflicts = detect_control_document_conflicts(&store, &[workspace_root.to_string()])
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(serde_json::json!({
        "workspace_root": workspace_root,
        "documents": docs,
        "conflicts": conflicts,
    }))
}

fn inspect_provider_health_json(
    llm_pool: &Arc<LlmBackendPool>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    serde_json::to_value(llm_pool.provider_circuit_state()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_workspace_locks_json() -> Result<serde_json::Value, (StatusCode, String)> {
    serde_json::to_value(workspace_lock_manager().snapshot()).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_retrieval_traces_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let traces = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_retrieval_traces(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(traces).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_context_inspections_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_context_inspections(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let redaction = TelemetryRedactionConfig::default();
    let values = records
        .into_iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(|value| {
                    redact_json_value(
                        &value,
                        TelemetryRedactionProfile::TrustedLocalInspect,
                        &redaction,
                    )
                })
                .map_err(|e| format!("serialize failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(serde_json::Value::Array(values))
}

fn inspect_provider_payloads_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_context_inspections(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let payloads = records
        .into_iter()
        .map(|record| {
            let value = serde_json::json!({
                "context_id": record.context_id,
                "request_id": record.request_id,
                "session_id": record.session_id,
                "agent_id": record.agent_id,
                "provider_model": record.provider_model,
                "created_at_us": record.created_at_us,
                "tool_selection": record.tool_selection,
                "tool_runtime_policy": record.tool_runtime_policy,
                "selected_tool_catalog": record.selected_tool_catalog,
                "hidden_tool_messages": record.hidden_tool_messages,
                "emitted_artifacts": record.emitted_artifacts,
                "tool_provider_readiness": record.tool_provider_readiness,
                "provider_request_payload": record.provider_request_payload,
            });
            redact_json_value(
                &value,
                TelemetryRedactionProfile::TrustedLocalInspect,
                &TelemetryRedactionConfig::default(),
            )
        })
        .collect::<Vec<_>>();
    serde_json::to_value(payloads).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn explain_context_inspections(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<String, String> {
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_context_inspections(session_id, agent_id)?;
    if records.is_empty() {
        return Ok("No context inspections found.".into());
    }
    let mut lines = Vec::new();
    for record in records {
        lines.push(format!(
            "Context {} | agent={} | model={} | created_at_us={}",
            record.context_id,
            record.agent_id,
            record.provider_model.unwrap_or_else(|| "<unknown>".into()),
            record.created_at_us
        ));
        lines.push(format!(
            "  tokens: system={} history={} context={} user={} tools={}",
            record.system_tokens,
            record.history_tokens,
            record.context_tokens,
            record.user_tokens,
            record.tool_count
        ));
        if let Some(selection) = record.tool_selection {
            lines.push(format!(
                "  tool mode: {:?} | choice={:?} | text_fallback={}",
                selection.tool_calling_mode,
                selection.tool_choice,
                selection.text_fallback_mode
            ));
            lines.push(format!(
                "  selected tools: {}",
                if selection.selected_tool_names.is_empty() {
                    "<none>".into()
                } else {
                    selection.selected_tool_names.join(", ")
                }
            ));
            let mut candidate_scores = selection.candidate_scores;
            candidate_scores.sort_by(|left, right| right.score.cmp(&left.score));
            for score in candidate_scores.into_iter().take(5) {
                lines.push(format!(
                    "    candidate {} score={} source={}",
                    score.tool_name, score.score, score.source
                ));
            }
        }
        if !record.selected_tool_catalog.is_empty() {
            lines.push("  selected provider/runner bindings:".into());
            for entry in &record.selected_tool_catalog {
                lines.push(format!(
                    "    {} => {:?} / {:?}",
                    entry.public_name, entry.provider_kind, entry.runner_class
                ));
            }
        }
        if !record.hidden_tool_messages.is_empty() {
            lines.push(format!(
                "  hidden tools: {}",
                record.hidden_tool_messages.join("; ")
            ));
        }
        if !record.emitted_artifacts.is_empty() {
            lines.push(format!(
                "  emitted artifacts: {}",
                record
                    .emitted_artifacts
                    .iter()
                    .map(|artifact| format!("{:?}:{}", artifact.kind, artifact.label))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if !record.tool_provider_readiness.is_empty() {
            lines.push("  tool provider readiness:".into());
            for readiness in &record.tool_provider_readiness {
                lines.push(format!(
                    "    {:?}/{} => {:?} installed={} bound={} auth_ready={}",
                    readiness.provider_kind,
                    readiness.provider_id,
                    readiness.status,
                    readiness.installed,
                    readiness.bound,
                    readiness.auth_ready
                ));
            }
        }
        lines.push(String::new());
    }
    Ok(lines.join("\n"))
}

fn explain_provider_payloads(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<String, String> {
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_context_inspections(session_id, agent_id)?;
    if records.is_empty() {
        return Ok("No provider payload inspections found.".into());
    }
    let mut sections = Vec::new();
    for record in records {
        let payload = record
            .provider_request_payload
            .map(|value| {
                let redacted = redact_json_value(
                    &value,
                    TelemetryRedactionProfile::TrustedLocalInspect,
                    &TelemetryRedactionConfig::default(),
                );
                serde_json::to_string_pretty(&redacted)
                    .unwrap_or_else(|_| "<serialize failed>".into())
            })
            .unwrap_or_else(|| "<none>".into());
        sections.push(format!(
            "Context {} | agent={} | model={}\nSelected tools: {}\nProvider bindings: {}\nEmitted artifacts: {}\nPayload:\n{}",
            record.context_id,
            record.agent_id,
            record.provider_model.unwrap_or_else(|| "<unknown>".into()),
            record
                .tool_selection
                .as_ref()
                .map(|selection| {
                    if selection.selected_tool_names.is_empty() {
                        "<none>".into()
                    } else {
                        selection.selected_tool_names.join(", ")
                    }
                })
                .unwrap_or_else(|| "<unknown>".into()),
            if record.selected_tool_catalog.is_empty() {
                "<none>".into()
            } else {
                record
                    .selected_tool_catalog
                    .iter()
                    .map(|entry| format!("{}({:?}/{:?})", entry.public_name, entry.provider_kind, entry.runner_class))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            if record.emitted_artifacts.is_empty() {
                "<none>".into()
            } else {
                record
                    .emitted_artifacts
                    .iter()
                    .map(|artifact| format!("{:?}:{}", artifact.kind, artifact.label))
                    .collect::<Vec<_>>()
                    .join(", ")
            },
            payload
        ));
    }
    Ok(sections.join("\n\n---\n\n"))
}

fn inspect_agent_runs_json(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed = uuid::Uuid::parse_str(session_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid session_id: {}", e),
        )
    })?;
    let runs = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_agent_runs_for_session(parsed)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(runs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_agent_run_tree_json(
    sessions_dir: &Path,
    session_id: &str,
    root_run_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed = uuid::Uuid::parse_str(session_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid session_id: {}", e),
        )
    })?;
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let runs = store
        .list_agent_runs_for_session(parsed)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let snapshot = store
        .build_agent_run_tree_snapshot(parsed)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let run_index: std::collections::BTreeMap<String, AgentRunRecord> = runs
        .iter()
        .cloned()
        .map(|run| (run.run_id.clone(), run))
        .collect();
    let mut children_by_parent: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    let mut continuations_by_source: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();
    for run in &runs {
        if let Some(parent_run_id) = run.parent_run_id.clone() {
            children_by_parent
                .entry(parent_run_id)
                .or_default()
                .push(run.run_id.clone());
        }
        if let Some(lineage_run_id) = run.lineage_run_id.clone() {
            continuations_by_source
                .entry(lineage_run_id)
                .or_default()
                .push(run.run_id.clone());
        }
    }

    fn build_run_tree_node(
        store: &RuntimeStore,
        run_id: &str,
        run_index: &std::collections::BTreeMap<String, AgentRunRecord>,
        children_by_parent: &std::collections::BTreeMap<String, Vec<String>>,
        continuations_by_source: &std::collections::BTreeMap<String, Vec<String>>,
        visited: &mut std::collections::BTreeSet<String>,
    ) -> Result<serde_json::Value, (StatusCode, String)> {
        if !visited.insert(run_id.to_string()) {
            return Ok(serde_json::json!({
                "run_id": run_id,
                "cycle_detected": true,
            }));
        }
        let run = run_index.get(run_id).cloned().ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                format!("run '{}' not found in session tree", run_id),
            )
        })?;
        let events = store
            .list_agent_run_events(run_id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        let mailbox = store
            .list_agent_mailbox_messages(run_id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;

        let mut child_nodes = Vec::new();
        if let Some(child_ids) = children_by_parent.get(run_id) {
            for child_id in child_ids {
                child_nodes.push(build_run_tree_node(
                    store,
                    child_id,
                    run_index,
                    children_by_parent,
                    continuations_by_source,
                    visited,
                )?);
            }
        }
        let mut continuation_nodes = Vec::new();
        if let Some(continuation_ids) = continuations_by_source.get(run_id) {
            for continuation_id in continuation_ids {
                continuation_nodes.push(build_run_tree_node(
                    store,
                    continuation_id,
                    run_index,
                    children_by_parent,
                    continuations_by_source,
                    visited,
                )?);
            }
        }

        Ok(serde_json::json!({
            "run": run,
            "event_count": events.len(),
            "events": events,
            "mailbox_count": mailbox.len(),
            "mailbox": mailbox,
            "children": child_nodes,
            "continuations": continuation_nodes,
        }))
    }

    let mut visited = std::collections::BTreeSet::new();
    let roots = if let Some(root_run_id) = root_run_id {
        vec![build_run_tree_node(
            &store,
            root_run_id,
            &run_index,
            &children_by_parent,
            &continuations_by_source,
            &mut visited,
        )?]
    } else {
        let mut root_ids = Vec::new();
        for run in &runs {
            let parent_missing = run
                .parent_run_id
                .as_ref()
                .map(|id| !run_index.contains_key(id))
                .unwrap_or(true);
            if parent_missing && run.lineage_run_id.is_none() {
                root_ids.push(run.run_id.clone());
            }
        }
        let mut nodes = Vec::new();
        for root_id in root_ids {
            nodes.push(build_run_tree_node(
                &store,
                &root_id,
                &run_index,
                &children_by_parent,
                &continuations_by_source,
                &mut visited,
            )?);
        }
        nodes
    };

    Ok(serde_json::json!({
        "session_id": session_id,
        "root_run_id": root_run_id,
        "snapshot": snapshot,
        "roots": roots,
    }))
}

fn inspect_agent_presence_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let presence = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_agent_presence()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(presence).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn parse_durable_queue_kind(value: &str) -> Result<crate::runtime_store::DurableQueueKind, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "ingress" => Ok(crate::runtime_store::DurableQueueKind::Ingress),
        "run" => Ok(crate::runtime_store::DurableQueueKind::Run),
        "outbox" => Ok(crate::runtime_store::DurableQueueKind::Outbox),
        other => Err(format!("unknown durable queue '{}'", other)),
    }
}

fn inspect_durable_queue_json(
    sessions_dir: &Path,
    queue_name: &str,
    tenant_id: &str,
    workspace_scope: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let queue =
        parse_durable_queue_kind(queue_name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_durable_messages(queue, tenant_id, workspace_scope)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(records).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_durable_dlq_json(
    sessions_dir: &Path,
    queue_name: &str,
    tenant_id: &str,
    workspace_scope: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let queue =
        parse_durable_queue_kind(queue_name).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_durable_dlq(queue, tenant_id, workspace_scope)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(records).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn replay_durable_dlq_json(
    sessions_dir: &Path,
    dlq_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let replayed = RuntimeStore::for_sessions_dir(sessions_dir)
        .replay_durable_dlq(dlq_id, chrono::Utc::now().timestamp_micros() as u64)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(replayed).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_session_overview_json(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed = uuid::Uuid::parse_str(session_id).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            format!("invalid session_id: {}", e),
        )
    })?;
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let runs = store
        .list_agent_runs_for_session(parsed)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let learning_traces = store
        .list_execution_traces_by_session(session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let request_policy_audits = store
        .list_request_policy_audits(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let pending_approvals = store
        .list_approvals(Some(parsed.into_bytes()), None, Some(aria_core::ApprovalStatus::Pending))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let repair_fallback_audits = store
        .list_repair_fallback_audits(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let streaming_decision_audits = store
        .list_streaming_decision_audits(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let streaming_activity = inspect_streaming_activity_json(sessions_dir, session_id, None)?;
    let scope_denials = store
        .list_scope_denials(None, Some(session_id))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let shell_exec_audits = store
        .list_shell_exec_audits(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let compaction_state = store.read_compaction_state(parsed).ok();
    let retrieval_traces = store
        .list_retrieval_traces(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let web_storage = inspect_web_storage_policy_json(sessions_dir)?;
    let browser_runtime_health = inspect_browser_runtime_health_json(sessions_dir)?;
    let value = serde_json::json!({
        "session_id": session_id,
        "runs": runs,
        "learning_traces": learning_traces,
        "request_policy_audits": request_policy_audits,
        "pending_approvals": pending_approvals,
        "repair_fallback_audits": repair_fallback_audits,
        "streaming_decision_audits": streaming_decision_audits,
        "streaming_activity": streaming_activity,
        "scope_denials": scope_denials,
        "shell_exec_audits": shell_exec_audits,
        "compaction_state": compaction_state,
        "retrieval_traces": retrieval_traces,
        "web_storage": web_storage,
        "browser_runtime_health": browser_runtime_health,
    });
    Ok(value)
}

fn inspect_agent_run_events_json(
    sessions_dir: &Path,
    run_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let events = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_agent_run_events(run_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(events).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_agent_mailbox_json(
    sessions_dir: &Path,
    run_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let messages = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_agent_mailbox_messages(run_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(messages).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_skill_packages_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let packages = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_skill_packages()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(packages).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_skill_bindings_json(
    sessions_dir: &Path,
    agent_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let bindings = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_skill_bindings_for_agent(agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(bindings).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_skill_activations_json(
    sessions_dir: &Path,
    agent_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let activations = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_skill_activations_for_agent(agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(activations).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_skill_signatures_json(
    sessions_dir: &Path,
    skill_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let signatures = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_skill_signatures(skill_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(signatures).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_mcp_servers_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    #[cfg(not(feature = "mcp-runtime"))]
    {
        let _ = sessions_dir;
        Ok(serde_json::json!({
            "enabled": false,
            "reason": "mcp-runtime feature is disabled in this build"
        }))
    }

    #[cfg(feature = "mcp-runtime")]
    {
        let servers = RuntimeStore::for_sessions_dir(sessions_dir)
            .list_mcp_servers()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        serde_json::to_value(servers).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize failed: {}", e),
            )
        })
    }
}

fn inspect_mcp_imports_json(
    sessions_dir: &Path,
    server_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    #[cfg(not(feature = "mcp-runtime"))]
    {
        let _ = sessions_dir;
        Ok(serde_json::json!({
            "enabled": false,
            "server_id": server_id,
            "reason": "mcp-runtime feature is disabled in this build"
        }))
    }

    #[cfg(feature = "mcp-runtime")]
    {
        let store = RuntimeStore::for_sessions_dir(sessions_dir);
        let value = serde_json::json!({
            "server_id": server_id,
            "tools": store.list_mcp_imported_tools(server_id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.clone()))?,
            "prompts": store.list_mcp_imported_prompts(server_id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.clone()))?,
            "resources": store.list_mcp_imported_resources(server_id).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.clone()))?,
        });
        Ok(value)
    }
}

fn inspect_mcp_bindings_json(
    sessions_dir: &Path,
    agent_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    #[cfg(not(feature = "mcp-runtime"))]
    {
        let _ = sessions_dir;
        Ok(serde_json::json!({
            "enabled": false,
            "agent_id": agent_id,
            "reason": "mcp-runtime feature is disabled in this build"
        }))
    }

    #[cfg(feature = "mcp-runtime")]
    {
        let bindings = RuntimeStore::for_sessions_dir(sessions_dir)
            .list_mcp_bindings_for_agent(agent_id)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
        serde_json::to_value(bindings).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize failed: {}", e),
            )
        })
    }
}

fn inspect_mcp_cache_json(
    sessions_dir: &Path,
    server_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    #[cfg(not(feature = "mcp-runtime"))]
    {
        let _ = sessions_dir;
        Ok(serde_json::json!({
            "enabled": false,
            "server_id": server_id,
            "reason": "mcp-runtime feature is disabled in this build"
        }))
    }

    #[cfg(feature = "mcp-runtime")]
    {
        let cache = RuntimeStore::for_sessions_dir(sessions_dir)
            .read_mcp_import_cache_record(server_id)
            .map_err(|e| (StatusCode::NOT_FOUND, e))?;
        serde_json::to_value(cache).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize failed: {}", e),
            )
        })
    }
}

fn inspect_mcp_boundary_json() -> Result<serde_json::Value, (StatusCode, String)> {
    #[cfg(feature = "mcp-runtime")]
    {
        return serde_json::to_value(aria_mcp::mcp_boundary_policy_snapshot()).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("serialize failed: {}", e),
            )
        });
    }

    #[cfg(not(feature = "mcp-runtime"))]
    {
        Ok(serde_json::json!({
            "enabled": false,
            "rule": "MCP runtime feature is disabled in this build. Keep trust-boundary subsystems native/internal and prefer MCP only for leaf external integrations when the feature is enabled.",
            "native_internal": [
                "browser_runtime",
                "crawl_runtime",
                "approval_engine",
                "vault",
                "policy_engine",
                "scheduler_core",
                "runtime_store"
            ],
            "leaf_external_examples": [
                "github",
                "slack",
                "jira",
                "linear",
                "notion",
                "google_drive",
                "s3"
            ]
        }))
    }
}

fn inspect_learning_metrics_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let metrics = RuntimeStore::for_sessions_dir(sessions_dir)
        .learning_metrics_snapshot()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(metrics).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_benchmark_summary_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let traces = store
        .list_all_execution_traces()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut success_count = 0u64;
    let mut failure_count = 0u64;
    let mut approval_required_count = 0u64;
    let mut clarification_required_count = 0u64;
    let mut total_latency_ms = 0u64;
    let mut tool_usage: BTreeMap<String, u64> = BTreeMap::new();
    for trace in &traces {
        total_latency_ms += trace.latency_ms as u64;
        match trace.outcome {
            aria_learning::TraceOutcome::Succeeded => success_count += 1,
            aria_learning::TraceOutcome::Failed => failure_count += 1,
            aria_learning::TraceOutcome::ApprovalRequired => approval_required_count += 1,
            aria_learning::TraceOutcome::ClarificationRequired => clarification_required_count += 1,
        }
        for tool in &trace.tool_names {
            *tool_usage.entry(tool.clone()).or_default() += 1;
        }
    }

    let context_inspections = store
        .list_context_inspections(None, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut provider_usage: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_prompt_tokens = 0u64;
    for record in &context_inspections {
        let provider = record
            .provider_model
            .as_deref()
            .and_then(|value| value.split_once('/').map(|(provider, _)| provider))
            .unwrap_or("unknown");
        *provider_usage.entry(provider.to_string()).or_default() += 1;
        total_prompt_tokens += record.system_tokens as u64
            + record.history_tokens as u64
            + record.context_tokens as u64
            + record.user_tokens as u64;
    }

    let streaming_metrics = inspect_streaming_metrics_json(sessions_dir, None, None)?;
    let learning_metrics = store
        .learning_metrics_snapshot()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let repair_fallback_audit_count = store
        .list_repair_fallback_audits(None, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        .len();

    Ok(serde_json::json!({
        "learning_metrics": learning_metrics,
        "trace_summary": {
            "trace_count": traces.len(),
            "success_count": success_count,
            "failure_count": failure_count,
            "approval_required_count": approval_required_count,
            "clarification_required_count": clarification_required_count,
            "average_latency_ms": if traces.is_empty() {
                0.0
            } else {
                total_latency_ms as f64 / traces.len() as f64
            },
            "average_prompt_tokens": if context_inspections.is_empty() {
                0.0
            } else {
                total_prompt_tokens as f64 / context_inspections.len() as f64
            },
        },
        "streaming_metrics": streaming_metrics,
        "repair_fallback_audit_count": repair_fallback_audit_count,
        "provider_usage": provider_usage,
        "tool_usage": tool_usage,
    }))
}

fn inspect_learning_derivatives_json(
    sessions_dir: &Path,
    fingerprint: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let events = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_learning_derivative_events(fingerprint)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(events).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_learning_traces_json(
    sessions_dir: &Path,
    session_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let traces = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_execution_traces_by_session(session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let redaction = TelemetryRedactionConfig::default();
    let values = traces
        .into_iter()
        .map(|trace| {
            serde_json::to_value(trace)
                .map(|value| {
                    redact_json_value(
                        &value,
                        TelemetryRedactionProfile::TrustedLocalInspect,
                        &redaction,
                    )
                })
                .map_err(|e| format!("serialize failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(serde_json::Value::Array(values))
}

fn inspect_scope_denials_json(
    sessions_dir: &Path,
    agent_id: Option<&str>,
    session_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let denials = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_scope_denials(agent_id, session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let enriched = denials
        .into_iter()
        .map(|record| {
            let mut value = serde_json::to_value(&record).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "denial_code".into(),
                    serde_json::Value::String(record.kind.code().to_string()),
                );
                obj.insert(
                    "diagnostic".into(),
                    serde_json::Value::String(record.reason.clone()),
                );
            }
            value
        })
        .collect::<Vec<_>>();
    Ok(serde_json::Value::Array(enriched))
}

fn inspect_shell_exec_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_shell_exec_audits(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_request_policy_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_request_policy_audits(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_secret_usage_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_secret_usage_audits(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_approvals_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    user_id: Option<&str>,
    status: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let parsed_status = status
        .map(|value| match value.trim().to_ascii_lowercase().as_str() {
            "pending" => Ok(aria_core::ApprovalStatus::Pending),
            "approved" => Ok(aria_core::ApprovalStatus::Approved),
            "denied" => Ok(aria_core::ApprovalStatus::Denied),
            other => Err((StatusCode::BAD_REQUEST, format!("invalid approval status: {}", other))),
        })
        .transpose()?;
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_approvals(parsed_session_id, user_id, parsed_status)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let descriptors = records
        .iter()
        .map(|record| {
            serde_json::json!({
                "record": record,
                "display": build_approval_descriptor(record),
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!(descriptors))
}

fn inspect_browser_profiles_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let profiles = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profiles()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(profiles).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_domain_access_decisions_json(
    sessions_dir: &Path,
    domain: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let decisions = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_domain_access_decisions(domain, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(decisions).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_crawl_jobs_json(sessions_dir: &Path) -> Result<serde_json::Value, (StatusCode, String)> {
    let jobs = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_crawl_jobs()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(jobs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_website_memory_json(
    sessions_dir: &Path,
    domain: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let records = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_website_memory(domain)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(records).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_browser_profile_bindings_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let bindings = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_profile_bindings(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(bindings).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_browser_sessions_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let sessions = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_sessions(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(sessions).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_browser_artifacts_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    browser_session_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let artifacts = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_artifacts(parsed_session_id, browser_session_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(artifacts).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_browser_action_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_action_audits(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_computer_profiles_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let profiles = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_computer_profiles()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(profiles).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_robot_runtime_states_json(
    sessions_dir: &Path,
    robot_id: Option<&str>,
) -> Result<serde_json::Value, (u16, String)> {
    let states = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_robot_runtime_states(robot_id)
        .map_err(|e| (500u16, e))?;
    let rows = states
        .into_iter()
        .map(|record| {
            serde_json::json!({
                "robot_id": record.robot_id,
                "execution_mode": record.execution_mode,
                "connection_kind": record.connection_kind,
                "bridge_profile_id": record.bridge_profile_id,
                "battery_percent": record.state.battery_percent,
                "active_faults": record.state.active_faults,
                "degraded_local_mode": record.state.degraded_local_mode,
                "last_heartbeat_us": record.state.last_heartbeat_us,
                "safety_envelope": record.safety_envelope,
                "updated_at_us": record.updated_at_us,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "count": rows.len(),
        "rows": rows,
    }))
}

fn inspect_ros2_bridge_profiles_json(
    sessions_dir: &Path,
    profile_id: Option<&str>,
) -> Result<serde_json::Value, (u16, String)> {
    let rows = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_ros2_bridge_profiles(profile_id)
        .map_err(|e| (500u16, e))?;
    Ok(serde_json::json!({
        "count": rows.len(),
        "rows": rows,
    }))
}

fn inspect_robotics_simulations_json(
    sessions_dir: &Path,
    robot_id: Option<&str>,
) -> Result<serde_json::Value, (u16, String)> {
    let rows = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_robotics_simulations(robot_id)
        .map_err(|e| (500u16, e))?
        .into_iter()
        .map(|record| {
            serde_json::json!({
                "simulation_id": record.simulation_id,
                "session_id": record.session_id.map(|id| uuid::Uuid::from_bytes(id).to_string()),
                "agent_id": record.agent_id,
                "robot_id": record.robot_id,
                "outcome": record.outcome,
                "ros2_profile_id": record.ros2_profile_id,
                "safety_events": record.safety_events,
                "directive": record.directive_json,
                "rejection_reason": record.rejection_reason,
                "contract": record.contract,
                "state": record.state,
                "safety_envelope": record.safety_envelope,
                "created_at_us": record.created_at_us,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "count": rows.len(),
        "rows": rows,
    }))
}

fn inspect_execution_backends_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let profiles = ensure_default_execution_backend_profiles(sessions_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(profiles).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_computer_sessions_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let sessions = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_computer_sessions(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(sessions).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_computer_artifacts_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let artifacts = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_computer_artifacts(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(artifacts).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_computer_action_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_computer_action_audits(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_execution_workers_json(
    sessions_dir: &Path,
    backend_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let workers = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_execution_workers(backend_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(workers).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_browser_challenge_events_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let parsed_session_id = session_id
        .map(|value| uuid::Uuid::parse_str(value).map(|id| *id.as_bytes()))
        .transpose()
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("invalid session_id: {}", e)))?;
    let events = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_challenge_events(parsed_session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(events).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_watch_jobs_json(
    sessions_dir: &Path,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let mut jobs = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_watch_jobs()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    if let Some(agent_id) = agent_id {
        jobs.retain(|job| job.agent_id == agent_id);
    }
    serde_json::to_value(jobs).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_web_storage_policy_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let policy = web_storage_policy();
    let usage = compute_web_storage_usage(sessions_dir)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(serde_json::json!({
        "policy": policy,
        "usage": usage,
    }))
}

fn inspect_browser_bridge_json() -> Result<serde_json::Value, (StatusCode, String)> {
    let bridge = resolve_trusted_browser_automation_bridge_for_mode("stdin_json")
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(serde_json::json!({
        "binary": bridge.binary,
        "sha256_hex": bridge.sha256_hex,
        "manifest": bridge.manifest,
        "required_protocol_version": REQUIRED_BROWSER_AUTOMATION_PROTOCOL_VERSION,
        "os_containment_requested": bridge.os_containment_requested,
        "os_containment_available": bridge.os_containment_available,
        "containment_backend": bridge.containment_backend,
        "os_containment_effective": bridge.os_containment_requested && bridge.os_containment_available,
    }))
}

fn browser_process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        std::process::Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

fn cleanup_stale_browser_sessions(
    sessions_dir: &Path,
    session_filter: Option<aria_core::Uuid>,
    agent_filter: Option<&str>,
) -> Result<Vec<aria_core::BrowserSessionRecord>, OrchestratorError> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let sessions = store
        .list_browser_sessions(session_filter, agent_filter)
        .map_err(OrchestratorError::ToolError)?;
    let mut cleaned = Vec::new();
    for mut session in sessions {
        let is_stale = session.status == aria_core::BrowserSessionStatus::Launched
            && session.pid.map(browser_process_is_alive) == Some(false);
        if !is_stale {
            continue;
        }
        let now_us = chrono::Utc::now().timestamp_micros() as u64;
        session.status = aria_core::BrowserSessionStatus::Exited;
        session.error = Some("browser process no longer alive; cleaned up stale launched session".into());
        session.updated_at_us = now_us;
        store
            .upsert_browser_session(&session, now_us)
            .map_err(OrchestratorError::ToolError)?;
        store
            .append_browser_action_audit(&aria_core::BrowserActionAuditRecord {
                audit_id: format!("browser-audit-{}", uuid::Uuid::new_v4()),
                browser_session_id: Some(session.browser_session_id.clone()),
                session_id: session.session_id,
                agent_id: session.agent_id.clone(),
                profile_id: Some(session.profile_id.clone()),
                action: aria_core::BrowserActionKind::SessionCleanup,
                target: session.start_url.clone(),
                metadata: serde_json::json!({
                    "reason": "stale_process",
                    "pid": session.pid,
                }),
                created_at_us: now_us,
            })
            .map_err(OrchestratorError::ToolError)?;
        cleaned.push(session);
    }
    Ok(cleaned)
}

fn inspect_browser_runtime_health_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let bridge = inspect_browser_bridge_json();
    let cleaned = cleanup_stale_browser_sessions(sessions_dir, None, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let engines = [
        aria_core::BrowserEngine::Chromium,
        aria_core::BrowserEngine::Chrome,
        aria_core::BrowserEngine::Edge,
        aria_core::BrowserEngine::SafariBridge,
    ];
    let binaries = engines
        .into_iter()
        .map(|engine| {
            let resolved = resolve_browser_binary(engine);
            serde_json::json!({
                "engine": engine,
                "resolved": resolved.as_ref().ok(),
                "error": resolved.err().map(|e| e.to_string()),
            })
        })
        .collect::<Vec<_>>();
    let sessions = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_browser_sessions(None, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let launched = sessions
        .iter()
        .filter(|session| session.status == aria_core::BrowserSessionStatus::Launched)
        .count();
    let paused = sessions
        .iter()
        .filter(|session| session.status == aria_core::BrowserSessionStatus::Paused)
        .count();
    let exited = sessions
        .iter()
        .filter(|session| {
            session.status == aria_core::BrowserSessionStatus::Exited
        })
        .count();
    let failed = sessions
        .iter()
        .filter(|session| session.status == aria_core::BrowserSessionStatus::Failed)
        .count();
    Ok(serde_json::json!({
        "bridge": bridge.unwrap_or_else(|(status, message)| serde_json::json!({
            "error": message,
            "status": status.as_u16(),
        })),
        "binaries": binaries,
        "sessions": {
            "total": sessions.len(),
            "launched": launched,
            "paused": paused,
            "exited": exited,
            "failed": failed,
            "cleaned_stale": cleaned.len(),
        },
        "cleaned_sessions": cleaned,
    }))
}

fn inspect_repair_fallback_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_repair_fallback_audits(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_streaming_decision_audits_json(
    sessions_dir: &Path,
    session_id: Option<&str>,
    agent_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_streaming_decision_audits(session_id, agent_id)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(audits).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_streaming_activity_json(
    sessions_dir: &Path,
    session_id: &str,
    request_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_streaming_decision_audits(Some(session_id), None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let mut grouped: BTreeMap<String, Vec<StreamingDecisionAuditRecord>> = BTreeMap::new();
    for audit in audits {
        if request_id.is_some() && Some(audit.request_id.as_str()) != request_id {
            continue;
        }
        grouped.entry(audit.request_id.clone()).or_default().push(audit);
    }

    let requests = grouped
        .into_iter()
        .map(|(request_id, mut events)| {
            events.sort_by_key(|event| event.created_at_us);
            let mut latest_modes: BTreeMap<String, String> = BTreeMap::new();
            let mut used_streaming: BTreeMap<String, bool> = BTreeMap::new();
            let mut used_fallback: BTreeMap<String, bool> = BTreeMap::new();
            let mut latest_model_ref = None;
            let mut agent_id = None;
            let mut user_id = None;
            for event in &events {
                latest_modes.insert(event.phase.clone(), event.mode.clone());
                if event.mode == "stream_used" {
                    used_streaming.insert(event.phase.clone(), true);
                }
                if event.mode == "fallback_used" {
                    used_fallback.insert(event.phase.clone(), true);
                }
                if event.model_ref.is_some() {
                    latest_model_ref = event.model_ref.clone();
                }
                agent_id = Some(event.agent_id.clone());
                user_id = Some(event.user_id.clone());
            }

            let phases = latest_modes
                .into_iter()
                .map(|(phase, latest_mode)| {
                    serde_json::json!({
                        "phase": phase.clone(),
                        "latest_mode": latest_mode,
                        "used_streaming": used_streaming.get(&phase).copied().unwrap_or(false),
                        "fell_back": used_fallback.get(&phase).copied().unwrap_or(false),
                    })
                })
                .collect::<Vec<_>>();

            serde_json::json!({
                "request_id": request_id,
                "agent_id": agent_id,
                "user_id": user_id,
                "latest_model_ref": latest_model_ref,
                "phases": phases,
                "events": events,
            })
        })
        .collect::<Vec<_>>();

    Ok(serde_json::json!({
        "session_id": session_id,
        "request_id_filter": request_id,
        "request_count": requests.len(),
        "requests": requests,
    }))
}

fn inspect_streaming_metrics_json(
    sessions_dir: &Path,
    provider_id: Option<&str>,
    model_ref: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let audits = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_streaming_decision_audits(None, None)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let filtered = audits
        .into_iter()
        .filter(|audit| {
            let provider_match = provider_id.map_or(true, |provider_id| {
                audit.model_ref
                    .as_deref()
                    .and_then(|model_ref| model_ref.split_once('/').map(|(provider, _)| provider))
                    == Some(provider_id)
            });
            let model_match =
                model_ref.map_or(true, |model_ref| audit.model_ref.as_deref() == Some(model_ref));
            provider_match && model_match
        })
        .collect::<Vec<_>>();

    let mut by_mode: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_phase: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_provider_id: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_model_ref: BTreeMap<String, u64> = BTreeMap::new();
    let mut request_outcomes: BTreeMap<(String, String), String> = BTreeMap::new();
    for audit in &filtered {
        *by_mode.entry(audit.mode.clone()).or_default() += 1;
        *by_phase.entry(audit.phase.clone()).or_default() += 1;
        if let Some(model_ref) = audit.model_ref.as_ref() {
            *by_model_ref.entry(model_ref.clone()).or_default() += 1;
            if let Some((provider, _)) = model_ref.split_once('/') {
                *by_provider_id.entry(provider.to_string()).or_default() += 1;
            }
        } else {
            *by_provider_id.entry("unknown".into()).or_default() += 1;
        }
        request_outcomes.insert((audit.request_id.clone(), audit.phase.clone()), audit.mode.clone());
    }

    let total_phase_outcomes = request_outcomes.len() as u64;
    let stream_used_outcomes = request_outcomes
        .values()
        .filter(|mode| mode.as_str() == "stream_used")
        .count() as u64;
    let fallback_outcomes = request_outcomes
        .values()
        .filter(|mode| mode.as_str() == "fallback_used")
        .count() as u64;

    Ok(serde_json::json!({
        "provider_id_filter": provider_id,
        "model_ref_filter": model_ref,
        "total_events": filtered.len(),
        "total_phase_outcomes": total_phase_outcomes,
        "stream_used_outcomes": stream_used_outcomes,
        "fallback_outcomes": fallback_outcomes,
        "stream_success_rate": if total_phase_outcomes == 0 {
            0.0
        } else {
            stream_used_outcomes as f64 / total_phase_outcomes as f64
        },
        "fallback_rate": if total_phase_outcomes == 0 {
            0.0
        } else {
            fallback_outcomes as f64 / total_phase_outcomes as f64
        },
        "by_mode": by_mode,
        "by_phase": by_phase,
        "by_provider_id": by_provider_id,
        "by_model_ref": by_model_ref,
    }))
}

fn inspect_provider_capabilities_json(
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let profiles = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_provider_capabilities()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(profiles).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

async fn inspect_registered_providers_json(
    provider_registry: &Arc<tokio::sync::Mutex<ProviderRegistry>>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let reg = provider_registry.lock().await;
    let descriptors = reg.provider_descriptors(chrono::Utc::now().timestamp_micros() as u64);
    serde_json::to_value(descriptors).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_model_capabilities_json(
    sessions_dir: &Path,
    provider_id: &str,
    model_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let json = if let Some(model_id) = model_id {
        serde_json::to_value(
            store
                .read_model_capability(&format!("{}/{}", provider_id, model_id))
                .map_err(|e| (StatusCode::NOT_FOUND, e))?,
        )
    } else {
        serde_json::to_value(
            store
                .list_model_capabilities_for_provider(provider_id)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?,
        )
    };
    json.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_model_capability_probes_json(
    sessions_dir: &Path,
    provider_id: &str,
    model_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let probes = RuntimeStore::for_sessions_dir(sessions_dir)
        .list_model_capability_probes(&format!("{}/{}", provider_id, model_id))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    serde_json::to_value(probes).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_model_capability_decision_json(
    config: &Config,
    sessions_dir: &Path,
    provider_id: &str,
    model_id: &str,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let effective_profile = store
        .read_model_capability(&format!("{}/{}", provider_id, model_id))
        .map_err(|e| (StatusCode::NOT_FOUND, e))?;
    let provider_profile = store.read_provider_capability(provider_id).ok();
    let probes = store
        .list_model_capability_probes(&format!("{}/{}", provider_id, model_id))
        .unwrap_or_default();
    let latest_probe = probes.first().cloned();
    let local_override = matching_local_capability_override(config, provider_id, model_id).cloned();
    serde_json::to_value(serde_json::json!({
        "provider_id": provider_id,
        "model_id": model_id,
        "effective_source": effective_profile.source,
        "effective_profile": effective_profile,
        "provider_profile": provider_profile,
        "latest_probe": latest_probe,
        "probe_count": probes.len(),
        "local_override": local_override,
        "override_defined": matching_local_capability_override(config, provider_id, model_id).is_some(),
        "resolution_order": ["local_override", "runtime_probe", "provider_default"],
    }))
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("serialize failed: {}", e),
        )
    })
}

fn inspect_channel_health_json() -> Result<serde_json::Value, (StatusCode, String)> {
    let snapshots = crate::channel_health::snapshot_channel_health();
    Ok(serde_json::json!({
        "channels": snapshots,
    }))
}

fn inspect_channel_transports_json(
    config: &Config,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let adapters = configured_gateway_adapters(&config.gateway);
    let channels = adapters
        .into_iter()
        .map(|adapter| match adapter.as_str() {
            "telegram" => serde_json::json!({
                "channel": "telegram",
                "transport": config.gateway.telegram_mode,
                "bind_address": config.gateway.bind_address,
                "port": config.gateway.telegram_port,
            }),
            "websocket" => serde_json::json!({
                "channel": "websocket",
                "transport": "websocket",
                "bind_address": config.gateway.websocket_bind_address,
                "port": config.gateway.websocket_port,
            }),
            "whatsapp" => serde_json::json!({
                "channel": "whatsapp",
                "transport": "webhook",
                "bind_address": config.gateway.whatsapp_bind_address,
                "port": config.gateway.whatsapp_port,
                "outbound_url_configured": config.gateway.whatsapp_outbound_url.as_ref().is_some_and(|value| !value.trim().is_empty()),
            }),
            "cli" => serde_json::json!({
                "channel": "cli",
                "transport": "stdin_stdout",
            }),
            other => serde_json::json!({
                "channel": other,
                "transport": "unknown",
            }),
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({ "channels": channels }))
}

fn inspect_operational_alerts_json(
    config: &Config,
    sessions_dir: &Path,
) -> Result<serde_json::Value, (StatusCode, String)> {
    let channel_health = crate::channel_health::snapshot_channel_health();
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let outbox_dlq = store
        .list_durable_dlq(
            crate::runtime_store::DurableQueueKind::Outbox,
            &config.cluster.tenant_id,
            &config.cluster.workspace_scope,
        )
        .unwrap_or_default();
    let scope_denials = store
        .list_scope_denials(None, None)
        .unwrap_or_default();
    let mut compaction_failures = 0usize;
    for session in std::fs::read_dir(sessions_dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.flatten())
    {
        if let Ok(session_id) = uuid::Uuid::parse_str(&session.file_name().to_string_lossy()) {
            if let Ok(state) = store.read_compaction_state(session_id) {
                if matches!(state.status, aria_core::CompactionStatus::Failed) {
                    compaction_failures += 1;
                }
            }
        }
    }
    let mut alerts = Vec::new();
    if !outbox_dlq.is_empty() {
        alerts.push(serde_json::json!({
            "severity": "warning",
            "code": "outbox_dlq_backlog",
            "message": format!("{} outbox deliveries are in DLQ", outbox_dlq.len()),
        }));
    }
    if compaction_failures > 0 {
        alerts.push(serde_json::json!({
            "severity": "warning",
            "code": "compaction_failures_present",
            "message": format!("{} sessions have failed compaction state", compaction_failures),
        }));
    }
    if scope_denials.len() > 100 {
        alerts.push(serde_json::json!({
            "severity": "info",
            "code": "high_scope_denial_volume",
            "message": format!("scope denial volume is elevated: {}", scope_denials.len()),
        }));
    }
    for snapshot in &channel_health {
        if snapshot.ingress_queue_depth > 128 {
            alerts.push(serde_json::json!({
                "severity": "warning",
                "code": "channel_queue_depth_high",
                "channel": snapshot.channel,
                "message": format!("channel queue depth high for {}", snapshot.channel),
            }));
        }
    }
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let recent_health_history = store.list_channel_health_snapshots(10).unwrap_or_default();
    let payload = serde_json::json!({
        "alerts": alerts,
        "channel_health": channel_health,
        "recent_channel_health_history": recent_health_history,
        "outbox_dlq_count": outbox_dlq.len(),
        "compaction_failure_count": compaction_failures,
        "scope_denial_count": scope_denials.len(),
    });
    let _ = store.append_operational_alert_snapshot(&payload, now_us);
    let mut payload = payload;
    if let Some(map) = payload.as_object_mut() {
        map.insert(
            "recent_operational_alert_history".into(),
            serde_json::Value::Array(
                store.list_operational_alert_snapshots(10).unwrap_or_default(),
            ),
        );
    }
    Ok(payload)
}
