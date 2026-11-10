//! Разбор текста для FTS5: основы слов и точная форма (`docs/architecture/schema.md`,
//! «Стемминг для FTS5»; `docs/knowledge/tokenization.md`, «Рекомендация для шагов 2.3.2 и 2.3.3»).
//!
//! Один и тот же разбор применяется к тексту чанков при индексации и к словам запроса при поиске.
//! Язык — свойство слова, а не документа: кириллица без `іїєґ` идёт в русский Snowball, латиница
//! в английский, остальное остаётся как есть. Сплошные отрезки китайских знаков и японской кана
//! режутся на биграммы; одиночный знак остаётся одним токеном.

use rust_stemmers::{Algorithm, Stemmer};

/// Версия разбора. Меняется вместе с правилами или набором колонок `chunks_fts`: тогда таблица
/// пересоздаётся и заполняется заново из `chunks`.
///
/// 2 — четыре колонки (название, заголовки, текст, точная форма), правила стемминга, формы
/// идентификаторов, биграммы CJK.
pub const STEM_VERSION: &str = "2";

/// Самое длинное слово из частей идентификатора, которое ещё считается идентификатором.
const MAX_IDENTIFIER_PARTS: usize = 8;

pub struct TextStemmer {
    russian: Stemmer,
    english: Stemmer,
}

impl Default for TextStemmer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextStemmer {
    pub fn new() -> Self {
        Self {
            russian: Stemmer::create(Algorithm::Russian),
            english: Stemmer::create(Algorithm::English),
        }
    }

    /// Основы слов текста через пробел.
    ///
    /// Не стеммятся слова ПРОПИСНЫМИ, короче четырёх знаков, со знаком-цифрой и с буквами
    /// `іїєґ` (украинского стеммера нет); `ё` приводится к `е`. CJK-отрезки — биграммы знаков.
    pub fn stem_text(&self, text: &str) -> String {
        let mut tokens = Vec::new();
        for piece in pieces(text) {
            if piece.cjk {
                push_bigrams(&piece.text, &mut tokens);
            } else {
                tokens.push(self.stem_word(&piece.text));
            }
        }
        tokens.join(" ")
    }

    /// Точная форма текста через пробел: слова без стемминга (регистр и `ё` не важны), CJK-биграммы
    /// и целые формы идентификаторов (`rag_query`, `274_1`, `58_фз`) после слов.
    pub fn exact_text(&self, text: &str) -> String {
        let mut tokens = exact_tokens(text);
        tokens.extend(identifier_forms(text));
        tokens.join(" ")
    }

    /// Только слова точной формы, без целых форм идентификаторов: так слова стоят в тексте подряд.
    /// По ним строится фраза оператора `"…"`: целые формы `exact_text` дописывает в конец списка
    /// токенов, и фраза вместе с ними подряд нигде не стоит (OQ-285).
    pub fn exact_words(&self, text: &str) -> String {
        exact_tokens(text).join(" ")
    }

    fn stem_word(&self, raw: &str) -> String {
        let lower = fold(raw);
        if lower.chars().count() < 4
            || is_all_caps(raw)
            || lower.chars().any(|c| c.is_numeric())
            || lower.chars().any(is_ukrainian_letter)
        {
            return lower;
        }
        if lower.chars().all(is_cyrillic_letter) {
            self.russian.stem(&lower).into_owned()
        } else if lower.chars().all(|c| c.is_ascii_alphabetic()) {
            self.english.stem(&lower).into_owned()
        } else {
            lower
        }
    }
}

/// Слова точной формы: без стемминга, регистр и `ё` не важны, CJK-биграммы.
fn exact_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for piece in pieces(text) {
        if piece.cjk {
            push_bigrams(&piece.text, &mut tokens);
        } else {
            tokens.push(fold(&piece.text));
        }
    }
    tokens
}

/// Целые формы идентификаторов текста: слово с `_`, `.`, `/` или `-` внутри даёт токен из частей,
/// склеенных `_` (`settings.toml` → `settings_toml`, `Т-12` → `т_12`). Знаки по краям не в счёт.
/// Отрезки с CJK идентификаторами не считаются.
pub fn identifier_forms(text: &str) -> Vec<String> {
    let mut forms: Vec<String> = Vec::new();
    let mut run = String::new();
    let flush = |run: &mut String, forms: &mut Vec<String>| {
        if let Some(form) = identifier_form(run)
            && !forms.contains(&form)
        {
            forms.push(form);
        }
        run.clear();
    };
    for c in text.chars() {
        if c.is_alphanumeric() || matches!(c, '_' | '.' | '/' | '-') {
            run.push(c);
        } else {
            flush(&mut run, &mut forms);
        }
    }
    flush(&mut run, &mut forms);
    forms
}

fn identifier_form(run: &str) -> Option<String> {
    let trimmed = run.trim_matches(|c: char| !c.is_alphanumeric());
    if trimmed.chars().any(is_cjk) {
        return None;
    }
    let parts: Vec<String> = trimmed
        .split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(fold)
        .collect();
    // Только слово с разделителем внутри; обычное слово — не идентификатор.
    if parts.len() < 2 || parts.len() > MAX_IDENTIFIER_PARTS {
        return None;
    }
    Some(parts.join("_"))
}

struct Piece {
    text: String,
    cjk: bool,
}

/// Куски текста: отрезки букв и цифр, CJK-знаки отдельно от остальных.
fn pieces(text: &str) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    let mut current = String::new();
    let mut current_cjk = false;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            flush_piece(&mut current, current_cjk, &mut out);
            continue;
        }
        let cjk = is_cjk(c);
        if !current.is_empty() && cjk != current_cjk {
            flush_piece(&mut current, current_cjk, &mut out);
        }
        current_cjk = cjk;
        current.push(c);
    }
    flush_piece(&mut current, current_cjk, &mut out);
    out
}

fn flush_piece(current: &mut String, cjk: bool, out: &mut Vec<Piece>) {
    if !current.is_empty() {
        out.push(Piece {
            text: std::mem::take(current),
            cjk,
        });
    }
}

fn push_bigrams(run: &str, out: &mut Vec<String>) {
    let chars: Vec<char> = run.chars().collect();
    if chars.len() == 1 {
        out.push(run.to_string());
        return;
    }
    for pair in chars.windows(2) {
        out.push(pair.iter().collect());
    }
}

fn fold(word: &str) -> String {
    word.to_lowercase().replace('ё', "е")
}

/// Слово из двух и больше букв, все буквы прописные: аббревиатура (`ОПИ`, `КИИ`).
fn is_all_caps(raw: &str) -> bool {
    raw.chars().filter(|c| c.is_alphabetic()).count() >= 2
        && raw.chars().all(|c| !c.is_alphabetic() || c.is_uppercase())
}

fn is_cyrillic_letter(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04ff}') && c.is_alphabetic()
}

fn is_ukrainian_letter(c: char) -> bool {
    matches!(c, 'і' | 'ї' | 'є' | 'ґ')
}

/// Китайские знаки и японская кана: слова между ними не отделяются пробелами.
pub fn is_cjk(c: char) -> bool {
    matches!(
        c,
        '\u{3040}'..='\u{30ff}'
            | '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2fa1f}'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn russian_forms_share_a_stem() {
        let stemmer = TextStemmer::new();
        assert_eq!(
            stemmer.stem_text("Настройки"),
            stemmer.stem_text("настройка")
        );
        assert_eq!(
            stemmer.stem_text("документами"),
            stemmer.stem_text("документ")
        );
    }

    #[test]
    fn english_forms_share_a_stem() {
        let stemmer = TextStemmer::new();
        assert_eq!(stemmer.stem_text("Indexing"), stemmer.stem_text("indexes"));
    }

    #[test]
    fn mixed_and_numeric_words_stay_as_they_are() {
        let stemmer = TextStemmer::new();
        assert_eq!(stemmer.stem_text("v2 h2o 2026"), "v2 h2o 2026");
        // Кириллица вперемешку с латиницей не стеммится ни одним из стеммеров.
        assert_eq!(stemmer.stem_text("Aбв"), "aбв");
    }

    #[test]
    fn punctuation_splits_words_and_whitespace_collapses() {
        let stemmer = TextStemmer::new();
        assert_eq!(
            stemmer.stem_text("rag_query,  `bars`;\n\tтест"),
            stemmer.stem_text("rag query bars тест")
        );
        assert_eq!(stemmer.stem_text("  ,.; "), "");
    }

    #[test]
    fn abbreviations_and_short_words_are_not_stemmed() {
        let stemmer = TextStemmer::new();
        // `ОПИ` и `КИИ` без правила стали бы `оп` и `ки`.
        assert_eq!(stemmer.stem_text("ОПИ КИИ"), "опи кии");
        assert_eq!(stemmer.stem_text("она был"), "она был");
        // Строчная форма того же слова, которую стеммер меняет, остаётся прежней у короткого слова.
        assert_ne!(stemmer.stem_text("настройки"), "настройки");
    }

    #[test]
    fn yo_is_folded_to_ye_in_both_forms() {
        let stemmer = TextStemmer::new();
        assert_eq!(
            stemmer.stem_text("Ёлка тёмный"),
            stemmer.stem_text("елка темный")
        );
        assert_eq!(stemmer.exact_text("ёлка"), "елка");
    }

    #[test]
    fn ukrainian_words_are_kept_as_written() {
        let stemmer = TextStemmer::new();
        assert_eq!(stemmer.stem_text("налаштування їжак"), "налаштування їжак");
    }

    #[test]
    fn exact_text_keeps_forms_and_adds_identifiers() {
        let stemmer = TextStemmer::new();
        assert_eq!(
            stemmer.exact_text("Настройки rag_query"),
            "настройки rag query rag_query"
        );
        assert_eq!(stemmer.exact_text("Т-12, 274_1."), "т 12 274 1 т_12 274_1");
        // Для фразы оператора — только слова, без целых форм (OQ-285).
        assert_eq!(
            stemmer.exact_words("BARS_MCP_CALL_TIMEOUT_MS"),
            "bars mcp call timeout ms"
        );
        // Дефисное слово и имя файла тоже дают целую форму.
        assert_eq!(identifier_forms("по-русски"), ["по_русски"]);
        assert_eq!(identifier_forms("см. settings.toml,"), ["settings_toml"]);
        // Слово без разделителя и знаки по краям — не идентификатор.
        assert!(identifier_forms("слово. -тире- ...").is_empty());
    }

    #[test]
    fn cjk_runs_become_bigrams_and_single_signs_stay() {
        let stemmer = TextStemmer::new();
        assert_eq!(stemmer.stem_text("向量检索"), "向量 量检 检索");
        assert_eq!(stemmer.exact_text("向量检索"), "向量 量检 检索");
        assert_eq!(stemmer.stem_text("设"), "设");
        // Латиница рядом с CJK отделяется и стеммится как обычное слово.
        assert_eq!(stemmer.stem_text("bars向量"), "bar 向量");
        assert!(identifier_forms("向量-検索").is_empty());
    }
}
