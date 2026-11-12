//! Вывод, одинаковый у `bars-mcp` и `bars-cli` (Т-20).

/// Ставит номера строк в том же виде, что у `outline`: `12 текст строки`. Нумерация с единицы.
pub fn number_lines(text: &str) -> String {
    number_lines_from(text, 1)
}

/// Число исходных строк в той же модели CRLF/LF/CR, что у нумерации чтения.
pub fn count_lines(text: &str) -> usize {
    source_lines(text).count()
}

fn number_lines_from(text: &str, from: usize) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for (offset, line) in source_lines(text).enumerate() {
        if offset > 0 {
            result.push('\n');
        }
        let _ = write!(result, "{} {line}", from + offset);
    }
    result
}

// CRLF считается одним переводом; срезы заимствованы, нормализованная копия не нужна.
fn source_lines(mut text: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if text.is_empty() {
            return None;
        }
        let end = text.find(['\r', '\n']).unwrap_or(text.len());
        let line = &text[..end];
        let skip = if text[end..].starts_with("\r\n") {
            2
        } else {
            usize::from(end < text.len())
        };
        text = &text[end + skip..];
        Some(line)
    })
}

/// Разделяет текст чанка на собственный текст и блок `other headers:` — заголовки разделов,
/// приклеенные к чанку после первого (ISS-28). Блок распознаётся по строке-маркеру в начале
/// текста или после перевода строки. `None`, если блока нет; собственный текст без маркера
/// возвращается как есть.
pub fn split_other_headers(text: &str) -> (&str, Option<&str>) {
    const MARK: &str = "other headers:";
    if let Some(rest) = text.strip_prefix(MARK) {
        ("", Some(rest.trim_start_matches('\n')))
    } else if let Some(at) = text.find(&format!("\n{MARK}")) {
        (&text[..at], Some(&text[at + 1 + MARK.len()..]))
    } else {
        (text, None)
    }
}

/// Общий вывод текста read: части всегда помечены, нумерация — из исходного файла.
pub fn read_text(output: &crate::ReadOutput) -> String {
    use std::fmt::Write;
    let total = output.lines_total.unwrap_or_default();
    if let Some(range) = &output.range {
        return format!(
            "lines: {}-{} of {total}\n{}",
            range.from,
            range.to,
            number_lines_from(&output.text, range.from)
        );
    }
    if output.sections.is_empty() {
        return number_lines(&output.text);
    }
    let mut lines = source_lines(&output.text);
    let mut text = String::new();
    let count = output.sections.len();
    for (index, section) in output.sections.iter().enumerate() {
        if count > 1 {
            let _ = writeln!(text, "=== PART {} of {count} ===", index + 1);
        }
        let _ = writeln!(
            text,
            "heading: {}\nlines: {}-{} of {total}",
            section.heading_chain, section.from, section.to
        );
        for (offset, line) in lines
            .by_ref()
            .take(section.to - section.from + 1)
            .enumerate()
        {
            let _ = writeln!(text, "{} {line}", section.from + offset);
        }
        if count > 1 {
            let _ = writeln!(text, "=== END PART {} ===", index + 1);
        }
    }
    text.trim_end_matches('\n').to_owned()
}

/// Путь без расширения `.md` — путь папки, а не документа: `bars_read` показывает её содержимое
/// (OQ-320). Пустой путь и `.` — корень документации.
pub fn is_folder_path(path: &str) -> bool {
    let path = path.trim();
    !path
        .get(path.len().saturating_sub(3)..)
        .is_some_and(|ext| ext.eq_ignore_ascii_case(".md"))
}

/// Просмотр папки текстом для агента и CLI: шапка с числом подпапок и документов, строка на
/// подпапку или документ, очередь индексации и продолжение (OQ-320).
pub fn folder_text(listing: &crate::FolderListing) -> String {
    use std::fmt::Write;
    let mut text = format!(
        "=== FOLDER {} ===\nfolders: {} · documents: {}\n",
        listing.path, listing.folders, listing.documents
    );
    for entry in &listing.entries {
        text.push_str(&entry.name);
        if entry.is_folder {
            let _ = write!(text, " · {} documents", entry.documents);
        }
        if !entry.title.is_empty() {
            let _ = write!(text, " · {}", entry.title);
        }
        text.push('\n');
    }
    if listing.pending > 0 {
        let _ = writeln!(text, "pending: {} documents in queue", listing.pending);
    }
    if let Some(cursor) = &listing.next_cursor {
        let _ = writeln!(text, "more: repeat with cursor {cursor}");
    }
    text.push_str("=== END FOLDER ===");
    text
}

#[cfg(test)]
mod tests {
    use super::number_lines;

    #[test]
    fn lines_are_numbered_from_one() {
        assert_eq!(number_lines("# a\n\nтекст"), "1 # a\n2 \n3 текст");
    }

    #[test]
    fn empty_text_has_no_lines() {
        assert_eq!(number_lines(""), "");
    }

    #[test]
    fn crlf_does_not_leave_carriage_returns() {
        assert_eq!(number_lines("a\r\nb"), "1 a\n2 b");
        for source in ["", "a", "a\r\nb", "a\rb\r", "\r\n", "a\n\n"] {
            assert_eq!(
                super::count_lines(source),
                number_lines(source).lines().count()
            );
        }
    }

    #[test]
    fn partial_render_retains_empty_lines_and_original_section_numbers() {
        let output = crate::ReadOutput {
            text: "## Б\r\n\r\n## Б\r\nтекст".into(),
            lines_total: Some(20),
            sections: vec![
                crate::ReadSection {
                    from: 5,
                    to: 6,
                    heading_chain: "А/Б".into(),
                },
                crate::ReadSection {
                    from: 19,
                    to: 20,
                    heading_chain: "А/Б".into(),
                },
            ],
            ..Default::default()
        };
        assert_eq!(
            super::read_text(&output),
            "=== PART 1 of 2 ===\nheading: А/Б\nlines: 5-6 of 20\n5 ## Б\n6 \n=== END PART 1 ===\n=== PART 2 of 2 ===\nheading: А/Б\nlines: 19-20 of 20\n19 ## Б\n20 текст\n=== END PART 2 ==="
        );
    }
}
