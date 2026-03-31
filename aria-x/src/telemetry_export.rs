#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryExportScope {
    Local,
    Shared,
}

#[derive(Debug, serde::Serialize)]
struct TelemetryExportBundle {
    generated_at_us: u64,
    scope: TelemetryExportScope,
    learning_metrics: aria_learning::LearningMetricsSnapshot,
    execution_traces: Vec<serde_json::Value>,
    reward_events: Vec<serde_json::Value>,
    retrieval_traces: Vec<serde_json::Value>,
    context_inspections: Vec<serde_json::Value>,
    streaming_decision_audits: Vec<serde_json::Value>,
    repair_fallback_audits: Vec<serde_json::Value>,
    channel_health_snapshots: Vec<serde_json::Value>,
    operational_alert_snapshots: Vec<serde_json::Value>,
}

fn telemetry_redaction_profile_for_scope(scope: TelemetryExportScope) -> TelemetryRedactionProfile {
    match scope {
        TelemetryExportScope::Local => TelemetryRedactionProfile::LocalExport,
        TelemetryExportScope::Shared => TelemetryRedactionProfile::SharedExport,
    }
}

fn resolve_telemetry_output_dir(
    config: &ResolvedAppConfig,
    output_dir_override: Option<&Path>,
) -> PathBuf {
    if let Some(path) = output_dir_override {
        return path.to_path_buf();
    }
    let configured = PathBuf::from(&config.telemetry.exporters.output_dir);
    if configured.is_absolute() {
        configured
    } else {
        config
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(configured)
    }
}

fn build_telemetry_export_bundle(
    config: &ResolvedAppConfig,
    scope: TelemetryExportScope,
) -> Result<TelemetryExportBundle, String> {
    let store = RuntimeStore::for_sessions_dir(Path::new(&config.ssmu.sessions_dir));
    let redaction = &config.telemetry.redaction;
    let profile = telemetry_redaction_profile_for_scope(scope);
    let redact = |value: serde_json::Value| redact_json_value(&value, profile, redaction);

    let execution_traces = store
        .list_all_execution_traces()?
        .into_iter()
        .map(|trace| {
            serde_json::to_value(trace)
                .map(redact)
                .map_err(|e| format!("serialize execution trace failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let reward_events = store
        .list_all_reward_events()?
        .into_iter()
        .map(|event| {
            serde_json::to_value(event)
                .map(redact)
                .map_err(|e| format!("serialize reward event failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let retrieval_traces = store
        .list_retrieval_traces(None, None)?
        .into_iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(redact)
                .map_err(|e| format!("serialize retrieval trace failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let context_inspections = store
        .list_context_inspections(None, None)?
        .into_iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(redact)
                .map_err(|e| format!("serialize context inspection failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let streaming_decision_audits = store
        .list_streaming_decision_audits(None, None)?
        .into_iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(redact)
                .map_err(|e| format!("serialize streaming decision audit failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let repair_fallback_audits = store
        .list_repair_fallback_audits(None, None)?
        .into_iter()
        .map(|record| {
            serde_json::to_value(record)
                .map(redact)
                .map_err(|e| format!("serialize repair fallback audit failed: {}", e))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let channel_health_snapshots = store
        .list_channel_health_snapshots(250)?
        .into_iter()
        .map(redact)
        .collect::<Vec<_>>();
    let operational_alert_snapshots = store
        .list_operational_alert_snapshots(250)?
        .into_iter()
        .map(redact)
        .collect::<Vec<_>>();

    Ok(TelemetryExportBundle {
        generated_at_us: chrono::Utc::now().timestamp_micros() as u64,
        scope,
        learning_metrics: store.learning_metrics_snapshot()?,
        execution_traces,
        reward_events,
        retrieval_traces,
        context_inspections,
        streaming_decision_audits,
        repair_fallback_audits,
        channel_health_snapshots,
        operational_alert_snapshots,
    })
}

fn export_telemetry_bundle(
    config: &ResolvedAppConfig,
    scope: TelemetryExportScope,
    output_dir_override: Option<&Path>,
) -> Result<String, String> {
    if !config.telemetry.exporters.enabled {
        return Err("telemetry exporters are disabled in config".into());
    }
    let output_dir = resolve_telemetry_output_dir(config, output_dir_override);
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        format!(
            "create telemetry output dir '{}' failed: {}",
            output_dir.display(),
            e
        )
    })?;
    let bundle = build_telemetry_export_bundle(config, scope)?;
    let timestamp = bundle.generated_at_us;
    let mut written = Vec::new();

    if config.telemetry.exporters.write_json_bundle {
        let bundle_path = output_dir.join(format!("telemetry-export-{}.json", timestamp));
        let body = serde_json::to_vec_pretty(&bundle)
            .map_err(|e| format!("serialize telemetry bundle failed: {}", e))?;
        std::fs::write(&bundle_path, body)
            .map_err(|e| format!("write telemetry bundle failed: {}", e))?;
        written.push(bundle_path);
    }

    if config.telemetry.exporters.write_jsonl {
        let jsonl_path = output_dir.join(format!("telemetry-export-{}.jsonl", timestamp));
        let mut lines = Vec::new();
        for (kind, items) in [
            ("execution_trace", &bundle.execution_traces),
            ("reward_event", &bundle.reward_events),
            ("retrieval_trace", &bundle.retrieval_traces),
            ("context_inspection", &bundle.context_inspections),
            ("streaming_decision_audit", &bundle.streaming_decision_audits),
            ("repair_fallback_audit", &bundle.repair_fallback_audits),
            ("channel_health_snapshot", &bundle.channel_health_snapshots),
            ("operational_alert_snapshot", &bundle.operational_alert_snapshots),
        ] {
            for item in items {
                lines.push(
                    serde_json::to_string(&serde_json::json!({
                        "kind": kind,
                        "scope": bundle.scope,
                        "payload": item,
                    }))
                    .map_err(|e| format!("serialize telemetry jsonl line failed: {}", e))?,
                );
            }
        }
        std::fs::write(&jsonl_path, lines.join("\n"))
            .map_err(|e| format!("write telemetry jsonl failed: {}", e))?;
        written.push(jsonl_path);
    }

    Ok(format!(
        "Telemetry export complete.\nscope: {:?}\noutput_dir: {}\nfiles:\n{}",
        scope,
        output_dir.display(),
        written
            .iter()
            .map(|path| format!("- {}", path.display()))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}
