use std::process::Command;

use aria_core::{
    ComputerActionAuditRecord, ComputerActionKind, ComputerActionRequest,
    ComputerActionScope, ComputerArtifactKind, ComputerArtifactRecord, ComputerExecutionProfile,
    ComputerPointerButton, ComputerRuntimeKind, ComputerSessionRecord,
};

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InteractionTaskClass {
    BrowserRead,
    BrowserWrite,
    ComputerObserve,
    ComputerAct,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SurfaceSelectionInput {
    pub explicit_surface: Option<aria_core::ComputerSurfaceKind>,
    pub task_class: InteractionTaskClass,
    pub browser_runtime_available: bool,
    pub chrome_devtools_available: bool,
    pub computer_runtime_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ComputerExecutionResult {
    pub profile: ComputerExecutionProfile,
    pub session: ComputerSessionRecord,
    pub action: ComputerActionRequest,
    pub surface: aria_core::SurfaceSelectionDecision,
    #[serde(default)]
    pub audit: Option<ComputerActionAuditRecord>,
    #[serde(default)]
    pub artifact: Option<ComputerArtifactRecord>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[allow(dead_code)]
pub(crate) fn resolve_interaction_surface(
    input: SurfaceSelectionInput,
) -> Result<aria_core::SurfaceSelectionDecision, String> {
    let SurfaceSelectionInput {
        explicit_surface,
        task_class,
        browser_runtime_available,
        chrome_devtools_available,
        computer_runtime_available,
    } = input;

    if let Some(explicit_surface) = explicit_surface {
        let available = match explicit_surface {
            aria_core::ComputerSurfaceKind::BrowserRuntime => browser_runtime_available,
            aria_core::ComputerSurfaceKind::ChromeDevtoolsMcp => chrome_devtools_available,
            aria_core::ComputerSurfaceKind::ComputerRuntime => computer_runtime_available,
        };
        if !available {
            return Err(format!(
                "requested surface '{:?}' is unavailable",
                explicit_surface
            ));
        }
        return Ok(aria_core::SurfaceSelectionDecision {
            surface: explicit_surface,
            reason: "explicit surface request".into(),
        });
    }

    match task_class {
        InteractionTaskClass::BrowserRead | InteractionTaskClass::BrowserWrite => {
            if browser_runtime_available {
                Ok(aria_core::SurfaceSelectionDecision {
                    surface: aria_core::ComputerSurfaceKind::BrowserRuntime,
                    reason: "browser task routed to managed browser runtime".into(),
                })
            } else if chrome_devtools_available {
                Ok(aria_core::SurfaceSelectionDecision {
                    surface: aria_core::ComputerSurfaceKind::ChromeDevtoolsMcp,
                    reason: "browser task routed to Chrome DevTools MCP fallback".into(),
                })
            } else {
                Err("no browser-capable surface is available".into())
            }
        }
        InteractionTaskClass::ComputerObserve | InteractionTaskClass::ComputerAct => {
            if computer_runtime_available {
                Ok(aria_core::SurfaceSelectionDecision {
                    surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                    reason: "desktop task routed to dedicated computer runtime".into(),
                })
            } else {
                Err("computer runtime is unavailable for desktop interaction tasks".into())
            }
        }
    }
}

pub(crate) fn default_local_computer_profiles() -> Vec<ComputerExecutionProfile> {
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    vec![
        ComputerExecutionProfile {
            profile_id: "desktop-safe".into(),
            display_name: "Desktop Safe".into(),
            runtime_kind: ComputerRuntimeKind::LocalDesktop,
            isolated: false,
            headless: false,
            allow_clipboard: true,
            allow_keyboard: true,
            allow_pointer: true,
            allowed_windows: Vec::new(),
            created_at_us: now_us,
        },
        ComputerExecutionProfile {
            profile_id: "desktop-observe".into(),
            display_name: "Desktop Observe".into(),
            runtime_kind: ComputerRuntimeKind::LocalDesktop,
            isolated: false,
            headless: false,
            allow_clipboard: false,
            allow_keyboard: false,
            allow_pointer: false,
            allowed_windows: Vec::new(),
            created_at_us: now_us,
        },
    ]
}

pub(crate) fn ensure_default_computer_profiles(sessions_dir: &Path) -> Result<Vec<ComputerExecutionProfile>, String> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    let existing = store.list_computer_profiles()?;
    if !existing.is_empty() {
        return Ok(existing);
    }
    let defaults = default_local_computer_profiles();
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    for profile in &defaults {
        store.upsert_computer_profile(profile, now_us)?;
    }
    Ok(defaults)
}

pub(crate) fn resolve_computer_profile(
    sessions_dir: &Path,
    profile_id: Option<&str>,
) -> Result<ComputerExecutionProfile, String> {
    let profiles = ensure_default_computer_profiles(sessions_dir)?;
    if let Some(profile_id) = profile_id.map(str::trim).filter(|value| !value.is_empty()) {
        return profiles
            .into_iter()
            .find(|profile| profile.profile_id == profile_id)
            .ok_or_else(|| format!("computer profile '{}' not found", profile_id));
    }
    profiles
        .iter()
        .find(|profile| profile.profile_id == "desktop-safe")
        .cloned()
        .or_else(|| profiles.into_iter().next())
        .ok_or_else(|| "no computer profiles are configured".into())
}

pub(crate) fn resolve_or_create_computer_session(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    request: &ComputerActionRequest,
    profile: &ComputerExecutionProfile,
) -> Result<ComputerSessionRecord, String> {
    let store = RuntimeStore::for_sessions_dir(sessions_dir);
    if let Some(computer_session_id) = request
        .computer_session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let sessions = store.list_computer_sessions(Some(session_id), Some(agent_id))?;
        return sessions
            .into_iter()
            .find(|session| session.computer_session_id == computer_session_id)
            .ok_or_else(|| format!("computer session '{}' not found", computer_session_id));
    }

    let mut sessions = store.list_computer_sessions(Some(session_id), Some(agent_id))?;
    sessions.sort_by_key(|record| std::cmp::Reverse(record.updated_at_us));
    if let Some(existing) = sessions
        .into_iter()
        .find(|session| session.profile_id == profile.profile_id)
    {
        return Ok(existing);
    }

    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let session = ComputerSessionRecord {
        computer_session_id: format!("computer-session-{}", uuid::Uuid::new_v4()),
        session_id,
        agent_id: agent_id.to_string(),
        profile_id: profile.profile_id.clone(),
        runtime_kind: profile.runtime_kind,
        selected_window_id: request.target_window_id.clone(),
        created_at_us: now_us,
        updated_at_us: now_us,
    };
    store.upsert_computer_session(&session, now_us)?;
    Ok(session)
}

pub(crate) fn infer_computer_task_class(action: ComputerActionKind) -> InteractionTaskClass {
    match action {
        ComputerActionKind::CaptureScreenshot | ComputerActionKind::ClipboardRead => {
            InteractionTaskClass::ComputerObserve
        }
        ComputerActionKind::PointerMove
        | ComputerActionKind::PointerClick
        | ComputerActionKind::KeyboardType
        | ComputerActionKind::KeyPress
        | ComputerActionKind::ClipboardWrite
        | ComputerActionKind::WindowFocus => InteractionTaskClass::ComputerAct,
    }
}

pub(crate) fn validate_computer_profile_request(
    capability_profile: Option<&AgentCapabilityProfile>,
    profile: &ComputerExecutionProfile,
    sessions_dir: Option<&Path>,
    session_id: Option<aria_core::Uuid>,
) -> Result<(), String> {
    let Some(agent) = capability_profile else {
        return Ok(());
    };
    if !agent.computer_profile_allowlist.is_empty()
        && !agent
            .computer_profile_allowlist
            .iter()
            .any(|allowed| allowed == &profile.profile_id)
    {
        append_scope_denial_record(
            sessions_dir,
            &agent.agent_id,
            session_id,
            ScopeDenialKind::ComputerProfileScope,
            profile.profile_id.clone(),
            format!(
                "computer profile '{}' not permitted for agent '{}'",
                profile.profile_id, agent.agent_id
            ),
        );
        return Err(format!(
            "computer profile '{}' not permitted for agent '{}'",
            profile.profile_id, agent.agent_id
        ));
    }
    Ok(())
}

pub(crate) fn validate_computer_action_request(
    capability_profile: Option<&AgentCapabilityProfile>,
    request: &ComputerActionRequest,
    profile: &ComputerExecutionProfile,
    sessions_dir: Option<&Path>,
    session_id: Option<aria_core::Uuid>,
) -> Result<(), String> {
    let Some(agent) = capability_profile else {
        return Ok(());
    };
    let allowed = match (agent.computer_action_scope, request.action) {
        (Some(ComputerActionScope::ObserveOnly), ComputerActionKind::CaptureScreenshot)
        | (Some(ComputerActionScope::ObserveOnly), ComputerActionKind::ClipboardRead) => true,
        (Some(ComputerActionScope::PointerOnly), ComputerActionKind::PointerMove)
        | (Some(ComputerActionScope::PointerOnly), ComputerActionKind::PointerClick)
        | (Some(ComputerActionScope::PointerOnly), ComputerActionKind::CaptureScreenshot) => true,
        (Some(ComputerActionScope::PointerAndKeyboard), ComputerActionKind::PointerMove)
        | (Some(ComputerActionScope::PointerAndKeyboard), ComputerActionKind::PointerClick)
        | (Some(ComputerActionScope::PointerAndKeyboard), ComputerActionKind::KeyboardType)
        | (Some(ComputerActionScope::PointerAndKeyboard), ComputerActionKind::KeyPress)
        | (Some(ComputerActionScope::PointerAndKeyboard), ComputerActionKind::CaptureScreenshot) => true,
        (Some(ComputerActionScope::ClipboardRead), ComputerActionKind::ClipboardRead)
        | (Some(ComputerActionScope::ClipboardRead), ComputerActionKind::CaptureScreenshot) => true,
        (Some(ComputerActionScope::ClipboardReadWrite), ComputerActionKind::ClipboardRead)
        | (Some(ComputerActionScope::ClipboardReadWrite), ComputerActionKind::ClipboardWrite)
        | (Some(ComputerActionScope::ClipboardReadWrite), ComputerActionKind::CaptureScreenshot) => true,
        (Some(ComputerActionScope::FullDesktopControl), _) | (None, _) => true,
        _ => false,
    };
    if !allowed {
        append_scope_denial_record(
            sessions_dir,
            &agent.agent_id,
            session_id,
            ScopeDenialKind::ComputerActionScope,
            format!("{:?}", request.action),
            format!(
                "computer action '{:?}' not permitted for agent '{}'",
                request.action, agent.agent_id
            ),
        );
        return Err(format!(
            "computer action '{:?}' not permitted for agent '{}'",
            request.action, agent.agent_id
        ));
    }
    match request.action {
        ComputerActionKind::PointerMove | ComputerActionKind::PointerClick if !profile.allow_pointer => {
            return Err(format!(
                "computer profile '{}' does not allow pointer control",
                profile.profile_id
            ));
        }
        ComputerActionKind::KeyboardType | ComputerActionKind::KeyPress if !profile.allow_keyboard => {
            return Err(format!(
                "computer profile '{}' does not allow keyboard input",
                profile.profile_id
            ));
        }
        ComputerActionKind::ClipboardRead | ComputerActionKind::ClipboardWrite if !profile.allow_clipboard => {
            return Err(format!(
                "computer profile '{}' does not allow clipboard access",
                profile.profile_id
            ));
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn computer_action_requires_approval(request: &ComputerActionRequest) -> bool {
    matches!(
        request.action,
        ComputerActionKind::PointerClick
            | ComputerActionKind::KeyboardType
            | ComputerActionKind::KeyPress
            | ComputerActionKind::ClipboardWrite
            | ComputerActionKind::WindowFocus
    )
}

fn artifact_path(sessions_dir: &Path, artifact_id: &str, extension: &str) -> Result<PathBuf, String> {
    let artifacts_dir = sessions_dir.join("computer_artifacts");
    std::fs::create_dir_all(&artifacts_dir)
        .map_err(|e| format!("create '{}': {}", artifacts_dir.display(), e))?;
    Ok(artifacts_dir.join(format!("{}.{}", artifact_id, extension)))
}

fn append_computer_action_audit(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    session: &ComputerSessionRecord,
    profile: &ComputerExecutionProfile,
    request: &ComputerActionRequest,
    target: Option<String>,
    metadata: serde_json::Value,
) -> Result<ComputerActionAuditRecord, String> {
    let audit = ComputerActionAuditRecord {
        audit_id: format!("computer-audit-{}", uuid::Uuid::new_v4()),
        session_id,
        agent_id: agent_id.to_string(),
        computer_session_id: Some(session.computer_session_id.clone()),
        profile_id: Some(profile.profile_id.clone()),
        action: request.action,
        target,
        metadata,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
    };
    RuntimeStore::for_sessions_dir(sessions_dir).append_computer_action_audit(&audit)?;
    Ok(audit)
}

#[allow(dead_code)]
pub(crate) fn persist_computer_screenshot_artifact(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    computer_session_id: Option<&str>,
    profile_id: Option<&str>,
    png_bytes: &[u8],
    metadata: serde_json::Value,
) -> Result<ComputerArtifactRecord, String> {
    let artifact_id = format!("computer-artifact-{}", uuid::Uuid::new_v4());
    let output_path = artifact_path(sessions_dir, &artifact_id, "png")?;
    std::fs::write(&output_path, png_bytes)
        .map_err(|e| format!("write '{}': {}", output_path.display(), e))?;

    let record = ComputerArtifactRecord {
        artifact_id,
        session_id,
        agent_id: agent_id.to_string(),
        computer_session_id: computer_session_id.map(ToString::to_string),
        profile_id: profile_id.map(ToString::to_string),
        kind: ComputerArtifactKind::Screenshot,
        mime_type: "image/png".into(),
        storage_path: output_path.display().to_string(),
        metadata,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
    };
    RuntimeStore::for_sessions_dir(sessions_dir).append_computer_artifact(&record)?;
    Ok(record)
}

fn persist_clipboard_artifact(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    computer_session_id: Option<&str>,
    profile_id: Option<&str>,
    contents: &str,
    metadata: serde_json::Value,
) -> Result<ComputerArtifactRecord, String> {
    let artifact_id = format!("computer-artifact-{}", uuid::Uuid::new_v4());
    let output_path = artifact_path(sessions_dir, &artifact_id, "txt")?;
    std::fs::write(&output_path, contents)
        .map_err(|e| format!("write '{}': {}", output_path.display(), e))?;
    let record = ComputerArtifactRecord {
        artifact_id,
        session_id,
        agent_id: agent_id.to_string(),
        computer_session_id: computer_session_id.map(ToString::to_string),
        profile_id: profile_id.map(ToString::to_string),
        kind: ComputerArtifactKind::ClipboardSnapshot,
        mime_type: "text/plain".into(),
        storage_path: output_path.display().to_string(),
        metadata,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
    };
    RuntimeStore::for_sessions_dir(sessions_dir).append_computer_artifact(&record)?;
    Ok(record)
}

#[cfg(target_os = "macos")]
fn run_osascript(script: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("spawn osascript failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "osascript exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn run_swift(script: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/swift")
        .arg("-e")
        .arg(script)
        .output()
        .map_err(|e| format!("spawn swift failed: {}", e))?;
    if !output.status.success() {
        return Err(format!(
            "swift exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(target_os = "macos")]
fn shell_escape_single_quoted(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}

#[cfg(target_os = "macos")]
fn apply_local_desktop_action(request: &ComputerActionRequest) -> Result<serde_json::Value, String> {
    match request.action {
        ComputerActionKind::PointerMove => {
            let x = request.x.ok_or_else(|| "pointer_move requires x".to_string())?;
            let y = request.y.ok_or_else(|| "pointer_move requires y".to_string())?;
            let move_script = format!(
                "import CoreGraphics\nlet event = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: CGPoint(x: {x}, y: {y}), mouseButton: .left)\nevent?.post(tap: .cghidEventTap)"
            );
            run_swift(&move_script)?;
            Ok(serde_json::json!({"x": x, "y": y}))
        }
        ComputerActionKind::PointerClick => {
            let x = request.x.ok_or_else(|| "pointer_click requires x".to_string())?;
            let y = request.y.ok_or_else(|| "pointer_click requires y".to_string())?;
            let button = request.button.unwrap_or(ComputerPointerButton::Left);
            let (down_event, up_event) = match button {
                ComputerPointerButton::Left => (".leftMouseDown", ".leftMouseUp"),
                ComputerPointerButton::Right => (".rightMouseDown", ".rightMouseUp"),
                ComputerPointerButton::Middle => (".otherMouseDown", ".otherMouseUp"),
            };
            let button_type = match button {
                ComputerPointerButton::Left => "CGMouseButton.left",
                ComputerPointerButton::Right => "CGMouseButton.right",
                ComputerPointerButton::Middle => "CGMouseButton.center",
            };
            let click_script = format!(
                "import CoreGraphics\nlet point = CGPoint(x: {x}, y: {y})\nlet move = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: point, mouseButton: {button_type})\nlet down = CGEvent(mouseEventSource: nil, mouseType: {down_event}, mouseCursorPosition: point, mouseButton: {button_type})\nlet up = CGEvent(mouseEventSource: nil, mouseType: {up_event}, mouseCursorPosition: point, mouseButton: {button_type})\nmove?.post(tap: .cghidEventTap)\ndown?.post(tap: .cghidEventTap)\nup?.post(tap: .cghidEventTap)"
            );
            run_swift(&click_script)?;
            Ok(serde_json::json!({"x": x, "y": y, "button": button}))
        }
        ComputerActionKind::KeyboardType => {
            let text = request
                .text
                .as_deref()
                .ok_or_else(|| "keyboard_type requires text".to_string())?;
            let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
            run_osascript(&format!(
                "tell application \"System Events\" to keystroke \"{}\"",
                escaped
            ))?;
            Ok(serde_json::json!({"typed_chars": text.chars().count()}))
        }
        ComputerActionKind::KeyPress => {
            let key = request
                .key
                .as_deref()
                .ok_or_else(|| "key_press requires key".to_string())?;
            let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
            run_osascript(&format!(
                "tell application \"System Events\" to keystroke \"{}\"",
                escaped
            ))?;
            Ok(serde_json::json!({"key": key}))
        }
        ComputerActionKind::ClipboardRead => {
            let output = Command::new("pbpaste")
                .output()
                .map_err(|e| format!("spawn pbpaste failed: {}", e))?;
            if !output.status.success() {
                return Err(format!("pbpaste exited with status {}", output.status));
            }
            Ok(serde_json::json!({
                "text": String::from_utf8_lossy(&output.stdout).to_string()
            }))
        }
        ComputerActionKind::ClipboardWrite => {
            let text = request
                .text
                .as_deref()
                .ok_or_else(|| "clipboard_write requires text".to_string())?;
            let escaped = shell_escape_single_quoted(text);
            let script = format!("do shell script \"printf '%s' '{}' | pbcopy\"", escaped);
            run_osascript(&script)?;
            Ok(serde_json::json!({"written_chars": text.chars().count()}))
        }
        ComputerActionKind::WindowFocus => {
            let window = request
                .target_window_id
                .as_deref()
                .ok_or_else(|| "window_focus requires target_window_id".to_string())?;
            let escaped = window.replace('\\', "\\\\").replace('"', "\\\"");
            run_osascript(&format!(
                "tell application \"System Events\" to tell process \"{}\" to set frontmost to true",
                escaped
            ))?;
            Ok(serde_json::json!({"target_window_id": window}))
        }
        ComputerActionKind::CaptureScreenshot => {
            Err("capture_screenshot should use dedicated screenshot path".into())
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn apply_local_desktop_action(_request: &ComputerActionRequest) -> Result<serde_json::Value, String> {
    Err("local desktop computer actions are currently supported only on macOS".into())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
pub(crate) fn capture_local_computer_screenshot(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    computer_session_id: Option<&str>,
    profile_id: Option<&str>,
) -> Result<ComputerArtifactRecord, String> {
    let artifact_id = format!("computer-artifact-{}", uuid::Uuid::new_v4());
    let output_path = artifact_path(sessions_dir, &artifact_id, "png")?;
    let status = Command::new("screencapture")
        .arg("-x")
        .arg(&output_path)
        .status()
        .map_err(|e| format!("spawn screencapture failed: {}", e))?;
    if !status.success() {
        return Err(format!("screencapture exited with status {}", status));
    }
    let metadata = serde_json::json!({
        "capture_backend": "screencapture",
        "runtime_kind": "local_desktop",
    });
    let record = ComputerArtifactRecord {
        artifact_id,
        session_id,
        agent_id: agent_id.to_string(),
        computer_session_id: computer_session_id.map(ToString::to_string),
        profile_id: profile_id.map(ToString::to_string),
        kind: ComputerArtifactKind::Screenshot,
        mime_type: "image/png".into(),
        storage_path: output_path.display().to_string(),
        metadata,
        created_at_us: chrono::Utc::now().timestamp_micros() as u64,
    };
    RuntimeStore::for_sessions_dir(sessions_dir).append_computer_artifact(&record)?;
    Ok(record)
}

#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
pub(crate) fn capture_local_computer_screenshot(
    _sessions_dir: &Path,
    _session_id: aria_core::Uuid,
    _agent_id: &str,
    _computer_session_id: Option<&str>,
    _profile_id: Option<&str>,
) -> Result<ComputerArtifactRecord, String> {
    Err("local computer screenshot capture is currently supported only on macOS".into())
}

pub(crate) fn execute_local_computer_action(
    sessions_dir: &Path,
    session_id: aria_core::Uuid,
    agent_id: &str,
    request: &ComputerActionRequest,
    profile: &ComputerExecutionProfile,
    session: &ComputerSessionRecord,
    surface: aria_core::SurfaceSelectionDecision,
) -> Result<ComputerExecutionResult, String> {
    let now_us = chrono::Utc::now().timestamp_micros() as u64;
    let (payload, artifact) = match request.action {
        ComputerActionKind::CaptureScreenshot => {
            let artifact = capture_local_computer_screenshot(
                sessions_dir,
                session_id,
                agent_id,
                Some(&session.computer_session_id),
                Some(&profile.profile_id),
            )?;
            (
                serde_json::json!({
                    "artifact_id": artifact.artifact_id,
                    "storage_path": artifact.storage_path,
                }),
                Some(artifact),
            )
        }
        ComputerActionKind::ClipboardRead => {
            let payload = apply_local_desktop_action(request)?;
            let text = payload
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default();
            let artifact = persist_clipboard_artifact(
                sessions_dir,
                session_id,
                agent_id,
                Some(&session.computer_session_id),
                Some(&profile.profile_id),
                text,
                serde_json::json!({"source": "clipboard_read"}),
            )?;
            (
                serde_json::json!({
                    "artifact_id": artifact.artifact_id,
                    "storage_path": artifact.storage_path,
                    "text_preview": text.chars().take(120).collect::<String>(),
                }),
                Some(artifact),
            )
        }
        _ => (apply_local_desktop_action(request)?, None),
    };
    let audit = append_computer_action_audit(
        sessions_dir,
        session_id,
        agent_id,
        session,
        profile,
        request,
        request
            .target_window_id
            .clone()
            .or_else(|| request.key.clone())
            .or_else(|| request.text.clone().map(|_| "<text>".into())),
        serde_json::json!({
            "profile_id": profile.profile_id,
            "surface": surface.surface,
            "surface_reason": surface.reason,
            "payload": payload,
            "recorded_at_us": now_us,
        }),
    )?;
    let mut updated_session = session.clone();
    updated_session.selected_window_id = request.target_window_id.clone().or_else(|| session.selected_window_id.clone());
    updated_session.updated_at_us = now_us;
    RuntimeStore::for_sessions_dir(sessions_dir).upsert_computer_session(&updated_session, now_us)?;
    Ok(ComputerExecutionResult {
        profile: profile.clone(),
        session: updated_session,
        action: request.clone(),
        surface,
        audit: Some(audit),
        artifact,
        payload,
    })
}

#[cfg(test)]
mod phase3_tests {
    use super::*;

    #[test]
    fn browser_tasks_prefer_browser_runtime_over_other_surfaces() {
        let decision = resolve_interaction_surface(SurfaceSelectionInput {
            explicit_surface: None,
            task_class: InteractionTaskClass::BrowserRead,
            browser_runtime_available: true,
            chrome_devtools_available: true,
            computer_runtime_available: true,
        })
        .expect("surface decision");
        assert_eq!(decision.surface, aria_core::ComputerSurfaceKind::BrowserRuntime);
    }

    #[test]
    fn browser_tasks_fall_back_to_chrome_devtools_mcp_when_browser_runtime_is_unavailable() {
        let decision = resolve_interaction_surface(SurfaceSelectionInput {
            explicit_surface: None,
            task_class: InteractionTaskClass::BrowserWrite,
            browser_runtime_available: false,
            chrome_devtools_available: true,
            computer_runtime_available: true,
        })
        .expect("surface decision");
        assert_eq!(decision.surface, aria_core::ComputerSurfaceKind::ChromeDevtoolsMcp);
    }

    #[test]
    fn desktop_tasks_require_dedicated_computer_runtime() {
        let decision = resolve_interaction_surface(SurfaceSelectionInput {
            explicit_surface: None,
            task_class: InteractionTaskClass::ComputerAct,
            browser_runtime_available: true,
            chrome_devtools_available: true,
            computer_runtime_available: true,
        })
        .expect("surface decision");
        assert_eq!(decision.surface, aria_core::ComputerSurfaceKind::ComputerRuntime);
    }

    #[test]
    fn desktop_tasks_do_not_silently_fall_back_to_browser_surfaces() {
        let err = resolve_interaction_surface(SurfaceSelectionInput {
            explicit_surface: None,
            task_class: InteractionTaskClass::ComputerObserve,
            browser_runtime_available: true,
            chrome_devtools_available: true,
            computer_runtime_available: false,
        })
        .expect_err("desktop task should fail without computer runtime");
        assert!(err.contains("computer runtime is unavailable"));
    }

    #[test]
    fn explicit_surface_selection_requires_availability() {
        let err = resolve_interaction_surface(SurfaceSelectionInput {
            explicit_surface: Some(aria_core::ComputerSurfaceKind::ComputerRuntime),
            task_class: InteractionTaskClass::BrowserRead,
            browser_runtime_available: true,
            chrome_devtools_available: true,
            computer_runtime_available: false,
        })
        .expect_err("unavailable explicit surface");
        assert!(err.contains("requested surface"));
    }

    #[test]
    fn default_profiles_are_seeded_when_missing() {
        let sessions = tempfile::tempdir().expect("sessions");
        let profiles = ensure_default_computer_profiles(sessions.path()).expect("seed profiles");
        assert!(profiles.iter().any(|profile| profile.profile_id == "desktop-safe"));
        assert_eq!(
            RuntimeStore::for_sessions_dir(sessions.path())
                .list_computer_profiles()
                .expect("list profiles")
                .len(),
            profiles.len()
        );
    }

    #[test]
    fn pointer_click_requires_approval_but_pointer_move_does_not() {
        assert!(computer_action_requires_approval(&ComputerActionRequest {
            computer_session_id: None,
            profile_id: None,
            target_window_id: None,
            action: ComputerActionKind::PointerClick,
            x: Some(1),
            y: Some(2),
            button: Some(ComputerPointerButton::Left),
            text: None,
            key: None,
        }));
        assert!(!computer_action_requires_approval(&ComputerActionRequest {
            computer_session_id: None,
            profile_id: None,
            target_window_id: None,
            action: ComputerActionKind::PointerMove,
            x: Some(1),
            y: Some(2),
            button: None,
            text: None,
            key: None,
        }));
    }

    #[test]
    fn resolve_or_create_computer_session_persists_target_window_and_profile() {
        let sessions = tempfile::tempdir().expect("sessions");
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let profile = resolve_computer_profile(sessions.path(), Some("desktop-safe")).expect("profile");
        let session = resolve_or_create_computer_session(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: None,
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("TextEdit".into()),
                action: ComputerActionKind::WindowFocus,
                x: None,
                y: None,
                button: None,
                text: None,
                key: None,
            },
            &profile,
        )
        .expect("session");
        assert_eq!(session.profile_id, "desktop-safe");
        assert_eq!(session.selected_window_id.as_deref(), Some("TextEdit"));
    }

    #[test]
    fn validate_computer_action_scope_blocks_keyboard_for_pointer_only_agents() {
        let sessions = tempfile::tempdir().expect("sessions");
        let profile = resolve_computer_profile(sessions.path(), Some("desktop-safe")).expect("profile");
        let request = ComputerActionRequest {
            computer_session_id: None,
            profile_id: Some(profile.profile_id.clone()),
            target_window_id: None,
            action: ComputerActionKind::KeyboardType,
            x: None,
            y: None,
            button: None,
            text: Some("hello".into()),
            key: None,
        };
        let capability = AgentCapabilityProfile {
            agent_id: "developer".into(),
            class: aria_core::AgentClass::Restricted,
            tool_allowlist: vec!["computer_act".into()],
            skill_allowlist: vec![],
            mcp_server_allowlist: vec![],
            mcp_tool_allowlist: vec![],
            mcp_prompt_allowlist: vec![],
            mcp_resource_allowlist: vec![],
            filesystem_scopes: vec![],
            retrieval_scopes: vec![],
            delegation_scope: None,
            web_domain_allowlist: vec![],
            web_domain_blocklist: vec![],
            browser_profile_allowlist: vec![],
            browser_action_scope: None,
            computer_profile_allowlist: vec![profile.profile_id.clone()],
            computer_action_scope: Some(ComputerActionScope::PointerOnly),
            browser_session_scope: None,
            crawl_scope: None,
            web_approval_policy: None,
            web_transport_allowlist: vec![],
            requires_elevation: false,
            side_effect_level: aria_core::SideEffectLevel::ReadOnly,
            trust_profile: None,
        };
        let err = validate_computer_action_request(
            Some(&capability),
            &request,
            &profile,
            Some(sessions.path()),
            Some(*uuid::Uuid::new_v4().as_bytes()),
        )
        .expect_err("keyboard blocked");
        assert!(err.contains("not permitted"));
    }

    #[test]
    fn persist_computer_screenshot_artifact_writes_file_and_store_record() {
        let sessions = tempfile::tempdir().expect("sessions");
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let artifact = persist_computer_screenshot_artifact(
            sessions.path(),
            session_id,
            "developer",
            Some("computer-session-1"),
            Some("desktop-safe"),
            b"\x89PNG\r\n\x1a\nfake",
            serde_json::json!({"source":"test"}),
        )
        .expect("persist artifact");
        assert!(Path::new(&artifact.storage_path).exists());
        let records = RuntimeStore::for_sessions_dir(sessions.path())
            .list_computer_artifacts(Some(session_id), Some("developer"))
            .expect("list computer artifacts");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, ComputerArtifactKind::Screenshot);
    }

    #[test]
    fn execute_local_computer_action_records_surface_metadata_in_audit() {
        let sessions = tempfile::tempdir().expect("sessions");
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let profile = resolve_computer_profile(sessions.path(), Some("desktop-safe")).expect("profile");
        let session = resolve_or_create_computer_session(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: None,
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("Notes".into()),
                action: ComputerActionKind::ClipboardWrite,
                x: None,
                y: None,
                button: None,
                text: Some("hello".into()),
                key: None,
            },
            &profile,
        )
        .expect("session");

        execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("Notes".into()),
                action: ComputerActionKind::ClipboardWrite,
                x: None,
                y: None,
                button: None,
                text: Some("hello".into()),
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "desktop action requires dedicated surface".into(),
            },
        )
        .expect("execute clipboard write");

        let audits = RuntimeStore::for_sessions_dir(sessions.path())
            .list_computer_action_audits(Some(session_id), Some("developer"))
            .expect("list audits");
        assert_eq!(audits.len(), 1);
        assert_eq!(audits[0].metadata["surface"], "computer_runtime");
        assert_eq!(
            audits[0].metadata["surface_reason"],
            "desktop action requires dedicated surface"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "live desktop verification requires Accessibility and clipboard permissions"]
    fn live_local_computer_clipboard_round_trip() {
        let sessions = tempfile::tempdir().expect("sessions");
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let profile = resolve_computer_profile(sessions.path(), Some("desktop-safe")).expect("profile");
        let session = resolve_or_create_computer_session(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: None,
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: None,
                action: ComputerActionKind::ClipboardWrite,
                x: None,
                y: None,
                button: None,
                text: None,
                key: None,
            },
            &profile,
        )
        .expect("session");
        let marker = format!("hiveclaw-live-{}", uuid::Uuid::new_v4());
        execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: None,
                action: ComputerActionKind::ClipboardWrite,
                x: None,
                y: None,
                button: None,
                text: Some(marker.clone()),
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "live test".into(),
            },
        )
        .expect("clipboard write");
        let read = execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: None,
                action: ComputerActionKind::ClipboardRead,
                x: None,
                y: None,
                button: None,
                text: None,
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "live test".into(),
            },
        )
        .expect("clipboard read");
        let preview = read
            .payload
            .get("text_preview")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        assert!(preview.contains("hiveclaw-live-"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "live desktop verification requires Accessibility permission"]
    fn live_local_computer_pointer_click_and_keyboard_type() {
        let sessions = tempfile::tempdir().expect("sessions");
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let profile = resolve_computer_profile(sessions.path(), Some("desktop-safe")).expect("profile");
        let session = resolve_or_create_computer_session(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: None,
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("TextEdit".into()),
                action: ComputerActionKind::WindowFocus,
                x: None,
                y: None,
                button: None,
                text: None,
                key: None,
            },
            &profile,
        )
        .expect("session");
        run_osascript("tell application \"TextEdit\" to activate").expect("activate textedit");
        run_osascript("tell application \"TextEdit\" to make new document").expect("new doc");
        let bounds = run_osascript(
            "tell application \"System Events\" to tell process \"TextEdit\" to get position of front window & size of front window",
        )
        .expect("window geometry");
        let nums = bounds
            .split(',')
            .filter_map(|part| part.trim().parse::<i32>().ok())
            .collect::<Vec<_>>();
        assert!(nums.len() >= 4, "unexpected geometry: {}", bounds);
        let x = nums[0] + (nums[2] / 2);
        let y = nums[1] + (nums[3] / 2);
        execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("TextEdit".into()),
                action: ComputerActionKind::PointerMove,
                x: Some(x),
                y: Some(y),
                button: None,
                text: None,
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "live test".into(),
            },
        )
        .expect("pointer move");
        execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("TextEdit".into()),
                action: ComputerActionKind::PointerClick,
                x: Some(x),
                y: Some(y),
                button: Some(ComputerPointerButton::Left),
                text: None,
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "live test".into(),
            },
        )
        .expect("pointer click");
        execute_local_computer_action(
            sessions.path(),
            session_id,
            "developer",
            &ComputerActionRequest {
                computer_session_id: Some(session.computer_session_id.clone()),
                profile_id: Some(profile.profile_id.clone()),
                target_window_id: Some("TextEdit".into()),
                action: ComputerActionKind::KeyboardType,
                x: None,
                y: None,
                button: None,
                text: Some("HiveClaw live desktop test".into()),
                key: None,
            },
            &profile,
            &session,
            aria_core::SurfaceSelectionDecision {
                surface: aria_core::ComputerSurfaceKind::ComputerRuntime,
                reason: "live test".into(),
            },
        )
        .expect("keyboard type");
    }
}
