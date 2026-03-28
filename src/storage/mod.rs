mod sqlite;

use crate::domain::{SessionProfile, TransferEventRecord, TransferJobRecord};

pub use sqlite::SqliteSessionStore;

pub trait SessionRepository {
    fn list_sessions(&self) -> Result<Vec<SessionProfile>, String>;
    fn find_session(&self, session_id: &str) -> Result<Option<SessionProfile>, String>;
    fn save_session(&self, session: &SessionProfile) -> Result<(), String>;
    fn delete_session(&self, session_id: &str) -> Result<(), String>;
    fn list_transfer_jobs(&self) -> Result<Vec<TransferJobRecord>, String>;
    fn save_transfer_job(&self, job: &TransferJobRecord) -> Result<(), String>;
    fn delete_transfer_job(&self, job_id: &str) -> Result<(), String>;
    fn list_transfer_events(&self, limit: usize) -> Result<Vec<TransferEventRecord>, String>;
    fn save_transfer_event(&self, event: &TransferEventRecord) -> Result<(), String>;
    fn clear_transfer_events(&self) -> Result<(), String>;
}
