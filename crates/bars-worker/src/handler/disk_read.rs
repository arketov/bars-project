use super::{WorkerHandler, errors::worker_error};
use bars_core::link_index::LinkIndex;
use bars_protocol::{
    DiskLinksOutput, DiskLinksParams, DiskTextOutput, DiskTextParams, ErrorKind, INTERNAL_ERROR,
    RpcError, TextSearchParams,
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicBool},
    time::Instant,
};

fn invalid(message: &str) -> RpcError {
    RpcError::bars(ErrorKind::InvalidParams, message)
}
fn internal(error: impl std::fmt::Display) -> RpcError {
    RpcError::standard(INTERNAL_ERROR, error.to_string())
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

fn document_path(path: &str) -> Result<(), RpcError> {
    if !path.trim().to_lowercase().ends_with(".md") {
        return Err(invalid("Нужен путь к файлу .md"));
    }
    Ok(())
}

/// Документ ссылок в форме индекса (от корня проекта). Файла нет на диске — первая форма пути:
/// `known: false` остаётся ответом, а не ошибкой; выход за корень проекта — `invalid_params`.
pub(super) fn links_document(
    project: &Path,
    docs_root: &str,
    path: &str,
) -> Result<String, RpcError> {
    match bars_core::doc_path::project_relative_path(project, Path::new(docs_root), path) {
        Ok(stored) => Ok(stored),
        Err(bars_core::doc_path::DocPathError::NotFound(_)) => {
            bars_core::doc_path::stored_candidates(docs_root, path)
                .into_iter()
                .next()
                .ok_or_else(|| invalid("Нужен путь к файлу .md внутри проекта"))
        }
        Err(error) => Err(super::errors::doc_path_error(error)),
    }
}

impl WorkerHandler {
    pub(super) async fn disk_text(&self, p: DiskTextParams) -> Result<DiskTextOutput, RpcError> {
        if p.limit_docs == Some(0) || p.limit_hits == Some(0) || p.offset > 100_000 {
            return Err(invalid(
                "Лимиты должны быть положительными; offset не больше 100000",
            ));
        }
        if p.path.is_none() && (p.offset != 0 || p.revision.is_some())
            || p.offset != 0 && p.revision.is_none()
        {
            return Err(invalid("Продолжение требует path и revision"));
        }
        if let Some(path) = &p.path {
            document_path(path)?;
        }
        // Агенту — только одна конкретная строка; широкий шаблон уходит в смысловой поиск bars_search (ISS-109).
        crate::text_search::breadth::check(&p.query, p.regex)?;
        // По виду worktree точный поиск идёт по диску W: обход корня вида без списка областей.
        let (owner, view) = self.resolve_view(&p.target)?;
        let docs = self.worker.docs_root(&owner).map_err(worker_error)?;
        let project = super::worktree_view::view_root(&owner, &view);
        let continued = p
            .path
            .as_deref()
            .map(|path| links_document(&project, &docs, path))
            .transpose()?;
        let docs_limit = p.limit_docs.unwrap_or(20).min(100);
        let hits_limit = p.limit_hits.unwrap_or(100).min(500);
        let requested = json!({"limit_docs":p.limit_docs,"limit_hits":p.limit_hits});
        let applied = json!({"limit_docs":docs_limit,"limit_hits":hits_limit,"page_size":if p.path.is_some(){hits_limit.min(100)}else{5},"offset":p.offset,
            "path":p.path,"revision":p.revision,"case_sensitive":p.case_sensitive,"regex":p.regex,
            "path_filter":p.path_filter,"modules":p.modules,"files":p.files,"paths":p.paths});
        let clamped =
            p.limit_docs.is_some_and(|n| n > 100) || p.limit_hits.is_some_and(|n| n > 500);
        let params = TextSearchParams {
            project: project.to_string_lossy().into_owned(),
            query: p.query,
            case_sensitive: p.case_sensitive,
            regex: p.regex,
            path_filter: p.path_filter,
            modules: p.modules,
            files: p.files,
            paths: p.paths,
            limit_docs: Some(docs_limit),
            limit_hits: Some(hits_limit),
            path: continued,
            offset: p.offset,
            revision: p.revision,
            ..Default::default()
        };
        // Контекст MCP не входит в ActiveSearches окна; отмена другого вызова его не затрагивает.
        let stop = Arc::new(AtomicBool::new(false));
        let _cancel = CancelOnDrop(Arc::clone(&stop));
        let result = tokio::task::spawn_blocking(move || {
            crate::text_search::search_with_policy(
                &project,
                Path::new(&docs),
                &params,
                &stop,
                true,
                20,
            )
        })
        .await
        .map_err(internal)??;
        if result.documents.is_empty() && result.skipped_total > 0 {
            return Err(internal(format!(
                "Поиск неполон: не прочитано {} файлов/папок; отсутствие вхождений не подтверждено",
                result.skipped_total
            )));
        }
        let mut limit_reasons: Vec<String> = result.truncated.iter().cloned().collect();
        if result.skipped_truncated {
            limit_reasons.push("skipped".into());
        }
        Ok(DiskTextOutput {
            result,
            requested,
            applied,
            clamped,
            limit_reasons,
        })
    }

    pub(super) async fn disk_links(&self, p: DiskLinksParams) -> Result<DiskLinksOutput, RpcError> {
        let started = Instant::now();
        document_path(&p.path)?;
        if !matches!(p.direction.as_str(), "outgoing" | "incoming") || p.limit == Some(0) {
            return Err(invalid(
                "direction: outgoing или incoming; limit должен быть положительным",
            ));
        }
        let project = self.resolve(&p.target)?;
        let docs = self.worker.docs_root(&project).map_err(worker_error)?;
        let path = links_document(&project, &docs, &p.path)?;
        let binding = blake3::hash(
            &serde_json::to_vec(&(project.to_string_lossy(), &path, &p.direction)).unwrap(),
        )
        .to_hex()
        .to_string();
        let lock = Arc::clone(&self.links_lock);
        tokio::task::spawn_blocking(move || {
            let _guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut index = LinkIndex::open(&project).map_err(internal)?;
            index
                .refresh_checked(&project, Path::new(&docs))
                .map_err(internal)?;
            let snapshot = index.snapshot().map_err(internal)?;
            let offset = if let Some(cursor) = &p.cursor {
                if cursor.len() > 512 {
                    return Err(invalid("Недопустимый cursor"));
                }
                let (owner, previous, offset): (String, String, usize) =
                    serde_json::from_str(cursor).map_err(|_| invalid("Недопустимый cursor"))?;
                if owner != binding {
                    return Err(invalid("Курсор другого проекта, документа или направления"));
                }
                if previous != snapshot {
                    return Err(RpcError::bars(
                        ErrorKind::StaleCursor,
                        "Снимок ссылок изменился; начните сначала",
                    ));
                }
                offset
            } else {
                0
            };
            let limit = p.limit.unwrap_or(50).min(200);
            let page = index
                .page(&path, p.direction == "incoming", offset, limit)
                .map_err(internal)?;
            let mut reasons = vec![];
            if page.has_more {
                reasons.push(if page.bytes_limited { "bytes" } else { "items" }.into());
            }
            if page.fields_truncated {
                reasons.push("fields".into());
            }
            let next_cursor = page
                .has_more
                .then(|| serde_json::to_string(&(binding, &snapshot, page.next_offset)).unwrap());
            let out = DiskLinksOutput {
                known: page.known,
                direction: p.direction,
                items: page.items,
                applied_limit: limit,
                clamped: p.limit.is_some_and(|n| n > 200),
                snapshot,
                has_more: page.has_more,
                next_cursor,
                truncated: !reasons.is_empty(),
                limit_reasons: reasons,
                elapsed_ms: started.elapsed().as_millis() as u64,
            };
            if serde_json::to_vec(&out).map_err(internal)?.len() > 512 * 1024 {
                return Err(internal("Ответ ссылок превышает бюджет"));
            }
            Ok(out)
        })
        .await
        .map_err(internal)?
    }
}
