use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum TransferEventLevel {
    Info,
    Warning,
    Error,
}

impl TransferEventLevel {
    pub fn as_label(&self) -> &'static str {
        match self {
            TransferEventLevel::Info => "info",
            TransferEventLevel::Warning => "warning",
            TransferEventLevel::Error => "error",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferEventRecord {
    pub id: String,
    pub job_id: String,
    pub session_id: String,
    pub level: TransferEventLevel,
    pub message: String,
    pub created_at: i64,
}
