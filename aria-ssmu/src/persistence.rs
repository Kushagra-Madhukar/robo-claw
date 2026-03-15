use crate::{Message, SessionState};
use rusqlite::{params, Connection, OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    AppendMessage {
        message: Message,
    },
    AddConstraint {
        constraint: String,
    },
    ClearHistory,
    UpdateOverrides {
        agent: Option<String>,
        model: Option<String>,
    },
    ReplaceHistory {
        remove_count: usize,
        summary: Message,
    },
    SnapshotReplace {
        state: PersistedSessionState,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSessionState {
    pub history: Vec<Message>,
    pub durable_constraints: Vec<String>,
    pub current_agent: Option<String>,
    pub current_model: Option<String>,
}

#[derive(Debug)]
pub enum PersistenceError {
    Sqlite(rusqlite::Error),
    Serde(serde_json::Error),
    VersionConflict { expected: u64, actual: u64 },
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(err) => write!(f, "sqlite error: {}", err),
            Self::Serde(err) => write!(f, "serde error: {}", err),
            Self::VersionConflict { expected, actual } => {
                write!(
                    f,
                    "version conflict: expected {}, actual {}",
                    expected, actual
                )
            }
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<rusqlite::Error> for PersistenceError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

/// Embedded SQLite persistence for ARIA-X session state.
pub struct SqlitePersistence {
    conn: Connection,
}

impl SqlitePersistence {
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        let _ = conn.execute("PRAGMA journal_mode = WAL", []);

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                current_agent TEXT,
                current_model TEXT,
                durable_constraints TEXT NOT NULL DEFAULT '[]',
                version INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS session_events (
                event_id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL,
                expected_version INTEGER NOT NULL,
                applied_version INTEGER NOT NULL,
                kind TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at_us INTEGER NOT NULL,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );
            CREATE INDEX IF NOT EXISTS idx_session_events_session_event_id
                ON session_events (session_id, event_id);
            CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                role TEXT,
                content TEXT,
                timestamp_us INTEGER,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            );",
        )?;
        migrate_legacy_session_schema(&conn, path)?;
        migrate_legacy_session_history_sources(&mut conn, path)?;

        Ok(Self { conn })
    }

    pub fn append_event(
        &mut self,
        session_id: Uuid,
        expected_version: u64,
        event: &SessionEvent,
    ) -> std::result::Result<u64, PersistenceError> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO sessions (id, durable_constraints, version) VALUES (?1, '[]', 0)",
            params![session_id.to_string()],
        )?;

        let row: (Option<String>, Option<String>, String, i64) = tx.query_row(
            "SELECT current_agent, current_model, durable_constraints, version FROM sessions WHERE id = ?1",
            params![session_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )?;
        let mut state = PersistedSessionState {
            history: replay_session_events_from_tx(&tx, session_id)?
                .into_iter()
                .collect(),
            durable_constraints: serde_json::from_str(&row.2).unwrap_or_default(),
            current_agent: row.0,
            current_model: row.1,
        };
        let actual_version = row.3 as u64;
        if actual_version != expected_version {
            return Err(PersistenceError::VersionConflict {
                expected: expected_version,
                actual: actual_version,
            });
        }

        apply_event_to_state(&mut state, event);
        let next_version = actual_version + 1;
        let updated = tx.execute(
            "UPDATE sessions
             SET current_agent=?2, current_model=?3, durable_constraints=?4, version=?5
             WHERE id=?1 AND version=?6",
            params![
                session_id.to_string(),
                state.current_agent,
                state.current_model,
                serde_json::to_string(&state.durable_constraints)?,
                next_version as i64,
                actual_version as i64
            ],
        )?;
        if updated != 1 {
            return Err(PersistenceError::VersionConflict {
                expected: expected_version,
                actual: actual_version,
            });
        }

        tx.execute(
            "INSERT INTO session_events
             (session_id, expected_version, applied_version, kind, payload_json, created_at_us)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id.to_string(),
                expected_version as i64,
                next_version as i64,
                event_kind(event),
                serde_json::to_string(event)?,
                event_timestamp_us(event) as i64
            ],
        )?;

        tx.commit()?;
        Ok(next_version)
    }

    pub fn save_session(&mut self, session_id: Uuid, state: &SessionState) -> Result<()> {
        let snapshot = SessionEvent::SnapshotReplace {
            state: PersistedSessionState {
                history: state.history.iter().cloned().collect(),
                durable_constraints: state.durable_constraints.clone(),
                current_agent: state.current_agent.clone(),
                current_model: state.current_model.clone(),
            },
        };
        let current_version: u64 = self
            .conn
            .query_row(
                "SELECT version FROM sessions WHERE id=?1",
                params![session_id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .map(|value| value as u64)
            .unwrap_or(0);
        self.append_event(session_id, current_version, &snapshot)
            .map(|_| ())
            .map_err(|err| match err {
                PersistenceError::Sqlite(e) => e,
                PersistenceError::Serde(e) => rusqlite::Error::ToSqlConversionFailure(Box::new(e)),
                PersistenceError::VersionConflict { expected, actual } => {
                    rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        format!("version conflict: expected {}, actual {}", expected, actual),
                    )))
                }
            })
    }

    pub fn load_session(&self, session_id: Uuid) -> Result<Option<SessionState>> {
        let row: Option<(Option<String>, Option<String>, String, i64)> = self
            .conn
            .query_row(
                "SELECT current_agent, current_model, durable_constraints, version FROM sessions WHERE id = ?1",
                params![session_id.to_string()],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        let Some((agent, model, constraints_json, version)) = row else {
            return Ok(None);
        };

        let history = self.replay_session_events(session_id)?;
        Ok(Some(SessionState {
            history,
            durable_constraints: serde_json::from_str(&constraints_json).unwrap_or_default(),
            current_agent: agent,
            current_model: model,
            version: version as u64,
        }))
    }

    pub fn list_sessions(&self) -> Result<Vec<Uuid>> {
        let mut stmt = self.conn.prepare("SELECT id FROM sessions")?;
        let iter = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil()))
        })?;

        let mut res = Vec::new();
        for id in iter {
            res.push(id?);
        }
        Ok(res)
    }

    fn replay_session_events(&self, session_id: Uuid) -> Result<VecDeque<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT payload_json FROM session_events WHERE session_id = ?1 ORDER BY event_id ASC",
        )?;
        let rows = stmt.query_map(
            params![session_id.to_string()],
            |row: &rusqlite::Row<'_>| row.get::<_, String>(0),
        )?;
        let mut state = PersistedSessionState {
            history: Vec::new(),
            durable_constraints: Vec::new(),
            current_agent: None,
            current_model: None,
        };
        for payload in rows {
            let event: SessionEvent =
                serde_json::from_str(&payload?).unwrap_or(SessionEvent::ClearHistory);
            apply_event_to_state(&mut state, &event);
        }
        Ok(state.history.into_iter().collect())
    }
}

fn migrate_legacy_session_schema(conn: &Connection, _path: &Path) -> Result<()> {
    if !table_has_column(conn, "sessions", "version")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN version INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !table_has_column(conn, "sessions", "durable_constraints")? {
        conn.execute(
            "ALTER TABLE sessions ADD COLUMN durable_constraints TEXT NOT NULL DEFAULT '[]'",
            [],
        )?;
    }
    if !table_has_column(conn, "sessions", "current_agent")? {
        conn.execute("ALTER TABLE sessions ADD COLUMN current_agent TEXT", [])?;
    }
    if !table_has_column(conn, "sessions", "current_model")? {
        conn.execute("ALTER TABLE sessions ADD COLUMN current_model TEXT", [])?;
    }
    Ok(())
}

fn migrate_legacy_session_history_sources(conn: &mut Connection, db_path: &Path) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    if let Some(sessions_dir) = db_path.parent() {
        import_jsonl_audits_without_events(&tx, sessions_dir)?;
    }
    import_legacy_messages_without_events(&tx)?;
    tx.commit()
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let name: String = row.get(1)?;
        if name == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn session_has_events(tx: &rusqlite::Transaction<'_>, session_id: Uuid) -> Result<bool> {
    let count: i64 = tx.query_row(
        "SELECT COUNT(1) FROM session_events WHERE session_id=?1",
        params![session_id.to_string()],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

fn import_jsonl_audits_without_events(
    tx: &rusqlite::Transaction<'_>,
    sessions_dir: &Path,
) -> Result<()> {
    let entries = match fs::read_dir(sessions_dir) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        let Ok(session_id) = Uuid::parse_str(stem) else {
            continue;
        };
        if session_has_events(tx, session_id)? {
            continue;
        }
        let messages = load_messages_from_jsonl(&path);
        if messages.is_empty() {
            continue;
        }
        import_snapshot_event(tx, session_id, messages)?;
    }
    Ok(())
}

fn import_legacy_messages_without_events(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    let mut stmt = tx.prepare("SELECT id FROM sessions")?;
    let session_ids = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(stmt);
    for raw_id in session_ids {
        let Ok(session_id) = Uuid::parse_str(&raw_id) else {
            continue;
        };
        if session_has_events(tx, session_id)? {
            continue;
        }
        let messages = load_legacy_messages(tx, session_id)?;
        if messages.is_empty() {
            continue;
        }
        import_snapshot_event(tx, session_id, messages)?;
    }
    Ok(())
}

fn load_messages_from_jsonl(path: &Path) -> Vec<Message> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut messages = BufReader::new(file)
        .lines()
        .filter_map(|line| line.ok())
        .filter_map(|line| serde_json::from_str::<Message>(&line).ok())
        .collect::<Vec<_>>();
    messages.sort_by_key(|message| message.timestamp_us);
    messages
}

fn load_legacy_messages(tx: &rusqlite::Transaction<'_>, session_id: Uuid) -> Result<Vec<Message>> {
    let mut stmt = tx.prepare(
        "SELECT role, content, timestamp_us
         FROM messages
         WHERE session_id=?1
         ORDER BY timestamp_us ASC, id ASC",
    )?;
    let rows = stmt.query_map(params![session_id.to_string()], |row| {
        Ok(Message {
            role: row.get(0)?,
            content: row.get(1)?,
            timestamp_us: row.get::<_, i64>(2)? as u64,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
}

fn import_snapshot_event(
    tx: &rusqlite::Transaction<'_>,
    session_id: Uuid,
    messages: Vec<Message>,
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO sessions (id, durable_constraints, version) VALUES (?1, '[]', 0)",
        params![session_id.to_string()],
    )?;
    let row: (Option<String>, Option<String>, String, i64) = tx.query_row(
        "SELECT current_agent, current_model, durable_constraints, version FROM sessions WHERE id=?1",
        params![session_id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let expected_version = row.3 as u64;
    let next_version = expected_version.saturating_add(1);
    let snapshot = SessionEvent::SnapshotReplace {
        state: PersistedSessionState {
            history: messages,
            durable_constraints: serde_json::from_str(&row.2).unwrap_or_default(),
            current_agent: row.0.clone(),
            current_model: row.1.clone(),
        },
    };
    tx.execute(
        "UPDATE sessions
         SET current_agent=?2, current_model=?3, durable_constraints=?4, version=?5
         WHERE id=?1",
        params![
            session_id.to_string(),
            row.0,
            row.1,
            row.2,
            next_version as i64
        ],
    )?;
    tx.execute(
        "INSERT INTO session_events
         (session_id, expected_version, applied_version, kind, payload_json, created_at_us)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            session_id.to_string(),
            expected_version as i64,
            next_version as i64,
            "snapshot_replace",
            serde_json::to_string(&snapshot)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
            snapshot_timestamp_us(&snapshot) as i64
        ],
    )?;
    Ok(())
}

fn snapshot_timestamp_us(event: &SessionEvent) -> u64 {
    match event {
        SessionEvent::SnapshotReplace { state } => state
            .history
            .last()
            .map(|message| message.timestamp_us)
            .unwrap_or_else(current_time_us),
        _ => current_time_us(),
    }
}

fn current_time_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_micros() as u64)
        .unwrap_or(0)
}

fn replay_session_events_from_tx(
    tx: &rusqlite::Transaction<'_>,
    session_id: Uuid,
) -> Result<VecDeque<Message>> {
    let mut stmt = tx.prepare(
        "SELECT payload_json FROM session_events WHERE session_id = ?1 ORDER BY event_id ASC",
    )?;
    let rows = stmt.query_map(
        params![session_id.to_string()],
        |row: &rusqlite::Row<'_>| row.get::<_, String>(0),
    )?;
    let mut state = PersistedSessionState {
        history: Vec::new(),
        durable_constraints: Vec::new(),
        current_agent: None,
        current_model: None,
    };
    for payload in rows {
        let event: SessionEvent =
            serde_json::from_str(&payload?).unwrap_or(SessionEvent::ClearHistory);
        apply_event_to_state(&mut state, &event);
    }
    Ok(state.history.into_iter().collect())
}

fn apply_event_to_state(state: &mut PersistedSessionState, event: &SessionEvent) {
    match event {
        SessionEvent::AppendMessage { message } => state.history.push(message.clone()),
        SessionEvent::AddConstraint { constraint } => {
            state.durable_constraints.push(constraint.clone());
        }
        SessionEvent::ClearHistory => state.history.clear(),
        SessionEvent::UpdateOverrides { agent, model } => {
            if let Some(agent) = agent {
                state.current_agent = Some(agent.clone());
            }
            if let Some(model) = model {
                state.current_model = Some(model.clone());
            }
        }
        SessionEvent::ReplaceHistory {
            remove_count,
            summary,
        } => {
            let remove = (*remove_count).min(state.history.len());
            for _ in 0..remove {
                state.history.remove(0);
            }
            state.history.insert(0, summary.clone());
        }
        SessionEvent::SnapshotReplace { state: snapshot } => {
            *state = snapshot.clone();
        }
    }
}

fn event_kind(event: &SessionEvent) -> &'static str {
    match event {
        SessionEvent::AppendMessage { .. } => "append_message",
        SessionEvent::AddConstraint { .. } => "add_constraint",
        SessionEvent::ClearHistory => "clear_history",
        SessionEvent::UpdateOverrides { .. } => "update_overrides",
        SessionEvent::ReplaceHistory { .. } => "replace_history",
        SessionEvent::SnapshotReplace { .. } => "snapshot_replace",
    }
}

fn event_timestamp_us(event: &SessionEvent) -> u64 {
    match event {
        SessionEvent::AppendMessage { message } => message.timestamp_us,
        SessionEvent::ReplaceHistory { summary, .. } => summary.timestamp_us,
        _ => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_micros() as u64)
            .unwrap_or(0),
    }
}
