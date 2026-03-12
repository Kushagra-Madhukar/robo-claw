use crate::{Message, SessionState};
use rusqlite::{params, Connection, Result};
use std::collections::VecDeque;
use uuid::Uuid;

/// Embedded SQLite persistence for ARIA-X session state.
///
/// Implements unified persistence by storing multiple sessions and their
/// message histories in a single WAL-mode database.
pub struct SqlitePersistence {
    conn: Connection,
}

impl SqlitePersistence {
    /// Open a connection to a SQLite database at the given path.
    /// Automatically applies migrations (schema creation).
    pub fn open(path: &std::path::Path) -> Result<Self> {
        let conn = Connection::open(path)?;

        // Enable WAL mode for better concurrency during RAG/orchestrator loops
        let _ = conn.execute("PRAGMA journal_mode = WAL", []);

        conn.execute(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                current_agent TEXT,
                current_model TEXT,
                durable_constraints TEXT
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                role TEXT,
                content TEXT,
                timestamp_us INTEGER,
                FOREIGN KEY(session_id) REFERENCES sessions(id)
            )",
            [],
        )?;

        Ok(Self { conn })
    }

    /// Save a session's full state (metadata + history) to the database.
    pub fn save_session(&mut self, session_id: Uuid, state: &SessionState) -> Result<()> {
        let tx = self.conn.transaction()?;

        tx.execute(
            "INSERT OR REPLACE INTO sessions (id, current_agent, current_model, durable_constraints)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                session_id.to_string(),
                state.current_agent,
                state.current_model,
                serde_json::to_string(&state.durable_constraints).unwrap_or("[]".to_string())
            ],
        )?;

        // Full snapshot replacement for now (TDD simplicity)
        tx.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id.to_string()],
        )?;

        let mut stmt = tx.prepare("INSERT INTO messages (session_id, role, content, timestamp_us) VALUES (?1, ?2, ?3, ?4)")?;
        for msg in &state.history {
            stmt.execute(params![
                session_id.to_string(),
                msg.role,
                msg.content,
                msg.timestamp_us
            ])?;
        }
        stmt.finalize()?;

        tx.commit()
    }

    /// Load a session's state from the database.
    pub fn load_session(&self, session_id: Uuid) -> Result<Option<SessionState>> {
        let mut stmt = self.conn.prepare(
            "SELECT current_agent, current_model, durable_constraints FROM sessions WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![session_id.to_string()], |row| {
            let constraints_json: String = row.get(2)?;
            let constraints: Vec<String> =
                serde_json::from_str(&constraints_json).unwrap_or_default();
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                constraints,
            ))
        });

        let (agent, model, constraints) = match row {
            Ok(data) => data,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e),
        };

        let mut stmt = self.conn.prepare("SELECT role, content, timestamp_us FROM messages WHERE session_id = ?1 ORDER BY id ASC")?;
        let msg_iter = stmt.query_map(params![session_id.to_string()], |row| {
            Ok(Message {
                role: row.get(0)?,
                content: row.get(1)?,
                timestamp_us: row.get(2)?,
            })
        })?;

        let mut history = VecDeque::new();
        for msg in msg_iter {
            history.push_back(msg?);
        }

        Ok(Some(SessionState {
            history,
            durable_constraints: constraints,
            current_agent: agent,
            current_model: model,
        }))
    }

    /// List all session IDs stored in the database.
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
}
