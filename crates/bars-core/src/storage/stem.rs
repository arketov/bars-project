//! Стемминг текста для FTS5 (`docs/storage/schema.md`, «Стемминг для FTS5»).
//!
//! Один и тот же разбор применяется к тексту чанков при индексации и к словам запроса при поиске.

use rust_stemmers::{Algorithm, Stemmer};

/// Версия разбора. Меняется вместе с правилами: тогда `chunks_fts` пересобирается.
pub const STEM_VERSION: &str = "1";

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
    /// Слово только из кириллических букв идёт в русский стеммер, только из латинских — в
    /// английский; остальные слова (числа, смесь алфавитов, цифры с буквами) остаются как есть.
    pub fn stem_text(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            if word.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            let lower = word.to_lowercase();
            if lower.chars().all(is_cyrillic_letter) {
                out.push_str(&self.russian.stem(&lower));
            } else if lower.chars().all(|c| c.is_ascii_alphabetic()) {
                out.push_str(&self.english.stem(&lower));
            } else {
                out.push_str(&lower);
            }
        }
        out
    }
}

fn is_cyrillic_letter(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04ff}') && c.is_alphabetic()
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
}
