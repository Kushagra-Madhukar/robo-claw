use super::*;

impl RuntimeStore {
    #[allow(dead_code)]
    pub fn upsert_execution_backend_profile(
        &self,
        profile: &ExecutionBackendProfile,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(profile)
            .map_err(|e| format!("serialize execution backend profile failed: {}", e))?;
        conn.execute(
            "INSERT INTO execution_backend_profiles (backend_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(backend_id) DO UPDATE SET
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![profile.backend_id, payload, updated_at_us as i64],
        )
        .map_err(|e| format!("upsert execution backend profile failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_execution_backend_profiles(&self) -> Result<Vec<ExecutionBackendProfile>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM execution_backend_profiles ORDER BY updated_at_us DESC",
            )
            .map_err(|e| format!("prepare execution backend query failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query execution backends failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read execution backend row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse execution backend profile failed: {}", e))?,
            );
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn upsert_execution_worker(
        &self,
        worker: &ExecutionWorkerRecord,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(worker)
            .map_err(|e| format!("serialize execution worker failed: {}", e))?;
        conn.execute(
            "INSERT INTO execution_workers
             (worker_id, backend_id, node_id, status, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(worker_id) DO UPDATE SET
               backend_id=excluded.backend_id,
               node_id=excluded.node_id,
               status=excluded.status,
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![
                worker.worker_id,
                worker.backend_id,
                worker.node_id,
                serde_json::to_string(&worker.status)
                    .map_err(|e| format!("serialize worker status failed: {}", e))?,
                payload,
                updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("upsert execution worker failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_execution_workers(
        &self,
        backend_id: Option<&str>,
    ) -> Result<Vec<ExecutionWorkerRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match backend_id {
            Some(backend_id) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM execution_workers
                         WHERE backend_id=?1 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare execution worker query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![backend_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query execution workers failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read execution worker row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse execution worker failed: {}", e))?,
                    );
                }
            }
            None => {
                let mut stmt = conn
                    .prepare("SELECT payload_json FROM execution_workers ORDER BY updated_at_us DESC")
                    .map_err(|e| format!("prepare execution worker query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query execution workers failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read execution worker row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse execution worker failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn mark_stale_execution_workers_offline(
        &self,
        heartbeat_timeout_us: u64,
        now_us: u64,
    ) -> Result<usize, String> {
        let mut workers = self.list_execution_workers(None)?;
        let stale_before = now_us.saturating_sub(heartbeat_timeout_us);
        let mut changed = 0usize;
        for worker in workers.iter_mut() {
            if worker.last_heartbeat_us < stale_before
                && worker.status != aria_core::ExecutionWorkerStatus::Offline
            {
                worker.status = aria_core::ExecutionWorkerStatus::Offline;
                self.upsert_execution_worker(worker, now_us)?;
                changed += 1;
            }
        }
        Ok(changed)
    }
}
