use super::*;

impl RuntimeStore {
    #[allow(dead_code)]
    pub fn upsert_computer_profile(
        &self,
        profile: &ComputerExecutionProfile,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(profile)
            .map_err(|e| format!("serialize computer profile failed: {}", e))?;
        conn.execute(
            "INSERT INTO computer_profiles (profile_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id) DO UPDATE SET
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![profile.profile_id, payload, updated_at_us as i64],
        )
        .map_err(|e| format!("upsert computer profile failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_computer_profiles(&self) -> Result<Vec<ComputerExecutionProfile>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM computer_profiles ORDER BY updated_at_us DESC")
            .map_err(|e| format!("prepare computer profile query failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query computer profiles failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read computer profile row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse computer profile failed: {}", e))?,
            );
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn upsert_computer_session(
        &self,
        session: &ComputerSessionRecord,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(session)
            .map_err(|e| format!("serialize computer session failed: {}", e))?;
        conn.execute(
            "INSERT INTO computer_sessions
             (computer_session_id, session_id, agent_id, profile_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(computer_session_id) DO UPDATE SET
               session_id=excluded.session_id,
               agent_id=excluded.agent_id,
               profile_id=excluded.profile_id,
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![
                session.computer_session_id,
                session.session_id.to_vec(),
                session.agent_id,
                session.profile_id,
                payload,
                updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("upsert computer session failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_computer_sessions(
        &self,
        session_id: Option<aria_core::Uuid>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ComputerSessionRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_sessions
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer session query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec(), agent_id], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|e| format!("query computer sessions failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer session row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer session failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_sessions
                         WHERE session_id=?1 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer session query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec()], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer sessions failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer session row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer session failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_sessions
                         WHERE agent_id=?1 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer session query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer sessions failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer session row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer session failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare("SELECT payload_json FROM computer_sessions ORDER BY updated_at_us DESC")
                    .map_err(|e| format!("prepare computer session query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer sessions failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer session row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer session failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn append_computer_action_audit(
        &self,
        audit: &ComputerActionAuditRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(audit)
            .map_err(|e| format!("serialize computer action audit failed: {}", e))?;
        conn.execute(
            "INSERT INTO computer_action_audits
             (audit_id, session_id, agent_id, profile_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                audit.audit_id,
                audit.session_id.to_vec(),
                audit.agent_id,
                audit.profile_id,
                payload,
                audit.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append computer action audit failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_computer_action_audits(
        &self,
        session_id: Option<aria_core::Uuid>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ComputerActionAuditRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_action_audits
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer action audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec(), agent_id], |row| {
                        row.get::<_, String>(0)
                    })
                    .map_err(|e| format!("query computer action audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer action audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer action audit failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_action_audits
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer action audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec()], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer action audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer action audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer action audit failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_action_audits
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer action audit query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer action audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer action audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer action audit failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_action_audits ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer action audit query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer action audits failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read computer action audit row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer action audit failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn append_computer_artifact(&self, artifact: &ComputerArtifactRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(artifact)
            .map_err(|e| format!("serialize computer artifact failed: {}", e))?;
        conn.execute(
            "INSERT INTO computer_artifacts
             (artifact_id, computer_session_id, session_id, agent_id, profile_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                artifact.artifact_id,
                artifact.computer_session_id,
                artifact.session_id.to_vec(),
                artifact.agent_id,
                artifact.profile_id,
                payload,
                artifact.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append computer artifact failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_computer_artifacts(
        &self,
        session_id: Option<aria_core::Uuid>,
        agent_id: Option<&str>,
    ) -> Result<Vec<ComputerArtifactRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match (session_id, agent_id) {
            (Some(session_id), Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_artifacts
                         WHERE session_id=?1 AND agent_id=?2 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer artifact query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec(), agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer artifacts failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read computer artifact row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer artifact failed: {}", e))?,
                    );
                }
            }
            (Some(session_id), None) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_artifacts
                         WHERE session_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer artifact query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![session_id.to_vec()], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer artifacts failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read computer artifact row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer artifact failed: {}", e))?,
                    );
                }
            }
            (None, Some(agent_id)) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM computer_artifacts
                         WHERE agent_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare computer artifact query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![agent_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer artifacts failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read computer artifact row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer artifact failed: {}", e))?,
                    );
                }
            }
            (None, None) => {
                let mut stmt = conn
                    .prepare("SELECT payload_json FROM computer_artifacts ORDER BY created_at_us DESC")
                    .map_err(|e| format!("prepare computer artifact query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query computer artifacts failed: {}", e))?;
                for row in rows {
                    let payload = row.map_err(|e| format!("read computer artifact row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse computer artifact failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }
}
