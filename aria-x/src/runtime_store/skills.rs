use super::*;

impl RuntimeStore {
    pub fn upsert_skill_package(
        &self,
        manifest: &SkillPackageManifest,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(manifest)
            .map_err(|e| format!("serialize skill package failed: {}", e))?;
        conn.execute(
            "INSERT INTO skill_packages (skill_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(skill_id) DO UPDATE SET
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![manifest.skill_id, payload, updated_at_us as i64],
        )
        .map_err(|e| format!("write skill package failed: {}", e))?;
        Ok(())
    }

    pub fn list_skill_packages(&self) -> Result<Vec<SkillPackageManifest>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM skill_packages ORDER BY updated_at_us ASC")
            .map_err(|e| format!("prepare list skill packages failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query skill packages failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read skill package row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse skill package failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn upsert_skill_binding(&self, binding: &SkillBinding) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(binding)
            .map_err(|e| format!("serialize skill binding failed: {}", e))?;
        conn.execute(
            "INSERT INTO skill_bindings (binding_id, agent_id, skill_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(agent_id, skill_id) DO UPDATE SET
               binding_id=excluded.binding_id,
               payload_json=excluded.payload_json,
               created_at_us=excluded.created_at_us",
            params![
                binding.binding_id,
                binding.agent_id,
                binding.skill_id,
                payload,
                binding.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write skill binding failed: {}", e))?;
        Ok(())
    }

    pub fn list_skill_bindings_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<SkillBinding>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM skill_bindings WHERE agent_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list skill bindings failed: {}", e))?;
        let rows = stmt
            .query_map(params![agent_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query skill bindings failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read skill binding row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse skill binding failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn list_skill_bindings_for_skill(
        &self,
        skill_id: &str,
    ) -> Result<Vec<SkillBinding>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM skill_bindings WHERE skill_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list skill bindings by skill failed: {}", e))?;
        let rows = stmt
            .query_map(params![skill_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query skill bindings by skill failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read skill binding row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse skill binding failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn delete_skill_binding(&self, agent_id: &str, skill_id: &str) -> Result<usize, String> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM skill_bindings WHERE agent_id=?1 AND skill_id=?2",
            params![agent_id, skill_id],
        )
        .map_err(|e| format!("delete skill binding failed: {}", e))
    }

    pub fn append_skill_activation(
        &self,
        activation: &SkillActivationRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(activation)
            .map_err(|e| format!("serialize skill activation failed: {}", e))?;
        conn.execute(
            "INSERT INTO skill_activations (activation_id, skill_id, agent_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                activation.activation_id,
                activation.skill_id,
                activation.agent_id,
                payload,
                activation.activated_at_us as i64,
            ],
        )
        .map_err(|e| format!("write skill activation failed: {}", e))?;
        Ok(())
    }

    pub fn list_skill_activations_for_agent(
        &self,
        agent_id: &str,
    ) -> Result<Vec<SkillActivationRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM skill_activations WHERE agent_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list skill activations failed: {}", e))?;
        let rows = stmt
            .query_map(params![agent_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query skill activations failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read skill activation row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse skill activation failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn list_skill_activations_for_skill(
        &self,
        skill_id: &str,
    ) -> Result<Vec<SkillActivationRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM skill_activations WHERE skill_id=?1 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list skill activations by skill failed: {}", e))?;
        let rows = stmt
            .query_map(params![skill_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query skill activations by skill failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read skill activation row failed: {}", e))?;
            out.push(
                serde_json::from_str(&payload)
                    .map_err(|e| format!("parse skill activation failed: {}", e))?,
            );
        }
        Ok(out)
    }

    pub fn append_skill_signature(&self, signature: &SkillSignatureRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(signature)
            .map_err(|e| format!("serialize skill signature failed: {}", e))?;
        conn.execute(
            "INSERT INTO skill_signatures (record_id, skill_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                signature.record_id,
                signature.skill_id,
                payload,
                signature.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write skill signature failed: {}", e))?;
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

    pub fn list_skill_signatures(
        &self,
        skill_id: Option<&str>,
    ) -> Result<Vec<SkillSignatureRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        if let Some(skill_id) = skill_id {
            let mut stmt = conn
                .prepare(
                    "SELECT payload_json FROM skill_signatures
                     WHERE skill_id=?1
                     ORDER BY created_at_us ASC",
                )
                .map_err(|e| format!("prepare list skill signatures failed: {}", e))?;
            let rows = stmt
                .query_map(params![skill_id], |row| row.get::<_, String>(0))
                .map_err(|e| format!("query skill signatures failed: {}", e))?;
            for row in rows {
                let payload = row.map_err(|e| format!("read skill signature row failed: {}", e))?;
                out.push(
                    serde_json::from_str(&payload)
                        .map_err(|e| format!("parse skill signature failed: {}", e))?,
                );
            }
        } else {
            let mut stmt = conn
                .prepare("SELECT payload_json FROM skill_signatures ORDER BY created_at_us ASC")
                .map_err(|e| format!("prepare list skill signatures failed: {}", e))?;
            let rows = stmt
                .query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| format!("query skill signatures failed: {}", e))?;
            for row in rows {
                let payload = row.map_err(|e| format!("read skill signature row failed: {}", e))?;
                out.push(
                    serde_json::from_str(&payload)
                        .map_err(|e| format!("parse skill signature failed: {}", e))?,
                );
            }
        }
        Ok(out)
    }

}
