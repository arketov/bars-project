//! Текстовый вид ответов инструментов (`format=txt`, по умолчанию): блоки `=== … ===` с полями
//! `ключ: значение`, чтобы агент однозначно находил границы результатов и текста документов.

use std::fmt::Write as _;

use bars_protocol::{
    AcceptedOutput, DuplicateKind, DuplicatesOutput, Heading, OutlineOutput, ProjectInfo,
    ProjectState, ProjectsOutput, ReadOutput, ScoreKind, SearchHit, SearchOutput, SimilarOutput,
    StatusOutput, number_lines,
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

/// Один результат: общий вид у поиска и у похожих чанков.
fn result_block(text: &mut String, n: usize, total: usize, hit: &SearchHit) {
    let _ = writeln!(text, "=== RESULT {n} of {total} ===");
    if !hit.chunk_id.is_empty() {
        let _ = writeln!(text, "id: {}", hit.chunk_id);
    }
    let _ = writeln!(text, "path: {}", hit.path);
    let _ = writeln!(text, "lines: {}-{}", hit.first_line, hit.last_line);
    let _ = writeln!(text, "module: {}", hit.module);
    let _ = writeln!(text, "title: {}", hit.title);
    let _ = writeln!(text, "heading: {}", hit.heading_chain);
    if !hit.sections.is_empty() {
        let _ = writeln!(text, "sections: {}", hit.sections.join("; "));
    }
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

pub fn search(output: &SearchOutput) -> String {
    let total = output.results.len();
    let mut text = format!("=== SEARCH: results {total} ===\n");
    for (index, hit) in output.results.iter().enumerate() {
        result_block(&mut text, index + 1, total, hit);
    }
    let applied = &output.applied;
    let _ = writeln!(text, "=== APPLIED PARAMETERS ===");
    text.push_str(&bars_protocol::search_scope_text(output, true));
    let _ = writeln!(text, "limit: {}", applied.results);
    let _ = writeln!(text, "returned: {total}");
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

pub fn similar(output: &SimilarOutput) -> String {
    let total = output.results.len();
    let source = &output.source;
    let mut text = format!(
        "=== SIMILAR: results {total} ===
"
    );
    let _ = writeln!(text, "=== SOURCE ===");
    let _ = writeln!(text, "id: {}", source.chunk_id);
    let _ = writeln!(text, "path: {}", source.path);
    let _ = writeln!(text, "lines: {}-{}", source.first_line, source.last_line);
    let _ = writeln!(text, "module: {}", source.module);
    let _ = writeln!(text, "heading: {}", source.heading_chain);
    let _ = writeln!(text, "=== END SOURCE ===");
    for (index, hit) in output.results.iter().enumerate() {
        result_block(&mut text, index + 1, total, hit);
    }
    let _ = writeln!(text, "=== APPLIED PARAMETERS ===");
    let _ = writeln!(text, "limit: {}", output.limit);
    let _ = writeln!(text, "returned: {total}");
    let _ = writeln!(
        text,
        "score: cosine similarity to the source chunk; closeness does not prove duplication"
    );
    let _ = writeln!(text, "clamped: {}", output.clamped);
    let _ = writeln!(text, "indexing: {}", output.indexing);
    let _ = writeln!(text, "pending: {}", output.pending);
    if output.indexing {
        let _ = writeln!(text, "Indexing in progress: results may be incomplete.");
    }
    text.push_str("=== END SIMILAR ===");
    text
}

fn duplicate_kind(kind: DuplicateKind) -> &'static str {
    match kind {
        DuplicateKind::Exact => "exact",
        DuplicateKind::Near => "near",
        DuplicateKind::Contained => "contained",
        DuplicateKind::Partial => "partial",
        DuplicateKind::Semantic => "semantic",
    }
}

pub fn duplicates(output: &DuplicatesOutput) -> String {
    let shown = output.results.len();
    let mut text = format!("=== DUPLICATES: items {shown} of {} ===\n", output.total);
    for (index, item) in output.results.iter().enumerate() {
        let _ = writeln!(text, "=== ITEM {} of {shown} ===", index + 1);
        let _ = writeln!(text, "kind: {}", duplicate_kind(item.kind));
        let _ = writeln!(text, "score: {:.3}", item.score);
        if item.repeated_block {
            let _ = writeln!(
                text,
                "repeated_block: true (identical text in {} sections: probably a template)",
                item.chunks.len()
            );
        }
        for (number, chunk) in item.chunks.iter().enumerate() {
            let _ = writeln!(text, "=== CHUNK {} ===", number + 1);
            let _ = writeln!(text, "id: {}", chunk.chunk_id);
            let _ = writeln!(text, "path: {}", chunk.path);
            let _ = writeln!(text, "lines: {}-{}", chunk.first_line, chunk.last_line);
            let _ = writeln!(text, "module: {}", chunk.module);
            let _ = writeln!(text, "heading: {}", chunk.heading_chain);
            let _ = writeln!(text, "=== END CHUNK ===");
        }
        for fragment in &item.shared {
            let _ = writeln!(text, "shared: {fragment}");
        }
        if let Some(differences) = &item.differences {
            for change in &differences.changes {
                let _ = writeln!(
                    text,
                    "changed: \"{}\" -> \"{}\"",
                    change.first, change.second
                );
            }
            if differences.changes_truncated {
                let _ = writeln!(
                    text,
                    "changed: more differences than shown (or text too long to compare)"
                );
            }
            if !differences.numbers_only_first.is_empty() {
                let _ = writeln!(
                    text,
                    "numbers only in chunk 1: {}",
                    differences.numbers_only_first.join(", ")
                );
            }
            if !differences.numbers_only_second.is_empty() {
                let _ = writeln!(
                    text,
                    "numbers only in chunk 2: {}",
                    differences.numbers_only_second.join(", ")
                );
            }
            if differences.negations_first != differences.negations_second {
                let _ = writeln!(
                    text,
                    "negation words: chunk 1 has {}, chunk 2 has {}",
                    differences.negations_first, differences.negations_second
                );
            }
        }
        let _ = writeln!(text, "=== END ITEM ===");
    }
    let _ = writeln!(text, "=== APPLIED PARAMETERS ===");
    let _ = writeln!(text, "limit: {}", output.limit);
    let _ = writeln!(text, "returned: {shown}");
    let _ = writeln!(
        text,
        "found: exact {}, near {}, contained {}, partial {}, semantic {}",
        output.counts.exact,
        output.counts.near,
        output.counts.contained,
        output.counts.partial,
        output.counts.semantic
    );
    let _ = writeln!(
        text,
        "score: exact 1; near Jaccard of shared 5-word windows; contained/partial share of the smaller chunk; semantic cosine. Semantic items are only a hint to read both chunks; a copy with changed numbers or negation is not proof of a contradiction."
    );
    let _ = writeln!(text, "clamped: {}", output.clamped);
    let _ = writeln!(text, "chunks_checked: {}", output.chunks_checked);
    let _ = writeln!(
        text,
        "chunks_skipped_short: {}",
        output.chunks_skipped_short
    );
    let _ = writeln!(text, "indexing: {}", output.indexing);
    let _ = writeln!(text, "pending: {}", output.pending);
    if output.indexing {
        let _ = writeln!(text, "Indexing in progress: results may be incomplete.");
    }
    text.push_str("=== END DUPLICATES ===");
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
        .map(|module| {
            let mut text = format!("{} ({})", module.name, module.documents);
            if let Some(title) = &module.title {
                let _ = write!(text, " — {}", super::instructions::clean(title, 160));
            }
            text
        })
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
        if graph.completeness == bars_protocol::graph_types::Completeness::Partial {
            text.push_str("\nGraph partial: coverage is incomplete, not necessarily stale. Missing relations do not prove absence; check layers and evidence. Reindexing does not complete graph coverage.");
        }
        let _ = write!(
            text,
            "\nGraph data: {}; completeness: {}; availability: {}; job_state: {}",
            serde_json::to_value(graph.freshness)
                .unwrap()
                .as_str()
                .unwrap(),
            serde_json::to_value(graph.completeness)
                .unwrap()
                .as_str()
                .unwrap(),
            serde_json::to_value(graph.availability)
                .unwrap()
                .as_str()
                .unwrap(),
            serde_json::to_value(graph.job_state)
                .unwrap()
                .as_str()
                .unwrap()
        );
        for (name, layer) in &graph.layers {
            if let Some(reason) = &layer.stale_reason {
                let _ = write!(text, "\nGraph layer {name} stale_reason: {reason}");
            }
            let _ = write!(
                text,
                "\nGraph layer {name}: freshness={}; completeness={}; coverage={}/{} {}; missing={}; comparisons={}; reason={}",
                serde_json::to_value(layer.freshness)
                    .unwrap()
                    .as_str()
                    .unwrap(),
                serde_json::to_value(layer.completeness)
                    .unwrap()
                    .as_str()
                    .unwrap(),
                layer.coverage.processed,
                layer.coverage.total,
                layer.coverage.unit,
                layer.coverage.missing,
                layer.coverage.comparisons,
                layer.coverage.reason.as_deref().unwrap_or("none")
            );
        }
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
    fn partial_graph_status_explains_coverage_separately_from_freshness() {
        let output: StatusOutput = serde_json::from_value(json!({
            "path":"p", "state":"ready", "updated_at":null, "pending":0,
            "model":{"index":"m", "settings":"m"}, "last_error":null, "watching":false,
            "graph":{"freshness":"current", "completeness":"partial", "availability":"enabled",
                "job_state":"idle", "state":"partial", "snapshot":"s", "active_job":null,
                "source_count":4, "error":null, "method":"test"}
        }))
        .unwrap();
        let text = status(&output);
        assert!(
            text.contains("coverage is incomplete, not necessarily stale"),
            "{text}"
        );
        assert!(
            text.contains("Missing relations do not prove absence"),
            "{text}"
        );
    }

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
        assert!(text.contains("title: Поиск\nheading: Поиск/Порог\nscore: "));
        assert!(text.contains("=== APPLIED PARAMETERS ===\nscope: whole project\nby_module: candidates none; results none\nlimit: 10\nreturned: 10\n"));
        assert!(text.contains("rerank_error: Ошибка провайдера\n"));
        assert!(
            text.ends_with("Indexing in progress: results may be incomplete.\n=== END SEARCH ===")
        );
        output.results[0].sections = vec!["## Б".into(), "### В".into()];
        assert!(search(&output).contains("heading: Поиск/Порог\nsections: ## Б; ### В\nscore: "));
        output.results.truncate(3);
        assert!(search(&output).contains("limit: 10\nreturned: 3\n"));
        output.results.clear();
        output.applied.scope.modules = vec!["features".into()];
        assert!(
            search(&output).contains("No results in the selected modules; try the whole project.")
        );
        output.applied.scope.modules.clear();
        assert!(!search(&output).contains("No results in the selected modules"));
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
                    revision: String::new(),
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
