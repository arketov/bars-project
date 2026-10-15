//! Путь документа из запроса агента или CLI (`docs/features/agent-tools/mcp-tools.md`, Т-26, OQ-35).
//!
//! Общий для `outline` и `read`: оба принимают один и тот же вид пути.

use std::path::{Path, PathBuf};

/// Путь не привёл к документу.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum DocPathError {
    /// Читаются только `.md` (`invalid_params`).
    #[error("читаются только файлы .md: {0}")]
    NotMarkdown(String),
    /// Файла нет ни по одному из допустимых отсчётов (`file_not_found`).
    #[error("файл не найден: {0}")]
    NotFound(String),
}

/// Находит файл документа на диске.
///
/// - абсолютный путь берётся как есть;
/// - `./x.md` отсчитывается от корня проекта;
/// - остальное — от корня документации, а если там файла нет, то от корня проекта: так работает
///   и `a/x.md`, и путь из выдачи поиска `docs/a/x.md` (OQ-35).
///
/// Разделители `\` и `/` равнозначны.
pub fn resolve_doc_path(
    project_root: &Path,
    docs_root: &Path,
    path: &str,
) -> Result<PathBuf, DocPathError> {
    let normalized = path.trim().replace('\\', "/");
    if !normalized
        .get(normalized.len().saturating_sub(3)..)
        .is_some_and(|ext| ext.eq_ignore_ascii_case(".md"))
    {
        return Err(DocPathError::NotMarkdown(path.to_string()));
    }

    let candidates: Vec<PathBuf> = if Path::new(&normalized).is_absolute() {
        vec![PathBuf::from(&normalized)]
    } else if let Some(explicit) = normalized.strip_prefix("./") {
        vec![project_root.join(explicit)]
    } else {
        vec![
            project_root.join(docs_root).join(&normalized),
            project_root.join(&normalized),
        ]
    };
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| DocPathError::NotFound(path.to_string()))
}

#[cfg(test)]
mod tests;
