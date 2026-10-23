//! Аудиты знаний: хранилище запусков и находок (`docs/features/knowledge-base/audits.md`, Т-36).
//!
//! Отдельная БД `.bars/audits.sqlite3`. Аудит читает документацию и ничего не пишет в Markdown;
//! здесь — только результат: запуск, находки с отпечатками и состояние «новая / прежняя / исчезла».
//! Сам расчёт видов лежит в воркере: он использует индекс ссылок и ядро дублей.

use std::path::Path;

pub mod contradictions;
mod roles;
mod store;

#[cfg(test)]
mod tests;

pub use roles::document_role;
pub use store::{
    AuditError, AuditStore, FindingRow, FindingsPage, NewFinding, RunRow, RunStatus, ScheduleMode,
    Standing, Subject, store_path,
};

/// Отпечаток находки: хеш частей, не зависящих от номеров строк (путь, заголовок, текст ссылки), —
/// по нему запуски сравниваются между собой.
pub fn fingerprint(parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.finalize().to_hex()[..32].to_string()
}

/// Метка состояния документации на диске: путь, размер и время правки каждого `.md`. Меняется при
/// любой правке; по ней режим «при изменении» решает, пора ли пересчитать вид.
pub fn docs_stamp(
    project_root: &Path,
    docs_root: &Path,
) -> Result<String, crate::indexing::walk::WalkError> {
    let walk = crate::indexing::walk::walk_docs(project_root, docs_root)?;
    let mut entries: Vec<String> = walk
        .files
        .iter()
        .map(|file| {
            let modified = file
                .modified
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos());
            format!("{}|{}|{modified}", file.path, file.size)
        })
        .collect();
    entries.sort();
    let joined = entries.join("\n");
    Ok(fingerprint(&[&joined]))
}

/// Вид аудита (OQ-137, OQ-143).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditKind {
    BrokenLinks,
    Duplicates,
    Contradictions,
}

impl AuditKind {
    pub const ALL: [AuditKind; 3] = [
        AuditKind::BrokenLinks,
        AuditKind::Duplicates,
        AuditKind::Contradictions,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::BrokenLinks => "broken_links",
            AuditKind::Duplicates => "duplicates",
            AuditKind::Contradictions => "contradictions",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }

    /// Расписание по умолчанию: фоновая нагрузка без согласия не появляется (OQ-137).
    pub fn default_schedule(self) -> ScheduleMode {
        if self == AuditKind::BrokenLinks {
            ScheduleMode::OnChange
        } else {
            ScheduleMode::Off
        }
    }

    /// Режим «при изменении» есть только у дешёвого вида без модели.
    pub fn allows(self, mode: ScheduleMode) -> bool {
        mode != ScheduleMode::OnChange || self == AuditKind::BrokenLinks
    }
}
