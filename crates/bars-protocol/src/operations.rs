//! Ограниченный журнал операций окна: эпоха, курсор и неизменяемые итоги.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEventsParams {
    pub epoch: Option<String>,
    pub after: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationRetryParams {
    pub epoch: String,
    pub id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationReport {
    pub added: usize,
    pub changed: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub problems: usize,
    /// До 20 путей из отчёта этой попытки; полный счётчик — problems.
    pub problem_paths: Vec<String>,
    pub index_applied: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    pub id: u64,
    pub sequence: u64,
    /// check, files, rebuild, download, prepare.
    pub kind: String,
    pub source: String,
    pub project: Option<String>,
    pub generation: u64,
    pub model: Option<String>,
    pub variant: Option<String>,
    pub reranker: bool,
    /// queued, running, pause_requested, paused, stop_requested; terminal outcomes.
    pub state: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub docs_done: Option<usize>,
    pub docs_total: Option<usize>,
    pub percent: Option<u8>,
    pub queue_position: Option<usize>,
    pub report: Option<OperationReport>,
    pub error: Option<String>,
    pub target_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationEventsOutput {
    pub epoch: String,
    pub cursor: u64,
    pub gap: bool,
    pub active: Vec<Operation>,
    pub completed: Vec<Operation>,
}
