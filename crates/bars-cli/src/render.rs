//! Человекочитаемый вывод команд. Строки структуры и текста документа собирают функции
//! `bars-protocol`, общие с `bars-mcp` (Т-20).

use std::fmt::Write as _;

use bars_protocol::{
    AcceptedOutput, AutostartOutput, ClientError, DuplicateKind, DuplicatesOutput, ErrorKind,
    Heading, InstallOutput, OutlineOutput, ProjectInfo, ProjectState, ProjectsOutput, ReadOutput,
    RegisterOutput, ScoreKind, SearchHit, SearchOutput, SimilarOutput, StatusOutput,
    UninstallOutput, UnregisterOutput,
};
use serde_json::json;

fn score_kind(kind: ScoreKind) -> &'static str {
    match kind {
        ScoreKind::Rerank => "rerank",
        ScoreKind::Cosine => "cosine",
    }
}

/// Один результат: общий вид у поиска и у похожих чанков.
fn hit_lines(text: &mut String, number: usize, hit: &SearchHit) {
    let _ = writeln!(
        text,
        "{}. {}:{}-{}  [{}]  {} {:.3}",
        number,
        hit.path,
        hit.first_line,
        hit.last_line,
        hit.module,
        score_kind(hit.score_kind),
        hit.score
    );
    if !hit.chunk_id.is_empty() {
        let _ = writeln!(text, "   id: {}", hit.chunk_id);
    }
    let _ = writeln!(text, "   {} — {}", hit.title, hit.heading_chain);
    if !hit.sections.is_empty() {
        let _ = writeln!(text, "   и разделы: {}", hit.sections.join("; "));
    }
    let _ = writeln!(
        text,
        "{}
",
        hit.text.trim_end()
    );
}

pub fn similar(output: &SimilarOutput) -> String {
    let source = &output.source;
    let mut text = format!(
        "Похожие на {} ({}:{}-{}, {}):

",
        source.chunk_id, source.path, source.first_line, source.last_line, source.heading_chain
    );
    if output.results.is_empty() {
        text.push_str(
            "Похожих чанков нет.
",
        );
    }
    for (index, hit) in output.results.iter().enumerate() {
        hit_lines(&mut text, index + 1, hit);
    }
    let _ = write!(
        text,
        "Применено: предел результатов {}, выдано {}{}. Оценка — косинусная близость к исходному чанку; близость не доказывает дублирование.",
        output.limit,
        output.results.len(),
        if output.clamped {
            "; значение прижато к пределу"
        } else {
            ""
        }
    );
    if output.indexing {
        let _ = write!(
            text,
            "
Идёт индексация: выдача может быть неполной, в очереди документов: {}.",
            output.pending
        );
    }
    text
}

fn duplicate_kind(kind: DuplicateKind) -> &'static str {
    match kind {
        DuplicateKind::Exact => "точный повтор",
        DuplicateKind::Near => "близкая копия",
        DuplicateKind::Contained => "раздел внутри другого",
        DuplicateKind::Partial => "частичная копия",
        DuplicateKind::Semantic => "смысловое сходство (повод проверить)",
    }
}

pub fn duplicates(output: &DuplicatesOutput) -> String {
    let mut text = String::new();
    if output.results.is_empty() {
        text.push_str("Повторов не найдено.\n");
    }
    for (index, item) in output.results.iter().enumerate() {
        let _ = writeln!(
            text,
            "{}. {} ({:.2}){}",
            index + 1,
            duplicate_kind(item.kind),
            item.score,
            if item.repeated_block {
                format!(
                    ": один текст в {} разделах, скорее шаблон",
                    item.chunks.len()
                )
            } else {
                String::new()
            }
        );
        for chunk in &item.chunks {
            let _ = writeln!(
                text,
                "   {}:{}-{} [{}] {} (id {})",
                chunk.path,
                chunk.first_line,
                chunk.last_line,
                chunk.module,
                chunk.heading_chain,
                chunk.chunk_id
            );
        }
        for fragment in &item.shared {
            let _ = writeln!(text, "   общее: {fragment}");
        }
        if let Some(differences) = &item.differences {
            for change in &differences.changes {
                let _ = writeln!(
                    text,
                    "   заменено: «{}» → «{}»",
                    change.first, change.second
                );
            }
            if differences.changes_truncated {
                let _ = writeln!(
                    text,
                    "   различий больше, чем показано, или текст слишком длинный"
                );
            }
            if !differences.numbers_only_first.is_empty() {
                let _ = writeln!(
                    text,
                    "   числа только в первом: {}",
                    differences.numbers_only_first.join(", ")
                );
            }
            if !differences.numbers_only_second.is_empty() {
                let _ = writeln!(
                    text,
                    "   числа только во втором: {}",
                    differences.numbers_only_second.join(", ")
                );
            }
            if differences.negations_first != differences.negations_second {
                let _ = writeln!(
                    text,
                    "   слов отрицания: в первом {}, во втором {}",
                    differences.negations_first, differences.negations_second
                );
            }
        }
        text.push('\n');
    }
    let _ = write!(
        text,
        "Найдено: точных {}, близких {}, внутри другого {}, частичных {}, смысловых {}; показано {} из {}{}. Проверено разделов: {}, коротких пропущено: {}. Смысловое сходство и копия с изменённым числом или отрицанием не доказывают противоречие: прочитай оба раздела.",
        output.counts.exact,
        output.counts.near,
        output.counts.contained,
        output.counts.partial,
        output.counts.semantic,
        output.results.len(),
        output.total,
        if output.clamped {
            "; значение прижато к пределу"
        } else {
            ""
        },
        output.chunks_checked,
        output.chunks_skipped_short
    );
    if output.indexing {
        let _ = write!(
            text,
            "\nИдёт индексация: выдача может быть неполной, в очереди документов: {}.",
            output.pending
        );
    }
    text
}

pub fn search(output: &SearchOutput) -> String {
    let mut text = String::new();
    if output.results.is_empty() {
        text.push_str("Ничего не найдено.\n");
    }
    for (index, hit) in output.results.iter().enumerate() {
        hit_lines(&mut text, index + 1, hit);
    }
    let applied = &output.applied;
    let mut filters = Vec::new();
    if !applied.scope.modules.is_empty() {
        filters.push(format!("модули {}", applied.scope.modules.join(", ")));
    }
    if !applied.scope.files.is_empty() {
        filters.push(format!("файлы {}", applied.scope.files.join(", ")));
    }
    let whole = filters.is_empty();
    let _ = writeln!(
        text,
        "Область: {}.",
        if whole {
            "весь проект".into()
        } else {
            filters.join("; ")
        }
    );
    let counts = |map: &std::collections::BTreeMap<String, u32>| {
        if map.is_empty() {
            "нет".into()
        } else {
            map.iter()
                .map(|(name, n)| format!("{name} {n}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    let _ = writeln!(
        text,
        "По модулям: кандидаты {}; результаты {}.",
        counts(&output.by_module.candidates),
        counts(&output.by_module.results)
    );
    if whole && output.by_module.results.len() > 1 {
        let _ = writeln!(
            text,
            "Результаты из нескольких модулей; область можно сузить через modules."
        );
    }
    if output.results.is_empty() && !applied.scope.modules.is_empty() {
        let _ = writeln!(
            text,
            "В выбранных модулях ничего не найдено; попробуйте весь проект. Ноль не доказывает отсутствия."
        );
    }
    let _ = write!(
        text,
        "Применено: предел результатов {}, выдано {}, кандидатов {}, порог {} ({}){}",
        applied.results,
        output.results.len(),
        applied.candidates,
        applied.min_score,
        score_kind(applied.score_kind),
        if applied.clamped {
            "; часть значений прижата к пределам"
        } else {
            ""
        }
    );
    if output.indexing {
        let _ = write!(
            text,
            "\nИдёт индексация: выдача может быть неполной, в очереди документов: {}.",
            output.pending
        );
    }
    if let Some(error) = &output.rerank_error {
        let _ = write!(text, "\nRerank не отработал, выдача без него: {error}");
    }
    text
}

pub fn outline(output: &OutlineOutput) -> String {
    output
        .headings
        .iter()
        .map(Heading::to_line)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn read(output: &ReadOutput) -> String {
    format!("{}\n\n{}", output.title, bars_protocol::read_text(output))
}

fn state_name(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Ready => "готов",
        ProjectState::Indexing => "идёт индексация",
        ProjectState::ReindexRequired => "нужна переиндексация",
        ProjectState::Unavailable => "папка недоступна",
    }
}

fn project(info: &ProjectInfo) -> String {
    let mut text = format!(
        "{} ({})\n  состояние: {}\n  корень документации: {}\n  модули:",
        info.path,
        info.name,
        state_name(info.state),
        info.docs_root
    );
    if info.modules.is_empty() {
        text.push_str(" нет");
    }
    for module in &info.modules {
        let _ = write!(text, " {} ({})", module.name, module.documents);
    }
    text
}

pub fn projects(output: &ProjectsOutput) -> String {
    if output.projects.is_empty() {
        return "Проектов нет. Зарегистрируйте папку: bars-cli register <путь>".to_string();
    }
    output
        .projects
        .iter()
        .map(project)
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn status(output: &StatusOutput) -> String {
    let mut text = format!(
        "Проект: {}\nСостояние: {}\nОбновлён: {}\nВ очереди документов: {}\n",
        output.path,
        state_name(output.state),
        output
            .updated_at
            .as_deref()
            .unwrap_or("ещё не завершена ни одна проверка"),
        output.pending
    );
    let _ = writeln!(
        text,
        "Модель индекса: {}",
        output.model.index.as_deref().unwrap_or("индекс пуст")
    );
    let _ = writeln!(text, "Модель настроек: {}", output.model.settings);
    if output.state == ProjectState::ReindexRequired {
        text.push_str(
            "Индекс построен другой моделью. Переиндексация: bars-cli reindex --accept-model-change\n",
        );
    }
    let _ = writeln!(
        text,
        "Слежение за файлами: {}",
        if output.watching {
            "работает"
        } else {
            "не работает"
        }
    );
    let _ = write!(
        text,
        "Последняя ошибка: {}",
        output.last_error.as_deref().unwrap_or("нет")
    );
    if let Some(graph) = &output.graph {
        let _ = write!(
            text,
            "\nГраф: {}; источников: {}; задание: {}",
            graph.state.label(),
            graph.source_count,
            graph.active_job.as_deref().unwrap_or("нет")
        );
    }
    text
}

pub fn reindex(output: &AcceptedOutput) -> String {
    if output.accepted {
        "Переиндексация принята. Ход — в bars-cli status.".to_string()
    } else {
        "Переиндексация не принята.".to_string()
    }
}

pub fn register(output: &RegisterOutput) -> String {
    format!("Проект зарегистрирован.\n{}", project(&output.project))
}

pub fn unregister(output: &UnregisterOutput) -> String {
    if output.removed {
        "Проект снят с регистрации. Файлы проекта не тронуты.".to_string()
    } else {
        "Проект не был зарегистрирован.".to_string()
    }
}

pub fn install(output: &InstallOutput) -> String {
    let mut text = if output.changed_files.is_empty() {
        "bars уже установлен: файлы клиента не изменились.".to_string()
    } else {
        format!(
            "bars установлен. Изменено:
{}
Копии чужих конфигов: <файл>.bars-backup.",
            lines(&output.changed_files)
        )
    };
    if output.needs_hook_approval {
        text.push_str(
            "
Выполните в Codex команду /hooks и одобрите хук bars: без этого он не запустится (MCP уже работает).",
        );
    }
    text.push_str(
        "
Перезапустите клиент, чтобы он подхватил MCP-сервер.",
    );
    text
}

pub fn uninstall(output: &UninstallOutput) -> String {
    if output.changed_files.is_empty() {
        "bars в клиенте не найден: ничего не изменено.".to_string()
    } else {
        format!(
            "bars убран. Изменено:
{}",
            lines(&output.changed_files)
        )
    }
}

pub fn autostart(output: &AutostartOutput) -> String {
    if !output.supported {
        return "Автозапуск на этой ОС не поддерживается.".to_string();
    }
    let mut text = if output.enabled {
        "Автозапуск включён: bars стартует при входе в систему.".to_string()
    } else {
        "Автозапуск выключен.".to_string()
    };
    if output.stale {
        text.push_str(
            "
Запись указывает не на текущий bars: воркер исправит её при следующем запуске.",
        );
    }
    text
}

fn lines(files: &[String]) -> String {
    files
        .iter()
        .map(|file| format!("  {file}"))
        .collect::<Vec<_>>()
        .join(
            "
",
        )
}

/// Текст ошибки для stderr: человеку — строка, с `--json` — объект с кодом и видом ошибки.
pub fn error(error: &ClientError, json: bool) -> String {
    if json {
        let (code, kind) = match error {
            ClientError::Rpc(rpc) => (Some(rpc.code), rpc.kind().map(ErrorKind::name)),
            _ => (None, None),
        };
        return json!({ "error": { "code": code, "kind": kind, "message": error.to_string() } })
            .to_string();
    }
    let mut text = format!("bars-cli: {error}");
    if let ClientError::Rpc(rpc) = error
        && matches!(
            rpc.kind(),
            Some(ErrorKind::ReindexRequired | ErrorKind::ReindexNeedsConsent)
        )
    {
        text.push_str(
            "\nИндекс построен другой моделью. Если вы согласны на переиндексацию: bars-cli reindex --accept-model-change",
        );
    }
    text
}
