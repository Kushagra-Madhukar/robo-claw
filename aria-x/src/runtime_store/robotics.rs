use super::*;

impl RuntimeStore {
    #[allow(dead_code)]
    pub fn upsert_ros2_bridge_profile(
        &self,
        profile: &Ros2BridgeProfile,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(profile)
            .map_err(|e| format!("serialize ros2 bridge profile failed: {}", e))?;
        conn.execute(
            "INSERT INTO ros2_bridge_profiles (profile_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(profile_id) DO UPDATE SET
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![profile.profile_id, payload, updated_at_us as i64],
        )
        .map_err(|e| format!("upsert ros2 bridge profile failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_ros2_bridge_profiles(
        &self,
        profile_id: Option<&str>,
    ) -> Result<Vec<Ros2BridgeProfile>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match profile_id {
            Some(profile_id) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM ros2_bridge_profiles
                         WHERE profile_id=?1 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare ros2 bridge profile query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![profile_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query ros2 bridge profiles failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read ros2 bridge profile row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse ros2 bridge profile failed: {}", e))?,
                    );
                }
            }
            None => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM ros2_bridge_profiles ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare ros2 bridge profile query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query ros2 bridge profiles failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read ros2 bridge profile row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse ros2 bridge profile failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn upsert_robot_runtime_state(
        &self,
        record: &RobotRuntimeRecord,
        updated_at_us: u64,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize robot runtime state failed: {}", e))?;
        conn.execute(
            "INSERT INTO robot_runtime_states (robot_id, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(robot_id) DO UPDATE SET
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![record.robot_id, payload, updated_at_us as i64],
        )
        .map_err(|e| format!("upsert robot runtime state failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_robot_runtime_states(
        &self,
        robot_id: Option<&str>,
    ) -> Result<Vec<RobotRuntimeRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match robot_id {
            Some(robot_id) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM robot_runtime_states
                         WHERE robot_id=?1 ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare robot runtime state query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![robot_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query robot runtime states failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read robot runtime state row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse robot runtime state failed: {}", e))?,
                    );
                }
            }
            None => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM robot_runtime_states ORDER BY updated_at_us DESC",
                    )
                    .map_err(|e| format!("prepare robot runtime state query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query robot runtime states failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read robot runtime state row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse robot runtime state failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }

    #[allow(dead_code)]
    pub fn append_robotics_simulation(
        &self,
        record: &RoboticsSimulationRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize robotics simulation failed: {}", e))?;
        conn.execute(
            "INSERT INTO robotics_simulations
             (simulation_id, robot_id, agent_id, session_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.simulation_id,
                record.robot_id,
                record.agent_id,
                record.session_id.map(|id| id.to_vec()),
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("append robotics simulation failed: {}", e))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn list_robotics_simulations(
        &self,
        robot_id: Option<&str>,
    ) -> Result<Vec<RoboticsSimulationRecord>, String> {
        let conn = self.connect()?;
        let mut out = Vec::new();
        match robot_id {
            Some(robot_id) => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM robotics_simulations
                         WHERE robot_id=?1 ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare robotics simulation query failed: {}", e))?;
                let rows = stmt
                    .query_map(params![robot_id], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query robotics simulations failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read robotics simulation row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse robotics simulation failed: {}", e))?,
                    );
                }
            }
            None => {
                let mut stmt = conn
                    .prepare(
                        "SELECT payload_json FROM robotics_simulations ORDER BY created_at_us DESC",
                    )
                    .map_err(|e| format!("prepare robotics simulation query failed: {}", e))?;
                let rows = stmt
                    .query_map([], |row| row.get::<_, String>(0))
                    .map_err(|e| format!("query robotics simulations failed: {}", e))?;
                for row in rows {
                    let payload =
                        row.map_err(|e| format!("read robotics simulation row failed: {}", e))?;
                    out.push(
                        serde_json::from_str(&payload)
                            .map_err(|e| format!("parse robotics simulation failed: {}", e))?,
                    );
                }
            }
        }
        Ok(out)
    }
}
