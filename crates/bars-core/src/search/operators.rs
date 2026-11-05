//! Операторы запроса: `"фраза"`, `-слово`, `-"фраза"`, `path:папка` / `папка:папка`
//! (`docs/features/search/hybrid-search.md`, раздел «Операторы запроса», ADR-0007, OQ-265).
//!
//! Разбор идёт до всего остального и одинаков для окна, MCP и CLI. Слова операторов не попадают в
//! обычный запрос: фраза даёт условие по колонке точной формы, исключение — `NOT` по основе слова
//! и по точной форме, `path:` — отбор по пути документа (`hybrid::Scope`).

use rusqlite::Connection;

use super::fulltext::{STEM_COLUMNS, group, join_groups, matching, push_unique, quote};
use crate::storage::stem::TextStemmer;

/// Приставки оператора папки (регистр не важен).
const PATH_PREFIXES: [&str; 2] = ["path:", "папка:"];

/// Запрос после разбора операторов.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedQuery {
    /// Слова запроса для вектора, rerank и полнотекстовой части: обычные слова и слова фраз,
    /// кавычки сняты, исключения и фильтры убраны.
    pub text: String,
    /// Фразы в кавычках: слова подряд.
    pub phrases: Vec<String>,
    /// Исключённые слова (`-слово`).
    pub excluded_words: Vec<String>,
    /// Исключённые фразы (`-"фраза"`).
    pub excluded_phrases: Vec<String>,
    /// Префиксы пути (`path:` и `папка:`), как их написал пользователь.
    pub paths: Vec<String>,
}

impl ParsedQuery {
    /// В запросе есть хотя бы один оператор.
    pub fn has_operators(&self) -> bool {
        !(self.phrases.is_empty()
            && self.excluded_words.is_empty()
            && self.excluded_phrases.is_empty()
            && self.paths.is_empty())
    }

    /// Разбирает запрос. Оператор — слово, которое начинается с `-` и дальше идёт буква, либо
    /// `path:` / `папка:` в начале слова, либо пара кавычек. `-5`, `по-русски`, `274-ФЗ`, одиночная
    /// кавычка и дефис внутри слова — обычные символы.
    pub fn parse(query: &str) -> Self {
        let chars: Vec<char> = query.chars().collect();
        let mut parsed = Self::default();
        let mut words: Vec<String> = Vec::new();
        let mut at = 0;
        while at < chars.len() {
            if chars[at].is_whitespace() {
                at += 1;
                continue;
            }
            // Оператор распознаётся только в начале слова.
            if let Some((value, end)) = quoted(&chars, at) {
                if has_content(&value) {
                    words.push(value.clone());
                    push_unique(&mut parsed.phrases, value);
                }
                at = end;
                continue;
            }
            if chars[at] == '-' {
                if let Some((value, end)) = quoted(&chars, at + 1) {
                    if has_content(&value) {
                        push_unique(&mut parsed.excluded_phrases, value);
                    }
                    at = end;
                    continue;
                }
                if chars.get(at + 1).is_some_and(|c| c.is_alphabetic()) {
                    let end = word_end(&chars, at);
                    let value: String = chars[at + 1..end].iter().collect();
                    let value = trim_edges(&value);
                    if has_content(&value) {
                        push_unique(&mut parsed.excluded_words, value);
                    }
                    at = end;
                    continue;
                }
            }
            if let Some(prefix_len) = path_prefix(&chars, at) {
                let value_start = at + prefix_len;
                let (value, end) = match quoted(&chars, value_start) {
                    Some(found) => found,
                    None => {
                        let end = word_end(&chars, value_start);
                        (chars[value_start..end].iter().collect(), end)
                    }
                };
                let value = value.trim().trim_matches('/').to_string();
                if !value.is_empty() {
                    push_unique(&mut parsed.paths, value);
                    at = end;
                    continue;
                }
            }
            let end = word_end(&chars, at);
            words.push(chars[at..end].iter().collect());
            at = end;
        }
        parsed.text = words.join(" ");
        parsed
    }

    /// Условия по полям FTS5 для фраз и исключений.
    pub fn filter(&self) -> OperatorFilter {
        let stemmer = TextStemmer::new();
        let required = self
            .phrases
            .iter()
            .filter_map(|phrase| exact_phrase(&stemmer, phrase))
            .map(|phrase| format!("exact : {phrase}"))
            .collect();
        let mut stem_phrases = Vec::new();
        let mut exact_phrases = Vec::new();
        for item in self.excluded_words.iter().chain(&self.excluded_phrases) {
            if let Some(stems) = phrase_of(&stemmer.stem_text(item)) {
                push_unique(&mut stem_phrases, stems);
            }
            if let Some(exact) = exact_phrase(&stemmer, item) {
                push_unique(&mut exact_phrases, exact);
            }
        }
        let excluded = join_groups([
            group(STEM_COLUMNS, &stem_phrases),
            group("exact", &exact_phrases),
        ]);
        OperatorFilter { required, excluded }
    }
}

/// `"текст"` с позиции `at`: значение и позиция за закрывающей кавычкой. Кавычка без пары —
/// обычный символ, и `None`.
fn quoted(chars: &[char], at: usize) -> Option<(String, usize)> {
    if chars.get(at) != Some(&'"') {
        return None;
    }
    let close = chars[at + 1..].iter().position(|c| *c == '"')? + at + 1;
    Some((chars[at + 1..close].iter().collect(), close + 1))
}

/// Конец слова: позиция первого пробельного знака после `from` (или конец строки).
fn word_end(chars: &[char], from: usize) -> usize {
    chars[from..]
        .iter()
        .position(|c| c.is_whitespace())
        .map_or(chars.len(), |offset| from + offset)
}

/// Длина приставки `path:` / `папка:` в начале слова с позиции `at`.
fn path_prefix(chars: &[char], at: usize) -> Option<usize> {
    PATH_PREFIXES.iter().find_map(|prefix| {
        let len = prefix.chars().count();
        let head: String = chars.get(at..at + len)?.iter().collect();
        (head.to_lowercase() == *prefix).then_some(len)
    })
}

fn trim_edges(value: &str) -> String {
    value
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_string()
}

fn has_content(value: &str) -> bool {
    value.chars().any(char::is_alphanumeric)
}

/// Фраза FTS5 из токенов точной формы: слова подряд, без стемминга. `None`, если токенов нет.
fn exact_phrase(stemmer: &TextStemmer, text: &str) -> Option<String> {
    phrase_of(&stemmer.exact_text(text))
}

fn phrase_of(tokens: &str) -> Option<String> {
    let tokens: Vec<&str> = tokens.split_whitespace().collect();
    (!tokens.is_empty()).then(|| quote(&tokens.join(" ")))
}

/// Условия операторов в виде выражений FTS5: обязательные фразы и исключения.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OperatorFilter {
    /// По одному условию на каждую фразу: `exact : "a b c"`; чанк без фразы в выдачу не попадает.
    required: Vec<String>,
    /// Любое из исключённых слов и фраз (по основе и по точной форме); чанк с ним в выдачу не
    /// попадает.
    excluded: Option<String>,
}

impl OperatorFilter {
    pub fn is_empty(&self) -> bool {
        self.required.is_empty() && self.excluded.is_none()
    }

    /// Выражение запроса с условиями: `((выражение) AND (фраза)) NOT (исключения)`.
    pub fn apply(&self, expression: &str) -> String {
        let mut combined = format!("({expression})");
        for required in &self.required {
            combined = format!("({combined} AND ({required}))");
        }
        if let Some(excluded) = &self.excluded {
            combined = format!("({combined} NOT ({excluded}))");
        }
        combined
    }

    /// Оставляет в `ids` только чанки, которые проходят условия: кандидатов, пришедших не из
    /// полнотекстового списка (из векторов), проверяют по колонкам FTS одним запросом на пачку.
    pub fn retain(&self, conn: &Connection, ids: &mut Vec<i64>) -> rusqlite::Result<()> {
        if self.is_empty() || ids.is_empty() {
            return Ok(());
        }
        if !self.required.is_empty() {
            let expression = self
                .required
                .iter()
                .map(|required| format!("({required})"))
                .collect::<Vec<_>>()
                .join(" AND ");
            let found = matching(conn, &expression, ids)?;
            ids.retain(|id| found.contains(id));
        }
        if let Some(excluded) = &self.excluded {
            let found = matching(conn, excluded, ids)?;
            ids.retain(|id| !found.contains(id));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
