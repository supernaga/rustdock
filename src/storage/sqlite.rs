use crate::domain::{
    AuthMethod, SessionProfile, SessionSyncState, TransferEventLevel, TransferEventRecord,
    TransferJobKind, TransferJobRecord, TransferJobStatus,
};
use crate::storage::SessionRepository;
use rusqlite::{params, types::Type, Connection, Error as SqlError, OptionalExtension};
use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SqliteSessionStore {
    database_path: PathBuf,
}

impl SqliteSessionStore {
    pub fn open_default() -> Result<Self, String> {
        let base_dir = default_data_dir()?;
        fs::create_dir_all(&base_dir).map_err(|err| err.to_string())?;
        let database_path = base_dir.join("workbench.sqlite3");
        let store = Self { database_path };
        store.initialize_schema()?;
        Ok(store)
    }

    pub fn database_path(&self) -> &Path {
        &self.database_path
    }

    fn open_connection(&self) -> Result<Connection, String> {
        Connection::open(&self.database_path).map_err(|err| err.to_string())
    }

    fn initialize_schema(&self) -> Result<(), String> {
        let connection = self.open_connection()?;
        connection
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS sessions (
                    id TEXT PRIMARY KEY,
                    name TEXT NOT NULL,
                    host TEXT NOT NULL,
                    port INTEGER NOT NULL,
                    username TEXT NOT NULL,
                    auth_method_json TEXT NOT NULL,
                    tags_json TEXT NOT NULL,
                    notes TEXT NOT NULL,
                    local_roots_json TEXT NOT NULL,
                    remote_roots_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    last_connected_at INTEGER,
                    sync_state TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS transfer_jobs (
                    id TEXT PRIMARY KEY,
                    session_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    local_path TEXT NOT NULL,
                    remote_path TEXT NOT NULL,
                    status TEXT NOT NULL,
                    message TEXT NOT NULL,
                    transferred INTEGER,
                    total INTEGER,
                    bytes INTEGER,
                    attempt_count INTEGER NOT NULL DEFAULT 0,
                    max_retries INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );

                CREATE TABLE IF NOT EXISTS transfer_events (
                    id TEXT PRIMARY KEY,
                    job_id TEXT NOT NULL,
                    session_id TEXT NOT NULL,
                    level TEXT NOT NULL,
                    message TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );
                ",
            )
            .map_err(|err| err.to_string())?;

        ensure_column(
            &connection,
            "transfer_jobs",
            "attempt_count",
            "ALTER TABLE transfer_jobs ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
        )?;
        ensure_column(
            &connection,
            "transfer_jobs",
            "max_retries",
            "ALTER TABLE transfer_jobs ADD COLUMN max_retries INTEGER NOT NULL DEFAULT 0",
        )?;

        Ok(())
    }
}

impl SessionRepository for SqliteSessionStore {
    fn list_sessions(&self) -> Result<Vec<SessionProfile>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    id,
                    name,
                    host,
                    port,
                    username,
                    auth_method_json,
                    tags_json,
                    notes,
                    local_roots_json,
                    remote_roots_json,
                    created_at,
                    updated_at,
                    last_connected_at,
                    sync_state
                FROM sessions
                ORDER BY updated_at DESC, name ASC
                ",
            )
            .map_err(|err| err.to_string())?;

        let mapped_rows = statement
            .query_map([], |row| {
                let auth_method_json: String = row.get(5)?;
                let tags_json: String = row.get(6)?;
                let local_roots_json: String = row.get(8)?;
                let remote_roots_json: String = row.get(9)?;
                let sync_state: String = row.get(13)?;

                Ok(SessionProfile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    host: row.get(2)?,
                    port: row.get(3)?,
                    username: row.get(4)?,
                    auth_method: deserialize_json(&auth_method_json)?,
                    tags: deserialize_json(&tags_json)?,
                    notes: row.get(7)?,
                    local_roots: deserialize_json(&local_roots_json)?,
                    remote_roots: deserialize_json(&remote_roots_json)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    last_connected_at: row.get(12)?,
                    sync_state: decode_sync_state(&sync_state),
                })
            })
            .map_err(|err| err.to_string())?;

        let mut sessions = Vec::new();
        for row in mapped_rows {
            sessions.push(row.map_err(|err| err.to_string())?);
        }
        Ok(sessions)
    }

    fn find_session(&self, session_id: &str) -> Result<Option<SessionProfile>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    id,
                    name,
                    host,
                    port,
                    username,
                    auth_method_json,
                    tags_json,
                    notes,
                    local_roots_json,
                    remote_roots_json,
                    created_at,
                    updated_at,
                    last_connected_at,
                    sync_state
                FROM sessions
                WHERE id = ?1
                ",
            )
            .map_err(|err| err.to_string())?;

        statement
            .query_row(params![session_id], |row| {
                let auth_method_json: String = row.get(5)?;
                let tags_json: String = row.get(6)?;
                let local_roots_json: String = row.get(8)?;
                let remote_roots_json: String = row.get(9)?;
                let sync_state: String = row.get(13)?;

                Ok(SessionProfile {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    host: row.get(2)?,
                    port: row.get(3)?,
                    username: row.get(4)?,
                    auth_method: deserialize_json(&auth_method_json)?,
                    tags: deserialize_json(&tags_json)?,
                    notes: row.get(7)?,
                    local_roots: deserialize_json(&local_roots_json)?,
                    remote_roots: deserialize_json(&remote_roots_json)?,
                    created_at: row.get(10)?,
                    updated_at: row.get(11)?,
                    last_connected_at: row.get(12)?,
                    sync_state: decode_sync_state(&sync_state),
                })
            })
            .optional()
            .map_err(|err| err.to_string())
    }

    fn save_session(&self, session: &SessionProfile) -> Result<(), String> {
        self.open_connection()?
            .execute(
                "
                INSERT INTO sessions (
                    id,
                    name,
                    host,
                    port,
                    username,
                    auth_method_json,
                    tags_json,
                    notes,
                    local_roots_json,
                    remote_roots_json,
                    created_at,
                    updated_at,
                    last_connected_at,
                    sync_state
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    host = excluded.host,
                    port = excluded.port,
                    username = excluded.username,
                    auth_method_json = excluded.auth_method_json,
                    tags_json = excluded.tags_json,
                    notes = excluded.notes,
                    local_roots_json = excluded.local_roots_json,
                    remote_roots_json = excluded.remote_roots_json,
                    updated_at = excluded.updated_at,
                    last_connected_at = excluded.last_connected_at,
                    sync_state = excluded.sync_state
                ",
                params![
                    &session.id,
                    &session.name,
                    &session.host,
                    i64::from(session.port),
                    &session.username,
                    serialize_json(&session.auth_method).map_err(|err| err.to_string())?,
                    serialize_json(&session.tags).map_err(|err| err.to_string())?,
                    &session.notes,
                    serialize_json(&session.local_roots).map_err(|err| err.to_string())?,
                    serialize_json(&session.remote_roots).map_err(|err| err.to_string())?,
                    session.created_at,
                    session.updated_at,
                    session.last_connected_at,
                    session.sync_state.as_label(),
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn delete_session(&self, session_id: &str) -> Result<(), String> {
        let connection = self.open_connection()?;
        connection
            .execute("DELETE FROM transfer_events WHERE session_id = ?1", params![session_id])
            .map_err(|err| err.to_string())?;
        connection
            .execute("DELETE FROM transfer_jobs WHERE session_id = ?1", params![session_id])
            .map_err(|err| err.to_string())?;
        connection
            .execute("DELETE FROM sessions WHERE id = ?1", params![session_id])
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn list_transfer_jobs(&self) -> Result<Vec<TransferJobRecord>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    id,
                    session_id,
                    kind,
                    local_path,
                    remote_path,
                    status,
                    message,
                    transferred,
                    total,
                    bytes,
                    attempt_count,
                    max_retries,
                    created_at,
                    updated_at
                FROM transfer_jobs
                ORDER BY created_at ASC, id ASC
                ",
            )
            .map_err(|err| err.to_string())?;

        let rows = statement
            .query_map([], |row| {
                let status: String = row.get(5)?;
                let normalized_status = decode_transfer_job_status(&status);
                let message: String = row.get(6)?;
                Ok(TransferJobRecord {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    kind: decode_transfer_job_kind(&row.get::<_, String>(2)?),
                    local_path: row.get(3)?,
                    remote_path: row.get(4)?,
                    status: if normalized_status == TransferJobStatus::Running {
                        TransferJobStatus::Queued
                    } else {
                        normalized_status
                    },
                    message: if status == "running" {
                        "Recovered after restart".to_string()
                    } else {
                        message
                    },
                    transferred: row.get(7)?,
                    total: row.get(8)?,
                    bytes: row.get(9)?,
                    attempt_count: row.get(10)?,
                    max_retries: row.get(11)?,
                    created_at: row.get(12)?,
                    updated_at: row.get(13)?,
                })
            })
            .map_err(|err| err.to_string())?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row.map_err(|err| err.to_string())?);
        }
        Ok(jobs)
    }

    fn save_transfer_job(&self, job: &TransferJobRecord) -> Result<(), String> {
        self.open_connection()?
            .execute(
                "
                INSERT INTO transfer_jobs (
                    id,
                    session_id,
                    kind,
                    local_path,
                    remote_path,
                    status,
                    message,
                    transferred,
                    total,
                    bytes,
                    attempt_count,
                    max_retries,
                    created_at,
                    updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                ON CONFLICT(id) DO UPDATE SET
                    session_id = excluded.session_id,
                    kind = excluded.kind,
                    local_path = excluded.local_path,
                    remote_path = excluded.remote_path,
                    status = excluded.status,
                    message = excluded.message,
                    transferred = excluded.transferred,
                    total = excluded.total,
                    bytes = excluded.bytes,
                    attempt_count = excluded.attempt_count,
                    max_retries = excluded.max_retries,
                    updated_at = excluded.updated_at
                ",
                params![
                    &job.id,
                    &job.session_id,
                    job.kind.as_label(),
                    &job.local_path,
                    &job.remote_path,
                    job.status.as_label(),
                    &job.message,
                    job.transferred,
                    job.total,
                    job.bytes,
                    job.attempt_count,
                    job.max_retries,
                    job.created_at,
                    job.updated_at,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn delete_transfer_job(&self, job_id: &str) -> Result<(), String> {
        let connection = self.open_connection()?;
        connection
            .execute("DELETE FROM transfer_events WHERE job_id = ?1", params![job_id])
            .map_err(|err| err.to_string())?;
        connection
            .execute("DELETE FROM transfer_jobs WHERE id = ?1", params![job_id])
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn list_transfer_events(&self, limit: usize) -> Result<Vec<TransferEventRecord>, String> {
        let connection = self.open_connection()?;
        let mut statement = connection
            .prepare(
                "
                SELECT
                    id,
                    job_id,
                    session_id,
                    level,
                    message,
                    created_at
                FROM transfer_events
                ORDER BY created_at DESC, id DESC
                LIMIT ?1
                ",
            )
            .map_err(|err| err.to_string())?;

        let rows = statement
            .query_map(params![limit as i64], |row| {
                Ok(TransferEventRecord {
                    id: row.get(0)?,
                    job_id: row.get(1)?,
                    session_id: row.get(2)?,
                    level: decode_transfer_event_level(&row.get::<_, String>(3)?),
                    message: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })
            .map_err(|err| err.to_string())?;

        let mut events = Vec::new();
        for row in rows {
            events.push(row.map_err(|err| err.to_string())?);
        }
        Ok(events)
    }

    fn save_transfer_event(&self, event: &TransferEventRecord) -> Result<(), String> {
        self.open_connection()?
            .execute(
                "
                INSERT INTO transfer_events (
                    id,
                    job_id,
                    session_id,
                    level,
                    message,
                    created_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ",
                params![
                    &event.id,
                    &event.job_id,
                    &event.session_id,
                    event.level.as_label(),
                    &event.message,
                    event.created_at,
                ],
            )
            .map_err(|err| err.to_string())?;
        Ok(())
    }

    fn clear_transfer_events(&self) -> Result<(), String> {
        self.open_connection()?
            .execute("DELETE FROM transfer_events", [])
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

fn default_data_dir() -> Result<PathBuf, String> {
    let home_dir = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
    Ok(PathBuf::from(home_dir)
        .join(".local")
        .join("share")
        .join("rustdock"))
}

fn serialize_json<T: Serialize>(value: &T) -> Result<String, SqlError> {
    serde_json::to_string(value).map_err(|err| SqlError::ToSqlConversionFailure(Box::new(err)))
}

fn deserialize_json<T: DeserializeOwned>(value: &str) -> Result<T, SqlError> {
    serde_json::from_str(value).map_err(|err| SqlError::FromSqlConversionFailure(0, Type::Text, Box::new(err)))
}

fn decode_sync_state(value: &str) -> SessionSyncState {
    match value {
        "Pending upload" => SessionSyncState::PendingUpload,
        "Synced" => SessionSyncState::Synced,
        "Conflict" => SessionSyncState::Conflict,
        _ => SessionSyncState::LocalOnly,
    }
}

fn decode_transfer_job_kind(value: &str) -> TransferJobKind {
    match value {
        "download" => TransferJobKind::Download,
        _ => TransferJobKind::Upload,
    }
}

fn decode_transfer_job_status(value: &str) -> TransferJobStatus {
    match value {
        "running" => TransferJobStatus::Running,
        "success" => TransferJobStatus::Success,
        "error" => TransferJobStatus::Error,
        _ => TransferJobStatus::Queued,
    }
}

fn decode_transfer_event_level(value: &str) -> TransferEventLevel {
    match value {
        "warning" => TransferEventLevel::Warning,
        "error" => TransferEventLevel::Error,
        _ => TransferEventLevel::Info,
    }
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    alter_sql: &str,
) -> Result<(), String> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut statement = connection.prepare(&pragma).map_err(|err| err.to_string())?;
    let exists = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|err| err.to_string())?
        .filter_map(Result::ok)
        .any(|name| name == column);

    if !exists {
        connection.execute(alter_sql, []).map_err(|err| err.to_string())?;
    }

    Ok(())
}

#[allow(dead_code)]
fn _auth_example() -> AuthMethod {
    AuthMethod::Agent
}
