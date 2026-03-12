// Default ARIA-X authorization policies.
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
        Action::"search_tool_registry",
        Action::"set_reminder",
        Action::"schedule_message"
    ],
    resource == Resource::"global"
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
