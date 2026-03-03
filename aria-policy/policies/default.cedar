// Default ARIA-X authorization policies.
//
// Entities:
//   Principal: Agent::"<agent_id>"
//   Action:    Action::"<tool_name>"
//   Resource:  Resource::"<target_path_or_node>"

// Allow any agent to read files under /workspace/
permit (
    principal,
    action == Action::"read_file",
    resource
)
when { resource.path like "/workspace/*" };

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
