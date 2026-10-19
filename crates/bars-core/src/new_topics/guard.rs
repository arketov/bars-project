//! Осторожные лексические проверки перед слиянием вопросов (`docs/knowledge/new-topics-threshold-2026-10-19.md`).
//!
//! Косинус не отличает вопросы, различающиеся числом, версией, идентификатором или противоположным
//! словом, поэтому автослияние идёт только если эти проверки отличий не видят. Список противоположностей
//! короткий и полноты не обещает (OQ-100).

const NUMBER_WORDS: &[(u8, &[&str])] = &[
    (0, &["ноль", "нуля"]),
    (
        1,
        &[
            "один", "одна", "одно", "одного", "первый", "первая", "первое", "первой", "первого",
            "первом",
        ],
    ),
    (
        2,
        &["два", "две", "двух", "второй", "вторая", "второе", "второго", "втором", "вторую"],
    ),
    (
        3,
        &[
            "три", "трёх", "трех", "третий", "третья", "третье", "третьей", "третьего", "третьем",
        ],
    ),
    (4, &["четыре", "четырёх", "четырех", "четвёртый", "четвертый"]),
    (5, &["пять", "пяти", "пятый", "пятой"]),
    (6, &["шесть", "шести"]),
    (7, &["семь", "семи"]),
    (8, &["восемь", "восьми"]),
    (9, &["девять", "девяти"]),
    (10, &["десять", "десяти"]),
];

/// Пары префиксов противоположных слов: (первое слово, второе слово).
const ANTONYMS: &[(&[&str], &[&str])] = &[
    (&["включ"], &["отключ", "выключ"]),
    (&["открыт"], &["закрыт"]),
    (&["успешн"], &["ошибк", "неудач", "сбо"]),
    (&["разреш"], &["запрещ", "запрет"]),
];

fn tokens(text: &str) -> Vec<&str> {
    text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
        .filter(|token| !token.is_empty())
        .collect()
}

fn trim_marks(token: &str) -> &str {
    token.trim_matches(|c| c == '.' || c == '-')
}

/// Идентификатор, версия или число: такие токены не нормализуются и не склеиваются.
pub fn is_special(token: &str) -> bool {
    let t = trim_marks(token);
    if t.is_empty() {
        return false;
    }
    let latin = t.chars().all(|c| c.is_ascii_alphabetic());
    if t.chars().any(char::is_numeric) || t.contains('_') {
        return true;
    }
    if t.chars().any(|c| c.is_ascii_alphabetic()) && t.contains('.') {
        return true;
    }
    if latin
        && t.chars().skip(1).any(|c| c.is_ascii_uppercase())
        && t.chars().any(|c| c.is_ascii_lowercase())
    {
        return true;
    }
    if latin && t.chars().count() >= 2 && t.chars().all(|c| c.is_ascii_uppercase()) {
        return true;
    }
    let parts: Vec<&str> = t.split('-').collect();
    parts.len() >= 2
        && parts[0].chars().count() >= 1
        && parts[0].chars().all(|c| c.is_ascii_alphabetic())
        && parts[1..]
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

struct Features {
    special: std::collections::BTreeSet<String>,
    numbers: Vec<u8>,
    lower: Vec<String>,
}

fn features(text: &str) -> Features {
    let raw = tokens(text);
    let lower: Vec<String> = raw.iter().map(|t| t.to_lowercase()).collect();
    let special = raw
        .iter()
        .filter(|t| is_special(t))
        .map(|t| trim_marks(t).to_string())
        .collect();
    let mut numbers: Vec<u8> = lower
        .iter()
        .filter_map(|t| {
            NUMBER_WORDS
                .iter()
                .find(|(_, words)| words.contains(&t.as_str()))
                .map(|(n, _)| *n)
        })
        .collect();
    numbers.sort_unstable();
    Features {
        special,
        numbers,
        lower,
    }
}

fn has_prefix(lower: &[String], prefixes: &[&str]) -> bool {
    lower
        .iter()
        .any(|t| prefixes.iter().any(|p| t.starts_with(p)))
}

/// Вопросы различаются по числу, версии, идентификатору или короткому списку противоположностей:
/// автоматически сливать их нельзя.
pub fn differs(a: &str, b: &str) -> bool {
    let (a, b) = (features(a), features(b));
    // У перефраза идентификатор может исчезнуть только вместе с вопросом: два пустых множества равны.
    if (!a.special.is_empty() || !b.special.is_empty()) && a.special != b.special {
        return true;
    }
    if !a.numbers.is_empty() && !b.numbers.is_empty() && a.numbers != b.numbers {
        return true;
    }
    ANTONYMS.iter().any(|(x, y)| {
        let (ax, ay) = (has_prefix(&a.lower, x), has_prefix(&a.lower, y));
        let (bx, by) = (has_prefix(&b.lower, x), has_prefix(&b.lower, y));
        (ax && !ay && by && !bx) || (ay && !ax && bx && !by)
    })
}

/// Ключ точного совпадения: пробелы и переносы схлопнуты, обычные слова приведены к нижнему регистру;
/// отрицания, числа, версии, идентификаторы и знаки кода сохраняются как есть.
pub fn normalize_question(text: &str) -> String {
    text.split_whitespace()
        .map(|word| {
            if is_special(word) {
                word.to_string()
            } else {
                word.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
