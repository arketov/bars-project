//! Выбор исходных строк для read (OQ-158); без индекса и повторной нарезки чанков.

use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub struct Part {
    pub from: usize,
    pub to: usize,
    pub heading_chain: Option<String>,
    pub bytes: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Selection {
    pub lines_total: usize,
    pub parts: Vec<Part>,
}

/// Границы строк с исходными CRLF/LF/CR; последняя пустая строка после перевода не добавляется.
fn line_offsets(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut offsets = vec![0];
    let mut i = 0;
    while i < bytes.len() {
        if matches!(bytes[i], b'\r' | b'\n') {
            if bytes[i] == b'\r' && bytes.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            offsets.push(i + 1);
        }
        i += 1;
    }
    if offsets.last() != Some(&source.len()) {
        offsets.push(source.len());
    }
    offsets
}

pub fn select(source: &str, lines: Option<&str>, under: Option<&str>) -> Result<Selection, String> {
    let offsets = line_offsets(source);
    let total = offsets.len() - 1;
    let invalid = || {
        format!(
            "Недопустимый диапазон lines; lines_total: {total}. Ожидается N, N-M или N-; строки с единицы"
        )
    };
    if lines.is_some() && under.is_some() {
        return Err(format!(
            "lines и under взаимоисключающие; lines_total: {total}"
        ));
    }
    let ranges = if let Some(lines) = lines {
        let lines = lines.trim();
        let parse = |s: &str| {
            if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
                return Err(invalid());
            }
            s.parse::<usize>().map_err(|_| invalid())
        };
        let (from, to) = match lines.split_once('-') {
            Some((from, to)) => (parse(from)?, if to.is_empty() { total } else { parse(to)? }),
            None => {
                let n = parse(lines)?;
                (n, n)
            }
        };
        if from == 0 || from > total || to < from {
            return Err(invalid());
        }
        vec![(from, to.min(total), None)]
    } else if let Some(under) = under {
        crate::outline::section_ranges(source, under, total)
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|(from, to, chain)| (from, to, Some(chain)))
            .collect()
    } else {
        return Ok(Selection {
            lines_total: total,
            parts: vec![],
        });
    };
    Ok(Selection {
        lines_total: total,
        parts: ranges
            .into_iter()
            .map(|(from, to, heading_chain)| Part {
                from,
                to,
                heading_chain,
                bytes: offsets[from - 1]..offsets[to],
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_matches_chunking_metadata_without_chunk_construction() {
        use crate::indexing::chunking::{ChunkingParams, chunk_document, document_title};
        for source in [
            "",
            "# А",
            "---\ntitle: 'Имя'\n---\n# А",
            "\u{feff}---\r\ntitle: Имя\r\n...\r\n# А",
            "---\rtitle: Имя\r---\r# А",
            "---\ntitle: Имя\n# незакрыто",
            "---text\ntitle: чужой\n---",
        ] {
            assert_eq!(
                document_title("test.md", source),
                chunk_document("test.md", source, &ChunkingParams::default()).title
            );
        }
    }

    #[test]
    fn many_sections_are_selected_in_document_order() {
        let source = format!(
            "# Реестр\n{}",
            "## Запись\nданные\n### Деталь\nтекст\n".repeat(5_000)
        );
        let started = std::time::Instant::now();
        let selection = select(&source, None, Some("Реестр/Запись")).unwrap();
        assert_eq!(selection.parts.len(), 5_000);
        assert_eq!(selection.parts[4_999].from, 19_998);
        assert_eq!(selection.parts[4_999].to, 20_001);
        println!(
            "partial_read_scale: bytes={}, sections=5000, elapsed_us={}",
            source.len(),
            started.elapsed().as_micros()
        );
    }

    #[test]
    fn inclusive_ranges_preserve_source_and_reject_invalid_values() {
        let source = "\u{feff}# А\r\n\r\nтекст\nконец";
        for (query, expected) in [
            ("2-3", "\r\nтекст\n"),
            ("3", "текст\n"),
            ("3-", "текст\nконец"),
            (" 4-99 ", "конец"),
        ] {
            let selection = select(source, Some(query), None).unwrap();
            assert_eq!(selection.lines_total, 4);
            assert_eq!(&source[selection.parts[0].bytes.clone()], expected);
        }
        for query in [
            "",
            "0",
            "0-1",
            "3-2",
            "5",
            "-2",
            "1-2-3",
            "1-0",
            "+1",
            "1.0",
            "9999999999999999999999999",
        ] {
            assert!(
                select(source, Some(query), None)
                    .unwrap_err()
                    .contains("lines_total: 4"),
                "{query}"
            );
        }
        assert!(select(source, Some("1"), Some("А")).is_err());
        assert!(
            select("", Some("1-"), None)
                .unwrap_err()
                .contains("lines_total: 0")
        );
        assert_eq!(select("a\r\nb\r", Some("2"), None).unwrap().lines_total, 2);
    }

    #[test]
    fn sections_include_descendants_and_all_repeated_chains() {
        let source = "---\ntitle: T\n# fake\n---\n# А\n## Б\nтекст\n```\n## fake\n```\n### В\nвложенный\n## Б\nповтор\n# Другой\nнет";
        let selection = select(source, None, Some(" А/Б ")).unwrap();
        assert_eq!(
            selection
                .parts
                .iter()
                .map(|p| (p.from, p.to))
                .collect::<Vec<_>>(),
            [(6, 12), (13, 14)]
        );
        assert_eq!(&source[selection.parts[1].bytes.clone()], "## Б\nповтор\n");
        for chain in ["Б", "А/fake", "fake", ""] {
            assert!(select(source, None, Some(chain)).is_err(), "{chain}");
        }
        let cr = "\u{feff}# А\r## Б\rтекст\r# Другой\r";
        let selection = select(cr, None, Some("А/Б")).unwrap();
        assert_eq!(&cr[selection.parts[0].bytes.clone()], "## Б\rтекст\r");
    }
}
