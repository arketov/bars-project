//! Содержимое папки для `bars_read` — аналог `ls` (`docs/features/agent-tools/mcp-tools.md`,
//! OQ-320).
//!
//! Источник — индекс, как у дерева окна: файлы не читаются. Список — один уровень: сначала
//! подпапки (число документов в поддереве и `title` её `Index.md`), затем документы самой папки.

use std::collections::{BTreeMap, BTreeSet};

use bars_core::doc_path::{docs_relative, stored_candidates, unknown_file_message};
use bars_core::storage::{Index, index_path};
use bars_protocol::{ErrorKind, FolderEntry, FolderListing, ReadOutput, ReadParams, RpcError};

use super::errors::worker_error;
use super::{WorkerHandler, internal, to_u32};

/// Строк на странице списка.
const PAGE: usize = 100;

impl WorkerHandler {
    /// Путь папки в `bars_read`: индекс под общим замком проекта, модель не нужна (Т-25).
    pub(super) async fn read_folder(&self, params: ReadParams) -> Result<ReadOutput, RpcError> {
        if params.lines.is_some() || params.under.is_some() {
            return Err(RpcError::bars(
                ErrorKind::InvalidParams,
                "lines и under относятся к документу, а путь указывает на папку",
            ));
        }
        let descending = match params.order.as_deref() {
            None | Some("asc") => false,
            Some("desc") => true,
            Some(other) => {
                return Err(RpcError::bars(
                    ErrorKind::InvalidParams,
                    format!("order — asc или desc, не {other}"),
                ));
            }
        };
        let offset = match params.cursor.as_deref() {
            None => 0,
            Some(cursor) => cursor.parse::<usize>().map_err(|_| {
                RpcError::bars(
                    ErrorKind::InvalidParams,
                    "cursor не от этого списка: берите его из предыдущего ответа",
                )
            })?,
        };
        let (project, view) = self.usable_view(&params.target)?;
        let docs_root = self.settings(&project)?.docs_root;
        // По виду worktree папка перечисляется по диску W: индекс и дельта не нужны.
        if let super::ViewSelector::Worktree(item) = &view {
            let (root, disk_root) = (item.view_root.clone(), docs_root.clone());
            let docs = tokio::task::spawn_blocking(move || {
                super::worktree_view::disk_documents(&root, &disk_root)
            })
            .await
            .map_err(internal)??;
            let listing = list_folder(&docs, &docs_root, &params.path, descending, offset)?;
            return Ok(ReadOutput {
                folder: Some(listing),
                ..ReadOutput::default()
            });
        }
        let pending = self.worker.status(&project).map_err(worker_error)?.pending;
        let docs = if index_path(&project).exists() {
            let gate = self
                .worker
                .gate(&project)
                .map_err(worker_error)?
                .read_owned()
                .await;
            tokio::task::spawn_blocking(move || {
                let _gate = gate;
                let index = Index::open_read_only(&index_path(&project))?;
                let mut statement = index
                    .conn()
                    .prepare("SELECT path, title FROM documents ORDER BY path")?;
                let rows = statement.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(bars_core::storage::StorageError::from)
            })
            .await
            .map_err(internal)?
            .map_err(internal)?
        } else {
            Vec::new()
        };
        let mut listing = list_folder(&docs, &docs_root, &params.path, descending, offset)?;
        listing.pending = to_u32(pending);
        Ok(ReadOutput {
            folder: Some(listing),
            ..ReadOutput::default()
        })
    }
}

/// Один уровень содержимого папки из пар (путь индекса от корня проекта, `title`).
pub(super) fn list_folder(
    docs: &[(String, String)],
    docs_root: &str,
    input: &str,
    descending: bool,
    offset: usize,
) -> Result<FolderListing, RpcError> {
    let candidates = stored_candidates(docs_root, input);
    if candidates.is_empty() {
        return Err(RpcError::bars(
            ErrorKind::InvalidParams,
            "путь вне проекта: просмотр папки не выходит за корень проекта",
        ));
    }
    let chosen = candidates.iter().find_map(|candidate| {
        let folder = candidate.trim_end_matches('/');
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        docs.iter()
            .any(|(path, _)| path.starts_with(&prefix))
            .then_some((folder.to_string(), prefix))
    });
    let Some((folder, prefix)) = chosen else {
        let mut known: BTreeSet<&str> = BTreeSet::new();
        for (path, _) in docs {
            for (end, _) in path.match_indices('/') {
                known.insert(&path[..end]);
            }
        }
        return Err(RpcError::bars(
            ErrorKind::InvalidParams,
            unknown_file_message(docs_root, known.iter().copied(), input),
        ));
    };

    let mut folders: BTreeMap<&str, (u32, String)> = BTreeMap::new();
    let mut own: Vec<(&str, &str)> = Vec::new();
    for (path, title) in docs {
        let Some(rest) = path.strip_prefix(&prefix) else {
            continue;
        };
        match rest.split_once('/') {
            Some((name, tail)) => {
                let entry = folders.entry(name).or_default();
                entry.0 += 1;
                if tail.eq_ignore_ascii_case("index.md") {
                    entry.1.clone_from(title);
                }
            }
            None => own.push((rest, title)),
        }
    }
    own.sort_unstable_by_key(|(name, _)| *name);
    let mut entries: Vec<FolderEntry> = folders
        .into_iter()
        .map(|(name, (documents, title))| FolderEntry {
            name: format!("{name}/"),
            is_folder: true,
            documents,
            title,
        })
        .collect();
    let mut documents: Vec<FolderEntry> = own
        .into_iter()
        .map(|(name, title)| FolderEntry {
            name: name.to_string(),
            is_folder: false,
            documents: 0,
            title: title.to_string(),
        })
        .collect();
    if descending {
        entries.reverse();
        documents.reverse();
    }
    let folders_total = to_u32(entries.len());
    let documents_total = to_u32(documents.len());
    entries.extend(documents);
    let next_cursor = (entries.len() > offset + PAGE).then(|| (offset + PAGE).to_string());
    let page = entries.into_iter().skip(offset).take(PAGE).collect();

    let path = if folder.is_empty() {
        "./".to_string()
    } else {
        let shown = docs_relative(docs_root, &format!("{folder}/"));
        match shown.trim_end_matches('/') {
            "" => ".".to_string(),
            other => other.to_string(),
        }
    };
    Ok(FolderListing {
        path,
        folders: folders_total,
        documents: documents_total,
        entries: page,
        next_cursor,
        pending: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docs() -> Vec<(String, String)> {
        [
            ("docs/Index.md", "Оглавление"),
            ("docs/chats/Index.md", "Журнал"),
            ("docs/chats/2026-11-10-a/handoff.md", "Итог A"),
            ("docs/chats/2026-11-12-b/handoff.md", "Итог B"),
            ("docs/chats/2026-11-12-b/notes.md", "Заметки B"),
            ("docs/adr/0001-x.md", "Решение"),
            ("README.md", "Корень"),
        ]
        .iter()
        .map(|(p, t)| (p.to_string(), t.to_string()))
        .collect()
    }

    #[test]
    fn root_lists_folders_first_then_documents() {
        let listing = list_folder(&docs(), "docs", ".", false, 0).unwrap();
        assert_eq!(listing.path, ".");
        assert_eq!((listing.folders, listing.documents), (2, 1));
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["adr/", "chats/", "Index.md"]);
        let chats = &listing.entries[1];
        assert_eq!((chats.documents, chats.title.as_str()), (5, "Журнал"));
    }

    #[test]
    fn descending_puts_latest_session_first() {
        let listing = list_folder(&docs(), "docs", "chats", true, 0).unwrap();
        assert_eq!(listing.path, "chats");
        assert_eq!(listing.entries[0].name, "2026-11-12-b/");
        assert_eq!(listing.entries[0].documents, 2);
        assert_eq!(listing.entries[2].name, "Index.md");
    }

    #[test]
    fn project_root_form_and_paging() {
        let by_project = list_folder(&docs(), "docs", "./docs/chats/", false, 0).unwrap();
        assert_eq!(by_project.entries.len(), 3);
        let page = list_folder(&docs(), "docs", "chats", false, 2).unwrap();
        assert_eq!(page.entries.len(), 1);
        assert!(page.next_cursor.is_none());
        let outside = list_folder(&docs(), "docs", "../x", false, 0);
        assert!(outside.is_err());
    }

    #[test]
    fn unknown_folder_is_invalid_params() {
        let error = list_folder(&docs(), "docs", "nope", false, 0).unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
    }
}
