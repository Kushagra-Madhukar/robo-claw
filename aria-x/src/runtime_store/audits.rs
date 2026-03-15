use super::*;
use aria_core::{ContextInspectionRecord, RetrievalTraceRecord, SecretUsageAuditRecord};

impl RuntimeStore {
    pub fn append_retrieval_trace(&self, record: &RetrievalTraceRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize retrieval trace failed: {}", e))?;
        conn.execute(
            "INSERT INTO retrieval_traces
             (trace_id, request_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.trace_id,
                uuid::Uuid::from_bytes(record.request_id).to_string(),
                uuid::Uuid::from_bytes(record.session_id).to_string(),
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append retrieval trace failed: {}", e))?;
        Ok(())
    }

    pub fn list_retrieval_traces(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<RetrievalTraceRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM retrieval_traces
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare retrieval trace query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query retrieval traces failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read retrieval trace row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse retrieval trace failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM retrieval_traces
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare retrieval trace query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query retrieval traces failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read retrieval trace row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse retrieval trace failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM retrieval_traces
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare retrieval trace query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query retrieval traces failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read retrieval trace row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse retrieval trace failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM retrieval_traces ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare retrieval trace query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query retrieval traces failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read retrieval trace row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse retrieval trace failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_context_inspection(
        &self,
        record: &ContextInspectionRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize context inspection failed: {}", e))?;
        conn.execute(
            "INSERT INTO context_inspections
             (context_id, request_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.context_id,
                uuid::Uuid::from_bytes(record.request_id).to_string(),
                uuid::Uuid::from_bytes(record.session_id).to_string(),
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append context inspection failed: {}", e))?;
        Ok(())
    }

    pub fn list_context_inspections(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ContextInspectionRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM context_inspections
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare context inspection query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query context inspections failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read context inspection row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse context inspection failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM context_inspections
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare context inspection query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query context inspections failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read context inspection row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse context inspection failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM context_inspections
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare context inspection query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query context inspections failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read context inspection row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse context inspection failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM context_inspections ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare context inspection query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query context inspections failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read context inspection row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse context inspection failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_secret_usage_audit(&self, record: &SecretUsageAuditRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize secret usage audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO secret_usage_audits
             (audit_id, session_id, agent_id, tool_name, key_name, target_domain, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.audit_id,
                record
                    .session_id
                    .map(|id| uuid::Uuid::from_bytes(id).to_string()),
                record.agent_id,
                record.tool_name,
                record.key_name,
                record.target_domain,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append secret usage audit failed: {}", e))?;
        Ok(())
    }

    pub fn list_secret_usage_audits(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<SecretUsageAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM secret_usage_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare secret usage audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query secret usage audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read secret usage audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse secret usage audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM secret_usage_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare secret usage audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query secret usage audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read secret usage audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse secret usage audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM secret_usage_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare secret usage audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query secret usage audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read secret usage audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse secret usage audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM secret_usage_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare secret usage audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query secret usage audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read secret usage audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse secret usage audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_scope_denial(&self, record: &ScopeDenialRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize scope denial failed: {}", e))?;
        conn.execute(
            "INSERT INTO scope_denials
             (denial_id, agent_id, session_id, kind, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.denial_id,
                record.agent_id,
                record
                    .session_id
                    .map(|id| uuid::Uuid::from_bytes(id).to_string()),
                serde_json::to_string(&record.kind)
                    .map_err(|e| format!("serialize scope denial kind failed: {}", e))?,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append scope denial failed: {}", e))?;
        self.prune_operator_records(
            SKILL_SIGNATURE_RETENTION_ROWS.load(Ordering::Relaxed),
            SHELL_EXEC_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            SCOPE_DENIAL_RETENTION_ROWS.load(Ordering::Relaxed),
            REQUEST_POLICY_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            REPAIR_FALLBACK_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            STREAMING_DECISION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_ACTION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_CHALLENGE_EVENT_RETENTION_ROWS.load(Ordering::Relaxed),
        )?;
        Ok(())
    }

    pub fn list_scope_denials(
        &self,
        agent_id: Option<&str>,
        session_id: Option<&str>,
    ) -> Result<Vec<ScopeDenialRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (agent_id, session_id) {
            (Some(agent_id), Some(session_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM scope_denials
                         WHERE agent_id=?1 AND session_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare scope denial query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id, session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query scope denials failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read scope denial row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse scope denial failed: {}", e))?,
                    );
                }
            }
            (Some(agent_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM scope_denials
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare scope denial query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query scope denials failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read scope denial row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse scope denial failed: {}", e))?,
                    );
                }
            }
            (None, Some(session_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM scope_denials
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare scope denial query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query scope denials failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read scope denial row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse scope denial failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare("SELECT payload_json FROM scope_denials ORDER BY created_at_us DESC")
                    .map_err(|e| format!("prepare scope denial query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query scope denials failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read scope denial row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse scope denial failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_shell_exec_audit(
        &self,
        record: &ShellExecutionAuditRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize shell exec audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO shell_exec_audits
             (audit_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                record.audit_id,
                record.session_id,
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append shell exec audit failed: {}", e))?;
        self.prune_operator_records(
            SKILL_SIGNATURE_RETENTION_ROWS.load(Ordering::Relaxed),
            SHELL_EXEC_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            SCOPE_DENIAL_RETENTION_ROWS.load(Ordering::Relaxed),
            REQUEST_POLICY_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            REPAIR_FALLBACK_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            STREAMING_DECISION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_ACTION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_CHALLENGE_EVENT_RETENTION_ROWS.load(Ordering::Relaxed),
        )?;
        Ok(())
    }

    pub fn prune_operator_records(
        &self,
        max_skill_signature_rows: u32,
        max_shell_exec_audit_rows: u32,
        max_scope_denial_rows: u32,
        max_request_policy_audit_rows: u32,
        max_repair_fallback_audit_rows: u32,
        max_streaming_decision_audit_rows: u32,
        max_browser_action_audit_rows: u32,
        max_browser_challenge_event_rows: u32,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        if max_skill_signature_rows > 0 {
            conn.execute(
                "DELETE FROM skill_signatures
                 WHERE record_id IN (
                     SELECT record_id
                     FROM skill_signatures
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_skill_signature_rows as i64],
            )
            .map_err(|e| format!("prune skill signature records failed: {}", e))?;
        }
        if max_shell_exec_audit_rows > 0 {
            conn.execute(
                "DELETE FROM shell_exec_audits
                 WHERE audit_id IN (
                     SELECT audit_id
                     FROM shell_exec_audits
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_shell_exec_audit_rows as i64],
            )
            .map_err(|e| format!("prune shell execution audit records failed: {}", e))?;
        }
        if max_scope_denial_rows > 0 {
            conn.execute(
                "DELETE FROM scope_denials
                 WHERE denial_id IN (
                     SELECT denial_id
                     FROM scope_denials
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_scope_denial_rows as i64],
            )
            .map_err(|e| format!("prune scope denial records failed: {}", e))?;
        }
        if max_request_policy_audit_rows > 0 {
            conn.execute(
                "DELETE FROM request_policy_audits
                 WHERE audit_id IN (
                     SELECT audit_id
                     FROM request_policy_audits
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_request_policy_audit_rows as i64],
            )
            .map_err(|e| format!("prune request policy audit records failed: {}", e))?;
        }
        if max_repair_fallback_audit_rows > 0 {
            conn.execute(
                "DELETE FROM repair_fallback_audits
                 WHERE audit_id IN (
                     SELECT audit_id
                     FROM repair_fallback_audits
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_repair_fallback_audit_rows as i64],
            )
            .map_err(|e| format!("prune repair fallback audit records failed: {}", e))?;
        }
        if max_streaming_decision_audit_rows > 0 {
            conn.execute(
                "DELETE FROM streaming_decision_audits
                 WHERE audit_id IN (
                     SELECT audit_id
                     FROM streaming_decision_audits
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_streaming_decision_audit_rows as i64],
            )
            .map_err(|e| format!("prune streaming decision audit records failed: {}", e))?;
        }
        if max_browser_action_audit_rows > 0 {
            conn.execute(
                "DELETE FROM browser_action_audits
                 WHERE audit_id IN (
                     SELECT audit_id
                     FROM browser_action_audits
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_browser_action_audit_rows as i64],
            )
            .map_err(|e| format!("prune browser action audit records failed: {}", e))?;
        }
        if max_browser_challenge_event_rows > 0 {
            conn.execute(
                "DELETE FROM browser_challenge_events
                 WHERE event_id IN (
                     SELECT event_id
                     FROM browser_challenge_events
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_browser_challenge_event_rows as i64],
            )
            .map_err(|e| format!("prune browser challenge event records failed: {}", e))?;
        }
        Ok(())
    }

    pub fn list_shell_exec_audits(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ShellExecutionAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM shell_exec_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare shell audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query shell audits failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read shell audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse shell audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM shell_exec_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare shell audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query shell audits failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read shell audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse shell audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM shell_exec_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare shell audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query shell audits failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read shell audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse shell audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM shell_exec_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare shell audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query shell audits failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read shell audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse shell audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_request_policy_audit(
        &self,
        record: &RequestPolicyAuditRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize request policy audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO request_policy_audits
             (audit_id, request_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.audit_id,
                record.request_id,
                record.session_id,
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append request policy audit failed: {}", e))?;
        self.prune_operator_records(
            SKILL_SIGNATURE_RETENTION_ROWS.load(Ordering::Relaxed),
            SHELL_EXEC_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            SCOPE_DENIAL_RETENTION_ROWS.load(Ordering::Relaxed),
            REQUEST_POLICY_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            REPAIR_FALLBACK_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            STREAMING_DECISION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_ACTION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_CHALLENGE_EVENT_RETENTION_ROWS.load(Ordering::Relaxed),
        )?;
        Ok(())
    }

    pub fn list_request_policy_audits(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<RequestPolicyAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM request_policy_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare request policy audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query request policy audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read request policy audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse request policy audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM request_policy_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare request policy audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query request policy audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read request policy audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse request policy audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM request_policy_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare request policy audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query request policy audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read request policy audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse request policy audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM request_policy_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare request policy audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query request policy audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read request policy audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse request policy audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_repair_fallback_audit(
        &self,
        record: &RepairFallbackAuditRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize repair fallback audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO repair_fallback_audits
             (audit_id, request_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.audit_id,
                record.request_id,
                record.session_id,
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append repair fallback audit failed: {}", e))?;
        self.prune_operator_records(
            SKILL_SIGNATURE_RETENTION_ROWS.load(Ordering::Relaxed),
            SHELL_EXEC_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            SCOPE_DENIAL_RETENTION_ROWS.load(Ordering::Relaxed),
            REQUEST_POLICY_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            REPAIR_FALLBACK_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            STREAMING_DECISION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_ACTION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_CHALLENGE_EVENT_RETENTION_ROWS.load(Ordering::Relaxed),
        )?;
        Ok(())
    }

    pub fn list_repair_fallback_audits(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<RepairFallbackAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM repair_fallback_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare repair fallback audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query repair fallback audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read repair fallback audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse repair fallback audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM repair_fallback_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare repair fallback audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query repair fallback audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read repair fallback audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse repair fallback audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM repair_fallback_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare repair fallback audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query repair fallback audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read repair fallback audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse repair fallback audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM repair_fallback_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare repair fallback audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query repair fallback audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read repair fallback audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse repair fallback audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    pub fn append_streaming_decision_audit(
        &self,
        record: &StreamingDecisionAuditRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize streaming decision audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO streaming_decision_audits
             (audit_id, request_id, session_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.audit_id,
                record.request_id,
                record.session_id,
                record.agent_id,
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append streaming decision audit failed: {}", e))?;
        self.prune_operator_records(
            SKILL_SIGNATURE_RETENTION_ROWS.load(Ordering::Relaxed),
            SHELL_EXEC_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            SCOPE_DENIAL_RETENTION_ROWS.load(Ordering::Relaxed),
            REQUEST_POLICY_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            REPAIR_FALLBACK_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            STREAMING_DECISION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_ACTION_AUDIT_RETENTION_ROWS.load(Ordering::Relaxed),
            BROWSER_CHALLENGE_EVENT_RETENTION_ROWS.load(Ordering::Relaxed),
        )?;
        Ok(())
    }

    pub fn list_streaming_decision_audits(
        &self,
        session_id: Option<&str>,
        agent_id: Option<&str>,
    ) -> Result<Vec<StreamingDecisionAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM streaming_decision_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare streaming decision audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id, agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query streaming decision audits failed: {}", e))?;
                for row in rows {
                    let payload = row
                        .map_err(|e| format!("read streaming decision audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse streaming decision audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM streaming_decision_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare streaming decision audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query streaming decision audits failed: {}", e))?;
                for row in rows {
                    let payload = row
                        .map_err(|e| format!("read streaming decision audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse streaming decision audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM streaming_decision_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare streaming decision audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query streaming decision audits failed: {}", e))?;
                for row in rows {
                    let payload = row
                        .map_err(|e| format!("read streaming decision audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse streaming decision audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM streaming_decision_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare streaming decision audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query streaming decision audits failed: {}", e))?;
                for row in rows {
                    let payload = row
                        .map_err(|e| format!("read streaming decision audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse streaming decision audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }
}
