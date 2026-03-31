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
            related_run_id: None,
            actor_agent_id: None,
        })?;
        if run.inbox_on_completion && run.requested_by_agent.is_some() {
            self.append_agent_mailbox_message(&AgentMailboxMessage {
                message_id: format!("msg-{}", uuid::Uuid::new_v4()),
                run_id: run.run_id.clone(),
                session_id: run.session_id,
                from_agent_id: Some(run.agent_id.clone()),
                to_agent_id: run.requested_by_agent.clone(),
                body: format!("Sub-agent '{}' cancelled: {}", run.agent_id, summary),
                created_at_us: timestamp_us,
                delivered: false,
            })?;
            self.append_agent_run_event(&AgentRunEvent {
                event_id: format!("evt-{}", uuid::Uuid::new_v4()),
                run_id: run.run_id.clone(),
                kind: aria_core::AgentRunEventKind::InboxNotification,
                summary: "queued inbox notification for parent run".into(),
                created_at_us: timestamp_us,
                related_run_id: None,
                actor_agent_id: None,
            })?;
        }
        Ok(Some(run))
    }

    pub fn retry_agent_run(
        &self,
        run_id: &str,
        requested_by_agent: Option<&str>,
        timestamp_us: u64,
    ) -> Result<Option<AgentRunRecord>, String> {
        let existing = match self.read_agent_run(run_id) {
            Ok(run) => run,
            Err(err) if err.contains("Query returned no rows") => return Ok(None),
            Err(err) => return Err(err),
        };

        let retried = AgentRunRecord {
            run_id: format!("run-{}", uuid::Uuid::new_v4()),
            parent_run_id: existing
                .parent_run_id
                .clone()
                .or_else(|| Some(existing.run_id.clone())),
            origin_kind: Some(aria_core::AgentRunOriginKind::Retry),
            lineage_run_id: Some(existing.run_id.clone()),
            session_id: existing.session_id,
            user_id: existing.user_id.clone(),
            requested_by_agent: requested_by_agent
                .map(ToString::to_string)
                .or_else(|| existing.requested_by_agent.clone()),
            agent_id: existing.agent_id.clone(),
            status: aria_core::AgentRunStatus::Queued,
            request_text: existing.request_text.clone(),
            inbox_on_completion: existing.inbox_on_completion,
            max_runtime_seconds: existing.max_runtime_seconds,
            created_at_us: timestamp_us,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };

        self.upsert_agent_run(&retried, timestamp_us)?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: existing.run_id.clone(),
            kind: aria_core::AgentRunEventKind::Retried,
            summary: format!("Run retried as '{}'", retried.run_id),
            created_at_us: timestamp_us,
            related_run_id: Some(retried.run_id.clone()),
            actor_agent_id: requested_by_agent.map(ToString::to_string),
        })?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: retried.run_id.clone(),
            kind: aria_core::AgentRunEventKind::Queued,
            summary: format!("Run retried from '{}'", existing.run_id),
            created_at_us: timestamp_us,
            related_run_id: Some(existing.run_id.clone()),
            actor_agent_id: requested_by_agent.map(ToString::to_string),
        })?;
        Ok(Some(retried))
    }

    pub fn take_over_agent_run(
        &self,
        run_id: &str,
        takeover_agent_id: &str,
        requested_by_agent: Option<&str>,
        timestamp_us: u64,
    ) -> Result<Option<AgentRunRecord>, String> {
        let mut existing = match self.read_agent_run(run_id) {
            Ok(run) => run,
            Err(err) if err.contains("Query returned no rows") => return Ok(None),
            Err(err) => return Err(err),
        };

        let takeover_run = AgentRunRecord {
            run_id: format!("run-{}", uuid::Uuid::new_v4()),
            parent_run_id: existing
                .parent_run_id
                .clone()
                .or_else(|| Some(existing.run_id.clone())),
            origin_kind: Some(aria_core::AgentRunOriginKind::Takeover),
            lineage_run_id: Some(existing.run_id.clone()),
            session_id: existing.session_id,
            user_id: existing.user_id.clone(),
            requested_by_agent: requested_by_agent
                .map(ToString::to_string)
                .or_else(|| existing.requested_by_agent.clone()),
            agent_id: takeover_agent_id.to_string(),
            status: aria_core::AgentRunStatus::Queued,
            request_text: existing.request_text.clone(),
            inbox_on_completion: existing.inbox_on_completion,
            max_runtime_seconds: existing.max_runtime_seconds,
            created_at_us: timestamp_us,
            started_at_us: None,
            finished_at_us: None,
            result: None,
        };

        if matches!(
            existing.status,
            aria_core::AgentRunStatus::Queued | aria_core::AgentRunStatus::Running
        ) {
            let _ = self.cancel_agent_run_tree(
                &existing.run_id,
                &format!(
                    "taken over by '{}' as '{}'",
                    takeover_agent_id, takeover_run.run_id
                ),
                timestamp_us,
            )?;
            existing = self.read_agent_run(run_id)?;
        }

        self.upsert_agent_run(&takeover_run, timestamp_us)?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: existing.run_id.clone(),
            kind: aria_core::AgentRunEventKind::TakeoverQueued,
            summary: format!(
                "Run taken over by '{}' as '{}'",
                takeover_agent_id, takeover_run.run_id
            ),
            created_at_us: timestamp_us,
            related_run_id: Some(takeover_run.run_id.clone()),
            actor_agent_id: Some(takeover_agent_id.to_string()),
        })?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: takeover_run.run_id.clone(),
            kind: aria_core::AgentRunEventKind::Queued,
            summary: format!(
                "Run taken over from '{}' by '{}'",
                existing.run_id, takeover_agent_id
            ),
            created_at_us: timestamp_us,
            related_run_id: Some(existing.run_id.clone()),
            actor_agent_id: Some(takeover_agent_id.to_string()),
        })?;
        self.append_agent_mailbox_message(&AgentMailboxMessage {
            message_id: format!("msg-{}", uuid::Uuid::new_v4()),
            run_id: existing.run_id.clone(),
            session_id: existing.session_id,
            from_agent_id: Some(takeover_agent_id.to_string()),
            to_agent_id: existing.requested_by_agent.clone(),
            body: format!(
                "Run '{}' was taken over by '{}' as '{}'.",
                existing.run_id, takeover_agent_id, takeover_run.run_id
            ),
            created_at_us: timestamp_us,
            delivered: false,
        })?;
        self.append_agent_run_event(&AgentRunEvent {
            event_id: format!("evt-{}", uuid::Uuid::new_v4()),
            run_id: existing.run_id.clone(),
            kind: aria_core::AgentRunEventKind::InboxNotification,
            summary: "queued inbox notification for takeover".into(),
            created_at_us: timestamp_us,
            related_run_id: Some(takeover_run.run_id.clone()),
            actor_agent_id: Some(takeover_agent_id.to_string()),
        })?;
        Ok(Some(takeover_run))
    }

    pub fn cancel_agent_run_tree(
        &self,
        run_id: &str,
        summary: &str,
        timestamp_us: u64,
    ) -> Result<Vec<AgentRunRecord>, String> {
        let root = match self.read_agent_run(run_id) {
            Ok(run) => run,
            Err(err) if err.contains("Query returned no rows") => return Ok(Vec::new()),
            Err(err) => return Err(err),
        };
        let runs = self.list_agent_runs_for_session(uuid::Uuid::from_bytes(root.session_id))?;
        let mut children_by_parent: std::collections::BTreeMap<String, Vec<AgentRunRecord>> =
            std::collections::BTreeMap::new();
        for run in runs {
            if let Some(parent_run_id) = run.parent_run_id.clone() {
                children_by_parent.entry(parent_run_id).or_default().push(run);
            }
        }

        let mut descendant_ids = Vec::new();
        let mut stack = vec![run_id.to_string()];
        while let Some(current) = stack.pop() {
            if let Some(children) = children_by_parent.get(&current) {
                for child in children {
                    descendant_ids.push(child.run_id.clone());
                    stack.push(child.run_id.clone());
                }
            }
        }

        let mut updated = Vec::new();
        for descendant_run_id in descendant_ids.into_iter().rev() {
            let descendant_summary = format!(
                "cancelled because ancestor '{}' was cancelled",
                run_id
            );
            if let Some(run) =
                self.cancel_agent_run(&descendant_run_id, &descendant_summary, timestamp_us)?
            {
                updated.push(run);
            }
        }
        if let Some(run) = self.cancel_agent_run(run_id, summary, timestamp_us)? {
            updated.push(run);
        }
        Ok(updated)
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

    pub fn build_agent_run_tree_snapshot(
        &self,
        session_id: uuid::Uuid,
    ) -> Result<aria_core::AgentRunTreeSnapshot, String> {
        let runs = self.list_agent_runs_for_session(session_id)?;
        let run_ids = runs
            .iter()
            .map(|run| run.run_id.clone())
            .collect::<std::collections::HashSet<_>>();
        let mut child_map = std::collections::BTreeMap::<String, Vec<String>>::new();
        let mut root_run_ids = Vec::new();
        let mut orphan_parent_refs = std::collections::BTreeSet::new();
        let mut nodes = Vec::new();
        let mut transitions = Vec::new();

        for run in &runs {
            if let Some(parent_run_id) = &run.parent_run_id {
                if run_ids.contains(parent_run_id) {
                    child_map
                        .entry(parent_run_id.clone())
                        .or_default()
                        .push(run.run_id.clone());
                } else {
                    orphan_parent_refs.insert(parent_run_id.clone());
                    root_run_ids.push(run.run_id.clone());
                }
            } else {
                root_run_ids.push(run.run_id.clone());
            }
        }
        root_run_ids.sort();
        root_run_ids.dedup();

        for run in runs {
            let events = self.list_agent_run_events(&run.run_id)?;
            let mailbox = self.list_agent_mailbox_messages(&run.run_id)?;
            let last_event = events.last().cloned();
            for event in &events {
                if matches!(
                    event.kind,
                    aria_core::AgentRunEventKind::Retried
                        | aria_core::AgentRunEventKind::TakeoverQueued
                ) {
                    if let Some(target_run_id) = &event.related_run_id {
                        transitions.push(aria_core::AgentRunTransition {
                            kind: event.kind,
                            source_run_id: run.run_id.clone(),
                            target_run_id: target_run_id.clone(),
                            summary: event.summary.clone(),
                            created_at_us: event.created_at_us,
                            actor_agent_id: event.actor_agent_id.clone(),
                        });
                    }
                }
            }

            nodes.push(aria_core::AgentRunTreeNode {
                child_run_ids: child_map.remove(&run.run_id).unwrap_or_default(),
                mailbox_count: mailbox.len() as u32,
                last_event_kind: last_event.as_ref().map(|event| event.kind),
                last_event_summary: last_event.as_ref().map(|event| event.summary.clone()),
                run,
            });
        }

        nodes.sort_by(|left, right| left.run.created_at_us.cmp(&right.run.created_at_us));
        transitions.sort_by(|left, right| left.created_at_us.cmp(&right.created_at_us));

        Ok(aria_core::AgentRunTreeSnapshot {
            session_id: *session_id.as_bytes(),
            root_run_ids,
            orphan_parent_refs: orphan_parent_refs.into_iter().collect(),
            nodes,
            transitions,
        })
    }
}
