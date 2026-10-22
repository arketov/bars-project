//! Разбор ссылок документа и разрешение их адресатов — общий слой графа и индекса ссылок
//! (`docs/features/knowledge-base/link-index.md`, «Общий разбор»).
//!
//! Чистые функции: вход — текст документа и набор путей, выход — ссылки в байтовых смещениях.
//! Ни диска, ни модели, ни SQLite.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};

/// Вид ссылки в тексте.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[текст](путь.md)`.
    Inline,
    /// `[текст][метка]`, `[метка][]`, `[метка]` с определением `[метка]: путь.md`.
    Reference,
    /// `[[Название]]`, `[[папка/Название|текст#раздел]]`.
    Wiki,
}

impl LinkKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::Reference => "reference",
            Self::Wiki => "wiki",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "inline" => Some(Self::Inline),
            "reference" => Some(Self::Reference),
            "wiki" => Some(Self::Wiki),
            _ => None,
        }
    }
}

/// Одна ссылка документа; смещения — в исходном тексте, включая BOM и перевод строк.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLink {
    pub kind: LinkKind,
    pub start: usize,
    pub end: usize,
    /// Строка начала ссылки, с единицы.
    pub line: u32,
    /// Адресат как написан: путь (с `#раздел`, если он есть) или текст между `[[` и `]]`.
    pub raw_target: String,
    /// Видимый текст ссылки.
    pub text: String,
    /// Часть после `#`: для обычной и ссылочной — якорь, для вики — раздел.
    pub fragment: Option<String>,
}

/// Куда ведёт ссылка по написанию, без учёта того, есть ли такой документ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Относительный путь `.md` (уже без якоря и в UTF-8).
    Path(String),
    /// Имя из вики-ссылки без `.md`.
    Name(String),
}

impl ParsedLink {
    /// Адресат-документ. `None`: внешняя ссылка, ссылка на раздел своего документа, вики без имени.
    pub fn target(&self) -> Option<Target> {
        match self.kind {
            LinkKind::Wiki => wikilink_name(&self.raw_target).map(Target::Name),
            _ => {
                let target = &self.raw_target;
                if target.contains(':') || target.starts_with("//") {
                    return None;
                }
                let file = target.split_once('#').map_or(target.as_str(), |(p, _)| p);
                (!file.is_empty()).then(|| Target::Path(file.to_string()))
            }
        }
    }
}

/// Начало тела: после BOM и frontmatter.
pub fn body_start(text: &str) -> usize {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut offset = bom;
    let mut lines = text[bom..].split_inclusive(['\r', '\n']);
    if lines.next().is_none_or(|l| l.trim_end() != "---") {
        return bom;
    }
    offset += text[bom..]
        .split_inclusive(['\r', '\n'])
        .next()
        .map_or(0, str::len);
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return offset;
        }
    }
    bom
}

/// Строка (с единицы), на которой лежит байтовое смещение.
fn line_of(text: &str, offset: usize) -> u32 {
    let lines = text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    u32::try_from(lines + 1).unwrap_or(u32::MAX)
}

/// Все ссылки документа по порядку появления: обычные и ссылочные, затем вики-ссылки.
/// Код (блок и строка) ссылок не содержит.
pub fn parse_links(text: &str) -> Vec<ParsedLink> {
    let start = body_start(text);
    let mut code: Vec<Range<usize>> = Vec::new();
    let mut found: Vec<ParsedLink> = Vec::new();
    // Открытая ссылка: диапазон, вид, адресат и накопленный текст.
    let mut open: Option<(Range<usize>, LinkKind, String, String)> = None;
    for (event, range) in Parser::new_ext(
        &text[start..],
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
    )
    .into_offset_iter()
    {
        let range = range.start + start..range.end + start;
        if matches!(event, Event::Code(_) | Event::Start(Tag::CodeBlock(_))) {
            code.push(range.clone());
        }
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let kind = match link_type {
                    LinkType::Inline | LinkType::Autolink | LinkType::Email => LinkKind::Inline,
                    _ => LinkKind::Reference,
                };
                open = Some((range, kind, dest_url.to_string(), String::new()));
            }
            Event::Text(part) | Event::Code(part) => {
                if let Some((_, _, _, label)) = &mut open {
                    label.push_str(&part);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((range, kind, target, label)) = open.take() {
                    let fragment = target.split_once('#').map(|(_, f)| f.to_string());
                    found.push(ParsedLink {
                        kind,
                        line: line_of(text, range.start),
                        start: range.start,
                        end: range.end,
                        raw_target: target,
                        text: label,
                        fragment,
                    });
                }
            }
            _ => {}
        }
    }
    let wikis = wikilinks(text, start, &code, &found);
    for (range, inner) in wikis {
        let (target_part, alias) = match inner.split_once('|') {
            Some((target, alias)) => (target, Some(alias.trim())),
            None => (inner.as_str(), None),
        };
        let fragment = target_part
            .split_once('#')
            .map(|(_, f)| f.trim().to_string())
            .filter(|f| !f.is_empty());
        let label = alias
            .filter(|a| !a.is_empty())
            .map_or_else(|| target_part.trim().to_string(), str::to_string);
        found.push(ParsedLink {
            kind: LinkKind::Wiki,
            line: line_of(text, range.start),
            start: range.start,
            end: range.end,
            raw_target: inner,
            text: label,
            fragment,
        });
    }
    found
}

/// Ссылки `[[…]]` (как в Obsidian) вне кода и вне обычных ссылок; возвращает диапазон и текст между скобками.
fn wikilinks(
    text: &str,
    start: usize,
    code: &[Range<usize>],
    links: &[ParsedLink],
) -> Vec<(Range<usize>, String)> {
    let mut found = Vec::new();
    let mut at = start;
    while let Some(open) = text[at..].find("[[") {
        let open = at + open;
        let body = open + 2;
        let line_end = text[body..].find('\n').map_or(text.len(), |n| body + n);
        let Some(close) = text[body..line_end].find("]]") else {
            at = body;
            continue;
        };
        let close = body + close;
        let inner = &text[body..close];
        let range = open..close + 2;
        at = close + 2;
        if inner.trim().is_empty() || inner.contains('[') || inner.contains(']') {
            continue;
        }
        let overlaps = |r: &Range<usize>| r.start < range.end && range.start < r.end;
        if code.iter().any(overlaps) || links.iter().any(|l| overlaps(&(l.start..l.end))) {
            continue;
        }
        found.push((range, inner.to_string()));
    }
    found
}

/// Имя документа из `Название`, `Название|текст`, `папка/Название#раздел`; `None` — ссылка на раздел этого же документа.
pub fn wikilink_name(inner: &str) -> Option<String> {
    let name = inner.split('|').next()?.split('#').next()?.trim();
    let name = name
        .strip_suffix(".md")
        .or_else(|| name.strip_suffix(".MD"))
        .unwrap_or(name);
    (!name.trim().is_empty()).then(|| name.trim().to_string())
}

/// Документ по имени из `[[…]]`: без учёта регистра; имя с `/` сверяется с путём от корня документации,
/// без `/` — с именем файла. Несколько кандидатов: в папке источника, затем самый короткий путь, затем по алфавиту.
/// `docs` — пары «путь от корня документации, что вернуть».
pub fn resolve_wikilink<'a, T>(
    docs: impl IntoIterator<Item = (&'a str, T)>,
    from: &str,
    name: &str,
) -> Option<T> {
    let name = name.replace('\\', "/").to_lowercase();
    let name = name.trim_matches('/');
    let from_dir = from.rsplit_once('/').map_or("", |(dir, _)| dir);
    docs.into_iter()
        .filter_map(|(docs_path, value)| {
            let path = docs_path.to_lowercase();
            let stem = path.strip_suffix(".md").unwrap_or(&path);
            let hit = if name.contains('/') {
                stem == name || stem.ends_with(&format!("/{name}"))
            } else {
                stem.rsplit('/').next() == Some(name)
            };
            hit.then(|| {
                let dir = docs_path.rsplit_once('/').map_or("", |(dir, _)| dir);
                ((dir != from_dir, docs_path.len(), docs_path), value)
            })
        })
        .min_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, value)| value)
}

/// Путь адресата относительно документа-источника; `None` — путь выходит за корень документации,
/// абсолютный или с схемой.
pub fn resolve_path(source: &str, target: &str) -> Option<String> {
    let target = decode_percent(target)?.replace('\\', "/");
    if target.starts_with('/') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<_> = source.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

pub fn decode_percent(text: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i] == b'%' {
            bytes.push(u8::from_str_radix(text.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            bytes.push(text.as_bytes()[i]);
            i += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

/// Набор документов проекта (пути от корня документации) для разрешения ссылок: имена вики-ссылок
/// ищутся по карте, а не обходом всех документов, поэтому разрешение десятков тысяч ссылок — миллисекунды.
#[derive(Debug, Default, Clone)]
pub struct DocSet {
    paths: std::collections::BTreeSet<String>,
    /// Имя файла без `.md` в нижнем регистре → пути с этим именем.
    by_name: std::collections::HashMap<String, Vec<String>>,
}

fn stem_of(lower_path: &str) -> &str {
    lower_path.strip_suffix(".md").unwrap_or(lower_path)
}

impl DocSet {
    pub fn new<I: IntoIterator<Item = String>>(paths: I) -> Self {
        let mut set = Self::default();
        for path in paths {
            let lower = path.to_lowercase();
            let name = stem_of(&lower).rsplit('/').next().unwrap_or("").to_string();
            set.by_name.entry(name).or_default().push(path.clone());
            set.paths.insert(path);
        }
        set
    }

    pub fn contains(&self, path: &str) -> bool {
        self.paths.contains(path)
    }

    pub fn len(&self) -> usize {
        self.paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.paths.iter().map(String::as_str)
    }

    /// То же правило, что [`resolve_wikilink`], но по карте имён.
    fn by_wikiname(&self, from: &str, name: &str) -> Option<&str> {
        let name = name.replace('\\', "/").to_lowercase();
        let name = name.trim_matches('/');
        let last = name.rsplit('/').next().unwrap_or("");
        let from_dir = from.rsplit_once('/').map_or("", |(dir, _)| dir);
        self.by_name
            .get(last)?
            .iter()
            .filter(|docs_path| {
                let lower = docs_path.to_lowercase();
                let stem = stem_of(&lower);
                !name.contains('/') || stem == name || stem.ends_with(&format!("/{name}"))
            })
            .min_by(|a, b| {
                let key = |p: &&String| {
                    let dir = p.rsplit_once('/').map_or("", |(dir, _)| dir);
                    (dir != from_dir, p.len(), p.as_str().to_string())
                };
                key(a).cmp(&key(b))
            })
            .map(String::as_str)
    }

    /// Документ, на который указывает ссылка из `from` (путь от корня документации).
    pub fn resolve<'a>(&'a self, from: &str, link: &ParsedLink) -> Resolved<'a> {
        match link.target() {
            None => Resolved::NotALink,
            Some(Target::Name(name)) => match self.by_wikiname(from, &name) {
                Some(path) => Resolved::Found(path),
                None => Resolved::Broken,
            },
            Some(Target::Path(file)) => match resolve_path(from, &file) {
                // Выход за корень документации — не документ проекта.
                None => Resolved::NotALink,
                Some(path) if !is_markdown(&path) => Resolved::NotALink,
                Some(path) => match self.paths.get(&path) {
                    Some(found) => Resolved::Found(found.as_str()),
                    None => Resolved::Broken,
                },
            },
        }
    }
}

fn is_markdown(path: &str) -> bool {
    path.len() > 3 && path[path.len() - 3..].eq_ignore_ascii_case(".md")
}

/// Итог разрешения ссылки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolved<'a> {
    /// Ведёт на документ проекта.
    Found(&'a str),
    /// Адресат — `.md` или имя, но такого документа нет.
    Broken,
    /// Не ссылка на документ: внешняя, на раздел своего документа, не `.md`, вне корня.
    NotALink,
}

#[cfg(test)]
mod tests;
