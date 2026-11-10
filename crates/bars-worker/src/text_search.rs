//! Т-39: ограниченный поиск по свежим файлам. Нет доступа к индексу или моделям.
pub mod breadth;

use bars_core::indexing::walk::{DocFile, read_utf8};
use bars_protocol::{
    ErrorKind, RpcError, TextDocument, TextHit, TextSearchOutput, TextSearchParams, TextSkipped,
};
use rayon::prelude::*;
use regex::{Regex, RegexBuilder};
use std::{
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

fn invalid(message: impl Into<String>) -> RpcError {
    RpcError::bars(ErrorKind::InvalidParams, message)
}
fn cancelled() -> RpcError {
    RpcError::bars(ErrorKind::Cancelled, "Поиск отменён новым запросом")
}

fn matcher(p: &TextSearchParams) -> Result<Regex, RpcError> {
    if p.query.is_empty() || p.query.len() > 8192 {
        return Err(invalid("Запрос должен содержать от 1 до 8192 байт"));
    }
    let pattern = if p.regex {
        p.query.clone()
    } else {
        regex::escape(&p.query)
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(!p.case_sensitive)
        .size_limit(2 * 1024 * 1024)
        .build()
        .map_err(|e| {
            let mut error = invalid(format!("Некорректное выражение: {e}"));
            error.data = Some(serde_json::json!({"kind":"invalid_regex", "detail":e.to_string()}));
            error
        })
}

fn path_matcher(filter: &str) -> Result<Option<Regex>, RpcError> {
    if filter.len() > 4096 {
        return Err(invalid("Фильтр пути слишком длинный"));
    }
    if filter.is_empty() {
        return Ok(None);
    }
    let pattern = if filter.contains(['*', '?']) {
        let mut pattern = String::from("^");
        for c in filter.replace('\\', "/").chars() {
            match c {
                '*' => pattern.push_str("[^/]*"),
                '?' => pattern.push_str("[^/]"),
                _ => pattern.push_str(&regex::escape(&c.to_string())),
            }
        }
        pattern.push('$');
        pattern
    } else {
        regex::escape(&filter.replace('\\', "/"))
    };
    RegexBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map(Some)
        .map_err(|e| invalid(e.to_string()))
}

fn hit(line: &str, number: usize, from: usize, to: usize) -> TextHit {
    // Обрамление в кодовых точках, координаты в UTF-16: кириллица/emoji не сдвигают редактор.
    let start = line[..from]
        .char_indices()
        .rev()
        .nth(60)
        .map_or(0, |(i, _)| i);
    let end = line[start..]
        .char_indices()
        .nth(240)
        .map_or(line.len(), |(i, _)| start + i);
    let prefix = if start > 0 { "…" } else { "" };
    let suffix = if end < line.len() { "…" } else { "" };
    let a = prefix.encode_utf16().count() + line[start..from].encode_utf16().count();
    let b = a + line[from..to.min(end)].encode_utf16().count();
    TextHit {
        line: number as u32,
        column: line[..from].encode_utf16().count() as u32 + 1,
        length: line[from..to].encode_utf16().count() as u32,
        text: format!("{prefix}{}{suffix}", &line[start..end]),
        ranges: vec![[a as u32, b as u32]],
    }
}

fn title(text: &str, path: &str) -> String {
    let mut lines = text.lines();
    if lines.next() == Some("---") {
        let mut found = None;
        for line in lines {
            if line == "---" {
                return found.unwrap_or_else(|| stem(path));
            }
            if let Some(value) = line.strip_prefix("title:") {
                let value = value.trim().trim_matches(['\'', '"']);
                if !value.is_empty() {
                    found = Some(value.to_owned());
                }
            }
        }
    }
    stem(path)
}
fn stem(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

enum FileSearchError {
    Read(String),
    Stale,
}

fn search_file(
    file: &DocFile,
    re: &Regex,
    p: &TextSearchParams,
    count_limit: usize,
    stop: &AtomicBool,
    strict_revision: bool,
) -> Result<Option<TextDocument>, FileSearchError> {
    if stop.load(Ordering::Relaxed) {
        return Ok(None);
    }
    let original = read_utf8(file).map_err(|e| FileSearchError::Read(e.to_string()))?;
    let text = original.strip_prefix('\u{feff}').unwrap_or(&original);
    let revision = blake3::hash(original.as_bytes()).to_hex().to_string();
    if strict_revision && p.revision.as_ref().is_some_and(|r| *r != revision) {
        return Err(FileSearchError::Stale);
    }
    let mut result = TextDocument {
        path: file.path.clone(),
        module: file.module.clone(),
        title: title(text, &file.path),
        revision,
        hits_total: 0,
        hits: vec![],
        has_more: false,
    };
    let page = if p.path.is_some() {
        if strict_revision {
            p.limit_hits.unwrap_or(100).min(100)
        } else {
            100
        }
    } else {
        5
    };
    let offset = if p.revision.as_deref().is_some_and(|r| r != result.revision) {
        0
    } else {
        p.offset
    };
    let count_limit = if offset == 0 && p.path.is_some() {
        page
    } else {
        count_limit
    };
    // split сохраняет завершающую пустую строку для ^/$.
    for (n, raw) in text.split('\n').enumerate() {
        if stop.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        for m in re.find_iter(line) {
            result.hits_total += 1;
            if result.hits_total > offset && result.hits.len() < page {
                result.hits.push(hit(line, n + 1, m.start(), m.end()));
            }
            if result.hits_total > count_limit {
                result.has_more = true;
                break;
            }
        }
        if result.hits_total > count_limit {
            break;
        }
    }
    result.has_more |= result.hits_total > offset + result.hits.len();
    Ok((result.hits_total > 0).then_some(result))
}

/// `project` и `docs_root` разрешены воркером; `stop` живёт только до конца обращения окна.
pub fn search(
    project: &Path,
    docs_root: &Path,
    p: &TextSearchParams,
    stop: &AtomicBool,
) -> Result<TextSearchOutput, RpcError> {
    search_with_policy(project, docs_root, p, stop, false, usize::MAX)
}

pub fn search_with_policy(
    project: &Path,
    docs_root: &Path,
    p: &TextSearchParams,
    stop: &AtomicBool,
    strict_revision: bool,
    skipped_limit: usize,
) -> Result<TextSearchOutput, RpcError> {
    let started = Instant::now();
    let re = matcher(p)?;
    let filter = path_matcher(&p.path_filter)?;
    let limit_hits = p.limit_hits.unwrap_or(500).clamp(1, 5000);
    let limit_docs = p.limit_docs.unwrap_or(100).clamp(1, 500);
    if p.offset > 100_000 {
        return Err(invalid("Слишком большое смещение вхождений"));
    }
    let walk = bars_core::indexing::walk::walk_docs_bounded(
        project,
        docs_root,
        || !stop.load(Ordering::Relaxed),
        skipped_limit,
    )
    .map_err(|e| invalid(e.to_string()))?;
    let prefix = docs_root
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    let relative = |path: &str| {
        path.strip_prefix(&format!("{}/", prefix.trim_end_matches('/')))
            .unwrap_or(path)
            .to_owned()
    };
    // Обе формы пути (от корня документации и от корня проекта) — OQ-169.
    let stored_forms = |inputs: &[&String]| -> std::collections::HashSet<String> {
        inputs
            .iter()
            .flat_map(|input| bars_core::doc_path::stored_candidates(&prefix, input))
            .collect()
    };
    let wanted_files = stored_forms(&p.files.iter().collect::<Vec<_>>());
    let wanted_path = p.path.as_ref().map(|path| stored_forms(&[path]));
    // Агентская область `in`: префиксы в формах хранения (от корня проекта); файл входит, если
    // совпал с префиксом целиком или лежит под ним. Разделитель приписан один раз, а не на
    // каждую пару «файл × префикс».
    let scope_prefixes: Vec<String> = stored_forms(&p.paths.iter().collect::<Vec<_>>())
        .into_iter()
        .collect();
    let scope_folders: Vec<String> = scope_prefixes
        .iter()
        .map(|prefix| format!("{prefix}/"))
        .collect();
    let in_scope = |stored: &str| {
        scope_prefixes.iter().any(|prefix| stored == prefix)
            || scope_folders
                .iter()
                .any(|folder| stored.starts_with(folder.as_str()))
    };
    let files: Vec<_> = walk
        .files
        .into_iter()
        .filter(|f| {
            let inner = relative(&f.path);
            (wanted_path.as_ref().is_none_or(|set| set.contains(&f.path)))
                && (p.modules.is_empty() && p.files.is_empty() && p.paths.is_empty()
                    || p.modules.contains(&f.module)
                    || wanted_files.contains(&f.path)
                    || in_scope(&f.path))
                && filter.as_ref().is_none_or(|r| r.is_match(&inner))
        })
        .collect();
    if p.path.is_some() && files.is_empty() {
        return Err(RpcError::bars(
            ErrorKind::FileNotFound,
            "Документ не найден в области поиска",
        ));
    }
    let mut out = TextSearchOutput {
        skipped_total: walk.problems_total,
        skipped: walk
            .problems
            .into_iter()
            .map(|p| TextSkipped {
                path: p.path,
                reason: p.reason,
            })
            .collect(),
        ..Default::default()
    };
    static POOL: OnceLock<Result<rayon::ThreadPool, String>> = OnceLock::new();
    let pool = POOL
        .get_or_init(|| {
            rayon::ThreadPoolBuilder::new()
                .num_threads(
                    std::thread::available_parallelism()
                        .map_or(2, usize::from)
                        .min(8),
                )
                .thread_name(|n| format!("text-search-{n}"))
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| invalid(e.clone()))?;
    let mut processed = 0;
    for batch in files.chunks(32) {
        if stop.load(Ordering::Relaxed) {
            return Err(cancelled());
        }
        let remaining = if p.path.is_some() {
            p.offset
                + if strict_revision {
                    p.limit_hits.unwrap_or(100).min(100)
                } else {
                    100
                }
        } else {
            limit_hits - out.hits_total
        };
        let found: Vec<_> = pool.install(|| {
            batch
                .par_iter()
                .map(|f| search_file(f, &re, p, remaining, stop, strict_revision))
                .collect()
        });
        for (file, result) in batch.iter().zip(found) {
            processed += 1;
            let doc = match result {
                Err(FileSearchError::Stale) => {
                    return Err(RpcError::bars(
                        ErrorKind::StaleRevision,
                        "Документ изменился; начните поиск документа заново",
                    ));
                }
                Err(FileSearchError::Read(reason)) => {
                    out.skipped_total += 1;
                    if out.skipped.len() < skipped_limit {
                        out.skipped.push(TextSkipped {
                            path: file.path.clone(),
                            reason,
                        });
                    }
                    continue;
                }
                Ok(None) => continue,
                Ok(Some(doc)) => doc,
            };
            if out.documents.len() == limit_docs || out.hits_total >= limit_hits && p.path.is_none()
            {
                out.truncated = Some(
                    if out.documents.len() == limit_docs {
                        "docs"
                    } else {
                        "hits"
                    }
                    .into(),
                );
                break;
            }
            let mut doc = doc;
            let remaining = if p.path.is_some() {
                usize::MAX
            } else {
                limit_hits - out.hits_total
            };
            if doc.hits_total > remaining {
                doc.hits_total = remaining;
                doc.has_more = true;
                out.truncated = Some("hits".into());
            }
            doc.hits.truncate(remaining);
            out.hits_total += doc.hits_total;
            out.documents.push(doc);
            if out.truncated.is_some() {
                break;
            }
        }
        if out.truncated.is_some() {
            break;
        }
        if processed < files.len()
            && (out.documents.len() >= limit_docs || out.hits_total >= limit_hits)
        {
            out.truncated = Some(
                if out.documents.len() >= limit_docs {
                    "docs"
                } else {
                    "hits"
                }
                .into(),
            );
            break;
        }
    }
    if stop.load(Ordering::Relaxed) {
        return Err(cancelled());
    }
    if p.path.is_some() && out.documents.first().is_some_and(|d| d.has_more) {
        out.truncated = Some("page".into());
    }
    let q = p.query.to_lowercase();
    out.documents.sort_by_key(|d| {
        (
            !(d.title.to_lowercase().contains(&q) || d.path.to_lowercase().contains(&q)),
            d.path.clone(),
        )
    });
    out.docs_total = out.documents.len();
    out.skipped_truncated = out.skipped_total > out.skipped.len();
    out.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("docs/sub")).unwrap();
        dir
    }
    fn run(root: &Path, p: TextSearchParams) -> TextSearchOutput {
        search(root, Path::new("docs"), &p, &AtomicBool::new(false)).unwrap()
    }
    fn params(query: &str) -> TextSearchParams {
        TextSearchParams {
            query: query.into(),
            ..Default::default()
        }
    }
    #[test]
    fn text_search_unicode_ranges_bom_crlf_and_disk_freshness() {
        let dir = fixture();
        let file = dir.path().join("docs/sub/a.md");
        std::fs::write(
            &file,
            "\u{feff}---\r\ntitle: Проверка\r\n---\r\n😀 ПрИвЕт привет\r\n",
        )
        .unwrap();
        let out = run(dir.path(), params("привет"));
        let doc = &out.documents[0];
        assert_eq!(doc.title, "Проверка");
        assert_eq!(doc.hits_total, 2);
        assert_eq!(doc.hits[0].line, 4);
        assert_eq!(doc.hits[0].column, 4);
        assert_eq!(doc.hits[0].length, 6);
        assert_eq!(doc.hits[0].ranges, vec![[3, 9]]);
        let mut p = params("привет");
        p.case_sensitive = true;
        assert_eq!(run(dir.path(), p).hits_total, 1);
        std::fs::write(file, "новое слово").unwrap();
        assert_eq!(run(dir.path(), params("новое")).hits_total, 1);
        assert_eq!(run(dir.path(), params("привет")).hits_total, 0);
    }
    #[test]
    fn text_search_scope_filter_and_exclusions() {
        let dir = fixture();
        for path in [
            "docs/a.md",
            "docs/sub/a.MD",
            "docs/BARS_INSTRUCTION.md",
            "docs/.hidden/b.md",
        ] {
            let file = dir.path().join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "needle").unwrap();
        }
        std::fs::write(dir.path().join("docs/bad.md"), [0xff]).unwrap();
        let mut p = params("needle");
        p.path_filter = "SUB/*.md".into();
        assert_eq!(run(dir.path(), p).documents[0].path, "docs/sub/a.MD");
        let mut p = params("needle");
        p.files = vec!["a.md".into()];
        assert_eq!(run(dir.path(), p).docs_total, 1);
        let mut p = params("needle");
        p.modules = vec!["sub".into()];
        assert_eq!(run(dir.path(), p).documents[0].module, "sub");
        let all = run(dir.path(), params("needle"));
        assert_eq!(all.docs_total, 2);
        assert_eq!(all.skipped.len(), 1);
        let mut p = params("needle");
        p.path = Some("../foreign.md".into());
        assert_eq!(
            search(dir.path(), Path::new("docs"), &p, &AtomicBool::new(false))
                .unwrap_err()
                .kind(),
            Some(ErrorKind::FileNotFound)
        );
    }
    #[test]
    fn text_search_regex_errors_empty_matches_and_line_anchors() {
        let dir = fixture();
        std::fs::write(dir.path().join("docs/a.md"), "a1\r\na2\n").unwrap();
        let mut p = params("^a[0-9]$");
        p.regex = true;
        assert_eq!(run(dir.path(), p).hits_total, 2);
        let mut p = params("^");
        p.regex = true;
        assert_eq!(run(dir.path(), p).hits_total, 3);
        let mut p = params("[");
        p.regex = true;
        let error = search(dir.path(), Path::new("docs"), &p, &AtomicBool::new(false)).unwrap_err();
        assert_eq!(error.data.unwrap()["kind"], "invalid_regex");
    }
    #[test]
    fn text_search_limits_pages_and_changed_revision() {
        let dir = fixture();
        let file = dir.path().join("docs/a.md");
        std::fs::write(&file, "x ".repeat(220)).unwrap();
        let initial = run(dir.path(), params("x"));
        assert_eq!(initial.documents[0].hits.len(), 5);
        let mut p = params("x");
        p.limit_hits = Some(7);
        let bounded = run(dir.path(), p);
        assert_eq!(bounded.hits_total, 7);
        assert_eq!(bounded.truncated.as_deref(), Some("hits"));
        let mut p = params("x");
        p.path = Some("docs/a.md".into());
        p.offset = 5;
        p.revision = Some(initial.documents[0].revision.clone());
        let page = run(dir.path(), p.clone());
        assert_eq!(page.documents[0].hits.len(), 100);
        assert_eq!(page.documents[0].hits[0].column, 11);
        std::fs::write(&file, "x").unwrap();
        let changed = run(dir.path(), p);
        assert_eq!(changed.documents[0].hits.len(), 1);
        assert!(!changed.documents[0].has_more);
        std::fs::write(dir.path().join("docs/b.md"), "x").unwrap();
        let mut p = params("x");
        p.limit_docs = Some(1);
        assert_eq!(run(dir.path(), p).truncated.as_deref(), Some("docs"));
    }
    #[tokio::test]
    async fn text_search_worker_dispatch_without_index_and_model() {
        use crate::{
            handler::WorkerHandler,
            worker::{Worker, WorkerConfig, models::fake::FakeProvider},
        };
        use bars_protocol::Call;
        let home = tempfile::tempdir().unwrap();
        let dir = fixture();
        let worker = Worker::start(
            WorkerConfig::new(home.path().to_path_buf()),
            FakeProvider::new("text-test"),
        )
        .unwrap();
        worker.pause();
        worker.register(dir.path()).unwrap();
        std::fs::write(dir.path().join("docs/new.md"), "свежее слово").unwrap();
        let handler = WorkerHandler::new(worker.clone());
        let p = TextSearchParams {
            project: dir.path().display().to_string(),
            window_id: "test".into(),
            ..params("свежее")
        };
        let output = handler.dispatch(Call::TextSearch(p.clone())).await.unwrap();
        assert_eq!(output["hits_total"], 1);
        std::fs::write(dir.path().join("docs/new.md"), "внешняя правка").unwrap();
        let output = handler
            .dispatch(Call::TextSearch(TextSearchParams {
                query: "внешняя".into(),
                ..p
            }))
            .await
            .unwrap();
        assert_eq!(output["hits_total"], 1);
        let error = handler
            .dispatch(Call::TextSearch(TextSearchParams {
                project: "relative".into(),
                ..params("x")
            }))
            .await
            .unwrap_err();
        assert_eq!(error.kind(), Some(ErrorKind::InvalidParams));
        worker.shutdown().await;
    }
    #[test]
    fn text_search_snippet_keeps_match_after_long_unicode_prefix_and_cancels() {
        let dir = fixture();
        std::fs::write(
            dir.path().join("docs/a.md"),
            format!("{}TARGET{}", "😀".repeat(1000), "я".repeat(1000)),
        )
        .unwrap();
        let out = run(dir.path(), params("TARGET"));
        let h = &out.documents[0].hits[0];
        assert!(h.text.contains("TARGET"));
        assert!(h.text.chars().count() <= 242);
        assert_eq!(h.column, 2001);
        let err = search(
            dir.path(),
            Path::new("docs"),
            &params("x"),
            &AtomicBool::new(true),
        )
        .unwrap_err();
        assert_eq!(err.kind(), Some(ErrorKind::Cancelled));
    }
}
