use std::collections::{HashMap, VecDeque};
use std::io::{stdout, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::{default_project_config_path, resolve_config_path};
use crate::{build_approval_descriptor, ensure_approval_handle, RuntimeStore};
use crossterm::cursor::{Hide, MoveTo, Show};
use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers,
    MouseEventKind,
};
use crossterm::style::{Attribute, Color, Print, ResetColor, SetAttribute, SetForegroundColor};
use crossterm::terminal::{
    self, disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
    LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use directories::ProjectDirs;
use futures_util::{SinkExt, StreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use tokio::net::TcpListener;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupMode {
    Runtime {
        config_path: String,
    },
    Tui {
        config_path: String,
        attach_url: Option<String>,
    },
}

pub(crate) fn parse_startup_mode(
    args: &[String],
    fallback_config_path: Option<String>,
) -> StartupMode {
    let default_config = fallback_config_path
        .unwrap_or_else(|| default_project_config_path().to_string_lossy().to_string());
    match args.get(1).map(|value| value.as_str()) {
        Some("channels")
        | Some("init")
        | Some("skills")
        | Some("inspect")
        | Some("explain")
        | Some("--inspect-context")
        | Some("--inspect-provider-payloads")
        | Some("--explain-context")
        | Some("--explain-provider-payloads")
        | Some("doctor")
        | Some("setup")
        | Some("status")
        | Some("stop")
        | Some("help")
        | Some("install")
        | Some("completion")
        | Some("-h")
        | Some("--help") => StartupMode::Runtime {
            config_path: default_config,
        },
        Some("run") => StartupMode::Runtime {
            config_path: args.get(2).cloned().unwrap_or(default_config),
        },
        Some("tui") | Some("--tui") => {
            let mut config_path = default_config;
            let mut attach_url = None;
            let mut idx = 2usize;
            while let Some(arg) = args.get(idx) {
                if arg == "--attach" {
                    attach_url = args.get(idx + 1).cloned();
                    idx += 2;
                    continue;
                }
                config_path = arg.clone();
                idx += 1;
            }
            StartupMode::Tui {
                config_path,
                attach_url,
            }
        }
        Some(path) => StartupMode::Runtime {
            config_path: path.to_string(),
        },
        None => StartupMode::Runtime {
            config_path: default_config,
        },
    }
}

pub(crate) async fn run_tui_mode(
    config_path: &str,
    attach_url: Option<&str>,
) -> Result<(), String> {
    let resolved = resolve_config_path(config_path);
    let websocket_port = if let Some(url) = attach_url {
        websocket_port_from_url(url).unwrap_or(0)
    } else {
        pick_open_port().await?
    };
    let parent = resolved.parent().ok_or_else(|| {
        format!(
            "config path '{}' has no parent directory",
            resolved.display()
        )
    })?;
    let stem = resolved
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("config");
    let suffix = uuid::Uuid::new_v4().to_string();
    let temp_config_path = parent.join(format!("{}.tui-{}.toml", stem, suffix));
    let temp_runtime_path = parent.join(format!("{}.tui-{}.runtime.json", stem, suffix));
    let temp_log_path = parent.join(format!("{}.tui-{}.runtime.log", stem, suffix));

    let mut child = None;
    let ws_url = if let Some(url) = attach_url {
        url.to_string()
    } else {
        prepare_tui_runtime_config(
            &resolved,
            &temp_config_path,
            &temp_runtime_path,
            websocket_port,
        )?;
        child = Some(spawn_tui_runtime(&temp_config_path, &temp_log_path)?);
        wait_for_websocket(websocket_port).await?;
        format!("ws://127.0.0.1:{}/ws", websocket_port)
    };
    let (stream, _) = connect_async(&ws_url)
        .await
        .map_err(|e| format!("connect TUI websocket failed: {}", e))?;
    let (mut sink, mut stream) = stream.split();

    let mut guard = TuiTerminalGuard::enter()?;
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || loop {
        match event::read() {
            Ok(evt) => {
                if event_tx.send(evt).is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    });

    let default_agent = crate::load_config(config_path)
        .ok()
        .map(|cfg| cfg.ui.default_agent.clone())
        .unwrap_or_else(default_tui_active_agent);
    let prefs = load_tui_preferences(&default_agent);
    let bootstrap_agent = prefs.active_agent.clone();
    let mut state = TuiState::new(
        config_path.to_string(),
        websocket_port,
        prefs,
        temp_log_path,
    );
    state.render(&mut guard.stdout)?;
    send_ws_text(
        &mut sink,
        state.make_request(&format!("/agent {}", bootstrap_agent)),
    )
    .await?;
    state.push_system(format!(
        "Connected. Establishing session agent override: {}.",
        bootstrap_agent
    ));
    send_ws_text(&mut sink, state.make_request("/agents")).await?;
    send_ws_text(&mut sink, state.make_request("/approvals")).await?;
    send_ws_text(&mut sink, state.make_request("/provider_health")).await?;
    send_ws_text(&mut sink, state.make_request("/workspace_locks")).await?;
    state.render(&mut guard.stdout)?;

    loop {
        tokio::select! {
            ctrl_c = tokio::signal::ctrl_c() => {
                if ctrl_c.is_ok() {
                    state.push_system("Received Ctrl+C. Shutting down TUI.".to_string());
                    state.render(&mut guard.stdout)?;
                }
                break;
            }
            maybe_msg = stream.next() => {
                match maybe_msg {
                    Some(Ok(Message::Text(text))) => {
                        state.push_assistant(text.to_string());
                        state.render(&mut guard.stdout)?;
                    }
                    Some(Ok(_)) => {}
                    Some(Err(err)) => {
                        state.push_system(format!("Connection error: {}", err));
                        state.render(&mut guard.stdout)?;
                        break;
                    }
                    None => {
                        state.push_system("Runtime connection closed.".to_string());
                        state.render(&mut guard.stdout)?;
                        break;
                    }
                }
            }
            maybe_evt = event_rx.recv() => {
                let Some(evt) = maybe_evt else { break; };
                if let Some(action) = state.handle_event(evt) {
                    match action {
                        TuiAction::Quit => break,
                        TuiAction::Send(text) => {
                            state.push_user(text.clone());
                            send_ws_text(&mut sink, state.make_request(&text)).await?;
                            state.persist_preferences();
                            state.render(&mut guard.stdout)?;
                        }
                    }
                } else {
                    state.persist_preferences();
                    state.render(&mut guard.stdout)?;
                }
            }
        }
    }

    cleanup_tui_runtime(child, sink).await;
    if attach_url.is_none() {
        let _ = std::fs::remove_file(&temp_config_path);
        let _ = std::fs::remove_file(&temp_runtime_path);
        let _ = std::fs::remove_file(&state.log_path);
    }
    Ok(())
}

async fn send_ws_text<S>(sink: &mut S, payload: String) -> Result<(), String>
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    sink.send(Message::Text(payload.into()))
        .await
        .map_err(|e| format!("send TUI websocket message failed: {}", e))
}

async fn cleanup_tui_runtime<S>(mut child: Option<Child>, mut sink: S)
where
    S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    let _ = sink.send(Message::Close(None)).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let Some(mut child) = child.take() else {
        return;
    };
    match child.try_wait() {
        Ok(Some(_)) => {}
        _ => {
            let _ = child.kill();
            for _ in 0..10 {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    _ => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
            let _ = child.wait();
        }
    }
}

fn websocket_port_from_url(url: &str) -> Option<u16> {
    let parsed = Url::parse(url).ok()?;
    parsed.port_or_known_default()
}

async fn pick_open_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| format!("bind temp websocket port failed: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("read temp websocket port failed: {}", e))?
        .port();
    drop(listener);
    Ok(port)
}

fn spawn_tui_runtime(config_path: &Path, log_path: &Path) -> Result<Child, String> {
    let exe =
        std::env::current_exe().map_err(|e| format!("resolve current executable failed: {}", e))?;
    let stderr = std::fs::File::create(log_path)
        .map_err(|e| format!("create TUI runtime log failed: {}", e))?;
    Command::new(exe)
        .arg(config_path)
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("spawn TUI runtime failed: {}", e))
}

fn prepare_tui_runtime_config(
    source_config_path: &Path,
    output_config_path: &Path,
    output_runtime_path: &Path,
    websocket_port: u16,
) -> Result<(), String> {
    let source = std::fs::read_to_string(source_config_path)
        .map_err(|e| format!("read source config failed: {}", e))?;
    let mut doc = source
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("parse source config failed: {}", e))?;

    if !doc.as_table().contains_key("gateway") {
        doc["gateway"] = toml_edit::table();
    }
    doc["gateway"]["adapter"] = toml_edit::value("websocket");
    let adapters = toml_edit::Array::from_iter(["websocket"]);
    doc["gateway"]["adapters"] = toml_edit::value(adapters);
    doc["gateway"]["websocket_bind_address"] = toml_edit::value("127.0.0.1");
    doc["gateway"]["websocket_port"] = toml_edit::value(i64::from(websocket_port));

    std::fs::write(output_config_path, doc.to_string())
        .map_err(|e| format!("write TUI config failed: {}", e))?;

    let runtime_source = source_config_path.with_extension("runtime.json");
    if runtime_source.exists() {
        let mut runtime_json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&runtime_source)
                .map_err(|e| format!("read runtime config failed: {}", e))?,
        )
        .map_err(|e| format!("parse runtime config failed: {}", e))?;
        runtime_json["gateway"]["adapter"] = serde_json::Value::String("websocket".into());
        runtime_json["gateway"]["adapters"] = serde_json::json!(["websocket"]);
        runtime_json["gateway"]["websocket_bind_address"] =
            serde_json::Value::String("127.0.0.1".into());
        runtime_json["gateway"]["websocket_port"] =
            serde_json::Value::Number(websocket_port.into());
        std::fs::write(
            output_runtime_path,
            serde_json::to_string_pretty(&runtime_json)
                .map_err(|e| format!("serialize TUI runtime config failed: {}", e))?,
        )
        .map_err(|e| format!("write TUI runtime config failed: {}", e))?;
    }
    Ok(())
}

async fn wait_for_websocket(port: u16) -> Result<(), String> {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    for _ in 0..160 {
        match connect_async(&url).await {
            Ok((mut stream, _)) => {
                let _ = stream.close(None).await;
                return Ok(());
            }
            Err(_) => tokio::time::sleep(Duration::from_millis(150)).await,
        }
    }
    Err(format!(
        "websocket runtime on port {} did not become ready",
        port
    ))
}

struct TuiTerminalGuard {
    stdout: std::io::Stdout,
}

impl TuiTerminalGuard {
    fn enter() -> Result<Self, String> {
        enable_raw_mode().map_err(|e| format!("enable raw mode failed: {}", e))?;
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen, Hide, EnableMouseCapture)
            .map_err(|e| format!("enter alternate screen failed: {}", e))?;
        Ok(Self { stdout })
    }
}

impl Drop for TuiTerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.stdout, Show, DisableMouseCapture, LeaveAlternateScreen);
    }
}

#[derive(Debug, Clone)]
struct TuiLine {
    role: &'static str,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayLine {
    color: Color,
    text: String,
}

struct TuiState {
    config_path: String,
    websocket_port: u16,
    session_id: u64,
    user_id: String,
    log_path: std::path::PathBuf,
    input: String,
    lines: Vec<TuiLine>,
    scroll: usize,
    sidebar_tab: SidebarTab,
    active_agent: String,
    pending_approvals: usize,
    recent_runs: usize,
    last_error: Option<String>,
    available_agents: Vec<String>,
    selected_agent_idx: usize,
    approval_items: Vec<ApprovalItem>,
    selected_approval_idx: usize,
    approval_detail_cache: HashMap<String, ApprovalDetail>,
    recent_run_signals: VecDeque<String>,
    recent_tool_events: VecDeque<String>,
    recent_context_events: VecDeque<String>,
    recent_health_events: VecDeque<String>,
    run_rows: Vec<String>,
    visible_tool_rows: Vec<String>,
    hidden_tool_rows: Vec<String>,
    context_plan_rows: Vec<String>,
    provider_health_rows: Vec<String>,
    provider_circuit_rows: Vec<String>,
    mcp_status_rows: Vec<String>,
    workspace_lock_rows: Vec<String>,
    failure_summary_rows: Vec<String>,
    error_events: usize,
    notifications: VecDeque<String>,
    last_layout: Option<TuiLayout>,
    show_approval_detail: bool,
    show_command_palette: bool,
    command_palette_query: String,
    selected_command_idx: usize,
    bootstrap_complete: bool,
}

impl TuiState {
    fn new(
        config_path: String,
        websocket_port: u16,
        prefs: TuiPreferences,
        log_path: std::path::PathBuf,
    ) -> Self {
        Self {
            config_path,
            websocket_port,
            session_id: stable_tui_session_id(),
            user_id: "tui_user".into(),
            log_path,
            input: String::new(),
            lines: vec![TuiLine {
                role: "system",
                text: "HiveClaw Terminal UI ready. Enter to send, F1 help, F2 /agents, F3 /approvals, F4 /runs, Ctrl+C to quit.".into(),
            }],
            scroll: 0,
            sidebar_tab: prefs.sidebar_tab,
            active_agent: prefs.active_agent,
            pending_approvals: 0,
            recent_runs: 0,
            last_error: None,
            available_agents: Vec::new(),
            selected_agent_idx: 0,
            approval_items: Vec::new(),
            selected_approval_idx: 0,
            approval_detail_cache: HashMap::new(),
            recent_run_signals: VecDeque::new(),
            recent_tool_events: VecDeque::new(),
            recent_context_events: VecDeque::new(),
            recent_health_events: VecDeque::new(),
            run_rows: Vec::new(),
            visible_tool_rows: Vec::new(),
            hidden_tool_rows: Vec::new(),
            context_plan_rows: Vec::new(),
            provider_health_rows: Vec::new(),
            provider_circuit_rows: Vec::new(),
            mcp_status_rows: Vec::new(),
            workspace_lock_rows: Vec::new(),
            failure_summary_rows: Vec::new(),
            error_events: 0,
            notifications: VecDeque::new(),
            last_layout: None,
            show_approval_detail: false,
            show_command_palette: false,
            command_palette_query: String::new(),
            selected_command_idx: 0,
            bootstrap_complete: false,
        }
    }

    fn push_user(&mut self, text: String) {
        if let Some(agent) = text.strip_prefix("/agent ") {
            let trimmed = agent.trim();
            if !trimmed.is_empty() && trimmed != "clear" && trimmed != "reset" {
                self.active_agent = trimmed.to_string();
            }
        }
        self.lines.push(TuiLine { role: "you", text });
        self.scroll = 0;
    }

    fn push_assistant(&mut self, text: String) {
        self.ingest_runtime_signal(&text);
        self.refresh_operator_snapshot();
        self.lines.push(TuiLine { role: "aria", text });
        self.scroll = 0;
    }

    fn push_system(&mut self, text: String) {
        self.push_health_event(text.clone());
        self.push_notification(text.clone());
        self.lines.push(TuiLine {
            role: "system",
            text,
        });
        self.scroll = 0;
    }

    fn make_request(&self, text: &str) -> String {
        serde_json::json!({
            "session_id": self.session_id,
            "user_id": self.user_id,
            "text": text,
            "timestamp_us": chrono::Utc::now().timestamp_micros() as u64,
        })
        .to_string()
    }

    fn handle_event(&mut self, evt: Event) -> Option<TuiAction> {
        if self.show_command_palette {
            return self.handle_command_palette_event(evt);
        }
        match evt {
            Event::Key(KeyEvent {
                code: KeyCode::Char('p'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => {
                self.show_command_palette = true;
                self.command_palette_query.clear();
                self.selected_command_idx = 0;
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::CONTROL) => Some(TuiAction::Quit),
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => {
                if self.show_approval_detail {
                    self.show_approval_detail = false;
                    None
                } else {
                    Some(TuiAction::Quit)
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::F(2),
                ..
            }) => {
                self.sidebar_tab = SidebarTab::Agents;
                Some(TuiAction::Send("/agents".into()))
            }
            Event::Key(KeyEvent {
                code: KeyCode::F(3),
                ..
            }) => {
                self.sidebar_tab = SidebarTab::Approvals;
                Some(TuiAction::Send("/approvals".into()))
            }
            Event::Key(KeyEvent {
                code: KeyCode::F(4),
                ..
            }) => {
                self.sidebar_tab = SidebarTab::Runs;
                Some(TuiAction::Send("/runs".into()))
            }
            Event::Key(KeyEvent {
                code: KeyCode::F(5),
                ..
            }) => {
                self.lines.clear();
                self.push_system("Transcript cleared.".into());
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::F(1),
                ..
            }) => {
                self.push_system(
                    "Shortcuts: F2 /agents, F3 /approvals, F4 /runs, Ctrl+C quit.".into(),
                );
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Tab, ..
            }) => {
                self.sidebar_tab = self.sidebar_tab.next();
                None
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    self.scroll = self.scroll.saturating_add(3);
                    None
                }
                MouseEventKind::ScrollDown => {
                    self.scroll = self.scroll.saturating_sub(3);
                    None
                }
                MouseEventKind::Down(_) => self.handle_mouse_click(mouse.column, mouse.row),
                _ => None,
            },
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_selection_up();
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_selection_down();
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::PageUp,
                ..
            }) => {
                self.scroll = self.scroll.saturating_add(5);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::PageDown,
                ..
            }) => {
                self.scroll = self.scroll.saturating_sub(5);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up,
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::SHIFT) => {
                self.scroll = self.scroll.saturating_add(1);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down,
                modifiers,
                ..
            }) if modifiers.contains(KeyModifiers::SHIFT) => {
                self.scroll = self.scroll.saturating_sub(1);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                self.input.pop();
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('i'),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && self.sidebar_tab == SidebarTab::Approvals =>
            {
                self.show_approval_detail = !self.show_approval_detail;
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('a'),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && self.sidebar_tab == SidebarTab::Approvals =>
            {
                self.selected_approval()
                    .map(|item| TuiAction::Send(format!("/approve {}", item.handle)))
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('d'),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && self.sidebar_tab == SidebarTab::Approvals =>
            {
                self.selected_approval()
                    .map(|item| TuiAction::Send(format!("/deny {}", item.handle)))
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                if self.input.trim().is_empty() {
                    if let Some(action) = self.activate_sidebar_selection() {
                        return Some(action);
                    }
                }
                let text = self.input.trim().to_string();
                self.input.clear();
                if text.is_empty() {
                    None
                } else if !self.bootstrap_complete && !is_bootstrap_safe_command(&text) {
                    self.push_system(format!(
                        "Waiting for session agent override '{}'. Retry in a moment or use /agent <name>.",
                        self.active_agent
                    ));
                    None
                } else {
                    Some(TuiAction::Send(text))
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.input.push(ch);
                None
            }
            _ => None,
        }
    }

    fn render(&mut self, stdout: &mut std::io::Stdout) -> Result<(), String> {
        let (cols, rows) =
            terminal::size().map_err(|e| format!("read terminal size failed: {}", e))?;
        let right_w: u16 = if cols < 96 { 26 } else { 32 };
        let input_h: u16 = 4;
        let header_h: u16 = 3;
        let chat_w = cols.saturating_sub(right_w + 3);
        let chat_h = rows.saturating_sub(header_h + input_h + 3);
        let layout = TuiLayout::new(cols, rows, header_h, input_h, chat_w, chat_h, right_w);

        queue!(stdout, MoveTo(0, 0), Clear(ClearType::All)).map_err(|e| e.to_string())?;
        draw_box(stdout, 0, 0, cols, header_h, " HiveClaw TUI ")?;
        draw_box(stdout, 0, header_h, chat_w, chat_h + 1, " Transcript ")?;
        draw_box(
            stdout,
            chat_w,
            header_h,
            right_w,
            chat_h + 1,
            if layout.compact_mode {
                " Panel "
            } else {
                " Status "
            },
        )?;
        draw_box(
            stdout,
            0,
            header_h + chat_h + 1,
            cols,
            input_h + 1,
            " Input ",
        )?;

        let ws_label = format!("ws {}", self.websocket_port);
        let ws_x = cols.saturating_sub((ws_label.len() as u16).saturating_add(2));
        let config_room = ws_x.saturating_sub(3);
        write_at(
            stdout,
            2,
            1,
            Color::Blue,
            &truncate_with_ellipsis(
                &format!("config {}", self.config_path),
                config_room.saturating_sub(2) as usize,
            ),
        )?;
        let scroll_hint = if self.scroll == 0 { "live" } else { "scroll" };
        let mut badge_x = 2u16.max(config_room.saturating_sub(38));
        badge_x = badge_x.max(22);
        badge_x = draw_header_badge(
            stdout,
            badge_x,
            1,
            Color::Green,
            &format!("agent {}", self.active_agent),
        )?;
        badge_x = draw_header_badge(
            stdout,
            badge_x,
            1,
            Color::Yellow,
            &format!("appr {}", self.pending_approvals),
        )?;
        badge_x = draw_header_badge(
            stdout,
            badge_x,
            1,
            Color::Cyan,
            &format!("runs {}", self.recent_runs),
        )?;
        if !self.notifications.is_empty() && badge_x < ws_x.saturating_sub(10) {
            badge_x = draw_header_badge(
                stdout,
                badge_x,
                1,
                Color::Magenta,
                &format!("notes {}", self.notifications.len()),
            )?;
        }
        if self.last_error.is_some() && badge_x < ws_x.saturating_sub(8) {
            let _ = draw_header_badge(stdout, badge_x, 1, Color::Red, "error")?;
        }
        write_at(stdout, ws_x, 1, Color::DarkCyan, &ws_label)?;
        if ws_x > 8 {
            write_at(
                stdout,
                ws_x.saturating_sub(scroll_hint.len() as u16 + 2),
                1,
                Color::Yellow,
                scroll_hint,
            )?;
        }

        self.draw_sidebar_tabs(stdout, &layout)?;

        let mut transcript_lines = Vec::new();
        for line in &self.lines {
            for wrapped in
                transcript_display_lines(line.role, &line.text, chat_w.saturating_sub(4) as usize)
            {
                transcript_lines.push(wrapped);
            }
        }
        let visible_capacity = chat_h.saturating_sub(1) as usize;
        let total_lines = transcript_lines.len();
        let end = total_lines.saturating_sub(self.scroll);
        let start = end.saturating_sub(visible_capacity);
        let visible = transcript_lines[start..end].to_vec();
        let mut y = header_h + 1;
        for line in visible {
            write_at(stdout, 2, y, line.color, &line.text)?;
            y += 1;
        }

        let mut sy = layout.sidebar_content_y;
        for line in self.sidebar_lines().lines() {
            for wrapped in wrap_text(line, right_w.saturating_sub(4) as usize) {
                if sy >= layout.sidebar_max_y {
                    break;
                }
                let color = if line.starts_with("tab ") {
                    Color::Cyan
                } else if line.starts_with("> ") {
                    Color::White
                } else if line.starts_with("error ") {
                    Color::Red
                } else if line.starts_with("approvals ") {
                    Color::Yellow
                } else {
                    Color::DarkYellow
                };
                write_at(stdout, chat_w + 2, sy, color, &wrapped)?;
                sy += 1;
            }
        }

        write_at(
            stdout,
            2,
            header_h + chat_h + 2,
            Color::Magenta,
            &truncate_from_end(&self.input, cols.saturating_sub(6) as usize),
        )?;
        let footer_hint = if layout.compact_mode {
            "F1 help  Tab"
        } else {
            "F1 help  Tab panes"
        };
        write_at(
            stdout,
            cols.saturating_sub(footer_hint.len() as u16 + 2),
            header_h + chat_h + 3,
            Color::DarkGrey,
            footer_hint,
        )?;
        if self.show_approval_detail {
            self.draw_approval_overlay(stdout, cols, rows)?;
        }
        if self.show_command_palette {
            self.draw_command_palette_overlay(stdout, cols, rows)?;
        }
        let cursor_input = truncate_from_end(&self.input, cols.saturating_sub(6) as usize);
        if self.show_command_palette {
            let overlay_width = cols.min(72).max(38);
            let overlay_height = rows.min(16).max(10);
            let overlay_x = cols.saturating_sub(overlay_width) / 2;
            let overlay_y = rows.saturating_sub(overlay_height) / 2;
            let query = truncate_from_end(
                &self.command_palette_query,
                overlay_width.saturating_sub(12) as usize,
            );
            queue!(stdout, MoveTo(overlay_x + 8 + query.len() as u16, overlay_y + 1))
                .map_err(|e| e.to_string())?;
        } else {
            queue!(
                stdout,
                MoveTo((2 + cursor_input.len()) as u16, header_h + chat_h + 2)
            )
            .map_err(|e| e.to_string())?;
        }
        // Render updates layout for mouse interaction after all coordinates are known.
        self.last_layout = Some(layout);
        stdout
            .flush()
            .map_err(|e| format!("flush TUI failed: {}", e))
    }

    fn draw_sidebar_tabs(
        &self,
        stdout: &mut std::io::Stdout,
        layout: &TuiLayout,
    ) -> Result<(), String> {
        let mut x = layout.sidebar_x + 2;
        let y = layout.header_h + 1;
        for tab in SidebarTab::all() {
            let label_text = if layout.compact_mode {
                tab.compact_label()
            } else {
                tab.label()
            };
            let (label, color) = if tab == self.sidebar_tab {
                (format!("[{}]", label_text), Color::Cyan)
            } else {
                (format!(" {} ", label_text), Color::DarkGrey)
            };
            write_at(stdout, x, y, color, &label)?;
            x = x.saturating_add(label.len() as u16 + 1);
        }
        Ok(())
    }

    fn draw_approval_overlay(
        &self,
        stdout: &mut std::io::Stdout,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let Some(selected) = self.selected_approval() else {
            return Ok(());
        };
        let width = cols.min(70).saturating_sub(4).max(32);
        let height = rows.min(14).max(10);
        let x = cols.saturating_sub(width) / 2;
        let y = rows.saturating_sub(height) / 2;
        draw_box(stdout, x, y, width, height, " Approval Detail ")?;
        let mut lines = vec![
            format!("handle   {}", selected.handle),
            format!("approval {}", selected.approval_id),
            format!("action   {}", selected.summary),
        ];
        if let Some(target) = selected
            .detail
            .as_ref()
            .and_then(|detail| detail.target_summary.as_ref())
            .or(selected.target_summary.as_ref())
        {
            lines.push(format!("target   {}", target));
        }
        if let Some(risk) = selected
            .detail
            .as_ref()
            .and_then(|detail| detail.risk_summary.as_ref())
        {
            lines.push(format!("risk     {}", risk));
        }
        if let Some(options) = selected
            .detail
            .as_ref()
            .and_then(|detail| detail.options_summary.as_ref())
        {
            lines.push(format!("options  {}", truncate_with_ellipsis(options, 44)));
        }
        if let Some(arguments) = selected
            .detail
            .as_ref()
            .and_then(|detail| detail.arguments_preview.as_ref())
        {
            lines.push(String::new());
            lines.push("arguments".into());
            for arg_line in arguments.lines().take(3) {
                lines.push(truncate_with_ellipsis(arg_line.trim(), 44));
            }
        }
        lines.push(String::new());
        lines.push("Enter/a approve".into());
        lines.push("d deny".into());
        lines.push("i or Esc close".into());
        let mut row = y + 1;
        for line in lines {
            if row >= y + height - 1 {
                break;
            }
            write_at(stdout, x + 2, row, Color::Yellow, &line)?;
            row += 1;
        }
        Ok(())
    }

    fn draw_command_palette_overlay(
        &self,
        stdout: &mut std::io::Stdout,
        cols: u16,
        rows: u16,
    ) -> Result<(), String> {
        let width = cols.min(72).max(38);
        let height = rows.min(16).max(10);
        let x = cols.saturating_sub(width) / 2;
        let y = rows.saturating_sub(height) / 2;
        draw_box(stdout, x, y, width, height, " Command Palette ")?;
        write_at(stdout, x + 2, y + 1, Color::Cyan, "query")?;
        write_at(
            stdout,
            x + 8,
            y + 1,
            Color::White,
            &truncate_from_end(
                &self.command_palette_query,
                width.saturating_sub(12) as usize,
            ),
        )?;
        let commands = self.filtered_palette_commands();
        let mut row = y + 3;
        if commands.is_empty() {
            write_at(stdout, x + 2, row, Color::DarkGrey, "no matching commands")?;
        } else {
            for (idx, item) in commands
                .iter()
                .take(height.saturating_sub(6) as usize)
                .enumerate()
            {
                let marker = if idx == self.selected_command_idx { ">" } else { " " };
                let color = if idx == self.selected_command_idx {
                    Color::Yellow
                } else {
                    Color::DarkYellow
                };
                let label = format!("{} {} - {}", marker, item.label, item.hint);
                write_at(
                    stdout,
                    x + 2,
                    row,
                    color,
                    &truncate_with_ellipsis(&label, width.saturating_sub(4) as usize),
                )?;
                row += 1;
            }
        }
        write_at(
            stdout,
            x + 2,
            y + height - 2,
            Color::DarkGrey,
            "Enter run  Esc close  Up/Down select",
        )?;
        Ok(())
    }

    fn sidebar_lines(&self) -> String {
        match self.sidebar_tab {
            SidebarTab::Summary => {
                let mut lines = vec![
                    format!("tab summary"),
                    format!("session {}", self.session_id),
                    format!("agent {}", self.active_agent),
                    format!("approvals {}", self.pending_approvals),
                    format!("runs {}", self.recent_runs),
                ];
                if !self.last_layout.is_some_and(|layout| layout.compact_mode) {
                    lines.insert(2, format!("user {}", self.user_id));
                }
                if let Some(err) = &self.last_error {
                    lines.push(String::new());
                    lines.push(format!("error {}", err));
                }
                lines.join("\n")
            }
            SidebarTab::Runs => {
                let mut lines = vec![
                    "tab runs".into(),
                    "F4 or /runs refresh".into(),
                    format!("known runs {}", self.recent_runs),
                    format!("messages {}", self.lines.len()),
                    String::new(),
                ];
                if !self.run_rows.is_empty() {
                    lines.push("latest runs".into());
                    lines.extend(self.run_rows.iter().cloned());
                } else if self.recent_run_signals.is_empty() {
                    lines.push("no run activity captured yet".into());
                } else {
                    lines.push("recent run activity".into());
                    lines.extend(self.recent_run_signals.iter().cloned());
                }
                lines.join("\n")
            }
            SidebarTab::Agents => {
                let mut lines = vec![
                    "tab agents".into(),
                    "Enter switch agent".into(),
                    if self.last_layout.is_some_and(|layout| layout.compact_mode) {
                        "F2 refresh".into()
                    } else {
                        "F2 refresh list".into()
                    },
                    String::new(),
                ];
                if self.available_agents.is_empty() {
                    lines.push("no agent list loaded".into());
                } else {
                    for (idx, agent) in self.available_agents.iter().enumerate() {
                        let marker = if idx == self.selected_agent_idx {
                            ">"
                        } else {
                            " "
                        };
                        let current = if agent == &self.active_agent {
                            " *"
                        } else {
                            ""
                        };
                        lines.push(format!("{} {}{}", marker, agent, current));
                    }
                }
                lines.join("\n")
            }
            SidebarTab::Approvals => {
                let mut lines = vec![
                    "tab approvals".into(),
                    "Enter/a approve".into(),
                    "d deny".into(),
                    if self.last_layout.is_some_and(|layout| layout.compact_mode) {
                        "i info".into()
                    } else {
                        "i detail".into()
                    },
                    String::new(),
                ];
                if self.approval_items.is_empty() {
                    lines.push("no pending approvals".into());
                } else {
                    for (idx, item) in self.approval_items.iter().enumerate() {
                        let marker = if idx == self.selected_approval_idx {
                            ">"
                        } else {
                            " "
                        };
                        let risk = item
                            .detail
                            .as_ref()
                            .and_then(|detail| detail.risk_summary.as_deref())
                            .unwrap_or("risk pending");
                        let target = item
                            .detail
                            .as_ref()
                            .and_then(|detail| detail.target_summary.as_deref())
                            .or(item.target_summary.as_deref())
                            .map(|value| format!(" ({})", value))
                            .unwrap_or_default();
                        lines.push(format!(
                            "{} {}{} [{}]",
                            marker, item.summary, target, item.handle
                        ));
                        lines.push(format!("  {}", truncate_with_ellipsis(risk, 42)));
                    }
                    if let Some(selected) = self.selected_approval() {
                        lines.push(String::new());
                        lines.push("preview".into());
                        lines.push(format!("handle {}", selected.handle));
                        lines.push(format!("approval {}", selected.approval_id));
                        lines.push(format!("action {}", selected.summary));
                        if let Some(target) = selected
                            .detail
                            .as_ref()
                            .and_then(|detail| detail.target_summary.as_ref())
                            .or(selected.target_summary.as_ref())
                        {
                            lines.push(format!("target {}", target));
                        }
                        if let Some(risk) = selected
                            .detail
                            .as_ref()
                            .and_then(|detail| detail.risk_summary.as_ref())
                        {
                            lines.push(format!("risk {}", risk));
                        }
                    }
                }
                lines.join("\n")
            }
            SidebarTab::ToolsContext => {
                let mut lines = vec![
                    "tab tools/context".into(),
                    format!("tool events {}", self.recent_tool_events.len()),
                    format!("context events {}", self.recent_context_events.len()),
                    String::new(),
                ];
                if !self.visible_tool_rows.is_empty() {
                    lines.push("visible tools".into());
                    lines.extend(self.visible_tool_rows.iter().cloned());
                } else if self.recent_tool_events.is_empty() {
                    lines.push("tools: no activity yet".into());
                } else {
                    lines.push("tools".into());
                    lines.extend(self.recent_tool_events.iter().cloned());
                }
                lines.push(String::new());
                if !self.hidden_tool_rows.is_empty() {
                    lines.push("hidden tools".into());
                    lines.extend(self.hidden_tool_rows.iter().cloned());
                    lines.push(String::new());
                }
                if !self.context_plan_rows.is_empty() {
                    lines.push("context plan".into());
                    lines.extend(self.context_plan_rows.iter().cloned());
                } else if self.recent_context_events.is_empty() {
                    lines.push("context: no activity yet".into());
                } else {
                    lines.push("context".into());
                    lines.extend(self.recent_context_events.iter().cloned());
                }
                lines.join("\n")
            }
            SidebarTab::SystemHealth => {
                let transcript_state = if self.scroll == 0 {
                    "live tail"
                } else {
                    "manual scroll"
                };
                let mut lines = vec![
                    "tab system/health".into(),
                    format!("bootstrap {}", if self.bootstrap_complete { "ok" } else { "pending" }),
                    format!("ws 127.0.0.1:{}", self.websocket_port),
                    format!("view {}", transcript_state),
                    format!("errors {}", self.error_events),
                ];
                if let Some(err) = &self.last_error {
                    lines.push(format!("last error {}", err));
                }
                if !self.recent_health_events.is_empty() {
                    lines.push(String::new());
                    lines.push("recent health".into());
                    lines.extend(self.recent_health_events.iter().cloned());
                }
                if !self.provider_health_rows.is_empty() {
                    lines.push(String::new());
                    lines.push("provider health".into());
                    lines.extend(self.provider_health_rows.iter().cloned());
                }
                if !self.provider_circuit_rows.is_empty() {
                    lines.push(String::new());
                    lines.push("provider circuits".into());
                    lines.extend(self.provider_circuit_rows.iter().cloned());
                }
                if !self.mcp_status_rows.is_empty() {
                    lines.push(String::new());
                    lines.push("mcp".into());
                    lines.extend(self.mcp_status_rows.iter().cloned());
                }
                if !self.workspace_lock_rows.is_empty() {
                    lines.push(String::new());
                    lines.push("workspace locks".into());
                    lines.extend(self.workspace_lock_rows.iter().cloned());
                }
                if !self.failure_summary_rows.is_empty() {
                    lines.push(String::new());
                    lines.push("why this happened".into());
                    lines.extend(self.failure_summary_rows.iter().cloned());
                }
                if let Some(log_tail) = self.runtime_log_tail() {
                    lines.push(String::new());
                    lines.push("runtime log".into());
                    lines.extend(log_tail.lines().map(|line| line.to_string()));
                }
                lines.join("\n")
            }
            SidebarTab::Notifications => {
                let mut lines = vec!["tab notifications".into()];
                if self.notifications.is_empty() {
                    lines.push("no recent notifications".into());
                } else {
                    lines.extend(self.notifications.iter().cloned());
                }
                lines.join("\n")
            }
        }
    }

    fn ingest_runtime_signal(&mut self, text: &str) {
        if let Some(agent) = text
            .strip_prefix("Session override set to agent: ")
            .and_then(|value| value.strip_suffix('.'))
        {
            self.active_agent = agent.trim().to_string();
            self.bootstrap_complete = true;
            self.push_notification(format!("agent {}", self.active_agent));
        }
        if let Some(agent) = text
            .strip_prefix("Current agent override: ")
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && *value != "none")
        {
            self.active_agent = agent.to_string();
            self.bootstrap_complete = true;
            self.push_notification(format!("agent {}", self.active_agent));
        }
        if text.starts_with("Available agents:") {
            self.available_agents = parse_agent_list(text);
            if self.selected_agent_idx >= self.available_agents.len() {
                self.selected_agent_idx = self.available_agents.len().saturating_sub(1);
            }
            self.push_notification(format!("loaded {} agents", self.available_agents.len()));
        }
        if text.starts_with("Pending approvals:") || text == "No pending approvals." {
            self.approval_items = parse_approval_list(text)
                .into_iter()
                .map(|mut item| {
                    item.detail = self
                        .approval_detail_cache
                        .get(&item.handle)
                        .cloned()
                        .or_else(|| self.approval_detail_cache.get(&item.approval_id).cloned());
                    if item.target_summary.is_none() {
                        item.target_summary = item
                            .detail
                            .as_ref()
                            .and_then(|detail| detail.target_summary.clone());
                    }
                    item
                })
                .collect();
            self.pending_approvals = self.approval_items.len();
            if self.selected_approval_idx >= self.approval_items.len() {
                self.selected_approval_idx = self.approval_items.len().saturating_sub(1);
            }
            self.push_notification(format!("approvals {}", self.pending_approvals));
        }
        if text.starts_with("Provider health:") || text == "No provider circuits are open." {
            self.provider_circuit_rows = parse_provider_health_list(text);
        }
        if text.starts_with("Workspace locks:") || text == "No active workspace locks." {
            self.workspace_lock_rows = parse_workspace_lock_list(text);
        }
        if let Some((handle, approval_id, detail)) = parse_pending_approval_detail(text) {
            self.approval_detail_cache
                .insert(handle.clone(), detail.clone());
            self.approval_detail_cache.insert(approval_id.clone(), detail.clone());
            if let Some(item) = self
                .approval_items
                .iter_mut()
                .find(|item| item.handle == handle || item.approval_id == approval_id)
            {
                item.detail = Some(detail);
            }
        }
        if text.contains("Stored pending approval") {
            self.pending_approvals = self.pending_approvals.saturating_add(1);
            self.push_notification("approval required".into());
        }
        if text.starts_with("Approved '") && self.pending_approvals > 0 {
            self.pending_approvals = self.pending_approvals.saturating_sub(1);
            self.push_notification("approval resolved".into());
        }
        if let Some(count) = extract_prefixed_count(text, "Found ", " runs") {
            self.recent_runs = count;
            self.push_run_signal(format!("found {} runs", count));
            self.push_notification(format!("runs {}", count));
        }
        if text.to_ascii_lowercase().contains("error")
            || text.to_ascii_lowercase().contains("failed")
        {
            self.error_events = self.error_events.saturating_add(1);
            self.last_error = Some(
                text.lines()
                    .next()
                    .unwrap_or(text)
                    .chars()
                    .take(72)
                    .collect(),
            );
            if let Some(err) = self.last_error.clone() {
                self.push_health_event(format!("error {}", err));
                self.push_notification(format!("error {}", err));
            }
        }
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            if is_run_signal_line(line) {
                self.push_run_signal(line.to_string());
            }
            if is_tool_signal_line(line) {
                self.push_tool_event(line.to_string());
            }
            if is_context_signal_line(line) {
                self.push_context_event(line.to_string());
            }
            if is_health_signal_line(line) {
                self.push_health_event(line.to_string());
            }
        }
        if text.contains("I am not confident which agent should handle this request") {
            self.push_system(format!(
                "Routing clarification triggered. Use /agent <name> to pin the session, for example /agent {}.",
                self.active_agent
            ));
        }
    }

    fn move_selection_up(&mut self) {
        match self.sidebar_tab {
            SidebarTab::Agents => {
                self.selected_agent_idx = self.selected_agent_idx.saturating_sub(1);
            }
            SidebarTab::Approvals => {
                self.selected_approval_idx = self.selected_approval_idx.saturating_sub(1);
            }
            _ => {}
        }
    }

    fn move_selection_down(&mut self) {
        match self.sidebar_tab {
            SidebarTab::Agents => {
                if self.selected_agent_idx + 1 < self.available_agents.len() {
                    self.selected_agent_idx += 1;
                }
            }
            SidebarTab::Approvals => {
                if self.selected_approval_idx + 1 < self.approval_items.len() {
                    self.selected_approval_idx += 1;
                }
            }
            _ => {}
        }
    }

    fn activate_sidebar_selection(&self) -> Option<TuiAction> {
        match self.sidebar_tab {
            SidebarTab::Agents => self
                .available_agents
                .get(self.selected_agent_idx)
                .map(|agent| TuiAction::Send(format!("/agent {}", agent))),
            SidebarTab::Approvals => self
                .selected_approval()
                .map(|item| TuiAction::Send(format!("/approve {}", item.handle))),
            _ => None,
        }
    }

    fn selected_approval(&self) -> Option<&ApprovalItem> {
        self.approval_items.get(self.selected_approval_idx)
    }

    fn runtime_log_tail(&self) -> Option<String> {
        let content = std::fs::read_to_string(&self.log_path).ok()?;
        let lines = content.lines().rev().take(6).collect::<Vec<_>>();
        if lines.is_empty() {
            None
        } else {
            Some(lines.into_iter().rev().collect::<Vec<_>>().join("\n"))
        }
    }

    fn refresh_operator_snapshot(&mut self) {
        let Ok(cfg) = crate::load_config(&self.config_path) else {
            return;
        };
        let session_uuid = websocket_session_uuid(self.session_id);
        let session_str = session_uuid.to_string();
        let store = RuntimeStore::for_sessions_dir(Path::new(&cfg.ssmu.sessions_dir));

        if let Ok(runs) = store.list_agent_runs_for_session(session_uuid) {
            self.recent_runs = runs.len();
            self.run_rows = summarize_run_rows(&runs);
        }

        if let Ok(approvals) = store.list_approvals(
            Some(*session_uuid.as_bytes()),
            Some(&self.user_id),
            Some(aria_core::ApprovalStatus::Pending),
        ) {
            self.pending_approvals = approvals.len();
            self.approval_items = approvals
                .into_iter()
                .take(12)
                .map(|record| {
                    let descriptor = build_approval_descriptor(&record);
                    let handle = ensure_approval_handle(Path::new(&cfg.ssmu.sessions_dir), &record)
                        .unwrap_or_else(|_| record.approval_id.clone());
                    let detail = ApprovalDetail {
                        action_summary: descriptor.action_summary.clone(),
                        target_summary: descriptor.target_summary.clone(),
                        risk_summary: Some(descriptor.risk_summary.clone()),
                        arguments_preview: Some(descriptor.arguments_preview.clone()),
                        options_summary: Some(descriptor.options.join(", ")),
                    };
                    self.approval_detail_cache
                        .insert(handle.clone(), detail.clone());
                    self.approval_detail_cache
                        .insert(record.approval_id.clone(), detail.clone());
                    ApprovalItem {
                        summary: descriptor.action_summary,
                        handle,
                        approval_id: record.approval_id,
                        target_summary: descriptor.target_summary,
                        detail: Some(detail),
                    }
                })
                .collect();
            if self.selected_approval_idx >= self.approval_items.len() {
                self.selected_approval_idx = self.approval_items.len().saturating_sub(1);
            }
        }

        if let Ok(records) = store.list_context_inspections(Some(&session_str), None) {
            let latest = records
                .iter()
                .find(|record| record.agent_id == self.active_agent)
                .or_else(|| records.first());
            if let Some(record) = latest {
                let visible_tools = record
                    .tool_selection
                    .as_ref()
                    .map(|selection| selection.selected_tool_names.clone())
                    .filter(|items| !items.is_empty())
                    .unwrap_or_else(|| record.active_tool_names.clone());
                self.visible_tool_rows = visible_tools
                    .into_iter()
                    .take(8)
                    .map(|name| format!("- {}", name))
                    .collect();
                self.hidden_tool_rows = record
                    .hidden_tool_messages
                    .iter()
                    .take(6)
                    .map(|msg| format!("- {}", truncate_with_ellipsis(msg, 44)))
                    .collect();
                self.context_plan_rows = record
                    .pack
                    .context_plan
                    .as_ref()
                    .map(|plan| {
                        let mut rows = Vec::new();
                        if let Some(summary) = &plan.summary {
                            rows.push(truncate_with_ellipsis(summary, 44));
                        }
                        for block in plan.block_records.iter().take(6) {
                            rows.push(format!(
                                "{:?} {} ({})",
                                block.decision,
                                truncate_with_ellipsis(&block.label, 20),
                                block.token_estimate
                            ));
                        }
                        if let Some(ambiguity) = &plan.ambiguity {
                            rows.push(format!(
                                "ambiguity {:?}",
                                ambiguity.outcome
                            ));
                        }
                        rows
                    })
                    .unwrap_or_default();
                self.provider_health_rows = record
                    .provider_model
                    .as_ref()
                    .map(|model| {
                        vec![format!(
                            "active backend {}",
                            truncate_with_ellipsis(model, 40)
                        )]
                    })
                    .unwrap_or_default();
                self.provider_health_rows.extend(summarize_tool_provider_readiness_rows(
                    &record.tool_provider_readiness,
                ));
                self.failure_summary_rows = summarize_failure_rows(
                    self.last_error.as_deref(),
                    &record.hidden_tool_messages,
                    record.pack.context_plan.as_ref(),
                    self.pending_approvals,
                );
            }
        }

        self.mcp_status_rows = summarize_mcp_status_rows(&store, &self.active_agent);

    }

    fn persist_preferences(&self) {
        let prefs = TuiPreferences {
            active_agent: self.active_agent.clone(),
            sidebar_tab: self.sidebar_tab,
        };
        let _ = save_tui_preferences(&prefs);
    }

    fn push_notification(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        push_capped(&mut self.notifications, text, 8);
    }

    fn push_run_signal(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        push_capped(&mut self.recent_run_signals, text, 6);
    }

    fn push_tool_event(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        push_capped(&mut self.recent_tool_events, text, 6);
    }

    fn push_context_event(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        push_capped(&mut self.recent_context_events, text, 6);
    }

    fn push_health_event(&mut self, text: String) {
        if text.trim().is_empty() {
            return;
        }
        push_capped(&mut self.recent_health_events, text, 8);
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16) -> Option<TuiAction> {
        if let Some(layout) = self.last_layout {
            if let Some(tab) = layout.tab_at(column, row) {
                self.sidebar_tab = tab;
                return None;
            }
            match self.sidebar_tab {
                SidebarTab::Agents => {
                    if let Some(idx) = layout.sidebar_row_index(row) {
                        let list_idx = idx.saturating_sub(4);
                        if list_idx < self.available_agents.len() {
                            if self.selected_agent_idx == list_idx {
                                return self.activate_sidebar_selection();
                            }
                            self.selected_agent_idx = list_idx;
                        }
                    }
                }
                SidebarTab::Approvals => {
                    if let Some(idx) = layout.sidebar_row_index(row) {
                        let list_idx = idx.saturating_sub(4);
                        if list_idx < self.approval_items.len() {
                            if self.selected_approval_idx == list_idx {
                                self.show_approval_detail = true;
                                return self.activate_sidebar_selection();
                            }
                            self.selected_approval_idx = list_idx;
                            self.show_approval_detail = true;
                        }
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn handle_command_palette_event(&mut self, evt: Event) -> Option<TuiAction> {
        match evt {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => {
                self.show_command_palette = false;
                self.command_palette_query.clear();
                self.selected_command_idx = 0;
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                ..
            }) => {
                self.command_palette_query.pop();
                self.selected_command_idx = 0;
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Up, ..
            }) => {
                self.selected_command_idx = self.selected_command_idx.saturating_sub(1);
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Down, ..
            }) => {
                let max_idx = self.filtered_palette_commands().len().saturating_sub(1);
                if self.selected_command_idx < max_idx {
                    self.selected_command_idx += 1;
                }
                None
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                ..
            }) => {
                let selected = self
                    .filtered_palette_commands()
                    .get(self.selected_command_idx)
                    .cloned();
                self.show_command_palette = false;
                self.command_palette_query.clear();
                self.selected_command_idx = 0;
                selected.and_then(|item| self.execute_palette_command(item))
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(ch),
                modifiers,
                ..
            }) if !modifiers.contains(KeyModifiers::CONTROL)
                && !modifiers.contains(KeyModifiers::ALT) =>
            {
                self.command_palette_query.push(ch);
                self.selected_command_idx = 0;
                None
            }
            _ => None,
        }
    }

    fn filtered_palette_commands(&self) -> Vec<PaletteCommand> {
        let query = self.command_palette_query.trim().to_ascii_lowercase();
        self.palette_commands()
            .into_iter()
            .filter(|item| {
                query.is_empty()
                    || item.label.to_ascii_lowercase().contains(&query)
                    || item.hint.to_ascii_lowercase().contains(&query)
                    || item.search_terms.iter().any(|term| term.contains(&query))
            })
            .collect()
    }

    fn palette_commands(&self) -> Vec<PaletteCommand> {
        let mut items = vec![
            PaletteCommand::new(
                "Refresh agents",
                "Load and open the agents panel",
                &["agents", "refresh agents", "switch agent"],
                PaletteCommandAction::SendAndTab("/agents".into(), SidebarTab::Agents),
            ),
            PaletteCommand::new(
                "Refresh approvals",
                "Load and open pending approvals",
                &["approvals", "approval queue", "review approvals"],
                PaletteCommandAction::SendAndTab("/approvals".into(), SidebarTab::Approvals),
            ),
            PaletteCommand::new(
                "Refresh runs",
                "Load and open recent runs",
                &["runs", "background jobs", "run state"],
                PaletteCommandAction::SendAndTab("/runs".into(), SidebarTab::Runs),
            ),
            PaletteCommand::new(
                "Refresh workspace locks",
                "Load current workspace lock ownership and waiters",
                &["workspace locks", "locks", "busy workspace", "contention"],
                PaletteCommandAction::SendAndTab(
                    "/workspace_locks".into(),
                    SidebarTab::SystemHealth,
                ),
            ),
            PaletteCommand::new(
                "Refresh provider health",
                "Load provider circuit and fallback state",
                &["provider health", "circuits", "fallback", "backends"],
                PaletteCommandAction::SendAndTab(
                    "/provider_health".into(),
                    SidebarTab::SystemHealth,
                ),
            ),
            PaletteCommand::new(
                "Open tools and context",
                "Switch to the tools/context panel",
                &["tools", "context", "tool visibility"],
                PaletteCommandAction::SwitchTab(SidebarTab::ToolsContext),
            ),
            PaletteCommand::new(
                "Open system health",
                "Switch to runtime and health status",
                &["system", "health", "errors", "runtime"],
                PaletteCommandAction::SwitchTab(SidebarTab::SystemHealth),
            ),
            PaletteCommand::new(
                "Inspect current context",
                "Show the CLI command for context inspection",
                &["inspect", "context", "why prompt", "debug context"],
                PaletteCommandAction::SystemMessage(format!(
                    "Inspect current context with: hiveclaw inspect context {} {}",
                    websocket_session_uuid(self.session_id),
                    self.active_agent
                )),
            ),
            PaletteCommand::new(
                "Explain current context",
                "Show the CLI command for human-readable context explanation",
                &["explain", "context", "context summary"],
                PaletteCommandAction::SystemMessage(format!(
                    "Explain current context with: hiveclaw explain context {} {}",
                    websocket_session_uuid(self.session_id),
                    self.active_agent
                )),
            ),
            PaletteCommand::new(
                "Inspect provider payloads",
                "Show the CLI command for provider payload diagnostics",
                &["inspect", "provider payload", "tool payload", "llm payload"],
                PaletteCommandAction::SystemMessage(format!(
                    "Inspect provider payloads with: hiveclaw inspect provider-payloads {} {}",
                    websocket_session_uuid(self.session_id),
                    self.active_agent
                )),
            ),
            PaletteCommand::new(
                "Inspect MCP servers",
                "Show the CLI command for MCP server diagnostics",
                &["inspect", "mcp", "chrome devtools", "mcp servers"],
                PaletteCommandAction::SystemMessage(
                    "Inspect MCP servers with: hiveclaw inspect mcp-servers".into(),
                ),
            ),
            PaletteCommand::new(
                "Inspect workspace locks",
                "Show the CLI command for workspace lock diagnostics",
                &["inspect", "workspace locks", "busy workspace", "contention"],
                PaletteCommandAction::SystemMessage(
                    "Inspect workspace locks with: hiveclaw inspect workspace-locks".into(),
                ),
            ),
            PaletteCommand::new(
                "Inspect provider health",
                "Show the CLI command for provider circuit diagnostics",
                &["inspect", "provider health", "circuit breaker", "fallback"],
                PaletteCommandAction::SystemMessage(
                    "Inspect provider health with: hiveclaw inspect provider-health".into(),
                ),
            ),
            PaletteCommand::new(
                "Open notifications",
                "Show recent operator notices",
                &["notifications", "notes", "messages"],
                PaletteCommandAction::SwitchTab(SidebarTab::Notifications),
            ),
            PaletteCommand::new(
                "Clear transcript",
                "Remove current transcript lines from the TUI",
                &["clear", "reset transcript", "clean panel"],
                PaletteCommandAction::ClearTranscript,
            ),
            PaletteCommand::new(
                "Show shortcuts help",
                "Add a shortcuts reminder to the transcript",
                &["help", "shortcuts", "keys", "palette"],
                PaletteCommandAction::SystemMessage(
                    "Shortcuts: Ctrl+P command palette, F2 agents, F3 approvals, F4 runs, Ctrl+C quit.".into(),
                ),
            ),
        ];

        for agent in &self.available_agents {
            items.push(PaletteCommand::new(
                &format!("Switch agent to {}", agent),
                "Pin the current session to this agent",
                &[agent.as_str(), "agent", "switch"],
                PaletteCommandAction::SendAndTab(
                    format!("/agent {}", agent),
                    SidebarTab::Agents,
                ),
            ));
        }

        items
    }

    fn execute_palette_command(&mut self, item: PaletteCommand) -> Option<TuiAction> {
        match item.action {
            PaletteCommandAction::SendAndTab(command, tab) => {
                self.sidebar_tab = tab;
                Some(TuiAction::Send(command))
            }
            PaletteCommandAction::SwitchTab(tab) => {
                self.sidebar_tab = tab;
                None
            }
            PaletteCommandAction::ClearTranscript => {
                self.lines.clear();
                self.push_system("Transcript cleared.".into());
                None
            }
            PaletteCommandAction::SystemMessage(message) => {
                self.push_system(message);
                None
            }
        }
    }
}

enum TuiAction {
    Quit,
    Send(String),
}

#[derive(Debug, Clone)]
struct PaletteCommand {
    label: String,
    hint: String,
    search_terms: Vec<String>,
    action: PaletteCommandAction,
}

impl PaletteCommand {
    fn new(label: &str, hint: &str, search_terms: &[&str], action: PaletteCommandAction) -> Self {
        Self {
            label: label.to_string(),
            hint: hint.to_string(),
            search_terms: search_terms
                .iter()
                .map(|value| value.to_ascii_lowercase())
                .collect(),
            action,
        }
    }
}

#[derive(Debug, Clone)]
enum PaletteCommandAction {
    SendAndTab(String, SidebarTab),
    SwitchTab(SidebarTab),
    ClearTranscript,
    SystemMessage(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarTab {
    Summary,
    Runs,
    ToolsContext,
    SystemHealth,
    Agents,
    Approvals,
    Notifications,
}

impl SidebarTab {
    fn all() -> [Self; 7] {
        [
            Self::Summary,
            Self::Runs,
            Self::ToolsContext,
            Self::SystemHealth,
            Self::Agents,
            Self::Approvals,
            Self::Notifications,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Runs => "runs",
            Self::ToolsContext => "tools/ctx",
            Self::SystemHealth => "system",
            Self::Agents => "agents",
            Self::Approvals => "approvals",
            Self::Notifications => "notes",
        }
    }

    fn compact_label(self) -> &'static str {
        match self {
            Self::Summary => "sum",
            Self::Runs => "run",
            Self::ToolsContext => "tool",
            Self::SystemHealth => "sys",
            Self::Agents => "agt",
            Self::Approvals => "apr",
            Self::Notifications => "note",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Summary => Self::Runs,
            Self::Runs => Self::ToolsContext,
            Self::ToolsContext => Self::SystemHealth,
            Self::SystemHealth => Self::Agents,
            Self::Agents => Self::Approvals,
            Self::Approvals => Self::Notifications,
            Self::Notifications => Self::Summary,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct TuiLayout {
    header_h: u16,
    sidebar_x: u16,
    sidebar_content_y: u16,
    sidebar_max_y: u16,
    compact_mode: bool,
}

impl TuiLayout {
    fn new(
        _cols: u16,
        _rows: u16,
        header_h: u16,
        input_h: u16,
        chat_w: u16,
        chat_h: u16,
        _right_w: u16,
    ) -> Self {
        let _ = input_h;
        Self {
            header_h,
            sidebar_x: chat_w,
            sidebar_content_y: header_h + 2,
            sidebar_max_y: header_h + chat_h,
            compact_mode: chat_w < 56 || _cols < 100,
        }
    }

    fn tab_at(self, column: u16, row: u16) -> Option<SidebarTab> {
        if row != self.header_h + 1 {
            return None;
        }
        let mut x = self.sidebar_x + 2;
        for tab in SidebarTab::all() {
            let label = if self.compact_mode {
                tab.compact_label()
            } else {
                tab.label()
            };
            let width = (label.len() + 2) as u16;
            if column >= x && column < x + width {
                return Some(tab);
            }
            x = x.saturating_add(width + 1);
        }
        None
    }

    fn sidebar_row_index(self, row: u16) -> Option<usize> {
        if row < self.sidebar_content_y || row >= self.sidebar_max_y {
            return None;
        }
        Some((row - self.sidebar_content_y) as usize)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApprovalItem {
    summary: String,
    handle: String,
    approval_id: String,
    target_summary: Option<String>,
    detail: Option<ApprovalDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ApprovalDetail {
    action_summary: String,
    target_summary: Option<String>,
    risk_summary: Option<String>,
    arguments_preview: Option<String>,
    options_summary: Option<String>,
}

fn draw_box(
    stdout: &mut std::io::Stdout,
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    title: &str,
) -> Result<(), String> {
    if w < 2 || h < 2 {
        return Ok(());
    }
    let horizontal = "─".repeat(w.saturating_sub(2) as usize);
    queue!(
        stdout,
        MoveTo(x, y),
        SetForegroundColor(Color::DarkCyan),
        Print("┌"),
        Print(&horizontal),
        Print("┐")
    )
    .map_err(|e| e.to_string())?;
    for row in 1..h - 1 {
        queue!(
            stdout,
            MoveTo(x, y + row),
            Print("│"),
            MoveTo(x + w - 1, y + row),
            Print("│")
        )
        .map_err(|e| e.to_string())?;
    }
    queue!(
        stdout,
        MoveTo(x, y + h - 1),
        Print("└"),
        Print(&horizontal),
        Print("┘"),
        ResetColor
    )
    .map_err(|e| e.to_string())?;
    let title_x = x.saturating_add(2);
    queue!(
        stdout,
        MoveTo(title_x, y),
        SetForegroundColor(Color::Cyan),
        SetAttribute(Attribute::Bold),
        Print(title),
        SetAttribute(Attribute::Reset),
        ResetColor
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

fn write_at(
    stdout: &mut std::io::Stdout,
    x: u16,
    y: u16,
    color: Color,
    text: &str,
) -> Result<(), String> {
    queue!(
        stdout,
        MoveTo(x, y),
        SetForegroundColor(color),
        Print(text),
        ResetColor
    )
    .map_err(|e| e.to_string())
}

fn draw_header_badge(
    stdout: &mut std::io::Stdout,
    x: u16,
    y: u16,
    color: Color,
    text: &str,
) -> Result<u16, String> {
    let badge = format!("[{}]", text);
    write_at(stdout, x, y, color, &badge)?;
    Ok(x.saturating_add(badge.len() as u16 + 1))
}

fn truncate_with_ellipsis(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let kept = width - 3;
    let mut result = text.chars().take(kept).collect::<String>();
    result.push_str("...");
    result
}

fn truncate_from_end(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let char_count = text.chars().count();
    if char_count <= width {
        return text.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let kept = width - 3;
    let suffix = text
        .chars()
        .skip(char_count.saturating_sub(kept))
        .collect::<String>();
    format!("...{}", suffix)
}

fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.is_empty() {
            current.push_str(word);
        } else if current.len() + 1 + word.len() <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(current);
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn transcript_display_lines(role: &str, text: &str, width: usize) -> Vec<DisplayLine> {
    let prefix = match role {
        "you" => "YOU",
        "aria" => "ARIA",
        _ => "SYS",
    };
    let mut lines = Vec::new();
    let mut in_code = false;
    for raw_line in text.lines() {
        let trimmed = raw_line.trim_end();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            lines.push(DisplayLine {
                color: Color::Blue,
                text: format!("{}  {}", prefix, if in_code { "┌ code" } else { "└ code" }),
            });
            continue;
        }
        if in_code {
            let visible = if trimmed.len() > width.saturating_sub(prefix.len() + 4) {
                &trimmed[..width.saturating_sub(prefix.len() + 7)]
            } else {
                trimmed
            };
            lines.push(DisplayLine {
                color: Color::Blue,
                text: format!("{}  │ {}", prefix, visible),
            });
            continue;
        }
        if let Some(json_lines) = transcript_json_lines(prefix, trimmed, width) {
            lines.extend(json_lines);
            continue;
        }
        if let Some(event_lines) = transcript_event_card_lines(prefix, role, trimmed, width) {
            lines.extend(event_lines);
            continue;
        }
        let source = if trimmed.is_empty() { " " } else { trimmed };
        let marker = transcript_marker(trimmed);
        let content_width = width.saturating_sub(prefix.len() + 2 + marker.len());
        for (index, wrapped) in wrap_text(source, width.saturating_sub(prefix.len() + 2))
            .into_iter()
            .enumerate()
        {
            let line = if marker.is_empty() || index > 0 {
                wrapped
            } else {
                let mut prefixed = marker.clone();
                prefixed.push_str(&wrap_text(&wrapped, content_width).join(" "));
                prefixed
            };
            lines.push(DisplayLine {
                color: transcript_color(role, trimmed),
                text: format!("{}  {}", prefix, line),
            });
        }
    }
    if lines.is_empty() {
        lines.push(DisplayLine {
            color: transcript_color(role, ""),
            text: format!("{}  ", prefix),
        });
    }
    lines
}

fn transcript_event_card_lines(
    prefix: &str,
    role: &str,
    line: &str,
    width: usize,
) -> Option<Vec<DisplayLine>> {
    let marker = transcript_marker(line);
    if marker.is_empty() {
        return None;
    }
    let label = marker.trim_matches(['[', ']', ' ']).to_string();
    let color = transcript_color(role, line);
    let body_width = width.saturating_sub(prefix.len() + 6).max(12);
    let mut lines = vec![DisplayLine {
        color,
        text: format!("{}  ┌ {}", prefix, label),
    }];
    for wrapped in wrap_text(line.trim(), body_width) {
        lines.push(DisplayLine {
            color,
            text: format!("{}  │ {}", prefix, wrapped),
        });
    }
    lines.push(DisplayLine {
        color,
        text: format!("{}  └ {}", prefix, label),
    });
    Some(lines)
}

fn transcript_json_lines(prefix: &str, line: &str, width: usize) -> Option<Vec<DisplayLine>> {
    let trimmed = line.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let pretty = serde_json::to_string_pretty(&value).ok()?;
    let mut lines = vec![DisplayLine {
        color: Color::Blue,
        text: format!("{}  ┌ json", prefix),
    }];
    for pretty_line in pretty.lines() {
        let visible = if pretty_line.len() > width.saturating_sub(prefix.len() + 6) {
            &pretty_line[..width.saturating_sub(prefix.len() + 9)]
        } else {
            pretty_line
        };
        lines.push(DisplayLine {
            color: Color::Blue,
            text: format!("{}  │ {}", prefix, visible),
        });
    }
    lines.push(DisplayLine {
        color: Color::Blue,
        text: format!("{}  └ json", prefix),
    });
    Some(lines)
}

fn transcript_marker(line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with("Stored pending approval")
        || trimmed.starts_with("Approved '")
        || trimmed.starts_with("Denied '")
    {
        "[approval] ".into()
    } else if trimmed.starts_with("Found ")
        || trimmed.starts_with("Queued async child agent")
        || trimmed.starts_with("Available agents:")
    {
        "[state] ".into()
    } else if trimmed.starts_with("Executed browser action")
        || trimmed.starts_with("Stored browser screenshot artifact")
        || trimmed.starts_with("Created file")
    {
        "[tool] ".into()
    } else if trimmed.to_ascii_lowercase().contains("error")
        || trimmed.to_ascii_lowercase().contains("failed")
    {
        "[error] ".into()
    } else {
        String::new()
    }
}

fn transcript_color(role: &str, line: &str) -> Color {
    let lowered = line.to_ascii_lowercase();
    if lowered.contains("error") || lowered.contains("failed") {
        Color::Red
    } else if line.starts_with("Stored pending approval")
        || line.starts_with("Approved '")
        || line.starts_with("Denied '")
    {
        Color::Yellow
    } else if line.starts_with("Found ")
        || line.starts_with("Queued async child agent")
        || line.starts_with("Available agents:")
    {
        Color::Cyan
    } else if line.starts_with("Executed browser action")
        || line.starts_with("Stored browser screenshot artifact")
        || line.starts_with("Created file")
    {
        Color::Green
    } else {
        match role {
            "you" => Color::Green,
            "aria" => Color::White,
            _ => Color::DarkGrey,
        }
    }
}

fn stable_tui_session_id() -> u64 {
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"aria-x-tui");
    hasher.update(std::env::var("USER").unwrap_or_default().as_bytes());
    hasher.update(std::env::var("TERM").unwrap_or_default().as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

fn websocket_session_uuid(session_seed: u64) -> uuid::Uuid {
    let mut session_id = [0u8; 16];
    session_id[0..8].copy_from_slice(&session_seed.to_le_bytes());
    uuid::Uuid::from_bytes(session_id)
}

fn summarize_run_rows(runs: &[aria_core::AgentRunRecord]) -> Vec<String> {
    let mut rows = runs.to_vec();
    rows.sort_by(|left, right| {
        run_sort_rank(left)
            .cmp(&run_sort_rank(right))
            .then(right.created_at_us.cmp(&left.created_at_us))
    });
    rows.into_iter()
        .take(6)
        .map(|run| {
            let scope = if run.parent_run_id.is_some() {
                "bg"
            } else {
                "top"
            };
            let status = format!("{:?}", run.status).to_ascii_lowercase();
            let request = truncate_with_ellipsis(run.request_text.trim(), 20);
            let run_id = truncate_from_end(&run.run_id, 10);
            format!("{} {} {} {} [{}]", scope, status, run.agent_id, request, run_id)
        })
        .collect()
}

fn run_sort_rank(run: &aria_core::AgentRunRecord) -> u8 {
    match run.status {
        aria_core::AgentRunStatus::Running => 0,
        aria_core::AgentRunStatus::Queued => 1,
        aria_core::AgentRunStatus::Completed => 2,
        aria_core::AgentRunStatus::Failed => 3,
        aria_core::AgentRunStatus::Cancelled => 4,
        aria_core::AgentRunStatus::TimedOut => 5,
    }
}

fn summarize_tool_provider_readiness_rows(
    readiness: &[aria_core::ToolProviderReadiness],
) -> Vec<String> {
    readiness
        .iter()
        .take(6)
        .map(|entry| {
            let mut row = format!(
                "{:?}/{} {:?}",
                entry.provider_kind, entry.provider_id, entry.status
            );
            if entry.bound {
                row.push_str(" bound");
            }
            if !entry.auth_ready {
                row.push_str(" auth?");
            }
            row
        })
        .collect()
}

fn summarize_mcp_status_rows(store: &RuntimeStore, agent_id: &str) -> Vec<String> {
    let servers = store.list_mcp_servers().unwrap_or_default();
    let bindings = store.list_mcp_bindings_for_agent(agent_id).unwrap_or_default();
    servers
        .into_iter()
        .take(4)
        .map(|server| {
            let tool_count = store
                .list_mcp_imported_tools(&server.server_id)
                .map(|items| items.len())
                .unwrap_or(0);
            let bound = bindings
                .iter()
                .filter(|binding| binding.server_id == server.server_id)
                .count();
            format!(
                "{} {} tools={} bound={}",
                server.server_id,
                if server.enabled { "ready" } else { "disabled" },
                tool_count,
                bound
            )
        })
        .collect()
}

fn summarize_failure_rows(
    last_error: Option<&str>,
    hidden_tool_messages: &[String],
    context_plan: Option<&aria_core::ContextPlan>,
    pending_approvals: usize,
) -> Vec<String> {
    let mut rows = Vec::new();
    if let Some(error) = last_error {
        let lower = error.to_ascii_lowercase();
        if lower.contains("missingrequiredartifact") || lower.contains("required artifact") {
            rows.push("artifact-required contract was not satisfied".into());
        } else if lower.contains("timeout") {
            rows.push("provider or tool timed out before completing".into());
        } else if lower.contains("busy") && lower.contains("workspace") {
            rows.push("workspace contention blocked a mutating run".into());
        } else if lower.contains("approval") {
            rows.push("action paused for approval or approval execution failed".into());
        } else {
            rows.push(truncate_with_ellipsis(error, 48));
        }
    }
    if let Some(plan) = context_plan {
        if let Some(ambiguity) = &plan.ambiguity {
            rows.push(format!(
                "reference resolution ended {:?}",
                ambiguity.outcome
            ));
        }
    }
    if let Some(hidden) = hidden_tool_messages.first() {
        rows.push(format!(
            "tool visibility: {}",
            truncate_with_ellipsis(hidden, 34)
        ));
    }
    if pending_approvals > 0 {
        rows.push(format!("{} approval(s) still pending", pending_approvals));
    }
    rows.truncate(4);
    rows
}

fn extract_prefixed_count(text: &str, prefix: &str, suffix: &str) -> Option<usize> {
    let trimmed = text.trim();
    let rest = trimmed.strip_prefix(prefix)?;
    let digits = rest.split(suffix).next()?.trim();
    digits.parse().ok()
}

fn parse_agent_list(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rest = trimmed.strip_prefix("- ")?;
            let agent = rest.split(':').next()?.trim();
            let agent = agent.split(' ').next()?.trim();
            if agent.is_empty() {
                None
            } else {
                Some(agent.to_string())
            }
        })
        .collect()
}

fn parse_approval_list(text: &str) -> Vec<ApprovalItem> {
    text.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
                return None;
            }
            let (_, rest) = trimmed.split_once(". ")?;
            let (summary_with_target, tail) = rest.split_once("[#")?;
            let handle = tail.split('|').next()?.trim();
            let approval_id = tail.split('|').nth(1)?.trim().trim_end_matches(']');
            let summary_with_target = summary_with_target.trim();
            let (summary, target_summary) = if let Some((prefix, suffix)) =
                summary_with_target.rsplit_once(" (")
            {
                if let Some(target) = suffix.strip_suffix(')') {
                    (prefix.trim().to_string(), Some(target.trim().to_string()))
                } else {
                    (summary_with_target.to_string(), None)
                }
            } else {
                (summary_with_target.to_string(), None)
            };
            Some(ApprovalItem {
                summary,
                handle: handle.to_string(),
                approval_id: approval_id.to_string(),
                target_summary,
                detail: None,
            })
        })
        .collect()
}

fn parse_workspace_lock_list(text: &str) -> Vec<String> {
    if text == "No active workspace locks." {
        return Vec::new();
    }
    text.lines()
        .skip_while(|line| !line.trim().starts_with("Workspace locks:"))
        .skip(1)
        .map(str::trim)
        .filter(|line| line.starts_with('-'))
        .map(|line| line.trim_start_matches('-').trim().to_string())
        .collect()
}

fn parse_provider_health_list(text: &str) -> Vec<String> {
    if text == "No provider circuits are open." {
        return Vec::new();
    }
    text.lines()
        .skip_while(|line| !line.trim().starts_with("Provider health:"))
        .skip(1)
        .map(str::trim)
        .filter(|line| line.starts_with('-'))
        .map(|line| line.trim_start_matches('-').trim().to_string())
        .collect()
}

fn parse_pending_approval_detail(text: &str) -> Option<(String, String, ApprovalDetail)> {
    if !text.contains("Approval required") || !text.contains("Stored pending approval") {
        return None;
    }
    let mut action_summary = None;
    let mut target_summary = None;
    let mut risk_summary = None;
    let mut options_summary = None;
    let mut arguments_lines = Vec::new();
    let mut in_arguments = false;
    let mut approval_id = None;
    let mut handle = None;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if let Some(value) = line.strip_prefix("Action: ") {
            action_summary = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("Target: ") {
            target_summary = Some(value.trim().to_string());
            continue;
        }
        if let Some(value) = line.strip_prefix("Risk: ") {
            risk_summary = Some(value.trim().to_string());
            continue;
        }
        if line == "Arguments:" {
            in_arguments = true;
            continue;
        }
        if let Some(value) = line.strip_prefix("Options: ") {
            options_summary = Some(value.trim().to_string());
            in_arguments = false;
            continue;
        }
        if in_arguments {
            if line.is_empty() {
                continue;
            }
            arguments_lines.push(raw_line.trim_end().to_string());
        }
        if let Some(rest) = line.strip_prefix("Stored pending approval '") {
            if let Some((id, tail)) = rest.split_once("' (handle: `") {
                approval_id = Some(id.to_string());
                if let Some(found_handle) = tail.split('`').next() {
                    handle = Some(found_handle.to_string());
                }
            }
        }
    }

    let approval_id = approval_id?;
    let handle = handle?;
    let action_summary = action_summary.unwrap_or_else(|| "approval required".to_string());
    let arguments_preview = if arguments_lines.is_empty() {
        None
    } else {
        Some(arguments_lines.join("\n"))
    };
    Some((
        handle,
        approval_id,
        ApprovalDetail {
            action_summary,
            target_summary,
            risk_summary,
            arguments_preview,
            options_summary,
        },
    ))
}

fn is_bootstrap_safe_command(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("/agent ")
        || matches!(
            trimmed,
            "/agents" | "/approvals" | "/runs" | "/workspace_locks" | "/provider_health" | "/help"
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TuiPreferences {
    #[serde(default = "default_tui_active_agent")]
    active_agent: String,
    #[serde(default)]
    sidebar_tab: SidebarTab,
}

impl Default for TuiPreferences {
    fn default() -> Self {
        Self {
            active_agent: default_tui_active_agent(),
            sidebar_tab: SidebarTab::Summary,
        }
    }
}

impl Default for SidebarTab {
    fn default() -> Self {
        Self::Runs
    }
}

fn push_capped(target: &mut VecDeque<String>, text: String, max_items: usize) {
    target.push_front(text);
    while target.len() > max_items {
        target.pop_back();
    }
}

fn is_run_signal_line(line: &str) -> bool {
    line.starts_with("Found ") && line.contains(" runs")
        || line.starts_with("Run ")
        || line.starts_with("Queued async child agent")
}

fn is_tool_signal_line(line: &str) -> bool {
    line.starts_with("Executed browser action")
        || line.starts_with("Stored browser screenshot artifact")
        || line.starts_with("Created file")
        || line.starts_with("Tool ")
}

fn is_context_signal_line(line: &str) -> bool {
    line.starts_with("Session override set to agent:")
        || line.starts_with("Current agent override:")
        || line.starts_with("Available agents:")
        || line.to_ascii_lowercase().contains("context")
}

fn is_health_signal_line(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    lower.contains("error")
        || lower.contains("failed")
        || lower.contains("timeout")
        || line.starts_with("Runtime connection")
        || line.starts_with("Connected.")
}

fn default_tui_active_agent() -> String {
    "omni".into()
}

fn tui_preferences_path() -> Option<std::path::PathBuf> {
    let dirs = ProjectDirs::from("ai", "anima", "hiveclaw")
        .or_else(|| ProjectDirs::from("ai", "anima", "aria-x"))?;
    let dir = dirs.preference_dir();
    std::fs::create_dir_all(dir).ok()?;
    Some(dir.join("tui_prefs.json"))
}

fn load_tui_preferences(default_agent: &str) -> TuiPreferences {
    let Some(path) = tui_preferences_path() else {
        return TuiPreferences {
            active_agent: default_agent.to_string(),
            ..TuiPreferences::default()
        };
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or(TuiPreferences {
            active_agent: default_agent.to_string(),
            ..TuiPreferences::default()
        })
}

fn save_tui_preferences(prefs: &TuiPreferences) -> Result<(), String> {
    let Some(path) = tui_preferences_path() else {
        return Ok(());
    };
    std::fs::write(
        path,
        serde_json::to_string_pretty(prefs)
            .map_err(|e| format!("serialize TUI preferences failed: {}", e))?,
    )
    .map_err(|e| format!("write TUI preferences failed: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_startup_mode_detects_tui_subcommand() {
        let args = vec![
            "aria-x".to_string(),
            "tui".to_string(),
            "custom.toml".to_string(),
        ];
        assert_eq!(
            parse_startup_mode(&args, Some("fallback.toml".into())),
            StartupMode::Tui {
                config_path: "custom.toml".into(),
                attach_url: None,
            }
        );
    }

    #[test]
    fn parse_startup_mode_detects_tui_attach_mode() {
        let args = vec![
            "aria-x".to_string(),
            "tui".to_string(),
            "config.live.toml".to_string(),
            "--attach".to_string(),
            "ws://127.0.0.1:8090/ws".to_string(),
        ];
        assert_eq!(
            parse_startup_mode(&args, Some("fallback.toml".into())),
            StartupMode::Tui {
                config_path: "config.live.toml".into(),
                attach_url: Some("ws://127.0.0.1:8090/ws".into()),
            }
        );
    }

    #[test]
    fn parse_startup_mode_defaults_runtime_path() {
        let args = vec!["aria-x".to_string()];
        assert_eq!(
            parse_startup_mode(&args, Some("fallback.toml".into())),
            StartupMode::Runtime {
                config_path: "fallback.toml".into()
            }
        );
    }

    #[test]
    fn wrap_text_keeps_lines_within_width() {
        let lines = wrap_text("alpha beta gamma delta", 10);
        assert_eq!(lines, vec!["alpha beta", "gamma", "delta"]);
        assert!(lines.iter().all(|line| line.len() <= 10));
    }

    #[test]
    fn truncate_with_ellipsis_shortens_long_text() {
        assert_eq!(truncate_with_ellipsis("abcdefgh", 6), "abc...");
        assert_eq!(truncate_with_ellipsis("abc", 6), "abc");
    }

    #[test]
    fn truncate_from_end_keeps_tail() {
        assert_eq!(truncate_from_end("abcdefgh", 6), "...fgh");
        assert_eq!(truncate_from_end("abc", 6), "abc");
    }

    #[test]
    fn compact_tab_labels_are_shorter() {
        assert_eq!(SidebarTab::Approvals.compact_label(), "apr");
        assert!(SidebarTab::Approvals.compact_label().len() < SidebarTab::Approvals.label().len());
    }

    #[test]
    fn bootstrap_safe_command_recognizes_agent_and_panel_commands() {
        assert!(is_bootstrap_safe_command("/agent omni"));
        assert!(is_bootstrap_safe_command("/agents"));
        assert!(is_bootstrap_safe_command("/approvals"));
        assert!(is_bootstrap_safe_command("/workspace_locks"));
        assert!(is_bootstrap_safe_command("/provider_health"));
        assert!(!is_bootstrap_safe_command("reply only OK"));
    }

    #[test]
    fn parse_startup_mode_treats_admin_commands_as_runtime_commands() {
        for command in [
            "doctor",
            "skills",
            "inspect",
            "explain",
            "--inspect-context",
            "--inspect-provider-payloads",
            "--explain-context",
            "--explain-provider-payloads",
        ] {
            let args = vec!["aria-x".into(), command.into(), "context".into()];
            let mode = parse_startup_mode(&args, Some("aria-x/config.toml".into()));
            assert_eq!(
                mode,
                StartupMode::Runtime {
                    config_path: "aria-x/config.toml".into()
                }
            );
        }
    }

    #[test]
    fn parse_startup_mode_treats_help_and_install_as_runtime_commands() {
        for command in ["help", "install", "completion", "--help"] {
            let args = vec!["aria-x".into(), command.into()];
            let mode = parse_startup_mode(&args, Some("aria-x/config.toml".into()));
            assert_eq!(
                mode,
                StartupMode::Runtime {
                    config_path: "aria-x/config.toml".into()
                }
            );
        }
    }

    #[test]
    fn parse_startup_mode_supports_run_subcommand() {
        let args = vec![
            "aria-x".into(),
            "run".into(),
            "nodes/orchestrator.toml".into(),
        ];
        let mode = parse_startup_mode(&args, Some("aria-x/config.toml".into()));
        assert_eq!(
            mode,
            StartupMode::Runtime {
                config_path: "nodes/orchestrator.toml".into()
            }
        );
    }

    #[test]
    fn handle_event_blocks_freeform_input_until_bootstrap_completes() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.input = "reply only OK".into();
        let action = state.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(action.is_none());
        assert!(state
            .lines
            .iter()
            .any(|line| line.text.contains("Waiting for session agent override")));
    }

    #[test]
    fn sidebar_tab_cycles_in_order() {
        assert_eq!(SidebarTab::Summary.next(), SidebarTab::Runs);
        assert_eq!(SidebarTab::Runs.next(), SidebarTab::ToolsContext);
        assert_eq!(SidebarTab::ToolsContext.next(), SidebarTab::SystemHealth);
        assert_eq!(SidebarTab::SystemHealth.next(), SidebarTab::Agents);
        assert_eq!(SidebarTab::Agents.next(), SidebarTab::Approvals);
        assert_eq!(SidebarTab::Approvals.next(), SidebarTab::Notifications);
        assert_eq!(SidebarTab::Notifications.next(), SidebarTab::Summary);
    }

    #[test]
    fn ingest_runtime_signal_tracks_agent_approvals_runs_and_errors() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.ingest_runtime_signal("Session override set to agent: researcher.");
        state.ingest_runtime_signal(
            "Approval required\n\nAction: browser action: click\nAgent: pending\nTarget: url=https://example.com\nRisk: high: side-effecting action\n\nArguments:\n{\n  \"action\": \"click\"\n}\n\nOptions: approve once, deny\n\nStored pending approval 'abc' (handle: `apv-x`).",
        );
        state.ingest_runtime_signal(
            "Pending approvals:\n 1. browser action: click (url=https://example.com) [#apv-x | abc]",
        );
        state.ingest_runtime_signal("Found 3 runs for current session.");
        state.ingest_runtime_signal(
            "Approved 'browser_act', but execution failed: tool error: browser session is paused",
        );
        assert_eq!(state.active_agent, "researcher");
        assert_eq!(state.pending_approvals, 0);
        assert_eq!(state.recent_runs, 3);
        assert!(state.approval_detail_cache.contains_key("apv-x"));
        assert_eq!(
            state
                .approval_items
                .first()
                .and_then(|item| item.detail.as_ref())
                .and_then(|detail| detail.risk_summary.as_deref()),
            Some("high: side-effecting action")
        );
        assert!(state
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("Approved 'browser_act'"));
        assert!(!state.notifications.is_empty());
    }

    #[test]
    fn extract_prefixed_count_parses_run_list_messages() {
        assert_eq!(
            extract_prefixed_count("Found 12 runs for current session.", "Found ", " runs"),
            Some(12)
        );
        assert_eq!(
            extract_prefixed_count("Nothing here", "Found ", " runs"),
            None
        );
    }

    #[test]
    fn parse_agent_list_reads_cli_agent_output() {
        let agents = parse_agent_list(
            "Available agents:\n - omni [available]: main agent\n - researcher [busy, active=2]: search\nCurrent agent override: omni",
        );
        assert_eq!(agents, vec!["omni", "researcher"]);
    }

    #[test]
    fn parse_approval_list_reads_handles_from_cli_output() {
        let approvals = parse_approval_list(
            "Pending approvals:\n 1. browser action: click (body) [#apv-123 | abc]\n 2. write file [#apv-999 | def]",
        );
        assert_eq!(
            approvals,
            vec![
                ApprovalItem {
                    summary: "browser action: click".into(),
                    handle: "apv-123".into(),
                    approval_id: "abc".into(),
                    target_summary: Some("body".into()),
                    detail: None,
                },
                ApprovalItem {
                    summary: "write file".into(),
                    handle: "apv-999".into(),
                    approval_id: "def".into(),
                    target_summary: None,
                    detail: None,
                },
            ]
        );
    }

    #[test]
    fn parse_approval_list_extracts_target_summary_when_present() {
        let approvals = parse_approval_list(
            "Pending approvals:\n 1. browser action: click (url=https://example.com) [#apv-123 | abc]",
        );
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].summary, "browser action: click");
        assert_eq!(
            approvals[0].target_summary.as_deref(),
            Some("url=https://example.com")
        );
    }

    #[test]
    fn parse_pending_approval_detail_reads_risk_target_and_arguments() {
        let detail = parse_pending_approval_detail(
            "Approval required\n\nAction: execute tool 'write_file'\nAgent: pending\nTarget: path=src/main.rs\nRisk: high: side-effecting action\n\nArguments:\n{\n  \"path\": \"src/main.rs\"\n}\n\nOptions: approve once, deny\n\nStored pending approval 'abc' (handle: `apv-123`). Inspect with `--inspect-approvals foo bar`.",
        )
        .expect("detail");
        assert_eq!(detail.0, "apv-123");
        assert_eq!(detail.1, "abc");
        assert_eq!(detail.2.action_summary, "execute tool 'write_file'");
        assert_eq!(detail.2.target_summary.as_deref(), Some("path=src/main.rs"));
        assert_eq!(
            detail.2.risk_summary.as_deref(),
            Some("high: side-effecting action")
        );
        assert!(detail
            .2
            .arguments_preview
            .as_deref()
            .unwrap_or_default()
            .contains("\"path\": \"src/main.rs\""));
    }

    #[test]
    fn parse_workspace_lock_list_reads_holder_and_waiters() {
        let rows = parse_workspace_lock_list(
            "Workspace locks:\n - workspace:/repo [holder=run-a | waiters=1 | active=1]",
        );
        assert_eq!(rows, vec!["workspace:/repo [holder=run-a | waiters=1 | active=1]"]);
        assert!(parse_workspace_lock_list("No active workspace locks.").is_empty());
    }

    #[test]
    fn parse_provider_health_list_reads_circuit_rows() {
        let rows = parse_provider_health_list(
            "Provider health:\n - openrouter [open=true | failures=2 | backends=primary,fallback]",
        );
        assert_eq!(
            rows,
            vec!["openrouter [open=true | failures=2 | backends=primary,fallback]"]
        );
        assert!(parse_provider_health_list("No provider circuits are open.").is_empty());
    }

    #[test]
    fn transcript_display_lines_formats_code_blocks() {
        let lines = transcript_display_lines(
            "aria",
            "Here is code:\n```rust\nfn main() {}\n```\nDone.",
            40,
        );
        assert!(lines.iter().any(|line| line.text.contains("┌ code")));
        assert!(lines
            .iter()
            .any(|line| line.text.contains("│ fn main() {}")));
        assert!(lines.iter().any(|line| line.text.contains("└ code")));
    }

    #[test]
    fn transcript_display_lines_marks_tool_and_error_events() {
        let lines = transcript_display_lines(
            "aria",
            "Stored pending approval 'abc'.\nExecuted browser action 'Click'.\nExecution failed: timeout",
            80,
        );
        assert!(lines.iter().any(|line| line.text.contains("┌ approval")));
        assert!(lines.iter().any(|line| line.text.contains("┌ tool")));
        assert!(lines.iter().any(|line| line.text.contains("┌ error")));
    }

    #[test]
    fn transcript_display_lines_wraps_long_tool_events_as_cards() {
        let lines = transcript_display_lines(
            "aria",
            "Executed browser action 'Click' for session 'browser-session-123' on profile 'work-default' with a very long explanation that should wrap across multiple lines cleanly.",
            60,
        );
        assert!(lines
            .first()
            .is_some_and(|line| line.text.contains("┌ tool")));
        assert!(lines
            .iter()
            .any(|line| line.text.contains("│ Executed browser action")));
        assert!(lines
            .last()
            .is_some_and(|line| line.text.contains("└ tool")));
    }

    #[test]
    fn transcript_display_lines_formats_json_payloads() {
        let lines = transcript_display_lines("aria", r#"{"ok":true,"count":2}"#, 80);
        assert!(lines.iter().any(|line| line.text.contains("┌ json")));
        assert!(lines.iter().any(|line| line.text.contains(r#""ok": true"#)));
        assert!(lines.iter().any(|line| line.text.contains("└ json")));
    }

    #[test]
    fn mouse_click_selects_sidebar_tab() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.last_layout = Some(TuiLayout::new(120, 40, 3, 4, 85, 30, 32));
        state.handle_mouse_click(90, 4);
        assert_eq!(state.sidebar_tab, SidebarTab::Summary);
        state.handle_mouse_click(100, 4);
        assert_eq!(state.sidebar_tab, SidebarTab::Runs);
    }

    #[test]
    fn ingest_runtime_signal_captures_tools_context_and_health_views() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.ingest_runtime_signal("Executed browser action 'click'");
        state.ingest_runtime_signal("Available agents:\n - omni [available]: main");
        state.ingest_runtime_signal("Execution failed: timeout");
        assert!(state
            .recent_tool_events
            .iter()
            .any(|line| line.contains("Executed browser action")));
        assert!(state
            .recent_context_events
            .iter()
            .any(|line| line.contains("Available agents")));
        assert!(state.error_events >= 1);
        assert!(state
            .recent_health_events
            .iter()
            .any(|line| line.to_ascii_lowercase().contains("failed")));
    }

    #[test]
    fn mouse_click_selects_approval_row() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.sidebar_tab = SidebarTab::Approvals;
        state.approval_items = vec![
            ApprovalItem {
                summary: "one".into(),
                handle: "apv-1".into(),
                approval_id: "id-1".into(),
                target_summary: None,
                detail: None,
            },
            ApprovalItem {
                summary: "two".into(),
                handle: "apv-2".into(),
                approval_id: "id-2".into(),
                target_summary: None,
                detail: None,
            },
        ];
        state.last_layout = Some(TuiLayout::new(120, 40, 3, 4, 85, 30, 32));
        let action = state.handle_mouse_click(90, 10);
        assert_eq!(state.selected_approval_idx, 1);
        assert!(action.is_none());
        assert!(state.show_approval_detail);
    }

    #[test]
    fn mouse_click_on_selected_agent_activates_switch() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.sidebar_tab = SidebarTab::Agents;
        state.available_agents = vec!["omni".into(), "researcher".into()];
        state.selected_agent_idx = 1;
        state.last_layout = Some(TuiLayout::new(120, 40, 3, 4, 85, 30, 32));
        let action = state.handle_mouse_click(90, 10);
        match action {
            Some(TuiAction::Send(text)) => assert_eq!(text, "/agent researcher"),
            _ => panic!("expected agent switch action"),
        }
    }

    #[test]
    fn approval_detail_toggles_from_keyboard() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.sidebar_tab = SidebarTab::Approvals;
        state.approval_items = vec![ApprovalItem {
            summary: "write file".into(),
            handle: "apv-1".into(),
            approval_id: "id-1".into(),
            target_summary: Some("path=src/main.rs".into()),
            detail: Some(ApprovalDetail {
                action_summary: "execute tool 'write_file'".into(),
                target_summary: Some("path=src/main.rs".into()),
                risk_summary: Some("high: side-effecting action".into()),
                arguments_preview: Some("{\n  \"path\": \"src/main.rs\"\n}".into()),
                options_summary: Some("approve once, deny".into()),
            }),
        }];
        let toggle = Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(state.handle_event(toggle.clone()).is_none());
        assert!(state.show_approval_detail);
        assert!(state
            .handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
            .is_none());
        assert!(!state.show_approval_detail);
    }

    #[test]
    fn ctrl_p_opens_command_palette_and_filters_commands() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        assert!(state
            .handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL,
            )))
            .is_none());
        assert!(state.show_command_palette);

        assert!(state
            .handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char('r'),
                KeyModifiers::NONE,
            )))
            .is_none());
        let filtered = state.filtered_palette_commands();
        assert!(!filtered.is_empty());
        assert!(filtered
            .iter()
            .all(|item| item.label.to_ascii_lowercase().contains('r') || item.hint.to_ascii_lowercase().contains('r')));
    }

    #[test]
    fn command_palette_can_switch_tabs_and_send_actions() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.show_command_palette = true;
        state.command_palette_query = "system".into();
        let action = state.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        assert!(action.is_none());
        assert_eq!(state.sidebar_tab, SidebarTab::SystemHealth);
        assert!(!state.show_command_palette);

        state.show_command_palette = true;
        state.command_palette_query = "runs".into();
        let action = state.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        match action {
            Some(TuiAction::Send(text)) => assert_eq!(text, "/runs"),
            _ => panic!("expected palette send action"),
        }
        assert_eq!(state.sidebar_tab, SidebarTab::Runs);
    }

    #[test]
    fn command_palette_includes_available_agent_switches() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.available_agents = vec!["developer".into(), "researcher".into()];
        state.show_command_palette = true;
        state.command_palette_query = "researcher".into();
        let action = state.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::NONE,
        )));
        match action {
            Some(TuiAction::Send(text)) => assert_eq!(text, "/agent researcher"),
            _ => panic!("expected agent switch action"),
        }
        assert_eq!(state.sidebar_tab, SidebarTab::Agents);
    }

    #[test]
    fn summarize_run_rows_prioritizes_active_and_background_runs() {
        let session_id = *uuid::Uuid::new_v4().as_bytes();
        let rows = summarize_run_rows(&[
            aria_core::AgentRunRecord {
                run_id: "run-complete".into(),
                parent_run_id: None,
                    origin_kind: None,
                    lineage_run_id: None,
                session_id,
                user_id: "u".into(),
                requested_by_agent: None,
                agent_id: "developer".into(),
                status: aria_core::AgentRunStatus::Completed,
                request_text: "finished task".into(),
                inbox_on_completion: true,
                max_runtime_seconds: None,
                created_at_us: 1,
                started_at_us: None,
                finished_at_us: Some(2),
                result: None,
            },
            aria_core::AgentRunRecord {
                run_id: "run-active".into(),
                parent_run_id: Some("parent-1".into()),
                    origin_kind: None,
                    lineage_run_id: None,
                session_id,
                user_id: "u".into(),
                requested_by_agent: Some("omni".into()),
                agent_id: "researcher".into(),
                status: aria_core::AgentRunStatus::Running,
                request_text: "search documentation".into(),
                inbox_on_completion: true,
                max_runtime_seconds: None,
                created_at_us: 3,
                started_at_us: Some(4),
                finished_at_us: None,
                result: None,
            },
        ]);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].starts_with("bg running researcher"));
        assert!(rows[0].contains("search documentation"));
        assert!(rows[1].starts_with("top completed developer"));
    }

    #[test]
    fn tools_context_sidebar_prefers_operator_snapshot_rows() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.sidebar_tab = SidebarTab::ToolsContext;
        state.visible_tool_rows = vec!["- write_file".into(), "- search_web".into()];
        state.hidden_tool_rows = vec!["- browser_act hidden: contract mismatch".into()];
        state.context_plan_rows = vec![
            "compact contract + retrieval".into(),
            "Include WorkingSet (84)".into(),
            "ambiguity Resolved".into(),
        ];
        let lines = state.sidebar_lines();
        assert!(lines.contains("visible tools"));
        assert!(lines.contains("- write_file"));
        assert!(lines.contains("hidden tools"));
        assert!(lines.contains("context plan"));
        assert!(lines.contains("ambiguity Resolved"));
    }

    #[test]
    fn system_health_sidebar_includes_provider_mcp_and_failure_sections() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.sidebar_tab = SidebarTab::SystemHealth;
        state.provider_health_rows = vec!["active backend gemini/gemini-3-flash-preview".into()];
        state.provider_circuit_rows = vec!["openrouter [open=true | failures=2]".into()];
        state.mcp_status_rows = vec!["chrome_devtools ready tools=29 bound=29".into()];
        state.workspace_lock_rows = vec!["/repo holder=run-a waiters=1".into()];
        state.failure_summary_rows = vec!["artifact-required contract was not satisfied".into()];
        let lines = state.sidebar_lines();
        assert!(lines.contains("provider health"));
        assert!(lines.contains("provider circuits"));
        assert!(lines.contains("mcp"));
        assert!(lines.contains("workspace locks"));
        assert!(lines.contains("why this happened"));
    }

    #[test]
    fn ingest_runtime_signal_updates_workspace_lock_rows() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.ingest_runtime_signal(
            "Workspace locks:\n - workspace:/repo [holder=run-a | waiters=1 | active=1]",
        );
        assert_eq!(state.workspace_lock_rows.len(), 1);
        assert!(state.workspace_lock_rows[0].contains("holder=run-a"));
    }

    #[test]
    fn ingest_runtime_signal_updates_provider_circuit_rows() {
        let mut state = TuiState::new(
            "config.toml".into(),
            1234,
            TuiPreferences::default(),
            std::path::PathBuf::from("runtime.log"),
        );
        state.ingest_runtime_signal(
            "Provider health:\n - openrouter [open=true | failures=2 | backends=primary,fallback]",
        );
        assert_eq!(state.provider_circuit_rows.len(), 1);
        assert!(state.provider_circuit_rows[0].contains("failures=2"));
    }

    #[test]
    fn summarize_failure_rows_explains_common_operator_failures() {
        let rows = summarize_failure_rows(
            Some("MissingRequiredArtifact for schedule contract"),
            &["write_file hidden: not selected for current contract".into()],
            Some(&aria_core::ContextPlan {
                summary: Some("working set resolved".into()),
                block_records: vec![],
                ambiguity: Some(aria_core::ReferenceResolution {
                    query_text: "modify it".into(),
                    matched_entry_ids: vec!["a".into(), "b".into()],
                    outcome: aria_core::ReferenceResolutionOutcome::Ambiguous,
                    active_target_entry_id: None,
                    reason: Some("multiple recent files".into()),
                }),
            }),
            2,
        );
        assert!(rows
            .iter()
            .any(|row| row.contains("artifact-required contract")));
        assert!(rows.iter().any(|row| row.contains("reference resolution")));
        assert!(rows.iter().any(|row| row.contains("tool visibility")));
        assert!(rows.iter().any(|row| row.contains("pending")));
    }
}
