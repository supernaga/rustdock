use crate::domain::{SessionProfile, SessionSyncState};

#[derive(Clone, Debug)]
pub struct SyncSummary {
    pub local_only: usize,
    pub pending_upload: usize,
    pub synced: usize,
    pub conflicts: usize,
}

impl SyncSummary {
    pub fn from_sessions(sessions: &[SessionProfile]) -> Self {
        let mut summary = Self {
            local_only: 0,
            pending_upload: 0,
            synced: 0,
            conflicts: 0,
        };

        for session in sessions {
            match session.sync_state {
                SessionSyncState::LocalOnly => summary.local_only += 1,
                SessionSyncState::PendingUpload => summary.pending_upload += 1,
                SessionSyncState::Synced => summary.synced += 1,
                SessionSyncState::Conflict => summary.conflicts += 1,
            }
        }

        summary
    }
}

pub trait CloudSyncService {
    fn provider_name(&self) -> &str;
    fn status_line(&self, summary: &SyncSummary) -> String;
}

pub struct NoopCloudSyncService;

impl CloudSyncService for NoopCloudSyncService {
    fn provider_name(&self) -> &str {
        "Disabled"
    }

    fn status_line(&self, summary: &SyncSummary) -> String {
        format!(
            "{} local, {} queued, {} synced, {} conflicts",
            summary.local_only, summary.pending_upload, summary.synced, summary.conflicts
        )
    }
}
