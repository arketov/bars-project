//! Строки о баллах по сигналам, разделах и «понято как» для текстового вида ответа поиска
//! (MCP, режимы с графом): служебное оформление на английском, как у остальных блоков MCP.
use crate::{AppliedParams, SearchHit, SearchOutput};
use std::fmt::Write as _;

/// `signals: rerank 0.912; cosine 0.601; fulltext 1.000; exact 0.500` — баллы лучшего куска раздела
/// по сигналам. `None` у результата без сигналов (`similar`).
pub fn hit_signals_line(hit: &SearchHit) -> Option<String> {
    let signals = hit.signals.as_ref()?;
    let mut parts = Vec::new();
    if let Some(rerank) = signals.rerank {
        parts.push(format!("rerank {rerank:.3}"));
    }
    if let Some(cosine) = signals.cosine {
        parts.push(format!("cosine {cosine:.3}"));
    }
    parts.push(format!("fulltext {:.3}", signals.fulltext));
    parts.push(format!("exact {:.3}", signals.exact));
    if let Some(links) = signals.links {
        parts.push(format!("links {links:.3}"));
    }
    Some(format!("signals: {}", parts.join("; ")))
}

/// `parts: 2 of 3 found` — сколько кусков раздела нашлось; `None`, если результат не раздел.
pub fn hit_parts_line(hit: &SearchHit) -> Option<String> {
    (hit.parts > 0).then(|| format!("parts: {} of {} found", hit.parts_found, hit.parts))
}

/// Строки `understood:` и `weights:` для блока параметров; пусто, если операторов нет и сигналов
/// нет (ответ старого воркера).
pub fn applied_signal_lines(output: &SearchOutput) -> String {
    let mut text = String::new();
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
        let _ = writeln!(text, "understood: {}", parts.join("; "));
    }
    if !output.applied.signals.is_empty() {
        text.push_str(&weights_line(&output.applied));
        text.push('\n');
    }
    text
}

fn weights_line(applied: &AppliedParams) -> String {
    let weights = &applied.weights;
    format!(
        "signals: {}; weights: cosine {}, fulltext {}, exact {}, links {}, fold_bonus {}",
        applied.signals.join(", "),
        weights.cosine,
        weights.fulltext,
        weights.exact,
        weights.links,
        weights.fold_bonus
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HitSignals, ScoreKind};

    fn hit(signals: Option<HitSignals>, parts: u32) -> SearchHit {
        SearchHit {
            source_hash: String::new(),
            chunk_id: String::new(),
            path: "a.md".into(),
            module: ".".into(),
            title: "A".into(),
            heading_chain: "A".into(),
            sections: vec![],
            first_line: 1,
            last_line: 2,
            text: String::new(),
            score: 0.5,
            score_kind: ScoreKind::Rerank,
            signals,
            parts,
            parts_found: 2,
        }
    }

    #[test]
    fn signals_line_lists_only_the_signals_that_exist() {
        let line = hit_signals_line(&hit(
            Some(HitSignals {
                rerank: Some(0.9),
                fulltext: 1.0,
                exact: 0.5,
                ..HitSignals::default()
            }),
            0,
        ))
        .unwrap();

        assert_eq!(line, "signals: rerank 0.900; fulltext 1.000; exact 0.500");
        assert_eq!(hit_signals_line(&hit(None, 0)), None);
    }

    #[test]
    fn parts_line_is_for_sections_only() {
        assert_eq!(
            hit_parts_line(&hit(None, 3)).as_deref(),
            Some("parts: 2 of 3 found")
        );
        assert_eq!(hit_parts_line(&hit(None, 0)), None);
    }
}
