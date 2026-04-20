use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TransferJobKind {
    Upload,
    Download,
    UploadDir,
    DownloadDir,
}

impl TransferJobKind {
    pub fn as_label(&self) -> &'static str {
        match self {
            TransferJobKind::Upload => "upload",
            TransferJobKind::Download => "download",
            TransferJobKind::UploadDir => "upload-dir",
            TransferJobKind::DownloadDir => "download-dir",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TransferJobStatus {
    Queued,
    Running,
    Success,
    Error,
}

impl TransferJobStatus {
    pub fn as_label(&self) -> &'static str {
        match self {
            TransferJobStatus::Queued => "queued",
            TransferJobStatus::Running => "running",
            TransferJobStatus::Success => "success",
            TransferJobStatus::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferJobRecord {
    pub id: String,
    pub session_id: String,
    pub kind: TransferJobKind,
    pub local_path: String,
    pub remote_path: String,
    pub status: TransferJobStatus,
    pub message: String,
    pub transferred: Option<u64>,
    pub total: Option<u64>,
    pub bytes: Option<u64>,
    pub attempt_count: i64,
    pub max_retries: i64,
    pub created_at: i64,
    pub updated_at: i64,
}
