//! Слежение за файлами проекта (`docs/architecture/worker.md`, `docs/indexing/updates.md`, Т-10).
//!
//! `notify` смотрит корень документации рекурсивно, события копятся в `notify-debouncer-full`
//! (500 мс) и уходят одной пачкой в `sink`. Фильтр отбрасывает всё, чего индексация не видит:
//! не `.md`, папки с точки, `BARS_INSTRUCTION.md`.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

/// Сколько события копятся до пачки.
pub const DEBOUNCE: Duration = Duration::from_millis(500);

/// Файл, который bars кладёт в корень документации сам; его правки не поводом для проверки.
const INSTRUCTION_FILE: &str = "BARS_INSTRUCTION.md";

/// Что слежение сообщает воркеру.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// Изменились эти файлы (или папки: удаление и переименование папки приходит без списка файлов).
    Files(Vec<PathBuf>),
    /// `notify` сообщил об ошибке: события могли потеряться, проект надо проверить целиком.
    Failed(String),
}

pub type Sink = Arc<dyn Fn(WatchEvent) + Send + Sync>;

/// Живое слежение за одним проектом; снимается, когда значение уничтожено.
pub struct ProjectWatcher {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl ProjectWatcher {
    pub fn start(
        project_root: &Path,
        docs_root: &str,
        debounce: Duration,
        sink: Sink,
    ) -> Result<Self, notify::Error> {
        let docs_dir = project_root.join(docs_root);
        let filter_dir = docs_dir.clone();
        let mut debouncer =
            new_debouncer(
                debounce,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(events) => {
                        let mut paths: Vec<PathBuf> = events
                            .into_iter()
                            .filter(|event| !matches!(event.event.kind, EventKind::Access(_)))
                            .flat_map(|event| event.event.paths.clone())
                            .filter(|path| is_relevant(&filter_dir, path))
                            .collect();
                        paths.sort();
                        paths.dedup();
                        if !paths.is_empty() {
                            sink(WatchEvent::Files(paths));
                        }
                    }
                    Err(errors) => {
                        let message = errors
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; ");
                        sink(WatchEvent::Failed(message));
                    }
                },
            )?;
        debouncer.watch(&docs_dir, RecursiveMode::Recursive)?;
        Ok(Self {
            _debouncer: debouncer,
        })
    }
}

/// Может ли изменение по `path` повлиять на индекс.
///
/// Путь без расширения может оказаться папкой: её удаление или переименование меняет набор
/// документов, а `notify` не называет файлы внутри.
pub fn is_relevant(docs_dir: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(docs_dir) else {
        return false;
    };
    let parts: Vec<_> = relative.components().collect();
    let Some((last, folders)) = parts.split_last() else {
        // Сам корень документации.
        return true;
    };
    if folders
        .iter()
        .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
    {
        return false;
    }
    if folders.is_empty() && last.as_os_str() == INSTRUCTION_FILE {
        return false;
    }
    match Path::new(last.as_os_str()).extension() {
        Some(extension) => extension.eq_ignore_ascii_case("md"),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::mpsc;
    use tempfile::TempDir;

    fn docs() -> PathBuf {
        PathBuf::from("proj").join("docs")
    }

    #[test]
    fn markdown_files_are_relevant() {
        let docs = docs();
        assert!(is_relevant(&docs, &docs.join("a.md")));
        assert!(is_relevant(
            &docs,
            &docs.join("mod").join("deep").join("B.MD")
        ));
        assert!(is_relevant(&docs, &docs.join(".hidden-file.md")));
    }

    #[test]
    fn everything_else_is_not() {
        let docs = docs();
        assert!(!is_relevant(&docs, &docs.join("notes.txt")));
        assert!(!is_relevant(&docs, &docs.join("img").join("pic.png")));
        assert!(!is_relevant(&docs, &docs.join(".git").join("x.md")));
        assert!(!is_relevant(
            &docs,
            &docs.join("a").join(".cache").join("x.md")
        ));
        assert!(!is_relevant(&docs, &docs.join(INSTRUCTION_FILE)));
        assert!(!is_relevant(
            &docs,
            &PathBuf::from("proj").join("src").join("a.md")
        ));
    }

    #[test]
    fn folders_and_the_root_are_relevant() {
        let docs = docs();
        assert!(is_relevant(&docs, &docs));
        assert!(is_relevant(&docs, &docs.join("module")));
        // BARS_INSTRUCTION.md в подпапке — обычный документ.
        assert!(is_relevant(&docs, &docs.join("sub").join(INSTRUCTION_FILE)));
    }

    fn collect(rx: &mpsc::Receiver<WatchEvent>, wanted: &Path) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while let Some(left) = deadline.checked_duration_since(std::time::Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(WatchEvent::Files(paths)) if paths.iter().any(|path| path.ends_with(wanted)) => {
                    return true;
                }
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        false
    }

    #[test]
    fn real_changes_reach_the_sink_in_one_batch() {
        let project = TempDir::new().unwrap();
        fs::create_dir_all(project.path().join("docs")).unwrap();
        let (tx, rx) = mpsc::channel();
        let sink: Sink = Arc::new(move |event| {
            let _ = tx.send(event);
        });
        let _watcher =
            ProjectWatcher::start(project.path(), "docs", Duration::from_millis(100), sink)
                .unwrap();

        fs::write(project.path().join("docs").join("ignored.txt"), "x").unwrap();
        fs::write(project.path().join("docs").join("new.md"), "# Новый").unwrap();
        assert!(
            collect(&rx, Path::new("new.md")),
            "правка .md дошла до sink"
        );

        // Файл не из документации событий не даёт.
        while rx.try_recv().is_ok() {}
        fs::write(project.path().join("outside.md"), "x").unwrap();
        assert!(!collect_short(&rx));
    }

    fn collect_short(rx: &mpsc::Receiver<WatchEvent>) -> bool {
        matches!(
            rx.recv_timeout(Duration::from_millis(600)),
            Ok(WatchEvent::Files(_))
        )
    }

    #[test]
    fn missing_docs_root_cannot_be_watched() {
        let project = TempDir::new().unwrap();
        let sink: Sink = Arc::new(|_| {});
        assert!(
            ProjectWatcher::start(project.path(), "docs", Duration::from_millis(100), sink)
                .is_err()
        );
    }
}
