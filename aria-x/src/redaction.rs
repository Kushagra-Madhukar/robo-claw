#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelemetryRedactionProfile {
    TrustedLocalInspect,
    LocalExport,
    SharedExport,
}

fn is_sensitive_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    [
        "token",
        "secret",
        "password",
        "authorization",
        "cookie",
        "api_key",
        "master_key",
        "bearer",
        "session_key",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn is_shared_content_key(key: &str) -> bool {
    matches!(
        key,
        "user_input_summary"
            | "response_summary"
            | "user_request"
            | "system_prompt"
            | "rendered_prompt"
            | "history_messages"
            | "context_blocks"
    )
}

fn string_looks_sensitive(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with("sk-")
        || trimmed.starts_with("AIza")
        || trimmed.starts_with("ghp_")
        || trimmed.contains("Bearer ")
        || trimmed.contains("BEGIN PRIVATE KEY")
}

pub fn redact_json_value(
    value: &serde_json::Value,
    profile: TelemetryRedactionProfile,
    config: &TelemetryRedactionConfig,
) -> serde_json::Value {
    redact_json_value_with_key(value, None, profile, config)
}

fn redact_json_value_with_key(
    value: &serde_json::Value,
    key_hint: Option<&str>,
    profile: TelemetryRedactionProfile,
    config: &TelemetryRedactionConfig,
) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (key, child) in map {
                let lower = key.to_ascii_lowercase();
                if config.redact_secret_like_values && is_sensitive_key(&lower) {
                    out.insert(key.clone(), serde_json::Value::String("<redacted>".into()));
                    continue;
                }
                if matches!(profile, TelemetryRedactionProfile::SharedExport)
                    && config.redact_provider_payloads_in_shared_export
                    && lower == "provider_request_payload"
                {
                    out.insert(
                        key.clone(),
                        serde_json::Value::String("<redacted-provider-payload>".into()),
                    );
                    continue;
                }
                if matches!(profile, TelemetryRedactionProfile::SharedExport)
                    && config.redact_user_content_in_shared_export
                    && is_shared_content_key(&lower)
                {
                    out.insert(
                        key.clone(),
                        serde_json::Value::String("<redacted-content>".into()),
                    );
                    continue;
                }
                out.insert(
                    key.clone(),
                    redact_json_value_with_key(child, Some(&lower), profile, config),
                );
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| redact_json_value_with_key(item, key_hint, profile, config))
                .collect(),
        ),
        serde_json::Value::String(text)
            if config.redact_secret_like_values && string_looks_sensitive(text) =>
        {
            serde_json::Value::String("<redacted>".into())
        }
        serde_json::Value::String(_)
            if matches!(profile, TelemetryRedactionProfile::SharedExport)
                && config.redact_user_content_in_shared_export
                && key_hint.is_some_and(is_shared_content_key) =>
        {
            serde_json::Value::String("<redacted-content>".into())
        }
        _ => value.clone(),
    }
}
