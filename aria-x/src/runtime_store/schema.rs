use super::*;

impl RuntimeStore {
    pub(super) fn init_schema(&self, conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            r#"
CREATE TABLE IF NOT EXISTS approvals (
    approval_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS approval_handles (
    handle_id TEXT PRIMARY KEY,
    approval_id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    created_at_us INTEGER NOT NULL,
    expires_at_us INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_approval_handles_session_user
    ON approval_handles (session_id, user_id, expires_at_us);
CREATE TABLE IF NOT EXISTS elevations (
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    granted_at_us INTEGER NOT NULL,
    expires_at_us INTEGER,
    payload_json TEXT NOT NULL,
    PRIMARY KEY (session_id, agent_id)
);
CREATE TABLE IF NOT EXISTS dedupe_keys (
    dedupe_key TEXT PRIMARY KEY,
    first_seen_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS outbound_deliveries (
    envelope_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    recipient_id TEXT NOT NULL,
    status TEXT NOT NULL,
    error TEXT,
    payload_json TEXT NOT NULL,
    recorded_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS channel_health_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    recorded_at_us INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channel_health_snapshots_recorded
    ON channel_health_snapshots (recorded_at_us DESC);
CREATE TABLE IF NOT EXISTS operational_alert_snapshots (
    snapshot_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    recorded_at_us INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_operational_alert_snapshots_recorded
    ON operational_alert_snapshots (recorded_at_us DESC);
CREATE TABLE IF NOT EXISTS durable_queue_messages (
    message_id TEXT PRIMARY KEY,
    queue_name TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    workspace_scope TEXT NOT NULL,
    dedupe_key TEXT,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    attempt_count INTEGER NOT NULL,
    last_error TEXT,
    visible_at_us INTEGER NOT NULL,
    claimed_by TEXT,
    claimed_until_us INTEGER,
    created_at_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_durable_queue_dedupe
    ON durable_queue_messages (queue_name, tenant_id, workspace_scope, dedupe_key)
    WHERE dedupe_key IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_durable_queue_claim
    ON durable_queue_messages (queue_name, tenant_id, workspace_scope, status, visible_at_us, updated_at_us);
CREATE TABLE IF NOT EXISTS durable_queue_dlq (
    dlq_id TEXT PRIMARY KEY,
    message_id TEXT NOT NULL,
    queue_name TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    workspace_scope TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    final_error TEXT NOT NULL,
    attempt_count INTEGER NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_durable_queue_dlq_lookup
    ON durable_queue_dlq (queue_name, tenant_id, workspace_scope, created_at_us);
CREATE TABLE IF NOT EXISTS cache_snapshots (
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    tools_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL,
    PRIMARY KEY (session_id, agent_id)
);
CREATE TABLE IF NOT EXISTS job_snapshots (
    job_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS job_leases (
    job_id TEXT PRIMARY KEY,
    worker_id TEXT NOT NULL,
    lease_until_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS resource_leases (
    resource_key TEXT PRIMARY KEY,
    lock_mode TEXT NOT NULL,
    holder_id TEXT NOT NULL,
    fencing_token INTEGER NOT NULL,
    lease_until_us INTEGER NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_runs (
    run_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_presence (
    agent_id TEXT PRIMARY KEY,
    availability TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_run_events (
    event_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_mailbox (
    message_id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_packages (
    skill_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_bindings (
    binding_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    skill_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_activations (
    activation_id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS skill_signatures (
    record_id TEXT PRIMARY KEY,
    skill_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mcp_servers (
    server_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mcp_imports (
    import_id TEXT PRIMARY KEY,
    server_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mcp_bindings (
    binding_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    server_id TEXT NOT NULL,
    target_kind TEXT NOT NULL,
    target_name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mcp_import_cache (
    server_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS control_documents (
    document_id TEXT PRIMARY KEY,
    workspace_root TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS compaction_state (
    session_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS provider_capabilities (
    provider_id TEXT PRIMARY KEY,
    adapter_family TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS model_capabilities (
    model_ref TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    adapter_family TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    observed_at_us INTEGER NOT NULL,
    expires_at_us INTEGER,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS model_capability_probes (
    probe_id TEXT PRIMARY KEY,
    model_ref TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    model_id TEXT NOT NULL,
    adapter_family TEXT NOT NULL,
    observed_at_us INTEGER NOT NULL,
    expires_at_us INTEGER,
    payload_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS scope_denials (
    denial_id TEXT PRIMARY KEY,
    agent_id TEXT NOT NULL,
    session_id TEXT,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS shell_exec_audits (
    audit_id TEXT PRIMARY KEY,
    session_id TEXT,
    agent_id TEXT,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS secret_usage_audits (
    audit_id TEXT PRIMARY KEY,
    session_id TEXT,
    agent_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    key_name TEXT NOT NULL,
    target_domain TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS request_policy_audits (
    audit_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS retrieval_traces (
    trace_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS context_inspections (
    context_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS repair_fallback_audits (
    audit_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS streaming_decision_audits (
    audit_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_profiles (
    profile_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_profile_bindings (
    binding_id TEXT PRIMARY KEY,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_sessions (
    browser_session_id TEXT PRIMARY KEY,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_session_states (
    state_id TEXT PRIMARY KEY,
    browser_session_id TEXT NOT NULL,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_artifacts (
    artifact_id TEXT PRIMARY KEY,
    browser_session_id TEXT NOT NULL,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_action_audits (
    audit_id TEXT PRIMARY KEY,
    browser_session_id TEXT,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_challenge_events (
    event_id TEXT PRIMARY KEY,
    browser_session_id TEXT NOT NULL,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS browser_login_states (
    login_state_id TEXT PRIMARY KEY,
    browser_session_id TEXT NOT NULL,
    session_id BLOB NOT NULL,
    agent_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    domain TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS domain_access_decisions (
    decision_id TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    agent_id TEXT,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS crawl_jobs (
    crawl_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS watch_jobs (
    watch_id TEXT PRIMARY KEY,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS website_memory (
    record_id TEXT PRIMARY KEY,
    domain TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS execution_traces (
    request_id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    agent_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    prompt_mode TEXT NOT NULL,
    task_fingerprint TEXT NOT NULL,
    outcome TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    recorded_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reward_events (
    event_id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    value INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    recorded_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS candidate_artifacts (
    candidate_id TEXT PRIMARY KEY,
    task_fingerprint TEXT NOT NULL,
    kind TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS candidate_eval_runs (
    run_id TEXT PRIMARY KEY,
    candidate_id TEXT NOT NULL,
    task_fingerprint TEXT NOT NULL,
    score INTEGER NOT NULL,
    passed INTEGER NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS candidate_promotions (
    promotion_id TEXT PRIMARY KEY,
    candidate_id TEXT NOT NULL,
    task_fingerprint TEXT NOT NULL,
    action TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS selector_models (
    model_id TEXT PRIMARY KEY,
    task_fingerprint TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    updated_at_us INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS learning_derivative_events (
    event_id TEXT PRIMARY KEY,
    task_fingerprint TEXT NOT NULL,
    kind TEXT NOT NULL,
    artifact_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at_us INTEGER NOT NULL
);
DELETE FROM skill_bindings
WHERE rowid NOT IN (
    SELECT MAX(rowid)
    FROM skill_bindings
    GROUP BY agent_id, skill_id
);
DELETE FROM mcp_bindings
WHERE rowid NOT IN (
    SELECT MAX(rowid)
    FROM mcp_bindings
    GROUP BY agent_id, server_id, target_kind, target_name
);
CREATE INDEX IF NOT EXISTS idx_dedupe_keys_first_seen
ON dedupe_keys(first_seen_us);
CREATE UNIQUE INDEX IF NOT EXISTS idx_skill_bindings_agent_skill
ON skill_bindings(agent_id, skill_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_bindings_agent_server_target
ON mcp_bindings(agent_id, server_id, target_kind, target_name);
"#,
        )
        .map_err(|e| format!("init runtime store schema failed: {}", e))
    }
}
