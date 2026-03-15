use super::*;

fn queue_kind_name(kind: DurableQueueKind) -> &'static str {
    match kind {
        DurableQueueKind::Ingress => "ingress",
        DurableQueueKind::Run => "run",
        DurableQueueKind::Outbox => "outbox",
    }
}

fn queue_status_name(status: DurableQueueStatus) -> &'static str {
    match status {
        DurableQueueStatus::Pending => "pending",
        DurableQueueStatus::Claimed => "claimed",
        DurableQueueStatus::Acked => "acked",
        DurableQueueStatus::DeadLetter => "dead_letter",
    }
}

fn parse_queue_kind(value: &str) -> Result<DurableQueueKind, String> {
    match value {
        "ingress" => Ok(DurableQueueKind::Ingress),
        "run" => Ok(DurableQueueKind::Run),
        "outbox" => Ok(DurableQueueKind::Outbox),
        other => Err(format!("unknown durable queue kind '{}'", other)),
    }
}

fn parse_queue_status(value: &str) -> Result<DurableQueueStatus, String> {
    match value {
        "pending" => Ok(DurableQueueStatus::Pending),
        "claimed" => Ok(DurableQueueStatus::Claimed),
        "acked" => Ok(DurableQueueStatus::Acked),
        "dead_letter" => Ok(DurableQueueStatus::DeadLetter),
        other => Err(format!("unknown durable queue status '{}'", other)),
    }
}

fn queue_message_from_row(row: &rusqlite::Row<'_>) -> Result<DurableQueueMessage, rusqlite::Error> {
    let queue_name: String = row.get("queue_name")?;
    let status_name: String = row.get("status")?;
    Ok(DurableQueueMessage {
        message_id: row.get("message_id")?,
        queue: parse_queue_kind(&queue_name).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                queue_name.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
            )
        })?,
        tenant_id: row.get("tenant_id")?,
        workspace_scope: row.get("workspace_scope")?,
        dedupe_key: row.get("dedupe_key")?,
        payload_json: row.get("payload_json")?,
        attempt_count: row.get::<_, i64>("attempt_count")? as u32,
        last_error: row.get("last_error")?,
        status: parse_queue_status(&status_name).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(
                status_name.len(),
                rusqlite::types::Type::Text,
                Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
            )
        })?,
        visible_at_us: row.get::<_, i64>("visible_at_us")? as u64,
        claimed_by: row.get("claimed_by")?,
        claimed_until_us: row
            .get::<_, Option<i64>>("claimed_until_us")?
            .map(|value| value as u64),
        created_at_us: row.get::<_, i64>("created_at_us")? as u64,
        updated_at_us: row.get::<_, i64>("updated_at_us")? as u64,
    })
}

impl RuntimeStore {
    pub fn enqueue_durable_message(&self, message: &DurableQueueMessage) -> Result<bool, String> {
        let conn = self.connect()?;
        let changed = conn
            .execute(
                "INSERT OR IGNORE INTO durable_queue_messages
                 (message_id, queue_name, tenant_id, workspace_scope, dedupe_key, status,
                  payload_json, attempt_count, last_error, visible_at_us, claimed_by,
                  claimed_until_us, created_at_us, updated_at_us)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    message.message_id,
                    queue_kind_name(message.queue),
                    message.tenant_id,
                    message.workspace_scope,
                    message.dedupe_key,
                    queue_status_name(message.status),
                    message.payload_json,
                    message.attempt_count as i64,
                    message.last_error,
                    message.visible_at_us as i64,
                    message.claimed_by,
                    message.claimed_until_us.map(|value| value as i64),
                    message.created_at_us as i64,
                    message.updated_at_us as i64,
                ],
            )
            .map_err(|e| format!("enqueue durable message failed: {}", e))?;
        Ok(changed > 0)
    }

    pub fn claim_durable_message(
        &self,
        queue: DurableQueueKind,
        tenant_id: &str,
        workspace_scope: &str,
        worker_id: &str,
        now_us: u64,
        lease_until_us: u64,
    ) -> Result<Option<DurableQueueMessage>, String> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin durable claim transaction failed: {}", e))?;
        let claimed: Option<DurableQueueMessage> = {
            let mut stmt = tx
                .prepare(
                    "SELECT message_id, queue_name, tenant_id, workspace_scope, dedupe_key,
                            status, payload_json, attempt_count, last_error, visible_at_us,
                            claimed_by, claimed_until_us, created_at_us, updated_at_us
                     FROM durable_queue_messages
                     WHERE queue_name=?1
                       AND tenant_id=?2
                       AND workspace_scope=?3
                       AND status=?4
                       AND visible_at_us <= ?5
                     ORDER BY updated_at_us ASC
                     LIMIT 1",
                )
                .map_err(|e| format!("prepare durable claim query failed: {}", e))?;
            stmt.query_row(
                params![
                    queue_kind_name(queue),
                    tenant_id,
                    workspace_scope,
                    queue_status_name(DurableQueueStatus::Pending),
                    now_us as i64,
                ],
                queue_message_from_row,
            )
            .optional()
            .map_err(|e| format!("query durable claim failed: {}", e))?
        };
        let Some(mut message) = claimed else {
            tx.commit()
                .map_err(|e| format!("commit empty durable claim failed: {}", e))?;
            return Ok(None);
        };
        let updated = tx
            .execute(
                "UPDATE durable_queue_messages
                 SET status=?1, claimed_by=?2, claimed_until_us=?3, updated_at_us=?4
                 WHERE message_id=?5 AND status=?6",
                params![
                    queue_status_name(DurableQueueStatus::Claimed),
                    worker_id,
                    lease_until_us as i64,
                    now_us as i64,
                    message.message_id,
                    queue_status_name(DurableQueueStatus::Pending),
                ],
            )
            .map_err(|e| format!("update durable claim failed: {}", e))?;
        tx.commit()
            .map_err(|e| format!("commit durable claim failed: {}", e))?;
        if updated == 0 {
            return Ok(None);
        }
        message.status = DurableQueueStatus::Claimed;
        message.claimed_by = Some(worker_id.to_string());
        message.claimed_until_us = Some(lease_until_us);
        message.updated_at_us = now_us;
        Ok(Some(message))
    }

    pub fn ack_durable_message(&self, message_id: &str, acked_at_us: u64) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE durable_queue_messages
             SET status=?1, updated_at_us=?2, claimed_by=NULL, claimed_until_us=NULL
             WHERE message_id=?3",
            params![
                queue_status_name(DurableQueueStatus::Acked),
                acked_at_us as i64,
                message_id
            ],
        )
        .map_err(|e| format!("ack durable message failed: {}", e))?;
        Ok(())
    }

    pub fn fail_durable_message(
        &self,
        message_id: &str,
        error: &str,
        failed_at_us: u64,
        retry_at_us: u64,
        max_attempts: u32,
    ) -> Result<DurableQueueStatus, String> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin durable fail transaction failed: {}", e))?;
        let mut message: DurableQueueMessage = tx
            .query_row(
                "SELECT message_id, queue_name, tenant_id, workspace_scope, dedupe_key,
                        status, payload_json, attempt_count, last_error, visible_at_us,
                        claimed_by, claimed_until_us, created_at_us, updated_at_us
                 FROM durable_queue_messages
                 WHERE message_id=?1",
                params![message_id],
                queue_message_from_row,
            )
            .map_err(|e| format!("read durable message failed: {}", e))?;
        message.attempt_count = message.attempt_count.saturating_add(1);
        message.last_error = Some(error.to_string());
        message.updated_at_us = failed_at_us;
        if message.attempt_count >= max_attempts {
            tx.execute(
                "UPDATE durable_queue_messages
                 SET status=?1, last_error=?2, attempt_count=?3, updated_at_us=?4,
                     claimed_by=NULL, claimed_until_us=NULL
                 WHERE message_id=?5",
                params![
                    queue_status_name(DurableQueueStatus::DeadLetter),
                    error,
                    message.attempt_count as i64,
                    failed_at_us as i64,
                    message_id,
                ],
            )
            .map_err(|e| format!("mark durable message dead-letter failed: {}", e))?;
            tx.execute(
                "INSERT INTO durable_queue_dlq
                 (dlq_id, message_id, queue_name, tenant_id, workspace_scope, payload_json,
                  final_error, attempt_count, created_at_us)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    format!("dlq-{}", uuid::Uuid::new_v4()),
                    message.message_id,
                    queue_kind_name(message.queue),
                    message.tenant_id,
                    message.workspace_scope,
                    message.payload_json,
                    error,
                    message.attempt_count as i64,
                    failed_at_us as i64,
                ],
            )
            .map_err(|e| format!("insert durable DLQ record failed: {}", e))?;
            tx.commit()
                .map_err(|e| format!("commit durable DLQ fail failed: {}", e))?;
            return Ok(DurableQueueStatus::DeadLetter);
        }
        tx.execute(
            "UPDATE durable_queue_messages
             SET status=?1, last_error=?2, attempt_count=?3, visible_at_us=?4, updated_at_us=?5,
                 claimed_by=NULL, claimed_until_us=NULL
             WHERE message_id=?6",
            params![
                queue_status_name(DurableQueueStatus::Pending),
                error,
                message.attempt_count as i64,
                retry_at_us as i64,
                failed_at_us as i64,
                message_id,
            ],
        )
        .map_err(|e| format!("requeue durable message failed: {}", e))?;
        tx.commit()
            .map_err(|e| format!("commit durable fail failed: {}", e))?;
        Ok(DurableQueueStatus::Pending)
    }

    pub fn list_durable_messages(
        &self,
        queue: DurableQueueKind,
        tenant_id: &str,
        workspace_scope: &str,
    ) -> Result<Vec<DurableQueueMessage>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT message_id, queue_name, tenant_id, workspace_scope, dedupe_key,
                        status, payload_json, attempt_count, last_error, visible_at_us,
                        claimed_by, claimed_until_us, created_at_us, updated_at_us
                 FROM durable_queue_messages
                 WHERE queue_name=?1 AND tenant_id=?2 AND workspace_scope=?3
                 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list durable messages failed: {}", e))?;
        let rows = stmt
            .query_map(
                params![queue_kind_name(queue), tenant_id, workspace_scope],
                queue_message_from_row,
            )
            .map_err(|e| format!("query durable messages failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("read durable message row failed: {}", e))?);
        }
        Ok(out)
    }

    pub fn list_durable_dlq(
        &self,
        queue: DurableQueueKind,
        tenant_id: &str,
        workspace_scope: &str,
    ) -> Result<Vec<DurableQueueDlqRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT dlq_id, message_id, queue_name, tenant_id, workspace_scope,
                        payload_json, final_error, attempt_count, created_at_us
                 FROM durable_queue_dlq
                 WHERE queue_name=?1 AND tenant_id=?2 AND workspace_scope=?3
                 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list durable DLQ failed: {}", e))?;
        let rows = stmt
            .query_map(
                params![queue_kind_name(queue), tenant_id, workspace_scope],
                |row| {
                    Ok(DurableQueueDlqRecord {
                        dlq_id: row.get("dlq_id")?,
                        message_id: row.get("message_id")?,
                        queue: parse_queue_kind(&row.get::<_, String>("queue_name")?).map_err(
                            |err| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        err,
                                    )),
                                )
                            },
                        )?,
                        tenant_id: row.get("tenant_id")?,
                        workspace_scope: row.get("workspace_scope")?,
                        payload_json: row.get("payload_json")?,
                        final_error: row.get("final_error")?,
                        attempt_count: row.get::<_, i64>("attempt_count")? as u32,
                        created_at_us: row.get::<_, i64>("created_at_us")? as u64,
                    })
                },
            )
            .map_err(|e| format!("query durable DLQ failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("read durable DLQ row failed: {}", e))?);
        }
        Ok(out)
    }

    pub fn replay_durable_dlq(
        &self,
        dlq_id: &str,
        replayed_at_us: u64,
    ) -> Result<Option<DurableQueueMessage>, String> {
        let mut conn = self.connect()?;
        let tx = conn
            .transaction()
            .map_err(|e| format!("begin durable DLQ replay transaction failed: {}", e))?;
        let dlq: Option<DurableQueueDlqRecord> = tx
            .query_row(
                "SELECT dlq_id, message_id, queue_name, tenant_id, workspace_scope,
                        payload_json, final_error, attempt_count, created_at_us
                 FROM durable_queue_dlq
                 WHERE dlq_id=?1",
                params![dlq_id],
                |row| {
                    Ok(DurableQueueDlqRecord {
                        dlq_id: row.get("dlq_id")?,
                        message_id: row.get("message_id")?,
                        queue: parse_queue_kind(&row.get::<_, String>("queue_name")?).map_err(
                            |err| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    0,
                                    rusqlite::types::Type::Text,
                                    Box::new(std::io::Error::new(
                                        std::io::ErrorKind::InvalidData,
                                        err,
                                    )),
                                )
                            },
                        )?,
                        tenant_id: row.get("tenant_id")?,
                        workspace_scope: row.get("workspace_scope")?,
                        payload_json: row.get("payload_json")?,
                        final_error: row.get("final_error")?,
                        attempt_count: row.get::<_, i64>("attempt_count")? as u32,
                        created_at_us: row.get::<_, i64>("created_at_us")? as u64,
                    })
                },
            )
            .optional()
            .map_err(|e| format!("read durable DLQ record failed: {}", e))?;
        let Some(dlq) = dlq else {
            tx.commit()
                .map_err(|e| format!("commit empty durable DLQ replay failed: {}", e))?;
            return Ok(None);
        };
        tx.execute(
            "UPDATE durable_queue_messages
             SET status=?1, last_error=NULL, visible_at_us=?2, updated_at_us=?3,
                 claimed_by=NULL, claimed_until_us=NULL
             WHERE message_id=?4",
            params![
                queue_status_name(DurableQueueStatus::Pending),
                replayed_at_us as i64,
                replayed_at_us as i64,
                dlq.message_id,
            ],
        )
        .map_err(|e| format!("replay durable message failed: {}", e))?;
        tx.execute(
            "DELETE FROM durable_queue_dlq WHERE dlq_id=?1",
            params![dlq_id],
        )
        .map_err(|e| format!("delete durable DLQ record failed: {}", e))?;
        tx.commit()
            .map_err(|e| format!("commit durable DLQ replay failed: {}", e))?;
        Ok(Some(DurableQueueMessage {
            message_id: dlq.message_id,
            queue: dlq.queue,
            tenant_id: dlq.tenant_id,
            workspace_scope: dlq.workspace_scope,
            dedupe_key: None,
            payload_json: dlq.payload_json,
            attempt_count: 0,
            last_error: None,
            status: DurableQueueStatus::Pending,
            visible_at_us: replayed_at_us,
            claimed_by: None,
            claimed_until_us: None,
            created_at_us: replayed_at_us,
            updated_at_us: replayed_at_us,
        }))
    }
}
