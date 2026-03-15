use super::*;
use crate::runtime_resource_budget;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeWriteDurability {
    Critical,
    Normal,
    Deferred,
}

enum DeferredRuntimeWrite {
    ExecutionTrace {
        path: PathBuf,
        trace: ExecutionTrace,
    },
    LearningDerivativeEvent {
        path: PathBuf,
        event: LearningDerivativeEvent,
    },
}

fn deferred_runtime_writer() -> &'static Mutex<Option<mpsc::SyncSender<DeferredRuntimeWrite>>> {
    static WRITER: OnceLock<Mutex<Option<mpsc::SyncSender<DeferredRuntimeWrite>>>> =
        OnceLock::new();
    WRITER.get_or_init(|| Mutex::new(None))
}

fn ensure_deferred_runtime_writer() -> Result<mpsc::SyncSender<DeferredRuntimeWrite>, String> {
    let mut guard = deferred_runtime_writer()
        .lock()
        .map_err(|_| "lock deferred runtime writer failed".to_string())?;
    if let Some(writer) = guard.as_ref() {
        return Ok(writer.clone());
    }
    let (tx, rx) = mpsc::sync_channel::<DeferredRuntimeWrite>(256);
    std::thread::Builder::new()
        .name("aria-runtime-store-deferred".into())
        .spawn(move || {
            while let Ok(item) = rx.recv() {
                match item {
                    DeferredRuntimeWrite::ExecutionTrace { path, trace } => {
                        let _ = RuntimeStore { path }.record_execution_trace_immediate(&trace);
                    }
                    DeferredRuntimeWrite::LearningDerivativeEvent { path, event } => {
                        let _ = RuntimeStore { path }
                            .append_learning_derivative_event_immediate(&event);
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        })
        .map_err(|e| format!("spawn deferred runtime writer failed: {}", e))?;
    *guard = Some(tx.clone());
    Ok(tx)
}

impl RuntimeStore {
    pub fn record_execution_trace(&self, trace: &ExecutionTrace) -> Result<(), String> {
        if !runtime_resource_budget().learning_enabled {
            return Ok(());
        }
        self.record_execution_trace_with_durability(trace, RuntimeWriteDurability::Deferred)
    }

    fn record_execution_trace_immediate(&self, trace: &ExecutionTrace) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(trace)
            .map_err(|e| format!("serialize execution trace failed: {}", e))?;
        conn.execute(
            "INSERT INTO execution_traces
             (request_id, session_id, user_id, agent_id, channel, prompt_mode, task_fingerprint, outcome, payload_json, recorded_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(request_id) DO UPDATE SET
               session_id=excluded.session_id,
               user_id=excluded.user_id,
               agent_id=excluded.agent_id,
               channel=excluded.channel,
               prompt_mode=excluded.prompt_mode,
               task_fingerprint=excluded.task_fingerprint,
               outcome=excluded.outcome,
               payload_json=excluded.payload_json,
               recorded_at_us=excluded.recorded_at_us",
            params![
                trace.request_id,
                trace.session_id,
                trace.user_id,
                trace.agent_id,
                format!("{:?}", trace.channel),
                trace.prompt_mode,
                trace.task_fingerprint.key,
                trace_outcome_name(trace.outcome),
                payload,
                trace.recorded_at_us as i64,
            ],
        )
        .map_err(|e| format!("write execution trace failed: {}", e))?;
        Ok(())
    }

    pub fn record_execution_trace_with_durability(
        &self,
        trace: &ExecutionTrace,
        durability: RuntimeWriteDurability,
    ) -> Result<(), String> {
        if !runtime_resource_budget().learning_enabled {
            return Ok(());
        }
        match durability {
            RuntimeWriteDurability::Critical | RuntimeWriteDurability::Normal => {
                self.record_execution_trace_immediate(trace)
            }
            RuntimeWriteDurability::Deferred => ensure_deferred_runtime_writer()?
                .send(DeferredRuntimeWrite::ExecutionTrace {
                    path: self.path.clone(),
                    trace: trace.clone(),
                })
                .map_err(|e| format!("queue deferred execution trace failed: {}", e)),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_execution_traces_by_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Vec<ExecutionTrace>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM execution_traces
                 WHERE task_fingerprint=?1
                 ORDER BY recorded_at_us ASC",
            )
            .map_err(|e| format!("prepare list execution traces failed: {}", e))?;
        let rows = stmt
            .query_map(params![fingerprint], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query execution traces failed: {}", e))?;

        let mut traces = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read execution trace row failed: {}", e))?;
            let trace = serde_json::from_str(&payload)
                .map_err(|e| format!("parse execution trace failed: {}", e))?;
            traces.push(trace);
        }
        Ok(traces)
    }

    pub fn list_execution_traces_by_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<ExecutionTrace>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM execution_traces
                 WHERE session_id=?1
                 ORDER BY recorded_at_us ASC",
            )
            .map_err(|e| format!("prepare list execution traces by session failed: {}", e))?;
        let rows = stmt
            .query_map(params![session_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query execution traces by session failed: {}", e))?;

        let mut traces = Vec::new();
        for row in rows {
            let payload =
                row.map_err(|e| format!("read execution trace by session row failed: {}", e))?;
            let trace = serde_json::from_str(&payload)
                .map_err(|e| format!("parse execution trace by session failed: {}", e))?;
            traces.push(trace);
        }
        Ok(traces)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_execution_traces_by_session_and_fingerprint(
        &self,
        session_id: &str,
        fingerprint: &str,
    ) -> Result<Vec<ExecutionTrace>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM execution_traces
                 WHERE session_id=?1 AND task_fingerprint=?2
                 ORDER BY recorded_at_us ASC",
            )
            .map_err(|e| format!("prepare list execution traces by session failed: {}", e))?;
        let rows = stmt
            .query_map(params![session_id, fingerprint], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| format!("query execution traces by session failed: {}", e))?;

        let mut traces = Vec::new();
        for row in rows {
            let payload =
                row.map_err(|e| format!("read execution trace by session row failed: {}", e))?;
            let trace = serde_json::from_str(&payload)
                .map_err(|e| format!("parse execution trace by session failed: {}", e))?;
            traces.push(trace);
        }
        Ok(traces)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn record_reward_event(&self, reward: &RewardEvent) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(reward)
            .map_err(|e| format!("serialize reward event failed: {}", e))?;
        conn.execute(
            "INSERT INTO reward_events
             (event_id, request_id, session_id, kind, value, payload_json, recorded_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(event_id) DO UPDATE SET
               request_id=excluded.request_id,
               session_id=excluded.session_id,
               kind=excluded.kind,
               value=excluded.value,
               payload_json=excluded.payload_json,
               recorded_at_us=excluded.recorded_at_us",
            params![
                reward.event_id,
                reward.request_id,
                reward.session_id,
                reward_kind_name(reward.kind),
                reward.value,
                payload,
                reward.recorded_at_us as i64,
            ],
        )
        .map_err(|e| format!("write reward event failed: {}", e))?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn prune_learning_records(
        &self,
        max_trace_rows: u32,
        max_reward_rows: u32,
        max_derivative_rows: u32,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        conn.execute(
            "DELETE FROM execution_traces
             WHERE request_id IN (
                 SELECT request_id FROM execution_traces
                 ORDER BY recorded_at_us DESC
                 LIMIT -1 OFFSET ?1
             )",
            params![max_trace_rows as i64],
        )
        .map_err(|e| format!("prune execution traces failed: {}", e))?;
        conn.execute(
            "DELETE FROM reward_events
             WHERE event_id IN (
                 SELECT event_id FROM reward_events
                 ORDER BY recorded_at_us DESC
                 LIMIT -1 OFFSET ?1
             )",
            params![max_reward_rows as i64],
        )
        .map_err(|e| format!("prune reward events failed: {}", e))?;
        if max_derivative_rows > 0 {
            conn.execute(
                "DELETE FROM learning_derivative_events
                 WHERE event_id IN (
                     SELECT event_id FROM learning_derivative_events
                     ORDER BY created_at_us DESC
                     LIMIT -1 OFFSET ?1
                 )",
                params![max_derivative_rows as i64],
            )
            .map_err(|e| format!("prune learning derivative events failed: {}", e))?;
        }
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_reward_events_for_request(
        &self,
        request_id: &str,
    ) -> Result<Vec<RewardEvent>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM reward_events
                 WHERE request_id=?1
                 ORDER BY recorded_at_us ASC",
            )
            .map_err(|e| format!("prepare list reward events failed: {}", e))?;
        let rows = stmt
            .query_map(params![request_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query reward events failed: {}", e))?;

        let mut rewards = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read reward event row failed: {}", e))?;
            let reward = serde_json::from_str(&payload)
                .map_err(|e| format!("parse reward event failed: {}", e))?;
            rewards.push(reward);
        }
        Ok(rewards)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn build_replay_samples_for_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Vec<ReplaySample>, String> {
        let traces = self.list_execution_traces_by_fingerprint(fingerprint)?;
        let mut samples = Vec::with_capacity(traces.len());
        for trace in traces {
            let rewards = self.list_reward_events_for_request(&trace.request_id)?;
            let reward_score = rewards.iter().map(|reward| reward.value).sum();
            samples.push(ReplaySample {
                trace,
                rewards,
                reward_score,
            });
        }
        Ok(samples)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn summarize_fingerprint_for_evaluation(
        &self,
        fingerprint: &str,
    ) -> Result<Option<FingerprintEvaluationSummary>, String> {
        let samples = self.build_replay_samples_for_fingerprint(fingerprint)?;
        if samples.is_empty() {
            return Ok(None);
        }

        let mut summary = FingerprintEvaluationSummary {
            task_fingerprint: fingerprint.to_string(),
            trace_count: samples.len() as u32,
            success_count: 0,
            approval_required_count: 0,
            clarification_count: 0,
            failure_count: 0,
            cumulative_reward: 0,
            latest_recorded_at_us: 0,
        };

        for sample in samples {
            match sample.trace.outcome {
                TraceOutcome::Succeeded => summary.success_count += 1,
                TraceOutcome::ApprovalRequired => summary.approval_required_count += 1,
                TraceOutcome::ClarificationRequired => summary.clarification_count += 1,
                TraceOutcome::Failed => summary.failure_count += 1,
            }
            summary.cumulative_reward += sample.reward_score;
            summary.latest_recorded_at_us = summary
                .latest_recorded_at_us
                .max(sample.trace.recorded_at_us);
        }

        Ok(Some(summary))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_fingerprint_clusters(&self) -> Result<Vec<FingerprintCluster>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT task_fingerprint FROM execution_traces ORDER BY recorded_at_us ASC",
            )
            .map_err(|e| format!("prepare list fingerprint clusters failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query fingerprint clusters failed: {}", e))?;

        let mut clusters = Vec::new();
        for row in rows {
            let fingerprint = row.map_err(|e| format!("read fingerprint row failed: {}", e))?;
            let traces = self.list_execution_traces_by_fingerprint(&fingerprint)?;
            let Some(summary) = self.summarize_fingerprint_for_evaluation(&fingerprint)? else {
                continue;
            };

            let mut agents = std::collections::BTreeSet::new();
            let mut modes = std::collections::BTreeSet::new();
            for trace in traces {
                agents.insert(trace.agent_id);
                modes.insert(trace.prompt_mode);
            }
            clusters.push(FingerprintCluster {
                summary,
                top_agents: agents.into_iter().collect(),
                top_prompt_modes: modes.into_iter().collect(),
            });
        }
        Ok(clusters)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn upsert_candidate_artifact(
        &self,
        record: &CandidateArtifactRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize candidate artifact failed: {}", e))?;
        conn.execute(
            "INSERT INTO candidate_artifacts
             (candidate_id, task_fingerprint, kind, status, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(candidate_id) DO UPDATE SET
               task_fingerprint=excluded.task_fingerprint,
               kind=excluded.kind,
               status=excluded.status,
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![
                record.candidate_id,
                record.task_fingerprint,
                candidate_kind_name(record.kind),
                candidate_status_name(record.status),
                payload,
                record.updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("write candidate artifact failed: {}", e))?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_candidate_artifacts_for_fingerprint(
        &self,
        fingerprint: &str,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM candidate_artifacts
                 WHERE task_fingerprint=?1
                 ORDER BY updated_at_us ASC",
            )
            .map_err(|e| format!("prepare list candidate artifacts failed: {}", e))?;
        let rows = stmt
            .query_map(params![fingerprint], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query candidate artifacts failed: {}", e))?;

        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read candidate artifact row failed: {}", e))?;
            let record = serde_json::from_str(&payload)
                .map_err(|e| format!("parse candidate artifact failed: {}", e))?;
            out.push(record);
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_promoted_candidates_for_request(
        &self,
        agent_id: &str,
        prompt_mode: &str,
        request_text: &str,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM candidate_artifacts
                 WHERE status=?1
                 ORDER BY updated_at_us ASC",
            )
            .map_err(|e| format!("prepare list promoted candidates failed: {}", e))?;
        let rows = stmt
            .query_map(
                params![candidate_status_name(
                    aria_learning::CandidateArtifactStatus::Promoted
                )],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| format!("query promoted candidates failed: {}", e))?;

        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read promoted candidate row failed: {}", e))?;
            let record: CandidateArtifactRecord = serde_json::from_str(&payload)
                .map_err(|e| format!("parse promoted candidate failed: {}", e))?;
            if task_fingerprint_matches_request(
                &record.task_fingerprint,
                agent_id,
                prompt_mode,
                request_text,
            ) {
                out.push(record);
            }
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn upsert_selector_model(&self, record: &SelectorModelRecord) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize selector model failed: {}", e))?;
        conn.execute(
            "INSERT INTO selector_models
             (model_id, task_fingerprint, kind, payload_json, updated_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(model_id) DO UPDATE SET
               task_fingerprint=excluded.task_fingerprint,
               kind=excluded.kind,
               payload_json=excluded.payload_json,
               updated_at_us=excluded.updated_at_us",
            params![
                record.model_id,
                record.task_fingerprint,
                selector_model_kind_name(record.kind),
                payload,
                record.updated_at_us as i64,
            ],
        )
        .map_err(|e| format!("write selector model failed: {}", e))?;
        Ok(())
    }

    pub fn append_learning_derivative_event(
        &self,
        event: &LearningDerivativeEvent,
    ) -> Result<(), String> {
        if !runtime_resource_budget().learning_enabled {
            return Ok(());
        }
        self.append_learning_derivative_event_with_durability(
            event,
            RuntimeWriteDurability::Deferred,
        )
    }

    fn append_learning_derivative_event_immediate(
        &self,
        event: &LearningDerivativeEvent,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(event)
            .map_err(|e| format!("serialize learning derivative event failed: {}", e))?;
        conn.execute(
            "INSERT INTO learning_derivative_events
             (event_id, task_fingerprint, kind, artifact_id, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event.event_id,
                event.task_fingerprint,
                learning_derivative_kind_name(event.kind),
                event.artifact_id,
                payload,
                event.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write learning derivative event failed: {}", e))?;
        Ok(())
    }

    pub fn append_learning_derivative_event_with_durability(
        &self,
        event: &LearningDerivativeEvent,
        durability: RuntimeWriteDurability,
    ) -> Result<(), String> {
        if !runtime_resource_budget().learning_enabled {
            return Ok(());
        }
        match durability {
            RuntimeWriteDurability::Critical | RuntimeWriteDurability::Normal => {
                self.append_learning_derivative_event_immediate(event)
            }
            RuntimeWriteDurability::Deferred => ensure_deferred_runtime_writer()?
                .send(DeferredRuntimeWrite::LearningDerivativeEvent {
                    path: self.path.clone(),
                    event: event.clone(),
                })
                .map_err(|e| format!("queue deferred learning derivative event failed: {}", e)),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_learning_derivative_events(
        &self,
        task_fingerprint: &str,
    ) -> Result<Vec<LearningDerivativeEvent>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM learning_derivative_events
                 WHERE task_fingerprint=?1
                 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list learning derivative events failed: {}", e))?;
        let rows = stmt
            .query_map(params![task_fingerprint], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query learning derivative events failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload =
                row.map_err(|e| format!("read learning derivative event row failed: {}", e))?;
            let event = serde_json::from_str(&payload)
                .map_err(|e| format!("parse learning derivative event failed: {}", e))?;
            out.push(event);
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn learning_metrics_snapshot(&self) -> Result<LearningMetricsSnapshot, String> {
        let conn = self.connect()?;
        let trace_count = count_rows(&conn, "execution_traces")?;
        let reward_count = count_rows(&conn, "reward_events")?;
        let candidate_count = count_rows(&conn, "candidate_artifacts")?;
        let selector_model_count = count_rows(&conn, "selector_models")?;
        let derivative_event_count = count_rows(&conn, "learning_derivative_events")?;
        let promoted_candidate_count = conn
            .query_row(
                "SELECT COUNT(*) FROM candidate_artifacts WHERE status='promoted'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|e| format!("count promoted candidates failed: {}", e))?;
        Ok(LearningMetricsSnapshot {
            trace_count: trace_count as u64,
            reward_count: reward_count as u64,
            candidate_count: candidate_count as u64,
            promoted_candidate_count: promoted_candidate_count as u64,
            selector_model_count: selector_model_count as u64,
            derivative_event_count: derivative_event_count as u64,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_selector_models_for_request(
        &self,
        agent_id: &str,
        prompt_mode: &str,
        request_text: &str,
    ) -> Result<Vec<SelectorModelRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare("SELECT payload_json FROM selector_models ORDER BY updated_at_us ASC")
            .map_err(|e| format!("prepare list selector models failed: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query selector models failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read selector model row failed: {}", e))?;
            let record: SelectorModelRecord = serde_json::from_str(&payload)
                .map_err(|e| format!("parse selector model failed: {}", e))?;
            if task_fingerprint_matches_request(
                &record.task_fingerprint,
                agent_id,
                prompt_mode,
                request_text,
            ) {
                out.push(record);
            }
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn read_candidate_artifact(
        &self,
        candidate_id: &str,
    ) -> Result<CandidateArtifactRecord, String> {
        let conn = self.connect()?;
        let payload: String = conn
            .query_row(
                "SELECT payload_json FROM candidate_artifacts WHERE candidate_id=?1",
                params![candidate_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("read candidate artifact failed: {}", e))?;
        serde_json::from_str(&payload)
            .map_err(|e| format!("parse candidate artifact failed: {}", e))
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn append_candidate_evaluation_run(
        &self,
        run: &CandidateEvaluationRun,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(run)
            .map_err(|e| format!("serialize candidate evaluation run failed: {}", e))?;
        conn.execute(
            "INSERT INTO candidate_eval_runs
             (run_id, candidate_id, task_fingerprint, score, passed, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.run_id,
                run.candidate_id,
                run.task_fingerprint,
                run.score,
                if run.passed { 1 } else { 0 },
                payload,
                run.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write candidate evaluation run failed: {}", e))?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_candidate_evaluation_runs(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<CandidateEvaluationRun>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM candidate_eval_runs
                 WHERE candidate_id=?1
                 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list candidate eval runs failed: {}", e))?;
        let rows = stmt
            .query_map(params![candidate_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query candidate eval runs failed: {}", e))?;

        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read candidate eval run row failed: {}", e))?;
            let run = serde_json::from_str(&payload)
                .map_err(|e| format!("parse candidate eval run failed: {}", e))?;
            out.push(run);
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn append_candidate_promotion_record(
        &self,
        record: &CandidatePromotionRecord,
    ) -> Result<(), String> {
        let conn = self.connect()?;
        let payload = serde_json::to_string(record)
            .map_err(|e| format!("serialize candidate promotion record failed: {}", e))?;
        conn.execute(
            "INSERT INTO candidate_promotions
             (promotion_id, candidate_id, task_fingerprint, action, status, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.promotion_id,
                record.candidate_id,
                record.task_fingerprint,
                candidate_promotion_action_name(record.action),
                candidate_promotion_status_name(record.status),
                payload,
                record.created_at_us as i64,
            ],
        )
        .map_err(|e| format!("write candidate promotion record failed: {}", e))?;
        Ok(())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn list_candidate_promotion_records(
        &self,
        candidate_id: &str,
    ) -> Result<Vec<CandidatePromotionRecord>, String> {
        let conn = self.connect()?;
        let mut stmt = conn
            .prepare(
                "SELECT payload_json FROM candidate_promotions
                 WHERE candidate_id=?1
                 ORDER BY created_at_us ASC",
            )
            .map_err(|e| format!("prepare list candidate promotions failed: {}", e))?;
        let rows = stmt
            .query_map(params![candidate_id], |row| row.get::<_, String>(0))
            .map_err(|e| format!("query candidate promotions failed: {}", e))?;
        let mut out = Vec::new();
        for row in rows {
            let payload = row.map_err(|e| format!("read candidate promotion row failed: {}", e))?;
            let record = serde_json::from_str(&payload)
                .map_err(|e| format!("parse candidate promotion record failed: {}", e))?;
            out.push(record);
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn synthesize_candidate_artifacts(
        &self,
        now_us: u64,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let clusters = self.list_fingerprint_clusters()?;
        let mut created = Vec::new();
        for cluster in clusters {
            let samples =
                self.build_replay_samples_for_fingerprint(&cluster.summary.task_fingerprint)?;
            let Some(candidate) = synthesize_candidate_for_cluster(&cluster, &samples, now_us)
            else {
                continue;
            };
            self.upsert_candidate_artifact(&candidate)?;
            let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
                event_id: format!("derivative:candidate:{}:{}", candidate.candidate_id, now_us),
                task_fingerprint: candidate.task_fingerprint.clone(),
                kind: LearningDerivativeKind::CandidateSynthesis,
                artifact_id: candidate.candidate_id.clone(),
                notes: candidate.summary.clone(),
                created_at_us: now_us,
            });
            created.push(candidate);
        }
        Ok(created)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn build_prompt_optimization_datasets(
        &self,
        min_examples: u32,
    ) -> Result<Vec<PromptOptimizationDataset>, String> {
        let clusters = self.list_fingerprint_clusters()?;
        let mut datasets = Vec::new();
        for cluster in clusters {
            let samples =
                self.build_replay_samples_for_fingerprint(&cluster.summary.task_fingerprint)?;
            let Some(dataset) =
                build_prompt_optimization_dataset(&cluster.summary.task_fingerprint, &samples)
            else {
                continue;
            };
            if dataset.success_count >= min_examples {
                datasets.push(dataset);
            }
        }
        Ok(datasets)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn compile_prompt_optimization_candidates(
        &self,
        now_us: u64,
        min_examples: u32,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let datasets = self.build_prompt_optimization_datasets(min_examples)?;
        let mut created = Vec::new();
        for dataset in datasets {
            let candidate = compile_prompt_candidate_from_dataset(&dataset, now_us);
            self.upsert_candidate_artifact(&candidate)?;
            let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
                event_id: format!("derivative:prompt:{}:{}", candidate.candidate_id, now_us),
                task_fingerprint: candidate.task_fingerprint.clone(),
                kind: LearningDerivativeKind::PromptCompile,
                artifact_id: candidate.candidate_id.clone(),
                notes: candidate.summary.clone(),
                created_at_us: now_us,
            });
            created.push(candidate);
        }
        Ok(created)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn build_macro_compilation_datasets(
        &self,
        min_examples: u32,
    ) -> Result<Vec<MacroCompilationDataset>, String> {
        let clusters = self.list_fingerprint_clusters()?;
        let mut datasets = Vec::new();
        for cluster in clusters {
            let samples =
                self.build_replay_samples_for_fingerprint(&cluster.summary.task_fingerprint)?;
            let Some(dataset) =
                build_macro_compilation_dataset(&cluster.summary.task_fingerprint, &samples)
            else {
                continue;
            };
            if dataset.success_count >= min_examples {
                datasets.push(dataset);
            }
        }
        Ok(datasets)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn compile_macro_candidates(
        &self,
        now_us: u64,
        min_examples: u32,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let datasets = self.build_macro_compilation_datasets(min_examples)?;
        let mut created = Vec::new();
        for dataset in datasets {
            let candidate = compile_macro_candidate_from_dataset(&dataset, now_us);
            self.upsert_candidate_artifact(&candidate)?;
            let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
                event_id: format!("derivative:macro:{}:{}", candidate.candidate_id, now_us),
                task_fingerprint: candidate.task_fingerprint.clone(),
                kind: LearningDerivativeKind::MacroCompile,
                artifact_id: candidate.candidate_id.clone(),
                notes: candidate.summary.clone(),
                created_at_us: now_us,
            });
            created.push(candidate);
        }
        Ok(created)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn build_wasm_compilation_datasets(
        &self,
        min_examples: u32,
    ) -> Result<Vec<WasmCompilationDataset>, String> {
        let clusters = self.list_fingerprint_clusters()?;
        let mut datasets = Vec::new();
        for cluster in clusters {
            let samples =
                self.build_replay_samples_for_fingerprint(&cluster.summary.task_fingerprint)?;
            let Some(dataset) =
                build_wasm_compilation_dataset(&cluster.summary.task_fingerprint, &samples)
            else {
                continue;
            };
            if dataset.success_count >= min_examples {
                datasets.push(dataset);
            }
        }
        Ok(datasets)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn compile_wasm_candidates(
        &self,
        now_us: u64,
        min_examples: u32,
    ) -> Result<Vec<CandidateArtifactRecord>, String> {
        let datasets = self.build_wasm_compilation_datasets(min_examples)?;
        let mut created = Vec::new();
        for dataset in datasets {
            let candidate = compile_wasm_candidate_from_dataset(&dataset, now_us);
            self.upsert_candidate_artifact(&candidate)?;
            let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
                event_id: format!("derivative:wasm:{}:{}", candidate.candidate_id, now_us),
                task_fingerprint: candidate.task_fingerprint.clone(),
                kind: LearningDerivativeKind::WasmCompile,
                artifact_id: candidate.candidate_id.clone(),
                notes: candidate.summary.clone(),
                created_at_us: now_us,
            });
            created.push(candidate);
        }
        Ok(created)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn synthesize_selector_models(
        &self,
        now_us: u64,
    ) -> Result<Vec<SelectorModelRecord>, String> {
        let clusters = self.list_fingerprint_clusters()?;
        let mut out = Vec::new();
        for cluster in clusters {
            let samples =
                self.build_replay_samples_for_fingerprint(&cluster.summary.task_fingerprint)?;
            let models = train_selector_models_for_samples(
                &cluster.summary.task_fingerprint,
                &samples,
                now_us,
            );
            for model in models {
                self.upsert_selector_model(&model)?;
                let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
                    event_id: format!("derivative:selector:{}:{}", model.model_id, now_us),
                    task_fingerprint: model.task_fingerprint.clone(),
                    kind: LearningDerivativeKind::SelectorSynthesis,
                    artifact_id: model.model_id.clone(),
                    notes: format!("selector_kind={}", selector_model_kind_name(model.kind)),
                    created_at_us: now_us,
                });
                out.push(model);
            }
        }
        Ok(out)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn evaluate_candidate_artifact(
        &self,
        candidate_id: &str,
        now_us: u64,
    ) -> Result<CandidateEvaluationRun, String> {
        let candidate = self.read_candidate_artifact(candidate_id)?;
        let samples = self.build_replay_samples_for_fingerprint(&candidate.task_fingerprint)?;
        let run = evaluate_candidate_against_replay(&candidate, &samples, now_us);
        self.append_candidate_evaluation_run(&run)?;
        Ok(run)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn apply_candidate_promotion(
        &self,
        candidate_id: &str,
        capability_profiles: &[AgentCapabilityProfile],
        approved_by: Option<String>,
        action: CandidatePromotionAction,
        now_us: u64,
    ) -> Result<CandidatePromotionRecord, String> {
        let candidate = self.read_candidate_artifact(candidate_id)?;
        let latest_eval = self
            .list_candidate_evaluation_runs(candidate_id)?
            .into_iter()
            .last();
        let record = build_candidate_promotion_record(
            &candidate,
            latest_eval.as_ref(),
            capability_profiles,
            action,
            approved_by,
            now_us,
        );
        self.append_candidate_promotion_record(&record)?;

        if record.status == CandidatePromotionStatus::Applied {
            let next_status = match action {
                CandidatePromotionAction::Promote => {
                    aria_learning::CandidateArtifactStatus::Promoted
                }
                CandidatePromotionAction::Rollback => {
                    aria_learning::CandidateArtifactStatus::Rejected
                }
            };
            let mut updated = candidate;
            updated.status = next_status;
            updated.updated_at_us = now_us;
            self.upsert_candidate_artifact(&updated)?;
        }

        let _ = self.append_learning_derivative_event(&LearningDerivativeEvent {
            event_id: format!("derivative:promotion:{}:{}", record.promotion_id, now_us),
            task_fingerprint: record.task_fingerprint.clone(),
            kind: match action {
                CandidatePromotionAction::Promote => LearningDerivativeKind::Promotion,
                CandidatePromotionAction::Rollback => LearningDerivativeKind::Rollback,
            },
            artifact_id: record.candidate_id.clone(),
            notes: record.notes.clone(),
            created_at_us: now_us,
        });

        Ok(record)
    }
}
