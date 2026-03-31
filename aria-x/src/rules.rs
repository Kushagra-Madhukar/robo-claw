fn hiveclaw_project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "anima", "hiveclaw")
}

fn legacy_hiveclaw_project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("com", "anima", "aria")
}

fn rule_scope_precedence(scope: aria_core::RuleScope) -> u8 {
    match scope {
        aria_core::RuleScope::Org => 0,
        aria_core::RuleScope::User => 1,
        aria_core::RuleScope::Project => 2,
        aria_core::RuleScope::Path => 3,
    }
}

fn build_rule_entry_from_file(
    path: &Path,
    workspace_root: Option<&Path>,
    scope: aria_core::RuleScope,
    source_kind: aria_core::RuleSourceKind,
    updated_at_us: u64,
) -> Result<aria_core::RuleEntry, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|e| format!("read rule file {} failed: {}", path.display(), e))?;
    let source_path = path.to_string_lossy().to_string();
    let workspace_root_string = workspace_root.map(|root| root.to_string_lossy().to_string());
    let applies_to_path = if matches!(scope, aria_core::RuleScope::Path) {
        workspace_root
            .and_then(|root| path.parent().and_then(|parent| parent.strip_prefix(root).ok()))
            .map(|relative| relative.to_string_lossy().to_string())
            .filter(|relative| !relative.is_empty())
    } else {
        None
    };
    let title = match scope {
        aria_core::RuleScope::Org => "Organization rules".to_string(),
        aria_core::RuleScope::User => "User rules".to_string(),
        aria_core::RuleScope::Project => path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("Project rules from {}", name))
            .unwrap_or_else(|| "Project rules".to_string()),
        aria_core::RuleScope::Path => {
            let relative = applies_to_path
                .as_deref()
                .unwrap_or(".");
            format!("Path rules for {}", relative)
        }
    };
    let stable_scope = match scope {
        aria_core::RuleScope::Org => "org",
        aria_core::RuleScope::User => "user",
        aria_core::RuleScope::Project => "project",
        aria_core::RuleScope::Path => "path",
    };
    Ok(aria_core::RuleEntry {
        rule_id: format!("{}::{}", stable_scope, source_path.replace('\\', "/")),
        scope,
        source_kind,
        workspace_root: workspace_root_string,
        source_path: Some(source_path),
        applies_to_path,
        title,
        body,
        updated_at_us,
    })
}

fn discover_global_rule_entries(
    updated_at_us: u64,
) -> Result<Vec<aria_core::RuleEntry>, String> {
    let mut out = Vec::new();
    let candidates = [
        (
            std::env::var("HIVECLAW_ORG_RULES_PATH")
                .ok()
                .or_else(|| std::env::var("ARIA_ORG_RULES_PATH").ok())
                .map(std::path::PathBuf::from)
                .or_else(|| hiveclaw_project_dirs().map(|dirs| dirs.config_dir().join("rules/org.md")))
                .or_else(|| legacy_hiveclaw_project_dirs().map(|dirs| dirs.config_dir().join("rules/org.md"))),
            aria_core::RuleScope::Org,
            aria_core::RuleSourceKind::OrgRulesFile,
        ),
        (
            std::env::var("HIVECLAW_USER_RULES_PATH")
                .ok()
                .or_else(|| std::env::var("ARIA_USER_RULES_PATH").ok())
                .map(std::path::PathBuf::from)
                .or_else(|| hiveclaw_project_dirs().map(|dirs| dirs.config_dir().join("rules/user.md")))
                .or_else(|| legacy_hiveclaw_project_dirs().map(|dirs| dirs.config_dir().join("rules/user.md"))),
            aria_core::RuleScope::User,
            aria_core::RuleSourceKind::UserRulesFile,
        ),
    ];
    for (path, scope, source_kind) in candidates {
        if let Some(path) = path.filter(|candidate| candidate.is_file()) {
            out.push(build_rule_entry_from_file(
                &path,
                None,
                scope,
                source_kind,
                updated_at_us,
            )?);
        }
    }
    Ok(out)
}

fn workspace_rule_source_kind(path: &Path) -> Option<aria_core::RuleSourceKind> {
    match path
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
        .as_deref()
    {
        Some("hiveclaw.md") => Some(aria_core::RuleSourceKind::HiveClaw),
        Some("agents.md") => Some(aria_core::RuleSourceKind::AgentsMd),
        Some("claude.md") => Some(aria_core::RuleSourceKind::ClaudeMd),
        _ => None,
    }
}

fn discover_workspace_rule_entries(
    workspace_root: &Path,
    updated_at_us: u64,
) -> Result<Vec<aria_core::RuleEntry>, String> {
    let mut entries = Vec::new();
    if !workspace_root.exists() || !workspace_root.is_dir() {
        return Ok(entries);
    }
    let mut stack = vec![workspace_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let read_dir = std::fs::read_dir(&dir)
            .map_err(|e| format!("read rule dir {} failed: {}", dir.display(), e))?;
        for entry in read_dir {
            let entry = entry.map_err(|e| format!("read rule entry failed: {}", e))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| format!("read rule file type failed for {}: {}", path.display(), e))?;
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(source_kind) = workspace_rule_source_kind(&path) else {
                continue;
            };
            let scope = match path.parent().and_then(|parent| parent.strip_prefix(workspace_root).ok()) {
                Some(relative) if !relative.as_os_str().is_empty() => aria_core::RuleScope::Path,
                _ => aria_core::RuleScope::Project,
            };
            entries.push(build_rule_entry_from_file(
                &path,
                Some(workspace_root),
                scope,
                source_kind,
                updated_at_us,
            )?);
        }
    }
    entries.sort_by(|left, right| {
        rule_scope_precedence(left.scope)
            .cmp(&rule_scope_precedence(right.scope))
            .then_with(|| left.source_path.cmp(&right.source_path))
    });
    Ok(entries)
}

fn extract_path_candidates(request_text: &str) -> Vec<String> {
    request_text
        .split_whitespace()
        .map(|token| token.trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '[' | ']')))
        .filter(|token| token.contains('/') || token.contains('\\') || token.ends_with(".md") || token.ends_with(".rs") || token.ends_with(".ts") || token.ends_with(".js") || token.ends_with(".toml"))
        .map(|token| token.trim_end_matches('.').to_string())
        .filter(|token| !token.is_empty())
        .collect()
}

fn resolve_rule_target_path(
    request_text: &str,
    working_set: Option<&aria_core::WorkingSet>,
    workspace_roots: &[String],
) -> Option<String> {
    let lower = request_text.to_ascii_lowercase();
    let from_working_set = working_set.and_then(|set| {
        let mut best: Option<(String, f32)> = None;
        for entry in &set.entries {
            let Some(locator) = &entry.locator else {
                continue;
            };
            let locator_lower = locator.to_ascii_lowercase();
            let basename = Path::new(locator)
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.to_ascii_lowercase());
            let mut score = 0.0;
            if lower.contains(&locator_lower) {
                score += 2.0;
            }
            if let Some(base) = &basename {
                if lower.contains(base) {
                    score += 1.0;
                }
            }
            if score > 0.0 {
                match &best {
                    Some((_, current)) if *current >= score => {}
                    _ => best = Some((locator.clone(), score)),
                }
            }
        }
        best.map(|(locator, _)| locator)
    });
    if from_working_set.is_some() {
        return from_working_set;
    }

    for candidate in extract_path_candidates(request_text) {
        let candidate_path = Path::new(&candidate);
        if candidate_path.is_absolute() {
            return Some(candidate);
        }
        for root in workspace_roots {
            let joined = Path::new(root).join(&candidate);
            if joined.exists() || joined.starts_with(root) {
                return Some(joined.to_string_lossy().to_string());
            }
        }
    }
    None
}

fn path_rule_matches(
    entry: &aria_core::RuleEntry,
    resolved_target_path: Option<&str>,
) -> bool {
    let Some(target_path) = resolved_target_path else {
        return false;
    };
    let Some(workspace_root) = entry.workspace_root.as_deref() else {
        return false;
    };
    let Some(applies_to_path) = entry.applies_to_path.as_deref() else {
        return false;
    };
    let target = std::path::Path::new(target_path);
    let root = std::path::Path::new(workspace_root);
    let Ok(relative) = target.strip_prefix(root) else {
        return false;
    };
    let relative = relative.to_string_lossy().replace('\\', "/");
    relative == applies_to_path || relative.starts_with(&format!("{}/", applies_to_path))
}

fn build_rule_resolution(
    workspace_roots: &[String],
    request_text: &str,
    working_set: Option<&aria_core::WorkingSet>,
    updated_at_us: u64,
) -> Result<aria_core::RuleResolution, String> {
    let mut entries = discover_global_rule_entries(updated_at_us)?;
    for root in workspace_roots {
        let path = Path::new(root);
        entries.extend(discover_workspace_rule_entries(path, updated_at_us)?);
    }
    entries.sort_by(|left, right| {
        rule_scope_precedence(left.scope)
            .cmp(&rule_scope_precedence(right.scope))
            .then_with(|| left.source_path.cmp(&right.source_path))
    });

    let resolved_target_path = resolve_rule_target_path(request_text, working_set, workspace_roots);
    let mut active_rules = Vec::new();
    let mut records = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    for entry in entries {
        let applicable = match entry.scope {
            aria_core::RuleScope::Org | aria_core::RuleScope::User | aria_core::RuleScope::Project => true,
            aria_core::RuleScope::Path => path_rule_matches(&entry, resolved_target_path.as_deref()),
        };
        if !applicable {
            records.push(aria_core::RuleInspectionRecord {
                rule_id: entry.rule_id.clone(),
                scope: entry.scope,
                source_kind: entry.source_kind,
                title: entry.title.clone(),
                source_path: entry.source_path.clone(),
                applies_to_path: entry.applies_to_path.clone(),
                decision: aria_core::RuleDecision::NotApplicable,
                reason: Some(match entry.scope {
                    aria_core::RuleScope::Path => "no matching path target resolved for this request".into(),
                    _ => "rule was outside the active workspace resolution".into(),
                }),
            });
            continue;
        }
        let fingerprint = format!("{:?}|{}", entry.scope, entry.body.trim());
        if !seen.insert(fingerprint) {
            records.push(aria_core::RuleInspectionRecord {
                rule_id: entry.rule_id.clone(),
                scope: entry.scope,
                source_kind: entry.source_kind,
                title: entry.title.clone(),
                source_path: entry.source_path.clone(),
                applies_to_path: entry.applies_to_path.clone(),
                decision: aria_core::RuleDecision::Duplicate,
                reason: Some("duplicate rule body already applied from a higher-precedence source".into()),
            });
            continue;
        }
        records.push(aria_core::RuleInspectionRecord {
            rule_id: entry.rule_id.clone(),
            scope: entry.scope,
            source_kind: entry.source_kind,
            title: entry.title.clone(),
            source_path: entry.source_path.clone(),
            applies_to_path: entry.applies_to_path.clone(),
            decision: aria_core::RuleDecision::Applied,
            reason: Some(format!(
                "applied at precedence layer {}",
                rule_scope_precedence(entry.scope)
            )),
        });
        active_rules.push(entry);
    }

    Ok(aria_core::RuleResolution {
        summary: Some(format!(
            "active_rules={} non_active_rules={}",
            active_rules.len(),
            records
                .iter()
                .filter(|record| !matches!(record.decision, aria_core::RuleDecision::Applied))
                .count()
        )),
        resolved_target_path,
        active_rules,
        records,
    })
}

fn build_rule_context(resolution: &aria_core::RuleResolution) -> String {
    if resolution.active_rules.is_empty() {
        return String::new();
    }
    let mut lines = Vec::new();
    lines.push("Active Rules:".to_string());
    for rule in &resolution.active_rules {
        lines.push(format!(
            "[{:?}/{:?}] {}{}",
            rule.scope,
            rule.source_kind,
            rule.title,
            rule.applies_to_path
                .as_deref()
                .map(|path| format!(" (path={})", path))
                .unwrap_or_default()
        ));
        if let Some(path) = &rule.source_path {
            lines.push(format!("source: {}", path));
        }
        lines.push(rule.body.trim().to_string());
        lines.push(String::new());
    }
    lines.join("\n").trim().to_string()
}

fn inspect_rule_resolution_json(
    workspace_root: &str,
    request_text: Option<&str>,
    target_path: Option<&str>,
) -> Result<serde_json::Value, (axum::http::StatusCode, String)> {
    let updated_at_us = chrono::Utc::now().timestamp_micros() as u64;
    let resolution = build_rule_resolution(
        &[workspace_root.to_string()],
        request_text.unwrap_or_default(),
        None,
        updated_at_us,
    )
    .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let effective_target = target_path
        .map(|value| value.to_string())
        .or(resolution.resolved_target_path.clone());
    Ok(serde_json::json!({
        "workspace_root": workspace_root,
        "request_text": request_text,
        "resolved_target_path": effective_target,
        "summary": resolution.summary,
        "active_rules": resolution.active_rules,
        "records": resolution.records,
    }))
}
