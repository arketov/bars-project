//! Нарезка Markdown-документа на чанки (`docs/indexing/chunking.md`, Т-11, Т-18, Т-23).
//!
//! Чистая функция: текст файла и параметры на входе, чанки на выходе. Ни диска,
//! ни базы, ни модели.

/// Заголовок списка подзаголовков в конце чанка родительского раздела.
const OTHER_HEADERS: &str = "other headers:";

/// Разделитель между блоками текста чанка: пустая строка.
const BLANK: &str = "\n\n";

/// Размеры чанка в символах Unicode (Т-18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkingParams {
    /// Раздел короче склеивается со следующим.
    pub min_chars: usize,
    /// Раздел длиннее режется на несколько чанков.
    pub max_chars: usize,
}

impl Default for ChunkingParams {
    fn default() -> Self {
        Self {
            min_chars: 250,
            max_chars: 6000,
        }
    }
}

/// Один чанк документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Порядковый номер чанка в документе, с нуля.
    pub ordinal: usize,
    /// Цепочка заголовков через `/`; у вступления — заголовок документа.
    pub heading_chain: String,
    /// Первая строка чанка в исходном файле, с единицы, вместе с frontmatter.
    pub first_line: usize,
    /// Последняя строка чанка в исходном файле.
    pub last_line: usize,
    /// Текст для выдачи: текст раздела и, если нужно, список подзаголовков.
    pub text: String,
    /// Текст для эмбеддинга: `path:`, цепочка и текст, без номера строки (OQ-19).
    pub embed_text: String,
    /// Хеш blake3 текста для эмбеддинга, шестнадцатеричный.
    pub embed_hash: String,
}

/// Результат нарезки одного документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkedDocument {
    /// `title` из frontmatter или имя файла без `.md` (Т-11).
    pub title: String,
    pub chunks: Vec<Chunk>,
}

/// Режет документ на чанки. `path` — путь от корня проекта, он входит в текст для эмбеддинга.
pub fn chunk_document(path: &str, source: &str, params: &ChunkingParams) -> ChunkedDocument {
    let max = params.max_chars.max(1);
    let min = params.min_chars.min(max);

    let normalized = normalize(source);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let (frontmatter_title, body_start) = split_frontmatter(&lines);
    let title = frontmatter_title.unwrap_or_else(|| file_stem(path));

    let sections = parse_sections(&lines, body_start, &title);
    let drafts = assemble(&sections, min, max);

    let chunks = drafts
        .into_iter()
        .enumerate()
        .map(|(ordinal, draft)| draft.into_chunk(ordinal, path))
        .collect();
    ChunkedDocument { title, chunks }
}

/// Отбрасывает BOM и приводит `CRLF` и `CR` к `LF`.
pub(crate) fn normalize(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    source.replace("\r\n", "\n").replace('\r', "\n")
}

/// Имя файла без `.md`: заголовок документа, у которого нет `title`.
fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.len().checked_sub(3) {
        Some(cut) if name.is_char_boundary(cut) && name[cut..].eq_ignore_ascii_case(".md") => {
            name[..cut].to_string()
        }
        _ => name.to_string(),
    }
}

/// Возвращает `title` из frontmatter и индекс первой строки после него.
pub(crate) fn split_frontmatter(lines: &[&str]) -> (Option<String>, usize) {
    if lines.first().is_none_or(|line| line.trim_end() != "---") {
        return (None, 0);
    }
    let Some(close) = lines
        .iter()
        .skip(1)
        .position(|line| matches!(line.trim_end(), "---" | "..."))
        .map(|position| position + 1)
    else {
        return (None, 0);
    };
    let title = lines[1..close].iter().find_map(|line| {
        let value = line.strip_prefix("title:")?.trim();
        let unquoted = ['"', '\'']
            .iter()
            .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote))
            .unwrap_or(value)
            .trim();
        (!unquoted.is_empty()).then(|| unquoted.to_string())
    });
    (title, close + 1)
}

/// Огороженный блок кода: внутри него нет заголовков, и пустая строка не разделяет абзацы.
#[derive(Default)]
pub(crate) struct Fence {
    open: Option<(char, usize)>,
}

impl Fence {
    /// Скармливает очередную строку; `true`, если строка относится к огороженному блоку
    /// (включая сами ограждающие строки).
    pub(crate) fn step(&mut self, line: &str) -> bool {
        let trimmed = line.trim_start_matches(' ');
        let marker = if line.len() - trimmed.len() <= 3 {
            fence_marker(trimmed)
        } else {
            None
        };
        match (self.open, marker) {
            (Some((ch, len)), Some((c, n, rest))) => {
                if c == ch && n >= len && rest.trim().is_empty() {
                    self.open = None;
                }
                true
            }
            (Some(_), None) => true,
            (None, Some((c, n, rest))) if !(c == '`' && rest.contains('`')) => {
                self.open = Some((c, n));
                true
            }
            (None, _) => false,
        }
    }
}

/// Символ ограждения, его длина и остаток строки, если строка начинается с ```` ``` ```` или `~~~`.
fn fence_marker(line: &str) -> Option<(char, usize, &str)> {
    let ch = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let count = line.chars().take_while(|c| *c == ch).count();
    (count >= 3).then(|| (ch, count, &line[count..]))
}

/// ATX-заголовок: уровень и текст. Заголовок без текста заголовком не считается.
pub(crate) fn parse_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    let mut text = rest.trim();
    let without_closing = text.trim_end_matches('#');
    if without_closing.len() != text.len()
        && (without_closing.is_empty() || without_closing.ends_with([' ', '\t']))
    {
        text = without_closing.trim_end();
    }
    (!text.is_empty()).then(|| (level, text.to_string()))
}

/// Раздел документа: заголовок и собственный текст до следующего заголовка любого уровня.
struct Section<'a> {
    /// 0 у вступления, иначе число решёток.
    level: usize,
    title: String,
    chain: String,
    /// Строка заголовка; у вступления — первая непустая строка.
    first_line: usize,
    /// Строки собственного текста: номер строки и текст, без пустых строк по краям.
    body: Vec<(usize, &'a str)>,
    /// Прямые подзаголовки: уровень и текст.
    children: Vec<(usize, String)>,
}

impl Section<'_> {
    /// Строка заголовка с решётками; у вступления пустая.
    fn heading_text(&self) -> String {
        if self.level == 0 {
            String::new()
        } else {
            format!("{} {}", "#".repeat(self.level), self.title)
        }
    }
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

/// Строит разделы документа по порядку. Вступление без текста не даёт раздела.
fn parse_sections<'a>(lines: &[&'a str], body_start: usize, doc_title: &str) -> Vec<Section<'a>> {
    let mut sections = vec![Section {
        level: 0,
        title: String::new(),
        chain: doc_title.to_string(),
        first_line: 0,
        body: Vec::new(),
        children: Vec::new(),
    }];
    // Открытые заголовки: уровень, текст, индекс раздела.
    let mut stack: Vec<(usize, String, usize)> = Vec::new();
    let mut fence = Fence::default();

    for (index, line) in lines.iter().enumerate().skip(body_start) {
        let line_no = index + 1;
        let in_fence = fence.step(line);
        let heading = if in_fence { None } else { parse_heading(line) };
        let Some((level, title)) = heading else {
            if let Some(section) = sections.last_mut() {
                section.body.push((line_no, *line));
            }
            continue;
        };

        while stack.last().is_some_and(|(open, _, _)| *open >= level) {
            stack.pop();
        }
        if let Some((_, _, parent)) = stack.last() {
            sections[*parent].children.push((level, title.clone()));
        }
        let chain = stack
            .iter()
            .map(|(_, text, _)| text.as_str())
            .chain([title.as_str()])
            .collect::<Vec<_>>()
            .join("/");
        stack.push((level, title.clone(), sections.len()));
        sections.push(Section {
            level,
            title,
            chain,
            first_line: line_no,
            body: Vec::new(),
            children: Vec::new(),
        });
    }

    for section in &mut sections {
        let start = section.body.iter().position(|(_, line)| !is_blank(line));
        let end = section.body.iter().rposition(|(_, line)| !is_blank(line));
        section.body = match (start, end) {
            (Some(start), Some(end)) => section.body[start..=end].to_vec(),
            _ => Vec::new(),
        };
    }
    if sections[0].body.is_empty() {
        sections.remove(0);
    } else {
        sections[0].first_line = sections[0].body[0].0;
    }
    sections
}

/// Часть текста раздела, помещающаяся в максимум.
struct Piece {
    first_line: usize,
    last_line: usize,
    text: String,
}

/// Неделимый кусок текста при дорезке: абзац, огороженный блок, таблица или их часть.
struct Atom {
    first_line: usize,
    last_line: usize,
    text: String,
    /// Что ставить перед куском, если он идёт не первым в чанке.
    separator: &'static str,
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Текст раздела одним куском или несколькими, если он длиннее максимума.
fn split_section(section: &Section, max: usize) -> Vec<Piece> {
    let Some(&(last_line, _)) = section.body.last() else {
        return vec![Piece {
            first_line: section.first_line,
            last_line: section.first_line,
            text: String::new(),
        }];
    };
    let whole = join_lines(&section.body);
    if char_len(&whole) <= max {
        return vec![Piece {
            first_line: section.first_line,
            last_line,
            text: whole,
        }];
    }
    let mut pieces = pack(atoms(&section.body, max), max);
    if let Some(first) = pieces.first_mut() {
        first.first_line = section.first_line;
    }
    pieces
}

fn join_lines(lines: &[(usize, &str)]) -> String {
    lines
        .iter()
        .map(|(_, line)| *line)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Делит текст на абзацы по пустым строкам вне огороженных блоков. Абзац длиннее максимума
/// режется по строкам, строка длиннее максимума — жёстко по символам.
fn atoms(body: &[(usize, &str)], max: usize) -> Vec<Atom> {
    let mut units: Vec<Vec<(usize, &str)>> = Vec::new();
    let mut fence = Fence::default();
    let mut open = false;
    for &(line_no, line) in body {
        let in_fence = fence.step(line);
        if !in_fence && is_blank(line) {
            open = false;
        } else if open {
            if let Some(unit) = units.last_mut() {
                unit.push((line_no, line));
            }
        } else {
            units.push(vec![(line_no, line)]);
            open = true;
        }
    }

    let mut atoms = Vec::new();
    for unit in units {
        let first_line = unit[0].0;
        let last_line = unit[unit.len() - 1].0;
        let text = join_lines(&unit);
        if char_len(&text) <= max {
            atoms.push(Atom {
                first_line,
                last_line,
                text,
                separator: BLANK,
            });
            continue;
        }
        for (position, &(line_no, line)) in unit.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            for (part, segment) in chars.chunks(max).enumerate() {
                let separator = match (position, part) {
                    (0, 0) => BLANK,
                    (_, 0) => "\n",
                    _ => "",
                };
                atoms.push(Atom {
                    first_line: line_no,
                    last_line: line_no,
                    text: segment.iter().collect(),
                    separator,
                });
            }
        }
    }
    atoms
}

/// Жадно собирает куски в части не длиннее максимума.
fn pack(atoms: Vec<Atom>, max: usize) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut current: Option<(Piece, usize)> = None;
    for atom in atoms {
        let atom_len = char_len(&atom.text);
        if let Some((piece, len)) = current.as_mut() {
            let added = char_len(atom.separator) + atom_len;
            if *len + added <= max {
                piece.text.push_str(atom.separator);
                piece.text.push_str(&atom.text);
                piece.last_line = atom.last_line;
                *len += added;
                continue;
            }
        }
        if let Some((piece, _)) = current.take() {
            pieces.push(piece);
        }
        current = Some((
            Piece {
                first_line: atom.first_line,
                last_line: atom.last_line,
                text: atom.text,
            },
            atom_len,
        ));
    }
    pieces.extend(current.map(|(piece, _)| piece));
    pieces
}

/// Чанк до приклейки списка подзаголовков.
struct Draft {
    chain: String,
    /// Строка заголовка первого раздела; у вступления пустая.
    heading: String,
    first_line: usize,
    last_line: usize,
    body: String,
    children: Vec<(usize, String)>,
    /// Чанк получился склейкой нескольких разделов: списка подзаголовков у него нет.
    merged: bool,
}

/// Заголовок раздела и его текст как они лежат в склеенном чанке.
fn with_heading(heading: &str, body: &str) -> String {
    match (heading.is_empty(), body.is_empty()) {
        (true, _) => body.to_string(),
        (false, true) => heading.to_string(),
        (false, false) => format!("{heading}\n{body}"),
    }
}

impl Draft {
    fn len(&self) -> usize {
        char_len(&self.body)
    }

    /// Помещается ли `addition` после текста чанка в максимум.
    fn fits(&self, addition: &str, max: usize) -> bool {
        let separator = if self.body.is_empty() { 0 } else { BLANK.len() };
        self.len() + separator + char_len(addition) <= max
    }

    fn absorb(&mut self, addition: &str, last_line: usize) {
        if !self.body.is_empty() && !addition.is_empty() {
            self.body.push_str(BLANK);
        }
        self.body.push_str(addition);
        self.last_line = last_line;
        self.merged = true;
        self.children.clear();
    }

    fn into_chunk(self, ordinal: usize, path: &str) -> Chunk {
        let mut text = self.body;
        if !self.children.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(OTHER_HEADERS);
            for (level, title) in &self.children {
                text.push_str(&format!("\n{} {title}", "#".repeat(*level)));
            }
        }
        let embed_text = format!("path: {path}\n{}\n{text}", self.chain);
        let embed_hash = blake3::hash(embed_text.as_bytes()).to_hex().to_string();
        Chunk {
            ordinal,
            heading_chain: self.chain,
            first_line: self.first_line,
            last_line: self.last_line,
            text,
            embed_text,
            embed_hash,
        }
    }
}

/// Превращает разделы в чанки: дорезка больших, склейка маленьких, добор до минимума.
fn assemble(sections: &[Section], min: usize, max: usize) -> Vec<Draft> {
    let mut out: Vec<Draft> = Vec::new();
    // Маленький неразрезанный чанк, который ещё ищет, с чем склеиться.
    let mut pending: Option<Draft> = None;

    for section in sections {
        let pieces = split_section(section, max);

        if let Some(mut current) = pending.take() {
            if let [piece] = pieces.as_slice() {
                let addition = with_heading(&section.heading_text(), &piece.text);
                if current.fits(&addition, max) {
                    current.absorb(&addition, piece.last_line);
                    if current.len() >= min {
                        out.push(current);
                    } else {
                        pending = Some(current);
                    }
                    continue;
                }
            }
            pad_from_above(&mut current, &out, min, max);
            out.push(current);
        }

        let count = pieces.len();
        for (index, piece) in pieces.into_iter().enumerate() {
            let is_last = index + 1 == count;
            let draft = Draft {
                chain: section.chain.clone(),
                heading: section.heading_text(),
                first_line: piece.first_line,
                last_line: piece.last_line,
                body: piece.text,
                children: if is_last {
                    section.children.clone()
                } else {
                    Vec::new()
                },
                merged: false,
            };
            if count == 1 && draft.len() < min {
                pending = Some(draft);
            } else {
                out.push(draft);
            }
        }
    }

    if let Some(mut current) = pending {
        let addition = with_heading(&current.heading, &current.body);
        if out
            .last()
            .is_some_and(|previous| previous.fits(&addition, max))
        {
            if let Some(previous) = out.last_mut() {
                previous.absorb(&addition, current.last_line);
            }
        } else {
            pad_from_above(&mut current, &out, min, max);
            out.push(current);
        }
    }
    out
}

/// Добирает чанк до `min` символов копией хвоста текста предыдущих чанков.
/// Если выше текста нет или его не хватает, чанк остаётся короче.
fn pad_from_above(current: &mut Draft, above: &[Draft], min: usize, max: usize) {
    let own_separator = if current.body.is_empty() {
        0
    } else {
        BLANK.len()
    };
    let mut parts: Vec<String> = Vec::new();
    let mut total = current.len();

    for previous in above.iter().rev() {
        if total >= min || previous.body.is_empty() {
            continue;
        }
        let separator = if parts.is_empty() {
            own_separator
        } else {
            BLANK.len()
        };
        let room = max.saturating_sub(total + separator);
        let take = (min - total)
            .saturating_sub(separator)
            .max(1)
            .min(room)
            .min(char_len(&previous.body));
        if take == 0 {
            break;
        }
        let skip = char_len(&previous.body) - take;
        parts.push(previous.body.chars().skip(skip).collect());
        total += take + separator;
    }
    if parts.is_empty() {
        return;
    }
    parts.reverse();
    let mut padded = parts.join(BLANK);
    if !current.body.is_empty() {
        padded.push_str(BLANK);
        padded.push_str(&current.body);
    }
    current.body = padded;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(min: usize, max: usize) -> ChunkingParams {
        ChunkingParams {
            min_chars: min,
            max_chars: max,
        }
    }

    fn chunk(source: &str, min: usize, max: usize) -> ChunkedDocument {
        chunk_document("docs/test/test_file.md", source, &params(min, max))
    }

    #[test]
    fn frontmatter_gives_title_and_keeps_line_numbers() {
        let doc = chunk("---\ntitle: \"Привет\"\n---\n# А\nтекст\n", 1, 100);
        assert_eq!(doc.title, "Привет");
        assert_eq!(doc.chunks.len(), 1);
        assert_eq!(doc.chunks[0].first_line, 4);
        assert_eq!(doc.chunks[0].last_line, 5);
        assert_eq!(doc.chunks[0].heading_chain, "А");
    }

    #[test]
    fn title_falls_back_to_file_name_and_names_the_intro() {
        let doc = chunk("вступление\n# А\nтекст\n", 1, 100);
        assert_eq!(doc.title, "test_file");
        assert_eq!(doc.chunks[0].heading_chain, "test_file");
        assert_eq!(doc.chunks[0].first_line, 1);
        assert_eq!(doc.chunks[1].heading_chain, "А");
    }

    #[test]
    fn empty_intro_gives_no_chunk() {
        let doc = chunk("---\ntitle: T\n---\n\n# А\nтекст\n", 1, 100);
        assert_eq!(doc.chunks.len(), 1);
    }

    #[test]
    fn crlf_and_bom_do_not_change_the_result() {
        let lf = chunk("# А\nодин\n\n## Б\nдва\n", 1, 100);
        let crlf = chunk("\u{feff}# А\r\nодин\r\n\r\n## Б\r\nдва\r\n", 1, 100);
        assert_eq!(lf, crlf);
    }

    #[test]
    fn headings_inside_code_fences_are_text() {
        let source = "# А\n```md\n# не заголовок\n```\n~~~\n## и это нет\n~~~\n";
        let doc = chunk(source, 1, 200);
        assert_eq!(doc.chunks.len(), 1);
        assert!(doc.chunks[0].text.contains("# не заголовок"));
        assert!(doc.chunks[0].text.contains("## и это нет"));
    }

    #[test]
    fn chain_and_other_headers_for_parent() {
        let source = "# А\nтекст родителя\n## Б\nтекст б\n### В\nтекст в\n## Г\nтекст г\n";
        let doc = chunk(source, 5, 200);
        let chains: Vec<&str> = doc
            .chunks
            .iter()
            .map(|c| c.heading_chain.as_str())
            .collect();
        assert_eq!(chains, ["А", "А/Б", "А/Б/В", "А/Г"]);
        assert_eq!(
            doc.chunks[0].text,
            "текст родителя\nother headers:\n## Б\n## Г"
        );
        assert_eq!(doc.chunks[1].text, "текст б\nother headers:\n### В");
        assert_eq!(doc.chunks[2].text, "текст в");
        assert_eq!((doc.chunks[0].first_line, doc.chunks[0].last_line), (1, 2));
    }

    #[test]
    fn sections_after_a_skipped_level_get_the_nearest_parent() {
        let doc = chunk("# А\nтекст\n### В\nтекст\n", 1, 100);
        assert_eq!(doc.chunks[1].heading_chain, "А/В");
        assert_eq!(doc.chunks[0].text, "текст\nother headers:\n### В");
    }

    #[test]
    fn big_section_is_split_by_paragraphs_with_one_chain() {
        let paragraph = "слово ".repeat(10).trim_end().to_string();
        let source = format!("# А\n{paragraph}\n\n{paragraph}\n\n{paragraph}\n## Б\nхвост\n");
        let doc = chunk(&source, 1, 125);
        let parent: Vec<&Chunk> = doc
            .chunks
            .iter()
            .filter(|c| c.heading_chain == "А")
            .collect();
        assert_eq!(parent.len(), 2);
        assert_eq!(parent[0].text, format!("{paragraph}\n\n{paragraph}"));
        assert_eq!((parent[0].first_line, parent[0].last_line), (1, 4));
        assert_eq!((parent[1].first_line, parent[1].last_line), (6, 6));
        assert!(parent[1].text.ends_with("other headers:\n## Б"));
    }

    #[test]
    fn code_block_is_not_cut_when_it_fits() {
        let code = "```\nа\n\nб\n```";
        let source = format!("# А\nабзац абзац абзац\n\n{code}\n");
        let doc = chunk(&source, 1, 30);
        assert!(doc.chunks.iter().any(|c| c.text == code));
    }

    #[test]
    fn long_line_is_cut_by_characters() {
        let source = format!("# А\n{}\n", "я".repeat(25));
        let doc = chunk(&source, 1, 10);
        let lengths: Vec<usize> = doc.chunks.iter().map(|c| char_len(&c.text)).collect();
        assert_eq!(lengths, [10, 10, 5]);
    }

    #[test]
    fn size_is_counted_in_characters_not_bytes() {
        let source = format!("# А\n{}\n", "я".repeat(300));
        assert_eq!(chunk(&source, 1, 300).chunks.len(), 1);
        assert_eq!(chunk(&source, 1, 299).chunks.len(), 2);
    }

    #[test]
    fn small_section_is_merged_with_the_next_without_other_headers() {
        let source = "# А\nкоротко\n## Б\nэтот текст достаточно длинный для минимума\n## В\nещё один длинный раздел для отдельного чанка\n";
        let doc = chunk(source, 30, 200);
        assert_eq!(doc.chunks[0].heading_chain, "А");
        assert_eq!(
            doc.chunks[0].text,
            "коротко\n\n## Б\nэтот текст достаточно длинный для минимума"
        );
        assert_eq!((doc.chunks[0].first_line, doc.chunks[0].last_line), (1, 4));
    }

    #[test]
    fn last_small_section_is_attached_to_the_previous_chunk() {
        let source = "# А\nэтот текст достаточно длинный для минимума\n## Б\nмало\n";
        let doc = chunk(source, 30, 200);
        assert_eq!(doc.chunks.len(), 1);
        assert_eq!(
            doc.chunks[0].text,
            "этот текст достаточно длинный для минимума\n\n## Б\nмало"
        );
        assert_eq!(doc.chunks[0].last_line, 4);
    }

    #[test]
    fn small_chunk_is_padded_from_above_when_the_next_does_not_fit() {
        let first = "первый раздел с достаточным текстом";
        let big = format!("{0}\n\n{0}\n\n{0}", "б".repeat(40));
        let source = format!("# А\n{first}\n## Малый\nх\n## Большой\n{big}\n");
        let doc = chunk(&source, 30, 100);
        let small = doc
            .chunks
            .iter()
            .find(|c| c.heading_chain == "А/Малый")
            .expect("чанк малого раздела");
        assert_eq!(char_len(&small.text), 30);
        assert!(small.text.ends_with("\n\nх"));
        let copied = small.text.trim_end_matches("\n\nх");
        assert!(first.ends_with(copied));
        // Номера строк остаются от собственного раздела.
        assert_eq!((small.first_line, small.last_line), (3, 4));
        // Копия: у источника текст на месте.
        assert!(doc.chunks[0].text.starts_with(first));
    }

    #[test]
    fn small_chunk_without_text_above_stays_short() {
        let big = format!("{0}\n\n{0}", "б".repeat(60));
        let source = format!("# Малый\nх\n## Большой\n{big}\n");
        let doc = chunk(&source, 30, 100);
        // Выше текста нет: чанк короче минимума, а список подзаголовков на месте.
        assert_eq!(doc.chunks[0].text, "х\nother headers:\n## Большой");
    }

    #[test]
    fn inserting_a_line_on_top_only_shifts_line_numbers() {
        let source = "# А\nодин два три\n## Б\nчетыре пять шесть\n";
        let before = chunk(source, 1, 100);
        let after = chunk(&format!("\n{source}"), 1, 100);
        assert_eq!(before.chunks.len(), after.chunks.len());
        for (old, new) in before.chunks.iter().zip(&after.chunks) {
            assert_eq!(old.embed_hash, new.embed_hash);
            assert_eq!(old.first_line + 1, new.first_line);
            assert_eq!(old.last_line + 1, new.last_line);
        }
    }

    #[test]
    fn embed_text_has_path_chain_and_no_line_number() {
        let doc = chunk("# А\n## Б\nтекст\n", 0, 100);
        let last = doc.chunks.last().expect("есть чанк");
        assert_eq!(last.embed_text, "path: docs/test/test_file.md\nА/Б\nтекст");
        assert_eq!(
            last.embed_hash,
            blake3::hash(last.embed_text.as_bytes())
                .to_hex()
                .to_string()
        );
    }

    #[test]
    fn renaming_the_file_changes_the_hash() {
        let a = chunk_document("a.md", "# А\nтекст\n", &params(1, 100));
        let b = chunk_document("b.md", "# А\nтекст\n", &params(1, 100));
        assert_ne!(a.chunks[0].embed_hash, b.chunks[0].embed_hash);
    }

    #[test]
    fn empty_and_frontmatter_only_documents_give_no_chunks() {
        assert!(chunk("", 1, 100).chunks.is_empty());
        assert!(chunk("---\ntitle: T\n---\n", 1, 100).chunks.is_empty());
    }

    #[test]
    fn unclosed_frontmatter_is_text() {
        let doc = chunk("---\ntitle: T\n# А\nтекст\n", 1, 100);
        assert_eq!(doc.title, "test_file");
    }
}
