//! Пункт 6.3.1: экспериментальная нарезка реестров только для бенча (OQ-178, OQ-179).
//! B — куски таблиц и списков с номерами не длиннее окна rerank, шапка таблицы повторяется;
//! C — строка таблицы или пункт списка отдельным чанком. Остальной текст чанка не меняется.
use bars_core::indexing::chunking::Chunk;

/// Окно rerank в знаках: 512 токенов пары «запрос + путь + цепочка + текст».
pub const WINDOW: usize = 1500;
/// Сколько подряд пунктов с номером делают список реестром; таблице хватает двух строк.
const MIN_ITEMS: usize = 3;

#[derive(Debug, Clone, PartialEq)]
enum Unit {
    /// Строки обычного текста вместе с пустыми.
    Text(Vec<String>),
    /// Строка таблицы и шапка её таблицы (две строки).
    Row { header: String, line: String },
    /// Пункт нумерованного списка с продолжением.
    Item(Vec<String>),
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

fn is_table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn is_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.contains('-') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// Пункт списка, который начинается с номера: `- **Т-25.**`, `- [ ] №30`, `1. ISS-18`.
fn is_numbered_item(line: &str) -> bool {
    let t = line.trim_start();
    let rest = if let Some(r) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
        r
    } else {
        let digits = t.chars().take_while(char::is_ascii_digit).count();
        match t[digits..].strip_prefix(". ") {
            Some(r) if digits > 0 => r,
            _ => return false,
        }
    };
    let rest = match rest.strip_prefix('[') {
        Some(r) if r.chars().nth(1) == Some(']') => r.chars().skip(2).collect::<String>(),
        _ => rest.to_string(),
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("**").unwrap_or(rest);
    if let Some(r) = rest.strip_prefix('№') {
        return r.trim_start().starts_with(|c: char| c.is_ascii_digit());
    }
    let letters = rest.chars().take_while(|c| c.is_alphabetic()).count();
    let tail: String = rest.chars().skip(letters).take(2).collect();
    (1..=8).contains(&letters)
        && tail.starts_with('-')
        && tail.chars().nth(1).is_some_and(|c| c.is_ascii_digit())
}

fn is_list_start(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("- ")
        || t.starts_with("* ")
        || (t.starts_with(|c: char| c.is_ascii_digit()) && t.contains(". "))
}

/// Делит текст чанка на обычный текст и строки реестров, сохраняя порядок строк.
fn units(text: &str) -> Vec<Unit> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<Unit> = Vec::new();
    let mut plain: Vec<String> = Vec::new();
    let flush = |plain: &mut Vec<String>, out: &mut Vec<Unit>| {
        if !plain.is_empty() {
            out.push(Unit::Text(std::mem::take(plain)));
        }
    };
    let mut i = 0;
    while i < lines.len() {
        if is_table_line(lines[i]) {
            let end = (i..lines.len())
                .find(|&j| !is_table_line(lines[j]))
                .unwrap_or(lines.len());
            if end - i >= 4 && is_separator(lines[i + 1]) {
                flush(&mut plain, &mut out);
                let header = format!("{}\n{}", lines[i], lines[i + 1]);
                for line in &lines[i + 2..end] {
                    out.push(Unit::Row {
                        header: header.clone(),
                        line: (*line).to_string(),
                    });
                }
            } else {
                plain.extend(lines[i..end].iter().map(|l| (*l).to_string()));
            }
            i = end;
            continue;
        }
        if is_numbered_item(lines[i]) {
            // Пункты подряд: продолжение — строки с отступом, пункт без номера тоже входит.
            let mut items: Vec<Vec<String>> = Vec::new();
            let mut j = i;
            while j < lines.len() {
                let line = lines[j];
                let indented = line.starts_with(' ') || line.starts_with('\t');
                if !indented && is_list_start(line) {
                    items.push(vec![line.to_string()]);
                } else if indented && !line.trim().is_empty() && !items.is_empty() {
                    if let Some(last) = items.last_mut() {
                        last.push(line.to_string());
                    }
                } else {
                    break;
                }
                j += 1;
            }
            let numbered = items.iter().filter(|it| is_numbered_item(&it[0])).count();
            if numbered >= MIN_ITEMS {
                flush(&mut plain, &mut out);
                out.extend(items.into_iter().map(Unit::Item));
            } else {
                plain.extend(lines[i..j].iter().map(|l| (*l).to_string()));
            }
            i = j.max(i + 1);
            continue;
        }
        plain.push(lines[i].to_string());
        i += 1;
    }
    flush(&mut plain, &mut out);
    out
}

fn has_registry(units: &[Unit]) -> bool {
    units.iter().any(|u| !matches!(u, Unit::Text(_)))
}

fn trim_blank(lines: &[String]) -> String {
    let start = lines.iter().position(|l| !l.trim().is_empty());
    let end = lines.iter().rposition(|l| !l.trim().is_empty());
    match (start, end) {
        (Some(s), Some(e)) => lines[s..=e].join("\n"),
        _ => String::new(),
    }
}

/// C: каждая строка реестра отдельно; обычный текст чанка — одним куском.
fn variant_c(units: &[Unit]) -> Vec<String> {
    let mut plain: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for unit in units {
        match unit {
            Unit::Text(lines) => {
                for line in lines {
                    // Пустые строки вокруг вынутой таблицы не удваиваются.
                    let blank = line.trim().is_empty();
                    if !(blank && plain.last().is_some_and(|l| l.trim().is_empty())) {
                        plain.push(line.clone());
                    }
                }
            }
            Unit::Row { header, line } => rows.push(format!("{header}\n{line}")),
            Unit::Item(lines) => rows.push(lines.join("\n")),
        }
    }
    let mut out = Vec::new();
    let rest = trim_blank(&plain);
    if !rest.is_empty() {
        out.push(rest);
    }
    out.extend(rows);
    out
}

/// B: жадная склейка по порядку в куски не длиннее окна; строка таблицы в куске без её шапки
/// получает шапку. Обычный текст режется по абзацам.
fn variant_b(units: &[Unit], window: usize) -> Vec<String> {
    let mut parts: Vec<(Option<&str>, Vec<String>)> = Vec::new();
    for unit in units {
        match unit {
            Unit::Text(lines) => {
                let mut paragraph = Vec::new();
                for line in lines {
                    if line.trim().is_empty() {
                        if !paragraph.is_empty() {
                            parts.push((None, std::mem::take(&mut paragraph)));
                        }
                    } else {
                        paragraph.push(line.clone());
                    }
                }
                if !paragraph.is_empty() {
                    parts.push((None, paragraph));
                }
            }
            Unit::Row { header, line } => parts.push((Some(header), vec![line.clone()])),
            Unit::Item(lines) => parts.push((None, lines.clone())),
        }
    }
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut header_in: Option<&str> = None;
    let mut len = 0;
    for (header, lines) in parts {
        let body = lines.join("\n");
        let needs_header = header.is_some() && header != header_in;
        let added = chars(&body)
            + header.filter(|_| needs_header).map_or(0, |h| chars(h) + 1)
            + usize::from(!current.is_empty());
        if !current.is_empty() && len + added > window {
            out.push(current.join("\n"));
            current.clear();
            header_in = None;
            len = 0;
        }
        if header.is_some() && header != header_in {
            if let Some(h) = header {
                current.push(h.to_string());
                len += chars(h) + usize::from(len > 0);
            }
            header_in = header;
        } else if header.is_none() {
            header_in = None;
        }
        len += chars(&body) + usize::from(len > 0);
        current.push(body);
    }
    if !current.is_empty() {
        out.push(current.join("\n"));
    }
    out
}

/// Переделывает чанки документа по варианту `B` или `C`; другие варианты не меняют чанки.
/// Номера строк кусков приблизительны: строка текста чанка считается строкой файла от начала
/// чанка; метрики бенча строки не используют.
pub fn rechunk(path: &str, chunks: &[Chunk], variant: &str) -> Vec<Chunk> {
    let mut out = Vec::new();
    for chunk in chunks {
        let parsed = units(&chunk.text);
        let texts = match variant {
            "B" if has_registry(&parsed) && chars(&chunk.text) > WINDOW => {
                variant_b(&parsed, WINDOW)
            }
            "C" if has_registry(&parsed) => variant_c(&parsed),
            _ => {
                out.push(chunk.clone());
                continue;
            }
        };
        let source_lines: Vec<&str> = chunk.text.split('\n').collect();
        for text in texts {
            let first = text.split('\n').next().unwrap_or("");
            let offset = source_lines.iter().position(|l| *l == first).unwrap_or(0);
            let first_line = (chunk.first_line + offset).min(chunk.last_line);
            let last_line = (first_line + text.matches('\n').count()).min(chunk.last_line);
            let embed_text = format!("path: {path}\n{}\n{text}", chunk.heading_chain);
            out.push(Chunk {
                ordinal: 0,
                heading_chain: chunk.heading_chain.clone(),
                first_line,
                last_line,
                embed_hash: blake3::hash(embed_text.as_bytes()).to_hex().to_string(),
                text,
                embed_text,
            });
        }
    }
    for (ordinal, chunk) in out.iter_mut().enumerate() {
        chunk.ordinal = ordinal;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(text: &str) -> Chunk {
        Chunk {
            ordinal: 0,
            heading_chain: "Док/Реестр".into(),
            first_line: 10,
            last_line: 10 + text.matches('\n').count(),
            text: text.into(),
            embed_text: String::new(),
            embed_hash: String::new(),
        }
    }

    fn table(rows: usize, width: usize) -> String {
        let mut t = String::from("Вступление.\n\n| ID | Что |\n|---|---|");
        for i in 0..rows {
            t.push_str(&format!("\n| ID-{i} | {} |", "х".repeat(width)));
        }
        t.push_str("\n\nХвост.");
        t
    }

    #[test]
    fn numbered_items_are_recognised() {
        assert!(is_numbered_item("- **Т-25.** Если embed"));
        assert!(is_numbered_item("- [ ] №30 — Часть ботов"));
        assert!(is_numbered_item("1. ISS-18 текст"));
        assert!(!is_numbered_item("- обычный пункт"));
        assert!(!is_numbered_item("- **Важно.** текст"));
    }

    #[test]
    fn c_puts_each_row_with_header_into_own_chunk() {
        let out = rechunk("docs/a.md", &[chunk(&table(4, 10))], "C");
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].text, "Вступление.\n\nХвост.");
        assert_eq!(
            out[2].text,
            format!("| ID | Что |\n|---|---|\n| ID-1 | {} |", "х".repeat(10))
        );
        assert!(out.iter().enumerate().all(|(i, c)| c.ordinal == i));
        assert!(
            out[1]
                .embed_text
                .starts_with("path: docs/a.md\nДок/Реестр\n")
        );
    }

    #[test]
    fn b_keeps_short_chunks_and_repeats_header_in_long_ones() {
        let short = chunk(&table(3, 10));
        assert_eq!(
            rechunk("docs/a.md", std::slice::from_ref(&short), "B"),
            vec![short]
        );
        let out = rechunk("docs/a.md", &[chunk(&table(40, 100))], "B");
        assert!(out.len() > 2);
        assert!(out.iter().all(|c| chars(&c.text) <= WINDOW));
        for c in out.iter().filter(|c| c.text.contains("| ID-")) {
            assert!(c.text.contains("| ID | Что |\n|---|---|"), "{}", c.text);
        }
        let rows: usize = out.iter().map(|c| c.text.matches("| ID-").count()).sum();
        assert_eq!(rows, 40);
    }

    #[test]
    fn numbered_list_is_split_by_items_with_continuation() {
        let text =
            "Требования:\n\n- **Т-1.** первый\n  продолжение\n- **Т-2.** второй\n- **Т-3.** третий";
        let out = rechunk("docs/a.md", &[chunk(text)], "C");
        assert_eq!(out.len(), 4);
        assert_eq!(out[1].text, "- **Т-1.** первый\n  продолжение");
    }

    #[test]
    fn other_variants_and_plain_text_are_unchanged() {
        let plain = chunk("Просто текст.\n\n- пункт\n- пункт\n- пункт");
        assert_eq!(
            rechunk("docs/a.md", std::slice::from_ref(&plain), "C"),
            vec![plain.clone()]
        );
        let t = chunk(&table(40, 100));
        assert_eq!(rechunk("docs/a.md", std::slice::from_ref(&t), "A"), vec![t]);
    }
}
