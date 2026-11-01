//! Вариант B (OQ-159): короткие части реестра с шапкой и точными исходными диапазонами.

use std::ops::Range;

use super::{Atom, BLANK, Fence, Piece, atoms, char_len, join_lines, source_range};

pub(super) const WINDOW: usize = 1500;

enum Kind {
    Text,
    Table,
    List(Vec<Range<usize>>),
}

struct Block {
    lines: Range<usize>,
    kind: Kind,
}

fn list_marker(line: &str) -> Option<&str> {
    let text = line.trim_start();
    if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")) {
        return Some(rest);
    }
    let digits = text.bytes().take_while(u8::is_ascii_digit).count();
    (digits > 0)
        .then(|| text[digits..].strip_prefix(". "))
        .flatten()
}

fn numbered(line: &str) -> bool {
    let Some(mut rest) = list_marker(line) else {
        return false;
    };
    for checkbox in ["[ ]", "[x]", "[X]"] {
        if let Some(tail) = rest.strip_prefix(checkbox) {
            rest = tail.trim_start();
            break;
        }
    }
    rest = rest
        .trim_start()
        .strip_prefix("**")
        .unwrap_or(rest.trim_start());
    if let Some(tail) = rest.strip_prefix('№') {
        return tail
            .trim_start()
            .starts_with(|ch: char| ch.is_ascii_digit());
    }
    let letters = rest.chars().take_while(|ch| ch.is_alphabetic()).count();
    let tail: String = rest.chars().skip(letters).take(2).collect();
    (1..=8).contains(&letters)
        && tail.starts_with('-')
        && tail.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit())
}

fn table_line(line: &str) -> bool {
    line.trim_start().starts_with('|')
}

fn separator(line: &str) -> bool {
    let text = line.trim();
    text.starts_with('|')
        && text.trim_matches('|').split('|').all(|cell| {
            let cell = cell.trim().trim_matches(':');
            cell.len() >= 3 && cell.bytes().all(|ch| ch == b'-')
        })
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// Один проход; огороженные примеры не превращаются в реестры.
fn blocks(body: &[(usize, &str)]) -> Vec<Block> {
    let mut fence = Fence::default();
    let fenced: Vec<bool> = body.iter().map(|(_, line)| fence.step(line)).collect();
    let mut result = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < body.len() {
        if fenced[i] {
            i += 1;
            continue;
        }
        let mut end = i;
        let kind = if table_line(body[i].1) {
            while end < body.len() && !fenced[end] && table_line(body[end].1) {
                end += 1;
            }
            if end - i >= 4 && separator(body[i + 1].1) {
                Some(Kind::Table)
            } else {
                None
            }
        } else if numbered(body[i].1) && indent(body[i].1) <= 3 {
            let base = indent(body[i].1);
            let mut items = Vec::new();
            while end < body.len()
                && !fenced[end]
                && indent(body[end].1) == base
                && numbered(body[end].1)
            {
                let start = end;
                end += 1;
                while end < body.len()
                    && !body[end].1.trim().is_empty()
                    && indent(body[end].1) > base
                {
                    end += 1;
                }
                items.push(start..end);
            }
            (items.len() >= 3).then_some(Kind::List(items))
        } else {
            None
        };
        if let Some(kind) = kind {
            if plain < i {
                result.push(Block {
                    lines: plain..i,
                    kind: Kind::Text,
                });
            }
            result.push(Block {
                lines: i..end,
                kind,
            });
            plain = end;
        }
        i = end.max(i + 1);
    }
    if plain < body.len() {
        result.push(Block {
            lines: plain..body.len(),
            kind: Kind::Text,
        });
    }
    result
}

struct Part {
    atom: Atom,
    header: Option<usize>,
}

pub(super) fn split(body: &[(usize, &str)], max: usize) -> Option<Vec<Piece>> {
    let blocks = blocks(body);
    if blocks.iter().all(|block| matches!(block.kind, Kind::Text)) {
        return None;
    }
    let max = max.min(WINDOW);
    let whole = join_lines(body);
    if char_len(&whole) <= max {
        return Some(vec![Piece {
            source_spans: body
                .iter()
                .filter(|(_, line)| !line.is_empty())
                .map(|(_, line)| source_range(line))
                .collect(),
            first_line: body[0].0,
            last_line: body[body.len() - 1].0,
            text: whole,
        }]);
    }
    let mut headers = Vec::new();
    let mut parts = Vec::new();
    for block in blocks {
        let lines = &body[block.lines];
        match block.kind {
            Kind::Text => parts.extend(
                atoms(lines, max)
                    .into_iter()
                    .map(|atom| Part { atom, header: None }),
            ),
            Kind::List(items) => {
                for item in items {
                    parts.extend(
                        atoms(&body[item], max)
                            .into_iter()
                            .map(|atom| Part { atom, header: None }),
                    );
                }
            }
            Kind::Table => {
                let header = join_lines(&lines[..2]);
                let overhead = char_len(&header) + 1;
                if overhead >= max {
                    parts.extend(
                        atoms(lines, max)
                            .into_iter()
                            .map(|atom| Part { atom, header: None }),
                    );
                    continue;
                }
                let id = headers.len();
                headers.push(header);
                for (row, line) in lines[2..].iter().enumerate() {
                    for (segment, mut atom) in atoms(std::slice::from_ref(line), max - overhead)
                        .into_iter()
                        .enumerate()
                    {
                        if row == 0 && segment == 0 {
                            atom.first_line = lines[0].0;
                            atom.source_spans.splice(
                                0..0,
                                lines[..2].iter().map(|(_, text)| source_range(text)),
                            );
                        }
                        if segment == 0 {
                            atom.separator = if row == 0 { BLANK } else { "\n" };
                        }
                        parts.push(Part {
                            atom,
                            header: Some(id),
                        });
                    }
                }
            }
        }
    }
    Some(pack(parts, &headers, max))
}

fn pack(parts: Vec<Part>, headers: &[String], max: usize) -> Vec<Piece> {
    let mut result = Vec::new();
    let mut current: Option<Piece> = None;
    let mut len = 0;
    let mut active_header = None;
    for Part { atom, header } in parts {
        let prefix = |active| {
            header
                .filter(|id| Some(*id) != active)
                .map(|id| headers[id].as_str())
        };
        let added = char_len(&atom.text)
            + prefix(active_header).map_or(0, |s| char_len(s) + 1)
            + current.as_ref().map_or(0, |_| char_len(atom.separator));
        if current.is_some() && len + added > max {
            result.push(current.take().unwrap());
            len = 0;
            active_header = None;
        }
        let piece = current.get_or_insert_with(|| Piece {
            source_spans: Vec::new(),
            first_line: atom.first_line,
            last_line: atom.last_line,
            text: String::new(),
        });
        if !piece.text.is_empty() {
            piece.text.push_str(atom.separator);
            len += char_len(atom.separator);
        }
        if let Some(text) = prefix(active_header) {
            piece.text.push_str(text);
            piece.text.push('\n');
            len += char_len(text) + 1;
        }
        piece.text.push_str(&atom.text);
        piece.last_line = atom.last_line;
        piece.source_spans.extend(atom.source_spans);
        len += char_len(&atom.text);
        active_header = header;
    }
    result.extend(current);
    result
}

#[cfg(test)]
mod tests;
