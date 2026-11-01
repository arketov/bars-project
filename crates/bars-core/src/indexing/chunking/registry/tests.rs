use super::super::{ChunkingParams, TRUNCATED, chunk_document, chunk_document_with_spans};

fn params(min: usize, max: usize) -> ChunkingParams {
    ChunkingParams {
        min_chars: min,
        max_chars: max,
    }
}

fn table(rows: usize, width: usize) -> String {
    let mut text = String::from("| Номер | Описание |\n|---|---|\n");
    for i in 0..rows {
        text.push_str(&format!("| ISS-{i} | {} |\n", "я".repeat(width)));
    }
    text.trim_end().to_string()
}

#[test]
fn table_tail_is_visible_with_header_and_correct_lines() {
    let source = format!("---\ntitle: Реестр\n---\n# Ошибки\n{}", table(30, 160));
    let mapped = chunk_document_with_spans("docs/issues.md", &source, &params(250, 6000));
    assert!(mapped.document.chunks.len() >= 4);
    for (i, chunk) in mapped.document.chunks.iter().enumerate() {
        assert_eq!(chunk.ordinal, i);
        assert_eq!(chunk.heading_chain, "Ошибки");
        assert!(chunk.text.starts_with("| Номер | Описание |\n|---|---|\n"));
        assert!(
            chunk
                .text
                .strip_suffix(TRUNCATED)
                .unwrap_or(&chunk.text)
                .chars()
                .count()
                <= 1500
        );
        assert!(chunk.embed_text.ends_with(&chunk.text));
        assert_eq!(
            chunk.embed_hash,
            blake3::hash(chunk.embed_text.as_bytes()).to_hex().as_str()
        );
        if i == 0 {
            assert_eq!(chunk.first_line, 4);
        } else {
            let first_own_line = source.lines().nth(chunk.first_line - 1).unwrap();
            assert!(first_own_line.starts_with("| ISS-"));
            assert!(chunk.text.contains(first_own_line));
            let header_end = source.find("| ISS-0").unwrap();
            assert!(
                mapped.source_spans[i]
                    .iter()
                    .all(|range| range.start >= header_end)
            );
        }
        assert!(
            source
                .lines()
                .nth(chunk.last_line - 1)
                .unwrap()
                .starts_with("| ISS-")
        );
    }
    for i in 0..30 {
        let id = format!("| ISS-{i} |");
        assert_eq!(
            mapped
                .document
                .chunks
                .iter()
                .filter(|c| c.text.contains(&id))
                .count(),
            1
        );
    }
    assert!(
        mapped
            .document
            .chunks
            .last()
            .unwrap()
            .text
            .contains("| ISS-29 |")
    );
}

#[test]
fn list_items_keep_indented_continuations() {
    for marker in ["- **Т-", "* [ ] №", "1. ISS-"] {
        let mut source = String::from("# Реестр\n");
        for i in 0..20 {
            source.push_str(&format!(
                "{marker}{i} {}\n  продолжение-{i}\n",
                "а".repeat(180)
            ));
        }
        let doc = chunk_document("a.md", &source, &params(250, 6000));
        assert!(doc.chunks.len() >= 3, "{marker}");
        for chunk in &doc.chunks {
            assert!(
                chunk
                    .text
                    .strip_suffix(TRUNCATED)
                    .unwrap_or(&chunk.text)
                    .chars()
                    .count()
                    <= 1500
            );
        }
        for i in 0..20 {
            let item = format!("{marker}{i} {}\n  продолжение-{i}", "а".repeat(180));
            assert_eq!(
                doc.chunks
                    .iter()
                    .filter(|chunk| chunk.text.contains(&item))
                    .count(),
                1
            );
        }
    }
}

#[test]
fn short_registries_and_nonregistries_stay_whole() {
    for body in [
        table(3, 100),
        "обычный текст ".repeat(200),
        format!("```md\n{}\n```", table(30, 80)),
        format!("~~~\n{}\n~~~", table(30, 80)),
        format!("- ISS-1 {}\n- ISS-2 {}", "я".repeat(900), "я".repeat(900)),
        table(1, 2000),
    ] {
        let source = format!("# Заголовок\n{body}");
        let doc = chunk_document("a.md", &source, &params(1, 6000));
        assert_eq!(doc.chunks.len(), 1);
        assert_eq!(doc.chunks[0].text, body);
    }
}

#[test]
fn unicode_bom_crlf_and_oversized_rows_preserve_all_source_spans() {
    let source = format!(
        "\u{feff}# Реестр\r\n{}",
        table(3, 1900).replace('\n', "\r\n")
    );
    for max in [1, 17, 55, 300, 1500] {
        let mapped = chunk_document_with_spans("a.md", &source, &params(0, max));
        let mut ranges: Vec<_> = mapped.source_spans.iter().flatten().cloned().collect();
        ranges.sort_by_key(|range| range.start);
        for range in &ranges {
            assert!(source.get(range.clone()).is_some());
        }
        // Каждая непустая буква исходника принадлежит ровно одному собственному диапазону.
        for (at, ch) in source
            .char_indices()
            .filter(|(_, ch)| !ch.is_whitespace() && *ch != '\u{feff}')
        {
            assert_eq!(
                ranges.iter().filter(|range| range.contains(&at)).count(),
                1,
                "{max}: {at} {ch}"
            );
        }
        for chunk in &mapped.document.chunks {
            assert!(
                chunk
                    .text
                    .strip_suffix(TRUNCATED)
                    .unwrap_or(&chunk.text)
                    .chars()
                    .count()
                    <= max
            );
        }
    }
}

#[test]
fn small_registry_cannot_be_merged_or_padded_past_window() {
    let source = format!(
        "# Маленький\n{}\n# Большой\n{}\n# Последний\n{}",
        table(3, 40),
        "текст ".repeat(700),
        table(3, 40)
    );
    let doc = chunk_document("a.md", &source, &params(5000, 6000));
    assert!(doc.chunks.len() >= 3);
    for chunk in doc.chunks.iter().filter(|c| c.heading_chain != "Большой") {
        assert!(chunk.text.chars().count() <= 1500);
    }
}

#[test]
fn separate_tables_repeat_their_own_headers() {
    let source = format!(
        "# Реестры\n{}\n\nМежду таблицами.\n\n{}",
        table(15, 100),
        table(15, 100).replace("Номер", "Код")
    );
    let doc = chunk_document("a.md", &source, &params(1, 6000));
    assert!(doc.chunks.iter().any(|c| c.text.starts_with("| Код |")));
    assert!(
        doc.chunks
            .iter()
            .any(|c| c.text.contains("Между таблицами."))
    );
    for chunk in &doc.chunks {
        assert!(
            chunk
                .text
                .strip_suffix(TRUNCATED)
                .unwrap_or(&chunk.text)
                .chars()
                .count()
                <= 1500
        );
    }
}

#[test]
fn shifting_lines_reuses_embed_hashes() {
    let source = format!("# Реестр\n{}", table(30, 180));
    let first = chunk_document("a.md", &source, &params(250, 6000));
    let shifted = chunk_document("a.md", &format!("\n{source}"), &params(250, 6000));
    assert_eq!(first.chunks.len(), shifted.chunks.len());
    for (a, b) in first.chunks.iter().zip(shifted.chunks) {
        assert_eq!(a.embed_hash, b.embed_hash);
        assert_eq!(a.first_line + 1, b.first_line);
        assert_eq!(a.last_line + 1, b.last_line);
    }
}
