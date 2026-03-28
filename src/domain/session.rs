use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AuthMethod {
    Password,
    PrivateKey { path: String },
    Agent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SessionSyncState {
    LocalOnly,
    PendingUpload,
    Synced,
    Conflict,
}

impl SessionSyncState {
    pub fn as_label(&self) -> &'static str {
        match self {
            SessionSyncState::LocalOnly => "Local only",
            SessionSyncState::PendingUpload => "Pending upload",
            SessionSyncState::Synced => "Synced",
            SessionSyncState::Conflict => "Conflict",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth_method: AuthMethod,
    pub tags: Vec<String>,
    pub notes: String,
    pub local_roots: Vec<String>,
    pub remote_roots: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub last_connected_at: Option<i64>,
    pub sync_state: SessionSyncState,
}

pub fn generate_session_id() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("session-{}", duration.as_nanos())
}

pub fn now_timestamp() -> i64 {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    duration.as_secs() as i64
}
