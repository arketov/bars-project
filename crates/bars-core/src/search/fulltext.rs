//! Полнотекстовый сигнал `f`: запрос к четырёхколонной `chunks_fts`, веса колонок, исправление
//! опечаток по словарю индекса, балл `f` для чанков (`docs/features/search/fulltext.md`,
//! `docs/knowledge/tokenization.md`, схема баллов — `docs/features/search/hybrid-search.md`).
//!
//! Сложение баллов и подбор весов — `hybrid.rs` (шаг 2.3.4), операторы запроса — `operators.rs`;
//! здесь запрос к FTS5 и сигналы `f` (слова по полям) и `p` (точное совпадение).

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params_from_iter;
use rusqlite::types::Value;

use crate::storage::stem::TextStemmer;

/// Составной `bm25` по колонкам `chunks_fts` с весами 5 / 3 / 1 / 0,3 (название и имя файла,
/// заголовки, текст, точная форма). Веса стартовые, подбираются в шаге 2.3.4 по бенчу. Чем
/// меньше значение, тем лучше.
pub const BM25: &str = "bm25(chunks_fts, 5.0, 3.0, 1.0, 0.3)";

/// Колонки с основами слов: слова запроса после стемминга ищутся в них.
pub(super) const STEM_COLUMNS: &str = "{name heading_chain body}";

/// Сколько ближайших слов словаря добавляется к слову запроса, которого нет в индексе.
const TYPO_VARIANTS: usize = 3;

/// Слово запроса короче этого числа знаков опечаток не исправляется.
const TYPO_MIN_LEN: usize = 4;

/// Запросы к FTS5 по одному запросу пользователя.
#[derive(Debug, PartialEq, Eq)]
pub struct FtsQuery {
    /// Все слова запроса `OR`-ом: основы — по колонкам названия, заголовков и текста, слова без
    /// стемминга и формы идентификаторов — по колонке точной формы (`bm25` сам поднимает чанки с
    /// большим числом совпавших слов). Слово запроса, которое разбор разрезал на несколько
    /// (`agent_min_score`, `по-русски`, китайская строка), идёт ещё и фразой.
    pub all: String,
    /// Только слова-идентификаторы (с `_`, `.` или `/` внутри): фраза их частей и целая форма.
    /// `None`, если таких слов нет.
    pub identifiers: Option<String>,
}

/// Слова запроса после разбора, до сборки выражения FTS5.
#[derive(Debug, Default)]
pub struct QueryTerms {
    /// Основы слов, без повторов.
    stems: Vec<String>,
    /// Фразы из основ слов запроса, разрезанных разбором, в кавычках.
    phrases: Vec<String>,
    /// Фразы из основ слов-идентификаторов, в кавычках.
    identifier_phrases: Vec<String>,
    /// Токены точной формы: слова, биграммы CJK и целые формы идентификаторов.
    exact: Vec<String>,
    /// Целые формы слов-идентификаторов.
    identifier_forms: Vec<String>,
    /// Токены точной формы запроса по порядку, с повторами: из них собираются фраза запроса и пары
    /// соседних слов сигнала `p`.
    exact_sequence: Vec<String>,
}

impl QueryTerms {
    /// `None`, если в запросе нет ни одного слова.
    pub fn parse(query: &str) -> Option<Self> {
        let stemmer = TextStemmer::new();
        let mut terms = Self::default();
        for raw in query.split_whitespace() {
            let stemmed = stemmer.stem_text(raw);
            if stemmed.split_whitespace().count() < 2 {
                continue;
            }
            let phrase = format!("\"{stemmed}\"");
            if is_identifier(raw) {
                push_unique(&mut terms.identifier_phrases, phrase.clone());
                for form in crate::storage::stem::identifier_forms(raw) {
                    push_unique(&mut terms.identifier_forms, form);
                }
            }
            push_unique(&mut terms.phrases, phrase);
        }
        for stem in stemmer.stem_text(query).split_whitespace() {
            push_unique(&mut terms.stems, stem.to_string());
        }
        for token in stemmer.exact_text(query).split_whitespace() {
            push_unique(&mut terms.exact, token.to_string());
            terms.exact_sequence.push(token.to_string());
        }
        if terms.stems.is_empty() && terms.exact.is_empty() {
            return None;
        }
        Some(terms)
    }

    /// Выражение FTS5; `corrections` — слова словаря вместо опечаток, к основам.
    pub fn expression(&self, corrections: &[String]) -> FtsQuery {
        let mut stem_terms = self.phrases.clone();
        for stem in self.stems.iter().chain(corrections) {
            push_unique(&mut stem_terms, quote(stem));
        }
        let exact_terms: Vec<String> = self.exact.iter().map(|token| quote(token)).collect();
        let all = join_groups([
            group(STEM_COLUMNS, &stem_terms),
            group("exact", &exact_terms),
        ]);
        let identifier_forms: Vec<String> = self
            .identifier_forms
            .iter()
            .map(|form| quote(form))
            .collect();
        let identifiers = join_groups([
            group(STEM_COLUMNS, &self.identifier_phrases),
            group("exact", &identifier_forms),
        ]);
        FtsQuery {
            all: all.unwrap_or_default(),
            identifiers,
        }
    }
}

/// Предел пар соседних слов в сигнале `p`: каждая пара — один запрос к FTS5.
const MAX_PAIRS: usize = 12;

/// Выражения FTS5 сигнала `p` — точного совпадения в колонке `exact`.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ExactProbe {
    /// Целая форма идентификатора запроса, по одному условию на каждый; нужны все.
    identifiers: Vec<String>,
    /// Фраза из всех слов запроса подряд; `None` у запроса из одного слова.
    whole: Option<String>,
    /// Пары соседних слов запроса.
    pairs: Vec<String>,
}

impl QueryTerms {
    /// Выражения сигнала `p`: идентификаторы целиком, запрос целиком, пары слов подряд.
    pub fn exact_probe(&self) -> ExactProbe {
        let sequence = &self.exact_sequence;
        let whole = (sequence.len() > 1).then(|| format!("exact : {}", quote(&sequence.join(" "))));
        let pairs = sequence
            .windows(2)
            .take(MAX_PAIRS)
            .map(|pair| format!("exact : {}", quote(&pair.join(" "))))
            .collect();
        let identifiers = self
            .identifier_forms
            .iter()
            .map(|form| format!("exact : {}", quote(form)))
            .collect();
        ExactProbe {
            identifiers,
            whole,
            pairs,
        }
    }
}

/// Выражение по запросу без исправления опечаток. `None`, если в запросе нет ни одного слова.
pub fn fts_query(query: &str) -> Option<FtsQuery> {
    QueryTerms::parse(query).map(|terms| terms.expression(&[]))
}

/// Выражение по запросу с исправлением опечаток по словарю индекса: слова запроса, которых в
/// индексе нет, дополняются ближайшими словами словаря. Индекс без таблицы словаря (записан
/// читателем до пересборки) ищет без исправлений.
pub fn fts_query_for(conn: &Connection, query: &str) -> rusqlite::Result<Option<FtsQuery>> {
    Ok(fts_plan_for(conn, query)?.map(|(expression, _)| expression))
}

/// То же вместе с выражениями сигнала `p` (точное совпадение).
pub fn fts_plan_for(
    conn: &Connection,
    query: &str,
) -> rusqlite::Result<Option<(FtsQuery, ExactProbe)>> {
    let Some(terms) = QueryTerms::parse(query) else {
        return Ok(None);
    };
    let corrections = if has_vocab(conn)? {
        typo_corrections(conn, &terms.stems)?
    } else {
        Vec::new()
    };
    Ok(Some((terms.expression(&corrections), terms.exact_probe())))
}

/// Слово запроса похоже на идентификатор или имя файла: `agent_min_score`, `settings.toml`,
/// `a/x.md`. Знаки по краям (точка в конце предложения, скобки) не в счёт.
pub fn is_identifier(raw: &str) -> bool {
    raw.trim_matches(|c: char| !c.is_alphanumeric())
        .contains(['_', '.', '/'])
}

/// Сырой полнотекстовый балл строки списка FTS: `bm25` с обратным знаком, больше — лучше.
pub fn raw_score(bm25: f64) -> f32 {
    (-bm25).max(0.0) as f32
}

/// Балл `f` чанков `ids`: сырой балл, делённый на лучший балл списка FTS этого запроса (`listed`,
/// по убыванию), от 0 до 1. Чанку вне списка `bm25` считается одним запросом по `rowid`; чанк, в
/// котором не совпало ни одно слово запроса, получает 0. Выражение — `FtsQuery::all`.
pub fn f_signal(
    conn: &Connection,
    expression: &str,
    listed: &[(i64, f32)],
    ids: &[i64],
) -> rusqlite::Result<HashMap<i64, f32>> {
    let best = listed
        .iter()
        .map(|(_, score)| *score)
        .fold(0.0_f32, f32::max);
    let mut known: HashMap<i64, f32> = listed.iter().copied().collect();
    let missing: Vec<i64> = ids
        .iter()
        .copied()
        .filter(|id| !known.contains_key(id))
        .collect();
    for part in missing.chunks(500) {
        let placeholders = vec!["?"; part.len()].join(", ");
        let sql = format!(
            "SELECT rowid, {BM25} FROM chunks_fts \
             WHERE chunks_fts MATCH ? AND rowid IN ({placeholders})"
        );
        let mut params = vec![Value::from(expression.to_string())];
        params.extend(part.iter().map(|id| Value::Integer(*id)));
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(params), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?))
        })?;
        for row in rows {
            let (id, bm25) = row?;
            known.insert(id, raw_score(bm25));
        }
    }
    Ok(ids
        .iter()
        .map(|id| {
            let raw = known.get(id).copied().unwrap_or(0.0);
            let score = if best > 0.0 {
                (raw / best).clamp(0.0, 1.0)
            } else {
                0.0
            };
            (*id, score)
        })
        .collect())
}

/// Чанки из `ids`, которые совпали с выражением FTS5.
pub(super) fn matching(
    conn: &Connection,
    expression: &str,
    ids: &[i64],
) -> rusqlite::Result<std::collections::HashSet<i64>> {
    let mut found = std::collections::HashSet::new();
    for part in ids.chunks(500) {
        let placeholders = vec!["?"; part.len()].join(", ");
        let sql = format!(
            "SELECT rowid FROM chunks_fts WHERE chunks_fts MATCH ? AND rowid IN ({placeholders})"
        );
        let mut params = vec![Value::from(expression.to_string())];
        params.extend(part.iter().map(|id| Value::Integer(*id)));
        let mut statement = conn.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(params), |row| row.get::<_, i64>(0))?;
        for row in rows {
            found.insert(row?);
        }
    }
    Ok(found)
}

/// Балл `p` чанков `ids`: 1, если в колонке точной формы есть все идентификаторы запроса или весь
/// запрос подряд; иначе доля пар соседних слов запроса, найденных подряд; 0, если запрос из одного
/// слова без идентификатора и это слово — единственное, что в нём есть (его даёт сигнал `f`).
pub fn p_signal(
    conn: &Connection,
    probe: &ExactProbe,
    ids: &[i64],
) -> rusqlite::Result<HashMap<i64, f32>> {
    let mut scores: HashMap<i64, f32> = ids.iter().map(|id| (*id, 0.0)).collect();
    if ids.is_empty() {
        return Ok(scores);
    }
    let mut full: std::collections::HashSet<i64> = std::collections::HashSet::new();
    if !probe.identifiers.is_empty() {
        let expression = probe.identifiers.join(" AND ");
        full.extend(matching(conn, &expression, ids)?);
    }
    if let Some(whole) = &probe.whole {
        full.extend(matching(conn, whole, ids)?);
    }
    let mut pair_hits: HashMap<i64, usize> = HashMap::new();
    if !probe.pairs.is_empty() {
        for pair in &probe.pairs {
            for id in matching(conn, pair, ids)? {
                *pair_hits.entry(id).or_default() += 1;
            }
        }
    }
    for id in ids {
        let score = if full.contains(id) {
            1.0
        } else if probe.pairs.is_empty() {
            0.0
        } else {
            pair_hits.get(id).copied().unwrap_or(0) as f32 / probe.pairs.len() as f32
        };
        scores.insert(*id, score);
    }
    Ok(scores)
}

pub(super) fn push_unique(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

pub(super) fn quote(token: &str) -> String {
    format!("\"{token}\"")
}

/// `колонки : (a OR b OR …)`; `None` для пустого списка.
pub(super) fn group(columns: &str, terms: &[String]) -> Option<String> {
    (!terms.is_empty()).then(|| format!("{columns} : ({})", terms.join(" OR ")))
}

pub(super) fn join_groups(groups: [Option<String>; 2]) -> Option<String> {
    let parts: Vec<String> = groups.into_iter().flatten().collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

fn has_vocab(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'chunks_fts_vocab')",
        [],
        |row| row.get(0),
    )
}

/// Слова словаря вместо опечаток: для каждой основы запроса, которой нет в индексе, до трёх
/// ближайших по Дамерау—Левенштейну слов с той же первой буквой (расстояние не больше 1 для основ
/// до 7 знаков и 2 для 8 и длиннее), сначала ближайшие, затем частые. Исправляются только слова от
/// четырёх знаков и только из букв; опечатка в первой букве не исправляется.
pub fn typo_corrections(conn: &Connection, stems: &[String]) -> rusqlite::Result<Vec<String>> {
    let mut corrections = Vec::new();
    for stem in stems {
        let chars: Vec<char> = stem.chars().collect();
        if chars.len() < TYPO_MIN_LEN || !chars.iter().all(|c| c.is_alphabetic()) {
            continue;
        }
        let known: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM chunks_fts_vocab WHERE term = ?1)",
            [stem],
            |row| row.get(0),
        )?;
        if known {
            continue;
        }
        let limit = if chars.len() <= 7 { 1 } else { 2 };
        let first = chars[0];
        let Some(next) = char::from_u32(first as u32 + 1) else {
            continue;
        };
        let mut statement =
            conn.prepare("SELECT term, doc FROM chunks_fts_vocab WHERE term >= ?1 AND term < ?2")?;
        let rows = statement.query_map([first.to_string(), next.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut near: Vec<(usize, i64, String)> = Vec::new();
        for row in rows {
            let (term, docs) = row?;
            let candidate: Vec<char> = term.chars().collect();
            if candidate.len().abs_diff(chars.len()) > limit
                || !candidate.iter().all(|c| c.is_alphabetic())
            {
                continue;
            }
            let distance = edit_distance(&chars, &candidate);
            if (1..=limit).contains(&distance) {
                near.push((distance, docs, term));
            }
        }
        near.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
        for (_, _, term) in near.into_iter().take(TYPO_VARIANTS) {
            push_unique(&mut corrections, term);
        }
    }
    Ok(corrections)
}

/// Расстояние Дамерау—Левенштейна (с перестановкой соседних знаков).
pub fn edit_distance(a: &[char], b: &[char]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, cell) in d[0].iter_mut().enumerate() {
        *cell = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut best = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                best = best.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = best;
        }
    }
    d[n][m]
}

#[cfg(test)]
mod tests;
