//! Аудит дублей проекта (`docs/features/search/maintenance-tools.md`, IDEA-13). Способ и пороги —
//! `docs/knowledge/duplicates.md`: точные повторы ищет хеш, близкие и частичные — общие пятисловные
//! окна («шинглы»), смысловые — косинус векторов при минимальном словесном совпадении. Новый embed
//! не нужен: используются готовые векторы индекса.

use std::collections::{HashMap, HashSet};

use super::hybrid::{Scope, SearchError};
use super::params::{SearchRequest, SearchSettings};
use crate::storage::Index;

/// Раздел короче этого числа слов не сравнивается: у него шумная доля общих окон.
const MIN_WORDS: usize = 15;
/// Окно близких и частичных копий и окно для смыслового условия, слов.
const SHINGLE: usize = 5;
const SHINGLE_LOOSE: usize = 3;
/// Окно, которое есть в стольких разделах и больше, считается шаблонным и в пары не идёт.
const TEMPLATE_DF: u32 = 4;
/// Мягкое окно, встречающееся в большем числе разделов, не даёт пар для смыслового условия.
const LOOSE_DF_CAP: usize = 24;
/// Сколько общих окон нужно для пары: меньше — совпадение случайное.
const MIN_SHARED: usize = 10;
/// Близкая копия: Жаккар общих окон не ниже.
const NEAR_JACCARD: f32 = 0.6;
/// «Содержится в другом разделе»: доля общих окон меньшего раздела не ниже.
const CONTAINED_SHARE: f32 = 0.9;
/// Частичная копия: доля общих окон меньшего раздела не ниже.
const PARTIAL_SHARE: f32 = 0.25;
/// Смысловое условие: косинус не ниже и доля общих трёхсловных окон не ниже.
const SEMANTIC_COSINE: f32 = 0.8;
const SEMANTIC_LOOSE_SHARE: f32 = 0.05;
/// Сколько записей в ответе по умолчанию.
const DEFAULT_RESULTS: usize = 20;
/// Сколько совпавших фрагментов и различий показывать у пары.
const MAX_SHARED: usize = 3;
const MAX_CHANGES: usize = 12;
const MAX_FRAGMENT_CHARS: usize = 300;
/// Пословное сравнение для разделов длиннее этого числа слов не строится.
const MAX_DIFF_WORDS: usize = 2500;

const NEGATIONS: [&str; 10] = [
    "не",
    "нет",
    "нельзя",
    "ни",
    "без",
    "запрещено",
    "нигде",
    "никогда",
    "отсутствует",
    "отклонен",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct DuplicatesRequest {
    /// Модули (корневой — `.`); пусто — весь проект.
    pub modules: Vec<String>,
    /// Сколько записей вернуть; без значения — 20, не больше предела поиска.
    pub results: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DuplicateKind {
    /// Совпадает весь текст раздела после нормализации (регистр, пробелы, разметка).
    Exact,
    /// Почти одинаковые тексты: правки отдельных слов, чисел, отрицаний.
    Near,
    /// Один раздел почти целиком входит в другой.
    Contained,
    /// Заметная часть текста скопирована.
    Partial,
    /// Близки по смыслу при небольшом словесном совпадении: только повод проверить.
    Semantic,
}

impl DuplicateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Near => "near",
            Self::Contained => "contained",
            Self::Partial => "partial",
            Self::Semantic => "semantic",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateChunk {
    pub public_id: String,
    pub path: String,
    pub module: String,
    pub heading_chain: String,
    pub first_line: usize,
    pub last_line: usize,
}

/// Замена слов между двумя копиями: пустая строка — слов с этой стороны нет.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub first: String,
    pub second: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Differences {
    /// Пословные различия близких копий, в порядке текста; у других видов пусты.
    pub changes: Vec<Change>,
    /// Различий больше, чем показано, или текст слишком длинный для сравнения.
    pub changes_truncated: bool,
    /// Числа и идентификаторы с цифрами, которых нет в другом разделе.
    pub numbers_only_first: Vec<String>,
    pub numbers_only_second: Vec<String>,
    /// Сколько слов отрицания («не», «нет», «без»…) в каждом разделе.
    pub negations: (usize, usize),
}

impl Differences {
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
            && self.numbers_only_first.is_empty()
            && self.numbers_only_second.is_empty()
            && self.negations.0 == self.negations.1
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DuplicateItem {
    pub kind: DuplicateKind,
    /// Две и больше записи; у пары — две.
    pub chunks: Vec<DuplicateChunk>,
    /// Мера совпадения: у точных 1; у близких — Жаккар общих окон; у содержащихся и частичных —
    /// доля общих окон меньшего раздела; у смысловых — косинус.
    pub score: f32,
    /// Самые длинные совпавшие отрывки текста (у пар лексических видов).
    pub shared: Vec<String>,
    /// Различия двух разделов; у точных групп нет.
    pub differences: Option<Differences>,
    /// Одинаковый блок в четырёх и больше разделах: скорее шаблон, чем случайная копия.
    pub repeated_block: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateCounts {
    pub exact: usize,
    pub near: usize,
    pub contained: usize,
    pub partial: usize,
    pub semantic: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DuplicatesResponse {
    pub items: Vec<DuplicateItem>,
    /// Сколько найдено по видам до ограничения числа записей.
    pub counts: DuplicateCounts,
    pub total: usize,
    /// Применённое число записей.
    pub results: usize,
    pub clamped: bool,
    pub chunks_checked: usize,
    /// Короткие разделы (меньше 15 слов) не сравнивались.
    pub chunks_skipped_short: usize,
}

struct Item {
    public_id: String,
    path: String,
    module: String,
    heading_chain: String,
    first_line: usize,
    last_line: usize,
    embed_hash: String,
    body: String,
    words: Vec<String>,
    spans: Vec<(usize, usize)>,
    s5: Vec<u64>,
    s3: Vec<u64>,
}

impl Item {
    fn chunk(&self) -> DuplicateChunk {
        DuplicateChunk {
            public_id: self.public_id.clone(),
            path: self.path.clone(),
            module: self.module.clone(),
            heading_chain: self.heading_chain.clone(),
            first_line: self.first_line,
            last_line: self.last_line,
        }
    }
}

/// Собственный текст раздела: без заголовков и без служебного списка подзаголовков чанка.
fn own_text(text: &str) -> String {
    const MARK: &str = "\nother headers:\n";
    let mut text = text;
    if let Some(at) = text.rfind(MARK) {
        let tail = &text[at + MARK.len()..];
        if tail.lines().all(|line| line.trim_start().starts_with('#')) {
            text = &text[..at];
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
        }
        let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
        let heading = !fenced && (1..=6).contains(&hashes) && trimmed[hashes..].starts_with(' ');
        if !heading {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

/// Слова текста в нижнем регистре, `ё` как `е`; границы слов в байтах исходного текста.
fn split_words(text: &str) -> (Vec<String>, Vec<(usize, usize)>) {
    let mut words = Vec::new();
    let mut spans = Vec::new();
    let mut current = String::new();
    let mut start = 0;
    for (at, symbol) in text.char_indices() {
        if symbol.is_alphanumeric() || symbol == '_' {
            if current.is_empty() {
                start = at;
            }
            for lower in symbol.to_lowercase() {
                current.push(if lower == 'ё' { 'е' } else { lower });
            }
        } else if !current.is_empty() {
            words.push(std::mem::take(&mut current));
            spans.push((start, at));
        }
    }
    if !current.is_empty() {
        words.push(current);
        spans.push((start, text.len()));
    }
    (words, spans)
}

fn fnv(parts: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in part.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn shingle_hashes(words: &[String], size: usize) -> Vec<u64> {
    if words.len() < size {
        return Vec::new();
    }
    words.windows(size).map(fnv).collect()
}

fn unique_sorted(mut hashes: Vec<u64>) -> Vec<u64> {
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn load_items(index: &Index, modules: &[String]) -> Result<(Vec<Item>, usize), SearchError> {
    let mut statement = index.conn().prepare(
        "SELECT c.id, d.path, d.module, c.heading_chain, c.first_line, c.last_line, c.text,
                d.hash, c.ordinal, c.embed_hash
         FROM chunks c JOIN documents d ON d.id = c.document_id
         ORDER BY d.path, c.ordinal",
    )?;
    let mut rows = statement.query([])?;
    let mut items = Vec::new();
    let mut skipped = 0;
    while let Some(row) = rows.next()? {
        let module: String = row.get(2)?;
        if !modules.is_empty() && !modules.contains(&module) {
            continue;
        }
        let id: i64 = row.get(0)?;
        let path: String = row.get(1)?;
        let text: String = row.get(6)?;
        let document_hash: String = row.get(7)?;
        let ordinal: i64 = row.get(8)?;
        let embed_hash: String = row.get(9)?;
        let body = own_text(&text);
        let (words, spans) = split_words(&body);
        if words.len() < MIN_WORDS {
            skipped += 1;
            continue;
        }
        items.push(Item {
            public_id: super::similar::public_id(id, &document_hash, &path, ordinal, &embed_hash),
            path,
            module,
            heading_chain: row.get(3)?,
            first_line: row.get::<_, i64>(4)? as usize,
            last_line: row.get::<_, i64>(5)? as usize,
            embed_hash,
            s5: unique_sorted(shingle_hashes(&words, SHINGLE)),
            s3: unique_sorted(shingle_hashes(&words, SHINGLE_LOOSE)),
            body,
            words,
            spans,
        });
    }
    Ok((items, skipped))
}

/// Пары разделов с числом общих окон; окна из `skip` и слишком частые не учитываются.
fn shared_counts(
    sets: &[&[u64]],
    skip: impl Fn(u64) -> bool,
    cap: usize,
) -> HashMap<(usize, usize), usize> {
    let mut postings: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, set) in sets.iter().enumerate() {
        for &hash in *set {
            if !skip(hash) {
                postings.entry(hash).or_default().push(index);
            }
        }
    }
    let mut counts: HashMap<(usize, usize), usize> = HashMap::new();
    for list in postings.values() {
        if list.len() < 2 || list.len() > cap {
            continue;
        }
        for (position, &first) in list.iter().enumerate() {
            for &second in &list[position + 1..] {
                *counts.entry((first, second)).or_insert(0) += 1;
            }
        }
    }
    counts
}

fn cosine(left: &[f32], right: &[f32]) -> f32 {
    left.iter()
        .zip(right)
        .map(|(a, b)| a * b)
        .sum::<f32>()
        .clamp(0.0, 1.0)
}

fn is_number(word: &str) -> bool {
    word.chars().any(|symbol| symbol.is_ascii_digit())
}

fn differences(first: &Item, second: &Item, with_changes: bool) -> Differences {
    let numbers = |item: &Item| -> HashSet<String> {
        item.words
            .iter()
            .filter(|word| is_number(word))
            .cloned()
            .collect()
    };
    let (left, right) = (numbers(first), numbers(second));
    let mut only_first: Vec<String> = left.difference(&right).cloned().collect();
    let mut only_second: Vec<String> = right.difference(&left).cloned().collect();
    only_first.sort();
    only_second.sort();
    let negations = |item: &Item| {
        item.words
            .iter()
            .filter(|word| NEGATIONS.contains(&word.as_str()))
            .count()
    };
    let mut result = Differences {
        numbers_only_first: only_first,
        numbers_only_second: only_second,
        negations: (negations(first), negations(second)),
        ..Differences::default()
    };
    if with_changes {
        match word_changes(first, second) {
            Some((changes, truncated)) => {
                result.changes = changes;
                result.changes_truncated = truncated;
            }
            None => result.changes_truncated = true,
        }
    }
    result
}

fn slice(item: &Item, from: usize, to: usize) -> &str {
    if from >= to {
        return "";
    }
    &item.body[item.spans[from].0..item.spans[to - 1].1]
}

/// Пословные замены между разделами (по наибольшей общей подпоследовательности слов).
fn word_changes(first: &Item, second: &Item) -> Option<(Vec<Change>, bool)> {
    let (a, b) = (&first.words, &second.words);
    if a.len() > MAX_DIFF_WORDS || b.len() > MAX_DIFF_WORDS {
        return None;
    }
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a_end, b_end) = (a.len() - suffix, b.len() - suffix);
    let (n, m) = (a_end - prefix, b_end - prefix);
    let width = m + 1;
    let mut table = vec![0u16; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i * width + j] = if a[prefix + i] == b[prefix + j] {
                table[(i + 1) * width + j + 1] + 1
            } else {
                table[(i + 1) * width + j].max(table[i * width + j + 1])
            };
        }
    }
    let mut changes = Vec::new();
    let (mut i, mut j) = (0, 0);
    let mut hunk: Option<(usize, usize, usize, usize)> = None;
    let flush = |hunk: &mut Option<(usize, usize, usize, usize)>, changes: &mut Vec<Change>| {
        if let Some((a_from, a_to, b_from, b_to)) = hunk.take() {
            changes.push(Change {
                first: slice(first, prefix + a_from, prefix + a_to).to_string(),
                second: slice(second, prefix + b_from, prefix + b_to).to_string(),
            });
        }
    };
    while i < n || j < m {
        if i < n && j < m && a[prefix + i] == b[prefix + j] {
            flush(&mut hunk, &mut changes);
            i += 1;
            j += 1;
            continue;
        }
        let delete = j >= m || (i < n && table[(i + 1) * width + j] >= table[i * width + j + 1]);
        let entry = hunk.get_or_insert((i, i, j, j));
        if delete {
            i += 1;
            entry.1 = i;
        } else {
            j += 1;
            entry.3 = j;
        }
    }
    flush(&mut hunk, &mut changes);
    let truncated = changes.len() > MAX_CHANGES;
    changes.truncate(MAX_CHANGES);
    Some((changes, truncated))
}

/// Самые длинные общие отрывки по совпадающим окнам из пяти слов, в тексте первого раздела.
fn shared_fragments(first: &Item, second: &Item) -> Vec<String> {
    let right: HashMap<u64, usize> = shingle_hashes(&second.words, SHINGLE)
        .into_iter()
        .enumerate()
        .rev()
        .map(|(position, hash)| (hash, position))
        .collect();
    let mut runs: Vec<(usize, usize)> = Vec::new(); // (начало в первом, длина в окнах)
    let mut previous: Option<(usize, usize)> = None; // (позиция в первом, во втором)
    for (position, hash) in shingle_hashes(&first.words, SHINGLE)
        .into_iter()
        .enumerate()
    {
        match right.get(&hash) {
            Some(&other) => {
                let continues = previous == Some((position.wrapping_sub(1), other.wrapping_sub(1)));
                match (continues, runs.last_mut()) {
                    (true, Some(run)) => run.1 += 1,
                    _ => runs.push((position, 1)),
                }
                previous = Some((position, other));
            }
            None => previous = None,
        }
    }
    runs.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    runs.into_iter()
        .take(MAX_SHARED)
        .map(|(start, length)| {
            let text = slice(first, start, start + length + SHINGLE - 1);
            let mut text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if text.chars().count() > MAX_FRAGMENT_CHARS {
                text = text.chars().take(MAX_FRAGMENT_CHARS).collect::<String>() + "…";
            }
            text
        })
        .collect()
}

struct Pair {
    kind: DuplicateKind,
    first: usize,
    second: usize,
    score: f32,
}

/// Повторы в выбранной области проекта: точные группы, близкие, содержащиеся, частичные и
/// смысловые пары. Все модули области должны существовать (`invalid_params`).
pub fn duplicates(
    index: &Index,
    settings: &SearchSettings,
    request: &DuplicatesRequest,
) -> Result<DuplicatesResponse, SearchError> {
    Scope::build(
        index,
        &SearchRequest {
            modules: request.modules.clone(),
            ..SearchRequest::default()
        },
    )?;
    let max = settings.max_results.max(1);
    let results = request
        .results
        .map_or(DEFAULT_RESULTS.min(max), |value| value.clamp(1, max));
    let clamped = request.results.is_some_and(|value| value != results);

    let (items, chunks_skipped_short) = load_items(index, &request.modules)?;

    // Точные группы: совпадает весь нормализованный текст.
    let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
    for (position, item) in items.iter().enumerate() {
        by_text
            .entry(item.words.join(" "))
            .or_default()
            .push(position);
    }
    let mut group_of: HashMap<usize, usize> = HashMap::new();
    let mut exact_groups: Vec<Vec<usize>> = by_text
        .into_values()
        .filter(|group| group.len() > 1)
        .collect();
    exact_groups.sort_by(|left, right| right.len().cmp(&left.len()).then(left[0].cmp(&right[0])));
    for (number, group) in exact_groups.iter().enumerate() {
        for &position in group {
            group_of.insert(position, number);
        }
    }
    let same_group = |first: usize, second: usize| {
        group_of
            .get(&first)
            .is_some_and(|group| group_of.get(&second) == Some(group))
    };

    // Шаблонные окна: в четырёх разделах и больше.
    let mut frequency: HashMap<u64, u32> = HashMap::new();
    for item in &items {
        for &hash in &item.s5 {
            *frequency.entry(hash).or_insert(0) += 1;
        }
    }

    let strict: Vec<&[u64]> = items.iter().map(|item| item.s5.as_slice()).collect();
    let counts = shared_counts(&strict, |hash| frequency[&hash] >= TEMPLATE_DF, usize::MAX);
    let mut pairs: Vec<Pair> = Vec::new();
    let mut lexical: HashSet<(usize, usize)> = HashSet::new();
    for (&(first, second), &shared) in &counts {
        if shared < MIN_SHARED || same_group(first, second) {
            continue;
        }
        let (left, right) = (items[first].s5.len(), items[second].s5.len());
        let share = shared as f32 / left.min(right) as f32;
        let jaccard = shared as f32 / (left + right - shared) as f32;
        let (kind, score) = if jaccard >= NEAR_JACCARD {
            (DuplicateKind::Near, jaccard)
        } else if share >= CONTAINED_SHARE {
            (DuplicateKind::Contained, share)
        } else if share >= PARTIAL_SHARE {
            (DuplicateKind::Partial, share)
        } else {
            continue;
        };
        // Частичное совпадение разделов одного документа — рамка, а не повтор (0 из 22 на разметке).
        if kind == DuplicateKind::Partial && items[first].path == items[second].path {
            continue;
        }
        lexical.insert((first, second));
        pairs.push(Pair {
            kind,
            first,
            second,
            score,
        });
    }

    // Смысловые кандидаты: есть словесное зацепление, но не копия.
    let loose: Vec<&[u64]> = items.iter().map(|item| item.s3.as_slice()).collect();
    let mut loose_frequency: HashMap<u64, u32> = HashMap::new();
    for item in &items {
        for &hash in &item.s3 {
            *loose_frequency.entry(hash).or_insert(0) += 1;
        }
    }
    let loose_counts = shared_counts(
        &loose,
        |hash| loose_frequency[&hash] >= TEMPLATE_DF,
        LOOSE_DF_CAP,
    );
    let mut vectors: HashMap<String, Option<Vec<f32>>> = HashMap::new();
    for (&(first, second), &shared) in &loose_counts {
        if lexical.contains(&(first, second)) || same_group(first, second) {
            continue;
        }
        // Смысловая близость разделов одного документа — их общая тема, а не повтор.
        if items[first].path == items[second].path {
            continue;
        }
        let (left, right) = (items[first].s3.len(), items[second].s3.len());
        if (shared as f32 / left.min(right) as f32) < SEMANTIC_LOOSE_SHARE {
            continue;
        }
        let strict_shared = counts.get(&(first, second)).copied().unwrap_or(0);
        let strict_share =
            strict_shared as f32 / items[first].s5.len().min(items[second].s5.len()) as f32;
        if strict_share >= PARTIAL_SHARE {
            continue;
        }
        let mut vector_of = |hash: &str| -> Result<Option<Vec<f32>>, SearchError> {
            if !vectors.contains_key(hash) {
                vectors.insert(hash.to_string(), index.vector(hash)?);
            }
            Ok(vectors[hash].clone())
        };
        let (Some(a), Some(b)) = (
            vector_of(&items[first].embed_hash)?,
            vector_of(&items[second].embed_hash)?,
        ) else {
            continue;
        };
        if a.len() != b.len() {
            continue;
        }
        let score = cosine(&a, &b);
        if score >= SEMANTIC_COSINE {
            pairs.push(Pair {
                kind: DuplicateKind::Semantic,
                first,
                second,
                score,
            });
        }
    }

    let rank = |kind: DuplicateKind| match kind {
        DuplicateKind::Exact => 0,
        DuplicateKind::Near => 1,
        DuplicateKind::Contained => 2,
        DuplicateKind::Partial => 3,
        DuplicateKind::Semantic => 4,
    };
    pairs.sort_by(|left, right| {
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then(right.score.total_cmp(&left.score))
            .then((left.first, left.second).cmp(&(right.first, right.second)))
    });

    let mut counts_by_kind = DuplicateCounts {
        exact: exact_groups.len(),
        ..DuplicateCounts::default()
    };
    for pair in &pairs {
        match pair.kind {
            DuplicateKind::Near => counts_by_kind.near += 1,
            DuplicateKind::Contained => counts_by_kind.contained += 1,
            DuplicateKind::Partial => counts_by_kind.partial += 1,
            DuplicateKind::Semantic => counts_by_kind.semantic += 1,
            DuplicateKind::Exact => {}
        }
    }
    let total = exact_groups.len() + pairs.len();

    let mut out = Vec::new();
    for group in exact_groups.iter().take(results) {
        let first = &items[group[0]];
        out.push(DuplicateItem {
            kind: DuplicateKind::Exact,
            chunks: group
                .iter()
                .map(|&position| items[position].chunk())
                .collect(),
            score: 1.0,
            shared: vec![shared_text(first)],
            differences: None,
            repeated_block: group.len() >= TEMPLATE_DF as usize,
        });
    }
    for pair in pairs.iter().take(results.saturating_sub(out.len())) {
        let (first, second) = (&items[pair.first], &items[pair.second]);
        let lexical_kind = pair.kind != DuplicateKind::Semantic;
        out.push(DuplicateItem {
            kind: pair.kind,
            chunks: vec![first.chunk(), second.chunk()],
            score: pair.score,
            shared: if lexical_kind {
                shared_fragments(first, second)
            } else {
                Vec::new()
            },
            differences: Some(differences(first, second, pair.kind == DuplicateKind::Near)),
            repeated_block: false,
        });
    }
    Ok(DuplicatesResponse {
        items: out,
        counts: counts_by_kind,
        total,
        results,
        clamped,
        chunks_checked: items.len(),
        chunks_skipped_short,
    })
}

fn shared_text(item: &Item) -> String {
    let text = item.body.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() > MAX_FRAGMENT_CHARS {
        text.chars().take(MAX_FRAGMENT_CHARS).collect::<String>() + "…"
    } else {
        text
    }
}

#[cfg(test)]
mod tests;
