//! Текстовый вид ответов инструментов (`format=txt`, по умолчанию): блоки `=== … ===` с полями
//! `ключ: значение`, чтобы агент однозначно находил границы результатов и текста документов.

use std::fmt::Write as _;

use bars_protocol::{
    AcceptedOutput, Heading, OutlineOutput, ProjectInfo, ProjectState, ProjectsOutput, ReadOutput,
    ScoreKind, SearchOutput, StatusOutput, number_lines,
};

fn score_kind(kind: ScoreKind) -> &'static str {
    match kind {
        ScoreKind::Rerank => "rerank",
        ScoreKind::Cosine => "cosine",
    }
}

fn state_name(state: ProjectState) -> &'static str {
    match state {
        ProjectState::Ready => "ready",
        ProjectState::Indexing => "indexing",
        ProjectState::ReindexRequired => "reindex_required",
        ProjectState::Unavailable => "unavailable",
    }
}

pub fn search(output: &SearchOutput) -> String {
    let total = output.results.len();
    let mut text = format!("=== ПОИСК: результатов {total} ===\n");
    for (index, hit) in output.results.iter().enumerate() {
        let n = index + 1;
        let _ = writeln!(text, "=== РЕЗУЛЬТАТ {n} из {total} ===");
        let _ = writeln!(text, "path: {}", hit.path);
        let _ = writeln!(text, "lines: {}-{}", hit.first_line, hit.last_line);
        let _ = writeln!(text, "module: {}", hit.module);
        let _ = writeln!(text, "title: {}", hit.title);
        let _ = writeln!(text, "heading: {}", hit.heading_chain);
        let _ = writeln!(
            text,
            "score: {:.3} ({})",
            hit.score,
            score_kind(hit.score_kind)
        );
        let _ = writeln!(text, "--- текст ---");
        let _ = writeln!(text, "{}", hit.text.trim_end());
        let _ = writeln!(text, "=== КОНЕЦ РЕЗУЛЬТАТА {n} ===");
    }
    let applied = &output.applied;
    let _ = writeln!(text, "=== ПРИМЕНЁННЫЕ ПАРАМЕТРЫ ===");
    let _ = writeln!(text, "results: {}", applied.results);
    let _ = writeln!(text, "candidates: {}", applied.candidates);
    let _ = writeln!(
        text,
        "min_score: {} ({})",
        applied.min_score,
        score_kind(applied.score_kind)
    );
    let _ = writeln!(text, "clamped: {}", applied.clamped);
    let _ = writeln!(text, "indexing: {}", output.indexing);
    let _ = writeln!(text, "pending: {}", output.pending);
    if let Some(error) = &output.rerank_error {
        let _ = writeln!(text, "rerank_error: {error}");
    }
    if output.indexing {
        let _ = writeln!(text, "Идёт индексация: выдача может быть неполной.");
    }
    text.push_str("=== КОНЕЦ ПОИСКА ===");
    text
}

pub fn outline(path: &str, output: &OutlineOutput) -> String {
    let lines = output
        .headings
        .iter()
        .map(Heading::to_line)
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "=== СТРУКТУРА {path}: заголовков {} ===\n{lines}\n=== КОНЕЦ СТРУКТУРЫ ===",
        output.headings.len()
    )
}

pub fn read(path: &str, output: &ReadOutput) -> String {
    format!(
        "=== ДОКУМЕНТ {path} ===\ntitle: {}\n--- текст (с номерами строк) ---\n{}\n=== КОНЕЦ ДОКУМЕНТА ===",
        output.title,
        number_lines(&output.text)
    )
}

fn project_block(text: &mut String, index: usize, total: usize, info: &ProjectInfo) {
    let _ = writeln!(text, "=== ПРОЕКТ {index} из {total} ===");
    let _ = writeln!(text, "path: {}", info.path);
    let _ = writeln!(text, "name: {}", info.name);
    let _ = writeln!(text, "docs_root: {}", info.docs_root);
    let _ = writeln!(text, "state: {}", state_name(info.state));
    let modules = info
        .modules
        .iter()
        .map(|module| format!("{} ({})", module.name, module.documents))
        .collect::<Vec<_>>()
        .join(", ");
    let _ = writeln!(text, "modules: {modules}");
    let _ = writeln!(text, "=== КОНЕЦ ПРОЕКТА {index} ===");
}

pub fn projects(output: &ProjectsOutput) -> String {
    let total = output.projects.len();
    let mut text = format!("=== ПРОЕКТЫ: {total} ===\n");
    for (index, info) in output.projects.iter().enumerate() {
        project_block(&mut text, index + 1, total, info);
    }
    if total == 0 {
        text.push_str("Проектов нет: зарегистрируйте папку проекта (bars-cli register).\n");
    }
    text.push_str("=== КОНЕЦ ПРОЕКТОВ ===");
    text
}

pub fn status(output: &StatusOutput) -> String {
    let mut text = String::from("=== СОСТОЯНИЕ ИНДЕКСА ===\n");
    let _ = writeln!(text, "path: {}", output.path);
    let _ = writeln!(text, "state: {}", state_name(output.state));
    let _ = writeln!(
        text,
        "updated_at: {}",
        output.updated_at.as_deref().unwrap_or("нет")
    );
    let _ = writeln!(text, "pending: {}", output.pending);
    let _ = writeln!(
        text,
        "index_model: {}",
        output.model.index.as_deref().unwrap_or("индекс пуст")
    );
    let _ = writeln!(text, "settings_model: {}", output.model.settings);
    let _ = writeln!(text, "watching: {}", output.watching);
    let _ = writeln!(
        text,
        "last_error: {}",
        output.last_error.as_deref().unwrap_or("нет")
    );
    if output.state == ProjectState::ReindexRequired {
        text.push_str(
            "Индекс построен другой моделью. Переиндексацию запускает пользователь, не агент.\n",
        );
    }
    text.push_str("=== КОНЕЦ СОСТОЯНИЯ ===");
    text
}

pub fn reindex(output: &AcceptedOutput) -> String {
    let note = if output.accepted {
        "Переиндексация принята. Ход — в bars_status."
    } else {
        "Переиндексация не принята."
    };
    format!(
        "=== ПЕРЕИНДЕКСАЦИЯ ===\naccepted: {}\n{note}\n=== КОНЕЦ ===",
        output.accepted
    )
}
