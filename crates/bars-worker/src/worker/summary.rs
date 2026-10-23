//! Сводка состояния для трея: цвет иконки, подсказка и строки меню (`docs/features/window/indexing-progress.md`,
//! «Трей»). Окно рисует трей, а что в нём писать, решает воркер: так логика проверяется тестами без Tauri.

use bars_protocol::ProjectState;

use super::{Worker, lock};

/// Общее состояние для иконки трея.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Всё актуально, воркер ждёт.
    Idle,
    /// Идёт индексация.
    Busy,
    /// Нужно внимание пользователя: переиндексация, ошибка файла или провайдера.
    Attention,
}

/// Строка меню трея с проектом, который она открывает.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub text: String,
    /// Путь проекта, который открывает пункт.
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub health: Health,
    pub tooltip: String,
    /// «Индексация: проект N %» или «Ждут N документов».
    pub indexing: Option<Line>,
    /// «Нужна переиндексация» или «Ошибка индексации» с проектом.
    pub attention: Option<Line>,
    pub projects: usize,
}

/// Процент выполненного; `None`, пока число документов неизвестно.
fn percent(done: usize, total: usize) -> Option<usize> {
    (done * 100).checked_div(total).map(|value| value.min(100))
}

impl Worker {
    /// Сводка для трея. Дешёвая: не открывает индексы.
    pub fn summary(&self) -> Summary {
        let projects = self.projects();
        let progress = self.progress();
        let queued = self.queued();
        let problems = self.problems();
        let name_of = |path: &std::path::Path| {
            projects
                .iter()
                .find(|project| project.path == path)
                .map(|project| project.name.clone())
                .unwrap_or_default()
        };

        let waiting: usize = queued.iter().map(|job| job.files.max(1)).sum();
        let documents: Vec<_> = {
            let docs = lock(&self.inner.docs);
            projects
                .iter()
                .map(|project| (project, docs.counts(&project.path)))
                .collect()
        };
        let paused: usize = documents.iter().map(|(_, counts)| counts.waiting).sum();
        let indexing = if let Some(progress) = &progress {
            let percent = percent(progress.docs_done, progress.docs_total);
            let text = match percent {
                Some(percent) => {
                    format!("Индексация: {} · {percent} %", name_of(&progress.project))
                }
                None => format!("Индексация: {}", name_of(&progress.project)),
            };
            Some(Line {
                text,
                project: progress.project.display().to_string(),
            })
        } else {
            queued
                .first()
                .map(|job| Line {
                    text: format!("В очереди: {}", name_of(&job.project)),
                    project: job.project.display().to_string(),
                })
                .or_else(|| {
                    documents
                        .iter()
                        .find(|(_, counts)| counts.waiting + counts.queued > 0)
                        .map(|(project, counts)| Line {
                            text: format!(
                                "{} · ждут паузы: {}, в очереди: {}",
                                project.name, counts.waiting, counts.queued
                            ),
                            project: project.path.display().to_string(),
                        })
                })
        };

        let needs_reindex = projects
            .iter()
            .find(|project| project.state == ProjectState::ReindexRequired);
        let attention = if let Some(project) = needs_reindex {
            Some(Line {
                text: format!("Нужна переиндексация: {}", project.name),
                project: project.path.display().to_string(),
            })
        } else {
            problems
                .first()
                .map(|issues| Line {
                    text: format!("Ошибка индексации: {}", name_of(&issues.project)),
                    project: issues.project.display().to_string(),
                })
                .or_else(|| {
                    documents.iter().find(|(_, counts)| counts.error > 0).map(
                        |(project, counts)| Line {
                            text: format!(
                                "{} · документов с ошибкой: {}",
                                project.name, counts.error
                            ),
                            project: project.path.display().to_string(),
                        },
                    )
                })
        };

        let health = if attention.is_some() {
            Health::Attention
        } else if indexing.is_some() {
            Health::Busy
        } else {
            Health::Idle
        };
        let mut tooltip = match (health, &progress, &attention) {
            (Health::Attention, _, Some(line)) => format!("bars · нужно внимание: {}", line.text),
            (Health::Busy, Some(progress), _) => {
                let percent = percent(progress.docs_done, progress.docs_total)
                    .map(|percent| format!(" {percent} %"))
                    .unwrap_or_default();
                format!(
                    "bars · {}{percent}, ждут {waiting}",
                    name_of(&progress.project)
                )
            }
            (Health::Busy, None, _) if waiting > 0 => format!("bars · в очереди {waiting}"),
            (Health::Busy, None, _) if paused == 0 => indexing.as_ref().map_or_else(
                || "bars".to_string(),
                |line| format!("bars · {}", line.text),
            ),
            _ => "bars".to_string(),
        };
        if paused > 0 {
            tooltip.push_str(&format!(" · ждут паузы: {paused}"));
        }
        Summary {
            health,
            tooltip,
            indexing,
            attention,
            projects: projects.len(),
        }
    }
}
