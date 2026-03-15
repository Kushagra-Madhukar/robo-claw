use super::*;

impl RuntimeStore {
    fn upsert_agent_presence_record(
        &self,
        record: &aria_core::AgentPresenceRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize agent presence failed: {}", e))?;
        conn.execute(
            "INSERT INTO agent_presence (agent_id, availability, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(agent_id) DO UPDATE SET
                availability=excluded.availability,
                payload_json=excluded.payload_json,
                updated_at_us=excluded.updated_at_us",
            params![
                record.agent_id,
                serde_json::to_string(&record.availability)
                    .unwrap_or_else(|_| "\"available\"".into())
                    .replace('"', ""),
                payload,
                record.updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("write agent presence failed: {}", e))?;
        Ok(())
    }

    fn count_active_agent_runs_for_agent(&self, agent_id: &str) -> Result<usize, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM agent_runs
                 WHERE agent_id=?1 AND status IN (?2, ?3)",
            )
            .map_err(|e| format!("prepare active agent count failed: {}", e))?;
        let rows = stmt
            .query_map(
                params![
                    agent_id,
                    agent_run_status_name(aria_core::AgentRunStatus::Queued),
                    agent_run_status_name(aria_core::AgentRunStatus::Running),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("query active agent runs for agent failed: {}", e))?;
        let mut count = 0_usize;
        for row in rows {
            let _payload = row.map_err(|e| format!("read active agent row failed: {}", e))?;
            count += 1;
        }
        Ok(count)
    }

    fn refresh_agent_presence(&self, agent_id: &str, updated_at_us: u64) -> Result<(), String> {
        let active_run_count = self.count_active_agent_runs_for_agent(agent_id)? as u32;
        let availability = if active_run_count == 0 {
            aria_core::AgentAvailabilityState::Available
        } else {
            aria_core::AgentAvailabilityState::Busy
        };
        let status_summary = if active_run_count == 0 {
            Some("idle".to_string())
        } else {
            Some(format!("{} active run(s)", active_run_count))
        };
        self.upsert_agent_presence_record(&aria_core::AgentPresenceRecord {
            agent_id: agent_id.to_string(),
            availability,
            active_run_count,
            status_summary,
            updated_at_us,
        })
    }

    pub fn upsert_agent_run(
        &self,
        record: &AgentRunRecord,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize agent run failed: {}", e))?;
        conn.execute(
            "INSERT INTO agent_runs
             (run_id, session_id, user_id, agent_id, status, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(run_id) DO UPDATE SET
               session_id=excluded.session_id,
               user_id=excluded.user_id,
               agent_id=excluded.agent_id,
               status=excluded.status,
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![
                record.run_id,
                uuid::Uuid::from_bytes(record.session_id).to_string(),
                record.user_id,
                record.agent_id,
                agent_run_status_name(record.status),
                payload,
                updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("write agent run failed: {}", e))?;
        self.refresh_agent_presence(&record.agent_id, updated_at_us)?;
        Ok(())
    }

    pub fn list_agent_presence(&self) -> Result<Vec<aria_core::AgentPresenceRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM agent_presence ORDER BY updated_at_us DESC, agent_id ASC",
            )
            .map_err(|e| format!("prepare list agent presence failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query agent presence failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read agent presence row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse agent presence record failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn read_agent_run(&self, run_id: &str) -> Result<AgentRunRecord, String> {
        let conn = self.connect()?;
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM agent_runs WHERE run_id=?1",
                params![run_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("read agent run failed: {}", e))?;
        serde_json::from_str(&payload).map_err(|e| format!("parse agent run failed: {}", e))
    }

    pub fn list_agent_runs_for_session(
        &self,
        session_id: uuid::Uuid,
    ) -> Result<Vec<AgentRunRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM agent_runs WHERE session_id=?1 ORDER BY updated_at_us ASC",
            )
            .map_err(|e| format!("prepare list agent runs failed: {}", e))?;
        let rows = stmt
            .query_map(params![session_id.to_string()], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| format!("query agent runs failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read agent run row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse agent run record failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn count_active_agent_runs_for_parent(&self, parent_run_id: &str) -> Result<usize, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json
                 FROM agent_runs
                 WHERE status IN (?1, ?2)",
            )
            .map_err(|e| format!("prepare active agent-run count failed: {}", e))?;
        let rows = stmt
            .query_map(
                params![
                    agent_run_status_name(aria_core::AgentRunStatus::Queued),
                    agent_run_status_name(aria_core::AgentRunStatus::Running),
                ],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("query active agent runs failed: {}", e))?;
        let mut count = 0_usize;
        for row in rows {
            let payload = row.map_err(|e| format!("read active agent run row failed: {}", e))?;
            let run: AgentRunRecord = serde_json::from_str(&payload)
                .map_err(|e| format!("parse active agent run failed: {}", e))?;
            if run.parent_run_id.as_deref() == Some(parent_run_id) {
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn claim_next_queued_agent_run(
        &self,
        started_at_us: u64,
    ) -> Result<Option<AgentRunRecord>, String> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin agent-run claim transaction failed: {}", e))?;

        let queued_json: Option<String> = {
            let mut stmt = tx
                .prepare(
                    "SELECT payload_json
                     FROM agent_runs
                     WHERE status = ?1
                     ORDER BY updated_at_us ASC
                     LIMIT 1",
                )
                .map_err(|e| format!("prepare queued agent-run query failed: {}", e))?;
            stmt.query_row(
                params![agent_run_status_name(aria_core::AgentRunStatus::Queued)],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("query queued agent-run failed: {}", e))?
        };

        let Some(payload) = queued_json else {
            tx.commit()
                .map_err(|e| format!("commit empty agent-run claim failed: {}", e))?;
            return Ok(None);
        };

        let mut record: AgentRunRecord = serde_json::from_str(&payload)
            .map_err(|e| format!("parse queued agent-run failed: {}", e))?;
        record.status = aria_core::AgentRunStatus::Running;
        record.started_at_us = Some(started_at_us);
        let updated_payload = serde_json::to_string(&record)
            .map_err(|e| format!("serialize claimed agent-run failed: {}", e))?;
        let updated = tx
            .execute(
                "UPDATE agent_runs
                 SET status = ?1,
                     payload_json = ?2,
                     updated_at_us = ?3
                 WHERE run_id = ?4 AND status = ?5",
                params![
                    agent_run_status_name(aria_core::AgentRunStatus::Running),
                    updated_payload,
                    started_at_us as i64,
                    record.run_id,
                    agent_run_status_name(aria_core::AgentRunStatus::Queued),
                ],
            )
            .map_err(|e| format!("claim queued agent-run failed: {}", e))?;
        tx.commit()
            .map_err(|e| format!("commit claimed agent-run failed: {}", e))?;

        if updated == 0 {
            return Ok(None);
        }
        Ok(Some(record))
    }

    pub fn append_agent_run_event(&self, event: &AgentRunEvent) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(event)
            .map_err(|e| format!("serialize agent run event failed: {}", e))?;
        conn.execute(
            "INSERT INTO agent_run_events (event_id, run_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                event.event_id,
                event.run_id,
                payload,
                event.created_at_us as i64
            ],
        )
        .map_err(|e| format!("write agent run event failed: {}", e))?;
        Ok(())
    }

    pub fn list_agent_run_events(&self, run_id: &str) -> Result<Vec<AgentRunEvent>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM agent_run_events WHERE run_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list agent run events failed: {}", e))?;
        let rows = stmt
            .query_map(params![run_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query agent run events failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read agent run event row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse agent run event failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn cancel_agent_run(
        &self,
        run_id: &str,
        summary: &str,
        timestamp_us: u64,
    ) -> Result<Option<AgentRunRecord>, String> {
        let mut run = match self.read_agent_run(run_id) {
            Ok(run) => run,
            Err(err) if err.contains("Query returned no rows") => return Ok(None),
            Err(err) => return Err(err),
        };
        if matches!(
            run.status,
            aria_core::AgentRunStatus::Completed
                | aria_core::AgentRunStatus::Failed
                | aria_core::AgentRunStatus::Cancelled
                | aria_core::AgentRunStatus::TimedOut
        ) {
            return Ok(Some(run));
        }

        run.status = aria_core::AgentRunStatus::Cancelled;
        run.finished_at_us = Some(timestamp_us);
        run.result = Some(aria_core::AgentRunResult {
            response_summary: None,
            error: Some(summary.to_string()),
            completed_at_us: Some(timestamp_us),
        });
        self.upsert_agent_run(&run, timestamp_us)?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: run.run_id.clone(),
            kind: aria_core::AgentRunEventKind::Cancelled,
            summary: summary.to_string(),
            created_at_us: timestamp_us,
        })?;
        Ok(Some(run))
    }

    pub fn append_agent_mailbox_message(
        &self,
        message: &AgentMailboxMessage,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(message)
            .map_err(|e| format!("serialize agent mailbox message failed: {}", e))?;
        conn.execute(
            "INSERT INTO agent_mailbox (message_id, run_id, session_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                message.message_id,
                message.run_id,
                uuid::Uuid::from_bytes(message.session_id).to_string(),
                payload,
                message.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write agent mailbox message failed: {}", e))?;
        Ok(())
    }

    pub fn list_agent_mailbox_messages(
        &self,
        run_id: &str,
    ) -> Result<Vec<AgentMailboxMessage>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM agent_mailbox WHERE run_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list agent mailbox failed: {}", e))?;
        let rows = stmt
            .query_map(params![run_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query agent mailbox failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read agent mailbox row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse agent mailbox message failed: {}", e))?,
            );
        }
        Ok(out)
    }
}
