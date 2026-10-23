//! План правки ссылок без диска: адресаты до и после перемещения, точечные замены.
use super::*;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub start: usize,
    pub end: usize,
    pub replacement: String,
}

#[derive(Debug, Clone, Default)]
pub struct Rewrite {
    pub edits: Vec<Edit>,
    /// Строки ссылок, меняющих адресат из-за нового набора имён (включая старые битые).
    pub warnings: Vec<u32>,
    pub invalid: Vec<u32>,
}

pub fn mapped(path: &str, from: &str, to: &str) -> String {
    if path == from {
        to.to_string()
    } else if let Some(rest) = path.strip_prefix(&format!("{from}/")) {
        format!("{to}/{rest}")
    } else {
        path.to_string()
    }
}

pub fn apply(text: &str, edits: &[Edit]) -> String {
    let mut out = text.to_string();
    for edit in edits.iter().rev() {
        out.replace_range(edit.start..edit.end, &edit.replacement);
    }
    out
}

fn relative(source: &str, target: &str) -> String {
    let mut a: Vec<_> = source.split('/').collect();
    a.pop();
    let b: Vec<_> = target.split('/').collect();
    let common = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    let mut parts = vec![".."; a.len() - common];
    parts.extend_from_slice(&b[common..]);
    parts.join("/")
}

// Уникальность проверяется по всем суффиксам, а не по текущему правилу выбора соседа.
fn wiki_name(target: &str, after: &DocSet) -> String {
    let stem = &target[..target.len() - 3];
    let parts: Vec<_> = stem.split('/').collect();
    for count in 1..=parts.len() {
        let name = parts[parts.len() - count..].join("/");
        let lower = name.to_lowercase();
        if after
            .iter()
            .filter(|p| {
                let p = p[..p.len() - 3].to_lowercase();
                p == lower || p.ends_with(&format!("/{lower}"))
            })
            .count()
            == 1
        {
            return name;
        }
    }
    stem.to_string()
}

/// Диапазон адреса после `](` или `]:`, без угловых скобок и title.
fn destination(text: &str, start: usize, end: usize, reference: bool) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut pos = start;
    let mut depth = 0usize;
    while pos + 1 < end {
        match bytes[pos] {
            b'\\' => {
                pos += 2;
                continue;
            }
            b'[' => depth += 1,
            b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 && bytes[pos + 1] == if reference { b':' } else { b'(' } {
                    pos += 2;
                    break;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    while pos < end && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    if pos >= end {
        return None;
    }
    let angle = bytes[pos] == b'<';
    if angle {
        pos += 1;
    }
    let begin = pos;
    let mut parens = 0usize;
    while pos < end {
        match bytes[pos] {
            b'\\' => {
                pos += 2;
                continue;
            }
            b'>' if angle => break,
            b'(' if !angle => parens += 1,
            b')' if !angle && parens == 0 => break,
            b')' if !angle => parens -= 1,
            b if !angle && b.is_ascii_whitespace() => break,
            _ => {}
        }
        pos += 1;
    }
    (begin < pos && pos <= end).then_some(begin..pos)
}

fn encode_path(path: &str) -> String {
    let mut out = String::new();
    for ch in path.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '%' => out.push_str("%25"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            '\\' => out.push_str("%5C"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn plan(text: &str, source: &str, before: &DocSet, from: &str, to: &str) -> Rewrite {
    let after = DocSet::new(before.iter().map(|p| mapped(p, from, to)));
    plan_with_sets(text, source, before, &after, from, to)
}

pub fn plan_with_sets(
    text: &str,
    source: &str,
    before: &DocSet,
    after: &DocSet,
    from: &str,
    to: &str,
) -> Rewrite {
    let new_source = mapped(source, from, to);
    let body = body_start(text);
    let parser = Parser::new_ext(&text[body..], Options::all());
    let definitions: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, d)| (d.dest.to_string(), d.span.start + body..d.span.end + body))
        .collect();
    let mut edits = BTreeMap::new();
    let mut warnings = Vec::new();
    for link in parse_links(text) {
        let old = before.resolve(source, &link);
        let new = after.resolve(&new_source, &link);
        let Resolved::Found(target) = old else {
            if old == Resolved::Broken && matches!(new, Resolved::Found(_)) {
                warnings.push(link.line);
            }
            continue;
        };
        let expected = mapped(target, from, to);
        if new == Resolved::Found(&expected) {
            continue;
        }
        if source == new_source && target == expected {
            warnings.push(link.line);
        }
        let (range, replacement) = if link.kind == LinkKind::Wiki {
            let inner = &text[link.start + 2..link.end - 2];
            let suffix_at = inner.find(['#', '|']).unwrap_or(inner.len());
            (
                link.start + 2..link.end - 2,
                format!("{}{}", wiki_name(&expected, after), &inner[suffix_at..]),
            )
        } else {
            let range = if link.kind == LinkKind::Reference {
                definitions
                    .iter()
                    .filter(|(dest, _)| dest == &link.raw_target)
                    .filter_map(|(_, span)| destination(text, span.start, span.end, true))
                    .collect::<Vec<_>>()
            } else {
                destination(text, link.start, link.end, false)
                    .into_iter()
                    .collect()
            };
            let replacement = format!(
                "{}{}",
                encode_path(&relative(&new_source, &expected)),
                link.fragment
                    .as_ref()
                    .map_or(String::new(), |f| format!("#{f}"))
            );
            for span in range {
                edits.insert((span.start, span.end), replacement.clone());
            }
            continue;
        };
        edits.insert((range.start, range.end), replacement);
    }
    let edits: Vec<Edit> = edits
        .into_iter()
        .map(|((start, end), replacement)| Edit {
            start,
            end,
            replacement,
        })
        .collect();
    let rewritten = apply(text, &edits);
    let original = parse_links(text);
    let updated = parse_links(&rewritten);
    let mut invalid = Vec::new();
    if original.len() != updated.len() {
        invalid.extend(original.iter().map(|link| link.line));
    } else {
        for (old, new) in original.iter().zip(&updated) {
            if let Resolved::Found(target) = before.resolve(source, old) {
                let expected = mapped(target, from, to);
                if after.resolve(&new_source, new) != Resolved::Found(&expected) {
                    invalid.push(old.line);
                }
            }
        }
    }
    Rewrite {
        edits,
        warnings,
        invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rewrite(text: &str, source: &str, paths: &[&str], from: &str, to: &str) -> String {
        apply(
            text,
            &plan(
                text,
                source,
                &DocSet::new(paths.iter().map(|p| p.to_string())),
                from,
                to,
            )
            .edits,
        )
    }
    #[test]
    fn preserves_markup_code_titles_aliases_and_fragments() {
        let text = "---\ntitle: Ссылка\n---\n[**текст**](a.md#якорь \"заголовок\") [[a#раздел|подпись]] `[[a]]`\n```\n[x](a.md)\n```\n";
        assert_eq!(
            rewrite(text, "x.md", &["x.md", "a.md"], "a.md", "новая тема.md"),
            text.replace("a.md#якорь", "новая%20тема.md#якорь")
                .replace("[[a#раздел|подпись]]", "[[новая тема#раздел|подпись]]")
        );
    }
    #[test]
    fn folder_internal_links_stay_and_crossing_links_change() {
        let paths = ["a/x.md", "a/y.md", "z.md"];
        assert_eq!(
            rewrite("[y](y.md) [z](../z.md)", "a/x.md", &paths, "a", "b/c"),
            "[y](y.md) [z](../../z.md)"
        );
        assert_eq!(
            rewrite("[x](a/x.md)", "z.md", &paths, "a", "b/c"),
            "[x](b/c/x.md)"
        );
    }
    #[test]
    fn references_edit_definition_once_keep_label_and_title() {
        assert_eq!(
            rewrite(
                "[a][r] [r][]\n\n[r]: <old.md#x> 'title'\n",
                "x.md",
                &["x.md", "old.md"],
                "old.md",
                "new.md"
            ),
            "[a][r] [r][]\n\n[r]: <new.md#x> 'title'\n"
        );
    }
    #[test]
    fn protects_unrelated_same_name_and_reports_capture() {
        let set = DocSet::new(["a/x.md", "a/item.md", "b/item.md"].map(str::to_string));
        let text = "[[item|подпись]]";
        let p = plan(text, "a/x.md", &set, "b/item.md", "item.md");
        assert!(p.edits.is_empty());
        let p = plan(text, "a/x.md", &set, "a/item.md", "b/new.md");
        assert_eq!(apply(text, &p.edits), "[[new|подпись]]");
        let p = plan(text, "a/x.md", &set, "b/item.md", "a/new.md");
        assert!(p.edits.is_empty());
    }
    #[test]
    fn broken_link_becoming_valid_is_warning_not_rewritten() {
        let set = DocSet::new(["x.md", "old.md"].map(str::to_string));
        let p = plan("[[new]]", "x.md", &set, "old.md", "new.md");
        assert_eq!(p.warnings, vec![1]);
        assert!(p.edits.is_empty());
    }

    #[test]
    fn capture_of_unmoved_target_is_repaired_and_warned() {
        let set = DocSet::new(["a/x.md", "b/item.md", "c/other.md"].map(str::to_string));
        let p = plan("[[item]]", "a/x.md", &set, "c/other.md", "a/item.md");
        assert_eq!(p.warnings, vec![1]);
        assert_eq!(apply("[[item]]", &p.edits), "[[b/item]]");
        assert!(p.invalid.is_empty());
    }

    #[test]
    fn target_with_wiki_delimiter_refuses_invalid_plan() {
        let set = DocSet::new(["x.md", "a.md"].map(str::to_string));
        let p = plan("[[a|подпись]]", "x.md", &set, "a.md", "a#b.md");
        assert_eq!(p.invalid, vec![1]);
    }
}
