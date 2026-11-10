//! Текстовый вид ответов инструментов (`format=txt`, по умолчанию): блоки `=== … ===` с полями
//! `ключ: значение`, чтобы агент однозначно находил границы результатов и текста документов.

use std::fmt::Write as _;

use bars_protocol::{
    AcceptedOutput, DuplicateKind, DuplicatesOutput, Heading, OutlineOutput, ProjectInfo,
    ProjectState, ProjectsOutput, ReadOutput, ScoreKind, SearchHit, SearchOutput, SimilarOutput,
    StatusOutput,
};

fn score_kind(kind: ScoreKind) -> &'static str {
    match kind {
        ScoreKind::Rerank => "rerank",
        ScoreKind::Cosine => "cosine",
        ScoreKind::None => "none",
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
    if let Some(line) = bars_protocol::hit_signals_line(hit) {
        let _ = writeln!(text, "{line}");
    }
    if let Some(line) = bars_protocol::hit_parts_line(hit) {
        let _ = writeln!(text, "{line}");
    }
    let _ = writeln!(text, "--- text ---");
    let _ = writeln!(text, "{}", hit.text.trim_end());
    let _ = writeln!(text, "=== END RESULT {n} ===");
}

pub fn search(output: &SearchOutput) -> String {
    let total = output.results.len();
    let kind = output.applied.score_kind;
    let mut text = format!(
        "{total} of {} · scope: {}\n",
        output.applied.candidates,
        scope_label(output)
    );
    for (index, hit) in output.results.iter().enumerate() {
        let support = support(hit, kind);
        let _ = writeln!(text, "{}", result_line(index + 1, hit, support));
        if stage_full(index, kind, support) {
            text.push_str(&limit_other_headers(
                &hit.text,
                &output.query_words,
                OTHER_HEADERS_MAX,
            ));
            text.push('\n');
        } else {
            let _ = writeln!(
                text,
                "{}",
                excerpt(&hit.text, &output.query_words, EXCERPT_CHARS)
            );
        }
    }
    for item in &output.weak {
        let _ = writeln!(
            text,
            "weak: {}:{}-{} · {} ({:.2})",
            item.path,
            item.first_line,
            item.last_line,
            last_heading(&item.heading_chain),
            item.score
        );
    }
    text.push_str(&tail(output));
    text
}

/// Опора результата, по которой работают ступени: rerank, а без него — косинус.
fn support(hit: &SearchHit, kind: ScoreKind) -> Option<f32> {
    let signals = hit.signals.as_ref()?;
    match kind {
        ScoreKind::Rerank => signals.rerank,
        ScoreKind::Cosine => signals.cosine,
        ScoreKind::None => None,
    }
}

/// 2–3-е место с rerank печатается полностью от этой опоры (проект агентского слоя; стартовое
/// значение, проверяется на сохранённых выдачах бенча).
const FULL_TEXT_RERANK_THRESHOLD: f32 = 0.875;
/// Отрывок вокруг слов запроса для результатов не в полном тексте.
const EXCERPT_CHARS: usize = 300;
/// Сколько заголовков показывать в блоке «other headers» (ISS-115).
const OTHER_HEADERS_MAX: usize = 8;

/// Полный текст печатается первым результатам; остальные — строка и отрывок. С rerank полностью
/// идут 1-е место и 2–3-е при опоре от `FULL_TEXT_RERANK_THRESHOLD`; без rerank оценка не
/// откалибрована, полностью печатаются 1-е и 2-е места.
fn stage_full(index: usize, kind: ScoreKind, support: Option<f32>) -> bool {
    match kind {
        ScoreKind::Rerank => {
            index == 0
                || (index <= 2 && support.is_some_and(|value| value >= FULL_TEXT_RERANK_THRESHOLD))
        }
        ScoreKind::Cosine | ScoreKind::None => index <= 1,
    }
}

/// Строка результата: место, оценка-опора (не `min(1, S)`), путь со строками, последний заголовок
/// цепочки (ISS-114).
fn result_line(n: usize, hit: &SearchHit, support: Option<f32>) -> String {
    let score = support.map_or(String::new(), |value| format!("{value:.2} "));
    format!(
        "[{n}] {score}{}:{}-{} · {}",
        hit.path,
        hit.first_line,
        hit.last_line,
        last_heading(&hit.heading_chain)
    )
}

/// Последний заголовок цепочки раздела; у вступления документа заголовка нет — `(intro)`.
fn last_heading(heading_chain: &str) -> &str {
    let last = heading_chain.split('/').next_back().unwrap_or_default();
    if last.trim().is_empty() {
        "(intro)"
    } else {
        last
    }
}

/// Область поиска одной строкой для шапки выдачи.
fn scope_label(output: &SearchOutput) -> String {
    let scope = &output.applied.scope;
    let mut filters = Vec::new();
    if !scope.modules.is_empty() {
        filters.push(format!("modules {}", scope.modules.join(", ")));
    }
    if !scope.files.is_empty() {
        filters.push(format!("files {}", scope.files.join(", ")));
    }
    if filters.is_empty() {
        "whole project".into()
    } else {
        filters.join("; ")
    }
}

/// Хвост выдачи печатается только при отклонениях: идёт индексация, очередь не пуста, параметр
/// прижат к пределу, сигнал недоступен, ответ пуст или результаты из нескольких папок.
fn tail(output: &SearchOutput) -> String {
    let mut tail = String::new();
    let scope = &output.applied.scope;
    let whole = scope.modules.is_empty() && scope.files.is_empty();
    if output.results.is_empty() || output.by_module.results.len() > 1 {
        let counts = |map: &std::collections::BTreeMap<String, u32>| {
            if map.is_empty() {
                "none".into()
            } else {
                map.iter()
                    .map(|(name, n)| format!("{name} {n}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        };
        let _ = writeln!(
            tail,
            "by_module: candidates {}; results {}",
            counts(&output.by_module.candidates),
            counts(&output.by_module.results)
        );
    }
    if whole && output.by_module.results.len() > 1 {
        let _ = writeln!(
            tail,
            "Results span several modules; pass modules to narrow the scope."
        );
    }
    if let Some(line) = bars_protocol::excluded_projects_line(&scope.excluded_projects) {
        let _ = writeln!(tail, "{line}");
    }
    if let Some(understood) = &output.understood {
        let mut parts = vec![format!("words \"{}\"", understood.text)];
        for phrase in &understood.phrases {
            parts.push(format!("phrase \"{phrase}\""));
        }
        for word in &understood.excluded_words {
            parts.push(format!("exclude {word}"));
        }
        for phrase in &understood.excluded_phrases {
            parts.push(format!("exclude \"{phrase}\""));
        }
        for path in &understood.paths {
            parts.push(format!("path {path}"));
        }
        let _ = writeln!(tail, "understood: {}", parts.join("; "));
    }
    if output.applied.clamped {
        let _ = writeln!(tail, "clamped: limit {}", output.applied.results);
    }
    if output.indexing {
        let _ = writeln!(tail, "Indexing in progress: results may be incomplete.");
    }
    if output.pending > 0 {
        let _ = writeln!(tail, "pending: {} documents in queue", output.pending);
    }
    if let Some(error) = &output.rerank_error {
        let _ = writeln!(tail, "rerank unavailable: {error}");
    }
    if let Some(error) = &output.semantic_error {
        let _ = writeln!(tail, "semantic unavailable: {error}");
    }
    if output.results.is_empty() && !scope.modules.is_empty() {
        let _ = writeln!(
            tail,
            "No results in the selected modules; try the whole project. Zero results do not prove absence."
        );
    }
    tail
}

/// Текст чанка без блока «other headers:»: он служебный — и в отрывок не идёт.
fn own_text(text: &str) -> &str {
    bars_protocol::split_other_headers(text).0
}

/// Отрывок до `max_chars` знаков вокруг слов запроса. Без совпадений — начало текста; срезы
/// помечаются многоточиями. Счёт идёт в знаках, границы слов — по буквам и цифрам.
fn excerpt(text: &str, words: &[String], max_chars: usize) -> String {
    let own = own_text(text);
    if words.is_empty() {
        return truncate_head(own, max_chars);
    }
    let chars: Vec<char> = own.chars().collect();
    // Строчная копия и карта «позиция в строчной → номер знака оригинала»: строчная форма редких
    // знаков занимает больше одного знака, поэтому ищем в строчной, а срезы берём по оригиналу.
    let mut lower: Vec<char> = Vec::with_capacity(chars.len());
    let mut lower_to_own: Vec<usize> = Vec::with_capacity(chars.len());
    for (index, c) in chars.iter().enumerate() {
        for lc in c.to_lowercase() {
            lower.push(lc);
            lower_to_own.push(index);
        }
    }
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for word in words {
        let needle: Vec<char> = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase()
            .chars()
            .collect();
        if needle.len() < 2 || needle.len() > lower.len() {
            continue;
        }
        let mut at = 0;
        while let Some(rel) = lower[at..]
            .windows(needle.len())
            .position(|window| window == needle.as_slice())
        {
            let start = at + rel;
            let end = start + needle.len();
            let own_start = lower_to_own[start];
            let own_end = lower_to_own[end - 1] + 1;
            let before_ok = own_start == 0 || !chars[own_start - 1].is_alphanumeric();
            let after_ok = own_end >= chars.len() || !chars[own_end].is_alphanumeric();
            if before_ok && after_ok {
                spans.push((own_start, own_end));
            }
            at = start + 1;
        }
    }
    spans.sort_unstable();
    spans.dedup();
    let Some(&(first, first_end)) = spans.first() else {
        return truncate_head(own, max_chars);
    };
    // Окно от первого совпадения: накрываем остальные, пока хватает бюджета, и растём в обе
    // стороны симметрично — чтобы срезы с обеих сторон помечались многоточиями.
    let mut end = first_end;
    for &(_, span_end) in &spans[1..] {
        if span_end - first > max_chars {
            break;
        }
        end = span_end;
    }
    let mut start = first;
    while end - start < max_chars && (start > 0 || end < chars.len()) {
        let grow_left = start > 0 && (end == chars.len() || first - start <= end - first_end);
        if grow_left {
            start -= 1;
        } else {
            end += 1;
        }
    }
    let cut_start = start > 0;
    let cut_end = end < chars.len();
    let mut out = String::new();
    if cut_start {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if cut_end {
        out.push('…');
    }
    out
}

/// Первые `max_chars` знаков текста со многоточием при обрезке.
fn truncate_head(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut cut = false;
    for (index, c) in text.chars().enumerate() {
        if index == max_chars {
            cut = true;
            break;
        }
        out.push(c);
    }
    if cut {
        out.push('…');
    }
    out
}

/// Блок «other headers:» — не больше `max` заголовков: сначала совпавшие со словами запроса, затем
/// остальные в исходном порядке; в конце — число непоказанных (ISS-115). Совпадение — слово
/// целиком, как в отрывке: «кот» не совпадает с «Коттедж».
fn limit_other_headers(text: &str, words: &[String], max: usize) -> String {
    let (own, block) = match bars_protocol::split_other_headers(text) {
        (_, None) => return text.to_string(),
        (own, Some(block)) => (own, block),
    };
    let headers: Vec<&str> = block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect();
    if headers.len() <= max {
        return text.to_string();
    }
    let total = headers.len();
    let needles: Vec<String> = words
        .iter()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .filter(|word| word.chars().count() >= 2)
        .collect();
    // Один проход: каждый заголовок размечается совпадением один раз.
    let mut matched: Vec<&str> = Vec::new();
    let mut rest_headers: Vec<&str> = Vec::new();
    for header in headers {
        let tokens: Vec<String> = header
            .split(|c: char| !c.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .map(|token| token.to_lowercase())
            .collect();
        if needles
            .iter()
            .any(|needle| tokens.iter().any(|token| token == needle))
        {
            matched.push(header);
        } else {
            rest_headers.push(header);
        }
    }
    let mut shown: Vec<&str> = matched.drain(..max.min(matched.len())).collect();
    for header in rest_headers {
        if shown.len() == max {
            break;
        }
        shown.push(header);
    }
    let rest = total - shown.len();
    let mut out = String::new();
    if !own.is_empty() {
        out.push_str(own);
        out.push('\n');
    }
    out.push_str("other headers:");
    out.push('\n');
    for header in shown {
        let _ = writeln!(out, "{header}");
    }
    let _ = writeln!(out, "(+{rest} more)");
    out.trim_end_matches('\n').to_string()
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
        bars_protocol::read_text(output)
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
    if let Some(line) = bars_protocol::excluded_projects_line(&info.nested) {
        let _ = writeln!(text, "{line}");
    }
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

    fn hit(
        path: &str,
        heading_chain: &str,
        text: &str,
        rerank: Option<f32>,
        lines: (u32, u32),
    ) -> SearchHit {
        SearchHit {
            source_hash: String::new(),
            chunk_id: String::new(),
            path: path.into(),
            module: "features".into(),
            title: "Документ".into(),
            heading_chain: heading_chain.into(),
            sections: vec![],
            first_line: lines.0,
            last_line: lines.1,
            text: text.into(),
            score: 1.0,
            score_kind: ScoreKind::Rerank,
            signals: Some(bars_protocol::HitSignals {
                rerank,
                ..bars_protocol::HitSignals::default()
            }),
            parts: 1,
            parts_found: 1,
        }
    }

    fn output(hits: Vec<SearchHit>, kind: ScoreKind) -> SearchOutput {
        SearchOutput {
            results: hits,
            applied: bars_protocol::AppliedParams {
                results: 10,
                candidates: 30,
                rerank_candidates: 15,
                min_score: 0.2,
                score_kind: kind,
                clamped: false,
                scope: Default::default(),
                weights: Default::default(),
                signals: vec![],
            },
            by_module: Default::default(),
            indexing: false,
            pending: 0,
            rerank_error: None,
            semantic_error: None,
            understood: None,
            weak: vec![],
            query_words: vec![],
        }
    }

    fn long_text(tail_marker: &str) -> String {
        // Больше EXCERPT_CHARS знаков: truncate считает знаки, не байты (кириллица — 2 байта).
        let filler = "слова ".repeat(70);
        let filler: String = filler.chars().take(400).collect();
        format!("{filler}\n\n{tail_marker}")
    }

    fn mid_text(marker: &str) -> String {
        // Совпадение в середине: до и после него по ~150+ знаков, чтобы окно отрывка
        // обрезало текст с обеих сторон.
        let filler = "слова ".repeat(70);
        let head: String = filler.chars().take(300).collect();
        let tail: String = filler.chars().take(200).collect();
        format!("{head}\n\n{marker}\n\n{tail}")
    }

    #[test]
    fn staged_search_prints_compact_header_and_support_scores() {
        // Длинные текста у коротких ступеней: отрывок не дотягивается до метки в конце.
        let hits = vec![
            hit(
                "docs/a.md",
                "Поиск/Порог",
                "Полный текст раздела",
                Some(0.92),
                (120, 129),
            ),
            hit(
                "docs/b.md",
                "Поиск/Сигналы",
                "Второй полный",
                Some(0.9),
                (5, 9),
            ),
            hit(
                "docs/c.md",
                "Поиск/Веса",
                &long_text("ХВОСТ-МЕТКА-3"),
                Some(0.8),
                (30, 40),
            ),
            hit(
                "docs/d.md",
                "",
                &long_text("ХВОСТ-МЕТКА-4"),
                Some(0.7),
                (1, 3),
            ),
        ];
        let mut found = output(hits, ScoreKind::Rerank);
        found.query_words = vec!["слова".into()];
        let text = search(&found);
        assert!(
            text.starts_with("4 of 30 · scope: whole project\n"),
            "{text}"
        );
        // Оценка в строке — опора rerank, а не обрезанный `min(1, S)` (ISS-114).
        assert!(
            text.contains("[1] 0.92 docs/a.md:120-129 · Порог\n"),
            "{text}"
        );
        assert!(
            text.contains("[2] 0.90 docs/b.md:5-9 · Сигналы\nВторой полный\n"),
            "{text}"
        );
        // 2–3-е место ниже 0.875 и дальше — отрывком, без полного текста.
        assert!(text.contains("[3] 0.80 docs/c.md:30-40 · Веса\n"), "{text}");
        assert!(!text.contains("ХВОСТ-МЕТКА-3"), "{text}");
        // Вступление без заголовка печатает `(intro)`.
        assert!(
            text.contains("[4] 0.70 docs/d.md:1-3 · (intro)\n"),
            "{text}"
        );
        assert!(!text.contains("ХВОСТ-МЕТКА-4"), "{text}");
        // Чистый ответ не несёт хвоста: ни by_module, ни служебных строк.
        assert!(!text.contains("by_module"), "{text}");
        assert!(!text.contains("indexing"), "{text}");
        assert!(!text.contains("1.000"), "{text}");
    }

    #[test]
    fn excerpt_wraps_query_words_and_marks_cuts() {
        // Первый результат полный; второй (ниже 0.875 у rerank) — отрывок вокруг слова.
        // Отрывок может содержать переводы строк внутри окна — проверяем весь блок после строки [2].
        let hits = vec![
            hit("docs/a.md", "A", "Полный", Some(0.95), (1, 2)),
            hit(
                "docs/b.md",
                "B",
                &mid_text("порог стоит здесь"),
                Some(0.5),
                (10, 20),
            ),
        ];
        let mut found = output(hits, ScoreKind::Rerank);
        found.query_words = vec!["порог".into()];
        let text = search(&found);
        let excerpt = text
            .split("[2] 0.50 docs/b.md:10-20 · B\n")
            .nth(1)
            .unwrap()
            .trim_end();
        assert!(excerpt.contains("порог"), "{excerpt}");
        assert!(
            excerpt.starts_with('…') && excerpt.ends_with('…'),
            "{excerpt}"
        );
        assert!(excerpt.chars().count() <= EXCERPT_CHARS + 2, "{excerpt}");
        // Граница слова: «пороговой» в конце не считается совпадением — отрывок идёт от начала.
        let hits = vec![
            hit("docs/a.md", "A", "Полный", Some(0.95), (1, 2)),
            hit(
                "docs/b.md",
                "B",
                &long_text("пороговой"),
                Some(0.5),
                (10, 20),
            ),
        ];
        let mut no_match = output(hits, ScoreKind::Rerank);
        no_match.query_words = vec!["порог".into()];
        let head = search(&no_match);
        let excerpt = head
            .split("[2] 0.50 docs/b.md:10-20 · B\n")
            .nth(1)
            .unwrap()
            .trim_end();
        assert!(
            excerpt.ends_with('…') && !excerpt.starts_with('…'),
            "{excerpt}"
        );
        assert!(!excerpt.contains("пороговой"), "{excerpt}");
    }

    #[test]
    fn other_headers_limited_with_matching_first() {
        let headers: Vec<String> = [
            "Альфа",
            "Бета",
            "Порог",
            "Гамма",
            "Дельта",
            "Эпсилон",
            "Ёж",
            "Жук",
            "Зет",
            "Ига",
        ]
        .iter()
        .map(|name| format!("## {name}"))
        .collect();
        let text = format!("Текст раздела\nother headers:\n{}", headers.join("\n"));
        let found = hit("docs/a.md", "A", &text, Some(0.95), (1, 2));
        let mut found = output(vec![found], ScoreKind::Rerank);
        found.query_words = vec!["порог".into()];
        let text = search(&found);
        let block = text.split("other headers:\n").nth(1).unwrap();
        let shown: Vec<&str> = block
            .lines()
            .take_while(|line| !line.starts_with('('))
            .collect();
        assert_eq!(shown.len(), OTHER_HEADERS_MAX, "{block}");
        assert_eq!(shown[0], "## Порог", "{block}");
        assert!(block.contains("(+2 more)"), "{block}");
        // Не больше восьми заголовков даже без слов запроса.
        found.query_words = vec!["неттакогослова".into()];
        let text = search(&found);
        let block = text.split("other headers:\n").nth(1).unwrap();
        let shown: Vec<&str> = block
            .lines()
            .take_while(|line| !line.starts_with('('))
            .collect();
        assert_eq!(shown.len(), OTHER_HEADERS_MAX, "{block}");
    }

    fn cosine_hit(
        path: &str,
        heading: &str,
        text: &str,
        cosine: f32,
        lines: (u32, u32),
    ) -> SearchHit {
        SearchHit {
            signals: Some(bars_protocol::HitSignals {
                cosine: Some(cosine),
                ..bars_protocol::HitSignals::default()
            }),
            ..hit(path, heading, text, None, lines)
        }
    }

    #[test]
    fn cosine_and_none_kinds_have_own_stages() {
        // Без rerank полностью печатаются 1-е и 2-е места, оценка — косинус.
        let hits = vec![
            cosine_hit("docs/a.md", "A", "Полный один", 0.66, (1, 2)),
            cosine_hit("docs/b.md", "B", "Полный два", 0.55, (3, 4)),
            cosine_hit("docs/c.md", "C", &long_text("метка-cos"), 0.44, (5, 6)),
        ];
        let found = output(hits, ScoreKind::Cosine);
        let text = search(&found);
        assert!(
            text.contains("[1] 0.66 docs/a.md:1-2 · A\nПолный один\n"),
            "{text}"
        );
        assert!(
            text.contains("[2] 0.55 docs/b.md:3-4 · B\nПолный два\n"),
            "{text}"
        );
        assert!(text.contains("[3] 0.44 docs/c.md:5-6 · C\n"), "{text}");
        assert!(!text.contains("метка-cos"), "{text}");
        // Без опоры числа в строке нет, полные — первые два.
        let hits = vec![
            hit("docs/a.md", "A", "Полный один", None, (1, 2)),
            hit("docs/b.md", "B", "Полный два", None, (3, 4)),
            hit("docs/c.md", "C", &long_text("метка-none"), None, (5, 6)),
        ];
        let found = output(hits, ScoreKind::None);
        let text = search(&found);
        assert!(
            text.contains("[1] docs/a.md:1-2 · A\nПолный один\n"),
            "{text}"
        );
        assert!(
            text.contains("[2] docs/b.md:3-4 · B\nПолный два\n"),
            "{text}"
        );
        assert!(text.contains("[3] docs/c.md:5-6 · C\n"), "{text}");
        assert!(!text.contains("метка-none"), "{text}");
    }

    #[test]
    fn excerpt_matches_words_case_insensitively() {
        let hits = vec![
            hit("docs/a.md", "A", "Полный", Some(0.95), (1, 2)),
            hit(
                "docs/b.md",
                "B",
                &mid_text("ПОРОГ заглавными"),
                Some(0.5),
                (10, 20),
            ),
        ];
        let mut found = output(hits, ScoreKind::Rerank);
        found.query_words = vec!["порог".into()];
        let text = search(&found);
        let excerpt = text
            .split("[2] 0.50 docs/b.md:10-20 · B\n")
            .nth(1)
            .unwrap()
            .trim_end();
        assert!(excerpt.contains("ПОРОГ"), "{excerpt}");
        assert!(
            excerpt.starts_with('…') && excerpt.ends_with('…'),
            "{excerpt}"
        );
    }

    #[test]
    fn weak_lines_replace_empty_result() {
        let mut empty = output(vec![], ScoreKind::Rerank);
        empty.weak = vec![
            bars_protocol::WeakHit {
                path: "docs/w.md".into(),
                heading_chain: "Слабый раздел".into(),
                first_line: 1,
                last_line: 9,
                score: 0.14,
            },
            bars_protocol::WeakHit {
                path: "docs/x.md".into(),
                heading_chain: "Ещё слабее".into(),
                first_line: 4,
                last_line: 8,
                score: 0.05,
            },
        ];
        let text = search(&empty);
        assert!(
            text.starts_with("0 of 30 · scope: whole project\n"),
            "{text}"
        );
        assert!(
            text.contains("weak: docs/w.md:1-9 · Слабый раздел (0.14)\n"),
            "{text}"
        );
        assert!(
            text.contains("weak: docs/x.md:4-8 · Ещё слабее (0.05)\n"),
            "{text}"
        );
        // Пустой ответ печатает by_module.
        assert!(
            text.contains("by_module: candidates none; results none\n"),
            "{text}"
        );
        // Область-модуль без результатов предлагает весь проект.
        empty.applied.scope.modules = vec!["features".into()];
        let scoped = search(&empty);
        assert!(scoped.contains("scope: modules features"), "{scoped}");
        assert!(
            scoped.contains("No results in the selected modules; try the whole project."),
            "{scoped}"
        );
    }

    #[test]
    fn tail_prints_only_deviations() {
        let hits = vec![hit("docs/a.md", "A/Б", "Текст", Some(0.9), (1, 2))];
        let mut found = output(hits, ScoreKind::Rerank);
        // Несколько папок в результатах — by_module и подсказка.
        found.by_module.results = [("features".into(), 1), ("adr".into(), 1)].into();
        let text = search(&found);
        assert!(
            text.contains("by_module: candidates none; results adr 1, features 1\n"),
            "{text}"
        );
        assert!(
            text.contains("Results span several modules; pass modules to narrow the scope."),
            "{text}"
        );
        // Отклонения: индексация, очередь, прижатие, ошибки, операторы.
        found.indexing = true;
        found.pending = 3;
        found.applied.clamped = true;
        found.rerank_error = Some("модель не ответила".into());
        found.semantic_error = Some("embed грузится".into());
        found.understood = Some(bars_protocol::UnderstoodQuery {
            text: "слова".into(),
            phrases: vec!["точная фраза".into()],
            excluded_words: vec!["лишнее".into()],
            excluded_phrases: vec![],
            paths: vec![],
        });
        let text = search(&found);
        assert!(
            text.contains("understood: words \"слова\"; phrase \"точная фраза\"; exclude лишнее\n"),
            "{text}"
        );
        assert!(text.contains("clamped: limit 10\n"), "{text}");
        assert!(
            text.contains("Indexing in progress: results may be incomplete.\n"),
            "{text}"
        );
        assert!(text.contains("pending: 3 documents in queue\n"), "{text}");
        assert!(
            text.contains("rerank unavailable: модель не ответила\n"),
            "{text}"
        );
        assert!(
            text.contains("semantic unavailable: embed грузится\n"),
            "{text}"
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
                    text: "# Заголовок\nТекст".into(),
                    ..ReadOutput::default()
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
