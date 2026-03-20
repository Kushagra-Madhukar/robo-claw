// Default HiveClaw authorization policies.
//
// Entities:
//   Principal: Agent::"<agent_id>"
//   Action:    Action::"<tool_name>"
//   Resource:  Resource::"<target_path_or_node>"

// Allow any agent to read files
permit (
    principal,
    action == Action::"read_file",
    resource
);

// Allow any agent to write files
permit (
    principal,
    action == Action::"write_file",
    resource
);

// Allow any agent to manage cron jobs
permit (
    principal,
    action == Action::"manage_cron",
    resource == Resource::"global"
);

// Allow any agent to run discovery and utility tools
permit (
    principal,
    action in [
        Action::"search_codebase",
        Action::"run_tests",
        Action::"run_shell",
        Action::"search_web",
        Action::"fetch_url",
        Action::"web_fetch",
        Action::"web_extract",
        Action::"search_tool_registry",
        Action::"set_reminder",
        Action::"schedule_message",
        Action::"set_domain_access_decision",
        Action::"browser_profile_create",
        Action::"browser_profile_list",
        Action::"browser_profile_use",
        Action::"browser_session_start",
        Action::"browser_session_list",
        Action::"browser_session_status",
        Action::"browser_session_pause",
        Action::"browser_session_resume",
        Action::"browser_session_cleanup",
        Action::"browser_open",
        Action::"browser_snapshot",
        Action::"browser_screenshot",
        Action::"browser_extract",
        Action::"browser_download",
        Action::"browser_act",
        Action::"browser_login_status",
        Action::"browser_login_begin_manual",
        Action::"browser_login_complete_manual",
        Action::"browser_login_fill_credentials",
        Action::"spawn_agent",
        Action::"cancel_agent_run",
        Action::"retry_agent_run",
        Action::"list_agent_runs",
        Action::"get_agent_run",
        Action::"get_agent_run_events",
        Action::"get_agent_mailbox",
        Action::"crawl_page",
        Action::"crawl_site",
        Action::"watch_page",
        Action::"watch_site",
        Action::"list_watch_jobs"
    ],
    resource == Resource::"global"
);

// Allow default web/browser/crawl authorization actions used by runtime checks.
permit (
    principal,
    action in [
        Action::"web_domain_fetch",
        Action::"web_domain_crawl",
        Action::"web_domain_screenshot",
        Action::"web_domain_interactive_read",
        Action::"web_domain_interactive_write",
        Action::"web_domain_login",
        Action::"web_domain_download",
        Action::"browser_profile_access",
        Action::"browser_action_access",
        Action::"crawl_scope_access"
    ],
    resource
);

// Allow MCP runtime actions; capability and binding allowlists still gate actual use.
permit (
    principal,
    action in [
        Action::"register_mcp_server",
        Action::"import_mcp_tool",
        Action::"import_mcp_prompt",
        Action::"import_mcp_resource",
        Action::"bind_mcp_import",
        Action::"invoke_mcp_tool",
        Action::"render_mcp_prompt",
        Action::"read_mcp_resource"
    ],
    resource
);

// Allow any agent to read sensors
permit (
    principal,
    action == Action::"read_sensor",
    resource
);

// Forbid reading sensitive system paths
forbid (
    principal,
    action,
    resource
)
when { resource.path like "/etc/*" };

forbid (
    principal,
    action,
    resource
)
when { resource.path like "/usr/*" };

forbid (
    principal,
    action,
    resource
)
when { resource.path like "/var/*" };
