use std::collections::VecDeque;
use std::io::{stdout, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::{default_project_config_path, resolve_config_path};
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

    let prefs = load_tui_preferences();
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
    info: Vec<String>,
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
    notifications: VecDeque<String>,
    last_layout: Option<TuiLayout>,
    show_approval_detail: bool,
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
                text: "ARIA-X Terminal UI ready. Enter to send, F1 help, F2 /agents, F3 /approvals, F4 /runs, Ctrl+C to quit.".into(),
            }],
            info: vec![
                "Shortcuts".into(),
                "F1 help".into(),
                "F2 agents".into(),
                "F3 approvals".into(),
                "F4 runs".into(),
                "F5 clear".into(),
                "Tab switch panel".into(),
                "PgUp/PgDn scroll".into(),
                "Use slash commands normally".into(),
            ],
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
            notifications: VecDeque::new(),
            last_layout: None,
            show_approval_detail: false,
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
        self.lines.push(TuiLine { role: "aria", text });
        self.scroll = 0;
    }

    fn push_system(&mut self, text: String) {
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
        match evt {
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
                self.sidebar_tab = SidebarTab::Session;
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
        draw_box(stdout, 0, 0, cols, header_h, " ARIA-X TUI ")?;
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
        let cursor_input = truncate_from_end(&self.input, cols.saturating_sub(6) as usize);
        queue!(
            stdout,
            MoveTo((2 + cursor_input.len()) as u16, header_h + chat_h + 2)
        )
        .map_err(|e| e.to_string())?;
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
        let lines = [
            format!("handle   {}", selected.handle),
            format!("approval {}", selected.approval_id),
            format!("action   {}", selected.summary),
            String::new(),
            "Enter/a approve".into(),
            "d deny".into(),
            "i or Esc close".into(),
        ];
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
                        lines.push(format!("{} {} [{}]", marker, item.summary, item.handle));
                    }
                    if let Some(selected) = self.selected_approval() {
                        lines.push(String::new());
                        lines.push("preview".into());
                        lines.push(format!("handle {}", selected.handle));
                        lines.push(format!("approval {}", selected.approval_id));
                        lines.push(format!("action {}", selected.summary));
                    }
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
            SidebarTab::Shortcuts => {
                let mut lines = vec!["tab shortcuts".into()];
                lines.extend(self.info.clone());
                lines.join("\n")
            }
            SidebarTab::Session => {
                let transcript_state = if self.scroll == 0 {
                    "live tail"
                } else {
                    "manual scroll"
                };
                let mut lines = vec![
                    "tab session".into(),
                    format!(
                        "config {}",
                        truncate_with_ellipsis(
                            &self.config_path,
                            if self.last_layout.is_some_and(|layout| layout.compact_mode) {
                                18
                            } else {
                                28
                            }
                        )
                    ),
                    format!("ws 127.0.0.1:{}", self.websocket_port),
                    format!("messages {}", self.lines.len()),
                    format!("view {}", transcript_state),
                    "Shift+Up/Down fine scroll".into(),
                    "PgUp/PgDn page scroll".into(),
                ];
                if let Some(log_tail) = self.runtime_log_tail() {
                    lines.push(String::new());
                    lines.push("runtime log".into());
                    lines.extend(log_tail.lines().map(|line| line.to_string()));
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
            self.approval_items = parse_approval_list(text);
            self.pending_approvals = self.approval_items.len();
            if self.selected_approval_idx >= self.approval_items.len() {
                self.selected_approval_idx = self.approval_items.len().saturating_sub(1);
            }
            self.push_notification(format!("approvals {}", self.pending_approvals));
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
            self.push_notification(format!("runs {}", count));
        }
        if text.to_ascii_lowercase().contains("error")
            || text.to_ascii_lowercase().contains("failed")
        {
            self.last_error = Some(
                text.lines()
                    .next()
                    .unwrap_or(text)
                    .chars()
                    .take(72)
                    .collect(),
            );
            if let Some(err) = &self.last_error {
                self.push_notification(format!("error {}", err));
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
        self.notifications.push_front(text);
        while self.notifications.len() > 8 {
            self.notifications.pop_back();
        }
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
}

enum TuiAction {
    Quit,
    Send(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SidebarTab {
    Summary,
    Agents,
    Approvals,
    Notifications,
    Shortcuts,
    Session,
}

impl SidebarTab {
    fn all() -> [Self; 6] {
        [
            Self::Summary,
            Self::Agents,
            Self::Approvals,
            Self::Notifications,
            Self::Shortcuts,
            Self::Session,
        ]
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "summary",
            Self::Agents => "agents",
            Self::Approvals => "approvals",
            Self::Notifications => "notes",
            Self::Shortcuts => "keys",
            Self::Session => "session",
        }
    }

    fn compact_label(self) -> &'static str {
        match self {
            Self::Summary => "sum",
            Self::Agents => "agt",
            Self::Approvals => "apr",
            Self::Notifications => "note",
            Self::Shortcuts => "key",
            Self::Session => "sess",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Summary => Self::Agents,
            Self::Agents => Self::Approvals,
            Self::Approvals => Self::Notifications,
            Self::Notifications => Self::Shortcuts,
            Self::Shortcuts => Self::Session,
            Self::Session => Self::Summary,
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
            let (summary, tail) = rest.split_once("[#")?;
            let handle = tail.split('|').next()?.trim();
            let approval_id = tail.split('|').nth(1)?.trim().trim_end_matches(']');
            Some(ApprovalItem {
                summary: summary.trim().to_string(),
                handle: handle.to_string(),
                approval_id: approval_id.to_string(),
            })
        })
        .collect()
}

fn is_bootstrap_safe_command(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.starts_with("/agent ")
        || matches!(trimmed, "/agents" | "/approvals" | "/runs" | "/help")
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
        Self::Summary
    }
}

fn default_tui_active_agent() -> String {
    "omni".into()
}

fn tui_preferences_path() -> Option<std::path::PathBuf> {
    let dirs = ProjectDirs::from("ai", "anima", "aria-x")?;
    let dir = dirs.preference_dir();
    std::fs::create_dir_all(dir).ok()?;
    Some(dir.join("tui_prefs.json"))
}

fn load_tui_preferences() -> TuiPreferences {
    let Some(path) = tui_preferences_path() else {
        return TuiPreferences::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
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
        assert!(!is_bootstrap_safe_command("reply only OK"));
    }

    #[test]
    fn parse_startup_mode_treats_admin_commands_as_runtime_commands() {
        for command in [
            "doctor",
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
        assert_eq!(SidebarTab::Summary.next(), SidebarTab::Agents);
        assert_eq!(SidebarTab::Agents.next(), SidebarTab::Approvals);
        assert_eq!(SidebarTab::Approvals.next(), SidebarTab::Notifications);
        assert_eq!(SidebarTab::Notifications.next(), SidebarTab::Shortcuts);
        assert_eq!(SidebarTab::Shortcuts.next(), SidebarTab::Session);
        assert_eq!(SidebarTab::Session.next(), SidebarTab::Summary);
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
        state.ingest_runtime_signal("Stored pending approval 'abc' (handle: `apv-x`).");
        state.ingest_runtime_signal("Found 3 runs for current session.");
        state.ingest_runtime_signal(
            "Approved 'browser_act', but execution failed: tool error: browser session is paused",
        );
        assert_eq!(state.active_agent, "researcher");
        assert_eq!(state.pending_approvals, 0);
        assert_eq!(state.recent_runs, 3);
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
                    summary: "browser action: click (body)".into(),
                    handle: "apv-123".into(),
                    approval_id: "abc".into(),
                },
                ApprovalItem {
                    summary: "write file".into(),
                    handle: "apv-999".into(),
                    approval_id: "def".into(),
                },
            ]
        );
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
        assert_eq!(state.sidebar_tab, SidebarTab::Agents);
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
            },
            ApprovalItem {
                summary: "two".into(),
                handle: "apv-2".into(),
                approval_id: "id-2".into(),
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
        }];
        let toggle = Event::Key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(state.handle_event(toggle.clone()).is_none());
        assert!(state.show_approval_detail);
        assert!(state
            .handle_event(Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)))
            .is_none());
        assert!(!state.show_approval_detail);
    }
}
