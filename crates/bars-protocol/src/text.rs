//! Вывод, одинаковый у `bars-mcp` и `bars-cli` (Т-20).

/// Ставит номера строк в том же виде, что у `outline`: `12 текст строки`. Нумерация с единицы.
pub fn number_lines(text: &str) -> String {
    text.lines()
        .enumerate()
        .map(|(index, line)| format!("{} {line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
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
    }
}
