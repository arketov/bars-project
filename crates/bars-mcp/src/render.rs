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
    let mut text = format!("=== SEARCH: results {total} ===\n");
    for (index, hit) in output.results.iter().enumerate() {
        let n = index + 1;
        let _ = writeln!(text, "=== RESULT {n} of {total} ===");
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
        let _ = writeln!(text, "--- text ---");
        let _ = writeln!(text, "{}", hit.text.trim_end());
        let _ = writeln!(text, "=== END RESULT {n} ===");
    }
    let applied = &output.applied;
    let _ = writeln!(text, "=== APPLIED PARAMETERS ===");
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
        let _ = writeln!(text, "Indexing in progress: results may be incomplete.");
    }
    text.push_str("=== END SEARCH ===");
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
        "=== OUTLINE {path}: headings {} ===\n{lines}\n=== END OUTLINE ===",
        output.headings.len()
    )
}

pub fn read(path: &str, output: &ReadOutput) -> String {
    format!(
        "=== DOCUMENT {path} ===\ntitle: {}\n--- text (with line numbers) ---\n{}\n=== END DOCUMENT ===",
        output.title,
        number_lines(&output.text)
    )
}

fn project_block(text: &mut String, index: usize, total: usize, info: &ProjectInfo) {
    let _ = writeln!(text, "=== PROJECT {index} of {total} ===");
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
    let _ = writeln!(text, "=== END PROJECT {index} ===");
}

pub fn projects(output: &ProjectsOutput) -> String {
    let total = output.projects.len();
    let mut text = format!("=== PROJECTS: {total} ===\n");
    for (index, info) in output.projects.iter().enumerate() {
        project_block(&mut text, index + 1, total, info);
    }
    if total == 0 {
        text.push_str("No projects: register a project folder (bars-cli register).\n");
    }
    text.push_str("=== END PROJECTS ===");
    text
}

pub fn status(output: &StatusOutput) -> String {
    let mut text = String::from("=== INDEX STATUS ===\n");
    let _ = writeln!(text, "path: {}", output.path);
    let _ = writeln!(text, "state: {}", state_name(output.state));
    let _ = writeln!(
        text,
        "updated_at: {}",
        output.updated_at.as_deref().unwrap_or("none")
    );
    let _ = writeln!(text, "pending: {}", output.pending);
    let _ = writeln!(
        text,
        "index_model: {}",
        output.model.index.as_deref().unwrap_or("empty index")
    );
    let _ = writeln!(text, "settings_model: {}", output.model.settings);
    let _ = writeln!(text, "watching: {}", output.watching);
    let _ = writeln!(
        text,
        "last_error: {}",
        output.last_error.as_deref().unwrap_or("none")
    );
    if output.state == ProjectState::ReindexRequired {
        text.push_str(
            "The index was built with another model. Reindexing must be started by the user, not the agent.\n",
        );
    }
    if let Some(graph) = &output.graph {
        let state = serde_json::to_value(graph.state).expect("graph state serializes");
        let _ = write!(
            text,
            "\nGraph: {}; sources: {}; job: {}; error: {}",
            state.as_str().expect("graph state is a string"),
            graph.source_count,
            graph.active_job.as_deref().unwrap_or("none"),
            graph.error.as_deref().unwrap_or("none")
        );
    }
    text.push_str("\n=== END STATUS ===");
    text
}

pub fn reindex(output: &AcceptedOutput) -> String {
    let note = if output.accepted {
        "Reindexing accepted. Check bars_status for progress."
    } else {
        "Reindexing not accepted."
    };
    format!(
        "=== REINDEX ===\naccepted: {}\n{note}\n=== END ===",
        output.accepted
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn english_search_framing_preserves_source_language() {
        let mut output: SearchOutput = serde_json::from_value(json!({
            "results": [{"path": "docs/поиск.md", "module": "features",
                "title": "Поиск", "heading_chain": "Поиск/Порог", "first_line": 3,
                "last_line": 4, "text": "Русский текст", "score": 0.5, "score_kind": "cosine"}],
            "applied": {"results": 10, "candidates": 30, "rerank_candidates": 0,
                "min_score": 0.0, "score_kind": "cosine", "clamped": false},
            "indexing": true, "pending": 1, "rerank_error": "Ошибка провайдера"
        }))
        .unwrap();
        output.results = vec![output.results[0].clone(); 10];
        let text = search(&output);
        assert!(text.starts_with("=== SEARCH: results 10 ===\n=== RESULT 1 of 10 ===\n"));
        assert!(text.contains("=== RESULT 10 of 10 ===\n"));
        assert!(text.contains("=== END RESULT 10 ===\n=== APPLIED PARAMETERS ===\n"));
        assert!(text.contains("--- text ---\nРусский текст\n"));
        assert!(text.contains("title: Поиск\nheading: Поиск/Порог\n"));
        assert!(text.contains("rerank_error: Ошибка провайдера\n"));
        assert!(
            text.ends_with("Indexing in progress: results may be incomplete.\n=== END SEARCH ===")
        );
        output.results.clear();
        assert!(
            search(&output).starts_with("=== SEARCH: results 0 ===\n=== APPLIED PARAMETERS ===\n")
        );
    }

    #[test]
    fn english_document_framing_preserves_numbered_source() {
        assert_eq!(
            read(
                "документ.md",
                &ReadOutput {
                    title: "Документ".into(),
                    text: "# Заголовок\nТекст".into()
                }
            ),
            "=== DOCUMENT документ.md ===\ntitle: Документ\n--- text (with line numbers) ---\n1 # Заголовок\n2 Текст\n=== END DOCUMENT ==="
        );
        assert_eq!(
            outline(
                "документ.md",
                &OutlineOutput {
                    headings: vec![Heading {
                        line: 1,
                        level: 1,
                        text: "Заголовок".into()
                    }]
                }
            ),
            "=== OUTLINE документ.md: headings 1 ===\n1 # Заголовок\n=== END OUTLINE ==="
        );
        assert!(projects(&ProjectsOutput { projects: vec![] }).is_ascii());
        for accepted in [true, false] {
            assert!(reindex(&AcceptedOutput { accepted }).is_ascii());
        }
    }
}
