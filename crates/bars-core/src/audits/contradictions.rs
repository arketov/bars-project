//! Числовые кандидаты по исследованию 3.8.1; не доказательство противоречия.
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};

use super::{NewFinding, Subject, document_role, fingerprint};

pub struct Chunk {
    pub path: String,
    pub module: String,
    pub heading: String,
    pub first_line: u32,
    pub last_line: u32,
    pub text: String,
}

#[derive(Debug, PartialEq)]
struct Number {
    /// Значение в основной единице семейства: секунды, байты; без единицы — как написано.
    value: f64,
    /// Семейство единицы («время», «размер», «%»…), пусто без единицы.
    unit: String,
    key: String,
    /// Число с единицей, как в тексте: показывается в находке.
    raw: String,
}
struct Statement {
    chunk: usize,
    text: String,
    words: BTreeSet<String>,
    ids: BTreeSet<String>,
    nums: Vec<Number>,
}

/// Слова перед номером, а не величиной: «раунд 16», «ответ 5», «table 3», «fixes 5». Сравниваются по корню `stem`.
const LABELS: &[&str] = &[
    "раунд",
    "ответ",
    "наблю",
    "пункт",
    "шаг",
    "этап",
    "срез",
    "блок",
    "вариа",
    "решен",
    "табли",
    "рисун",
    "глава",
    "главе",
    "разде",
    "строк",
    "верси",
    "сцена",
    "задач",
    "round",
    "step",
    "steps",
    "stage",
    "table",
    "figur",
    "secti",
    "chapt",
    "claus",
    "fix",
    "fixes",
    "item",
    "items",
    "issue",
    "run",
    "versi",
    "line",
    "lines",
    "page",
    "phase",
    "optio",
    "case",
    "no",
    "part",
    "pr",
];
/// Месяц перед числом делает его датой: «Jul 28», «28 октября» здесь не ловится, но и не мешает.
const MONTHS: &[&str] = &[
    "янв",
    "фев",
    "мар",
    "апр",
    "мая",
    "июн",
    "июл",
    "авг",
    "сен",
    "сент",
    "окт",
    "ноя",
    "дек",
    "jan",
    "feb",
    "mar",
    "apr",
    "may",
    "jun",
    "jul",
    "aug",
    "sep",
    "sept",
    "oct",
    "nov",
    "dec",
    "january",
    "february",
    "march",
    "april",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

fn stem(word: &str) -> String {
    word.chars().take(5).collect()
}

fn identifiers(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric() && !"-.".contains(c))
        .filter_map(|part| {
            let (prefix, rest) = part.split_once('-')?;
            if !["т", "нфт", "oq", "iss", "idea", "adr", "в", "д"]
                .contains(&prefix.to_lowercase().as_str())
            {
                return None;
            }
            let number: String = rest
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            if number.is_empty() {
                None
            } else {
                Some(format!(
                    "{}-{}",
                    prefix.to_uppercase(),
                    number.trim_end_matches('.')
                ))
            }
        })
        .collect()
}
fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .replace('ё', "е")
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Единица после числа: семейство и множитель к основной единице семейства.
/// Единицы одного семейства сравниваются между собой: «120 мс» и «0,12 с» согласованы.
fn unit(word: &str) -> Option<(&'static str, f64)> {
    const KIB: f64 = 1024.0;
    let exact = match word {
        "мс" | "ms" => Some(("время", 0.001)),
        "с" | "сек" | "s" | "sec" | "secs" => Some(("время", 1.0)),
        "мин" | "min" | "mins" => Some(("время", 60.0)),
        "ч" | "h" | "hr" | "hrs" => Some(("время", 3600.0)),
        "день" | "сутки" | "суток" | "day" | "days" => Some(("время", 86400.0)),
        "кб" | "kb" | "киб" | "kib" => Some(("размер", KIB)),
        "мб" | "mb" | "миб" | "mib" => Some(("размер", KIB * KIB)),
        "гб" | "gb" | "гиб" | "gib" => Some(("размер", KIB * KIB * KIB)),
        "%" => Some(("%", 1.0)),
        "шт" => Some(("шт", 1.0)),
        "раз" | "раза" | "times" => Some(("раз", 1.0)),
        "char" | "chars" => Some(("знак", 1.0)),
        _ => None,
    };
    exact.or_else(|| {
        [
            ("миллисекунд", "время", 0.001),
            ("millisecond", "время", 0.001),
            ("секунд", "время", 1.0),
            ("second", "время", 1.0),
            ("минут", "время", 60.0),
            ("minute", "время", 60.0),
            ("час", "время", 3600.0),
            ("hour", "время", 3600.0),
            ("дн", "время", 86400.0),
            ("кбайт", "размер", KIB),
            ("килобайт", "размер", KIB),
            ("мбайт", "размер", KIB * KIB),
            ("мегабайт", "размер", KIB * KIB),
            ("гбайт", "размер", KIB * KIB * KIB),
            ("гигабайт", "размер", KIB * KIB * KIB),
            ("байт", "размер", 1.0),
            ("byte", "размер", 1.0),
            ("слов", "слов", 1.0),
            ("word", "слов", 1.0),
            ("знак", "знак", 1.0),
            ("символ", "знак", 1.0),
            ("character", "знак", 1.0),
            ("строк", "строк", 1.0),
            ("line", "строк", 1.0),
            ("штук", "шт", 1.0),
            ("чанк", "чанк", 1.0),
            ("chunk", "чанк", 1.0),
            ("документ", "док", 1.0),
            ("document", "док", 1.0),
            ("токен", "токен", 1.0),
            ("token", "токен", 1.0),
            ("пар", "пар", 1.0),
            ("измерен", "изм", 1.0),
        ]
        .into_iter()
        .find(|(prefix, _, _)| word.starts_with(prefix))
        .map(|(_, family, factor)| (family, factor))
    })
}

/// «18.10» — день и месяц, а не дробь; дробь в русском тексте пишется через запятую.
fn day_month(raw: &str) -> bool {
    raw.split_once('.').is_some_and(|(day, month)| {
        day.len() <= 2
            && month.len() == 2
            && day.parse::<u32>().is_ok_and(|d| (1..=31).contains(&d))
            && month.parse::<u32>().is_ok_and(|m| (1..=12).contains(&m))
    })
}

fn numbers(text: &str) -> Vec<Number> {
    let text = text
        .to_lowercase()
        .replace('ё', "е")
        .replace(['`', '*'], "");
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !chars[i].1.is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i].1.is_ascii_digit() {
            i += 1;
        }
        if i + 1 < chars.len() && matches!(chars[i].1, '.' | ',') && chars[i + 1].1.is_ascii_digit()
        {
            i += 1;
            while i < chars.len() && chars[i].1.is_ascii_digit() {
                i += 1;
            }
        }
        let end = i;
        // Буква или знак вплотную перед числом: идентификатор, путь, хеш, номер (`№36`), сумма (`5+6`).
        let forbidden = |c: char| c.is_alphanumeric() || "_.-:/#`\\№+".contains(c);
        if start > 0 && forbidden(chars[start - 1].1)
            || i + 1 < chars.len()
                && "-:/.,".contains(chars[i].1)
                && chars[i + 1].1.is_ascii_digit()
        {
            continue;
        }
        let byte = chars[start].0;
        let end_byte = chars.get(end).map_or(text.len(), |c| c.0);
        let raw = &text[byte..end_byte];
        if day_month(raw) {
            continue;
        }
        let Ok(value) = raw.replace(',', ".").parse::<f64>() else {
            continue;
        };
        if value >= 10_000_000.0 {
            continue;
        }
        let head = text[..byte].trim_end_matches([' ', '\u{a0}']);
        let prev: String = {
            let reversed: Vec<char> = head
                .chars()
                .rev()
                .take_while(|c| c.is_alphabetic())
                .collect();
            reversed.into_iter().rev().collect()
        };
        if head.ends_with('№')
            || !prev.is_empty()
                && (LABELS.contains(&stem(&prev).as_str()) || MONTHS.contains(&prev.as_str()))
        {
            continue;
        }
        let before: Vec<_> = text[..byte]
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| w.chars().count() >= 3)
            .collect();
        let key = before
            .iter()
            .rev()
            .take(2)
            .rev()
            .map(|w| stem(w))
            .collect::<Vec<_>>()
            .join(" ");
        let rest = &text[end_byte..];
        let name: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphabetic() || *c == '%')
            .collect();
        let found = unit(&name);
        // Буквы вплотную после числа без известной единицы: хеш `1b9dad6`, модель `4b`, время `7pm`.
        if rest.starts_with(char::is_alphabetic) && found.is_none() {
            continue;
        }
        let (family, factor) = found.unwrap_or(("", 1.0));
        result.push(Number {
            value: value * factor,
            key,
            unit: family.to_owned(),
            raw: if family.is_empty() {
                raw.to_owned()
            } else {
                format!("{raw} {name}")
            },
        });
    }
    result
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.02 * a.abs().max(b.abs())
}
fn disjoint(a: &[&Number], b: &[&Number]) -> bool {
    !a.is_empty() && !b.is_empty() && a.iter().all(|x| b.iter().all(|y| !close(x.value, y.value)))
}
fn display(values: &[&Number]) -> String {
    values
        .iter()
        .map(|n| n.raw.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}
fn field(n: &Number, by_unit: bool) -> &str {
    if by_unit { &n.unit } else { &n.key }
}

struct Conflict {
    tag: &'static str,
    reason: String,
    /// Сколько значений разошлось на стороне, где их больше.
    changed: usize,
}

fn conflict(a: &[Number], b: &[Number]) -> Option<Conflict> {
    for (tag, by_unit) in [("num", true), ("slot", false)] {
        let keys: BTreeSet<&str> = a
            .iter()
            .map(|n| field(n, by_unit))
            .filter(|k| !k.is_empty())
            .collect();
        for key in keys {
            let av: Vec<&Number> = a.iter().filter(|n| field(n, by_unit) == key).collect();
            let bv: Vec<&Number> = b.iter().filter(|n| field(n, by_unit) == key).collect();
            if disjoint(&av, &bv) {
                return Some(Conflict {
                    tag,
                    reason: format!("{key}: {} ↔ {}", display(&av), display(&bv)),
                    changed: av.len().max(bv.len()),
                });
            }
        }
    }
    let av: Vec<&Number> = a
        .iter()
        .filter(|n| b.iter().all(|m| !close(n.value, m.value)))
        .collect();
    let bv: Vec<&Number> = b
        .iter()
        .filter(|n| a.iter().all(|m| !close(n.value, m.value)))
        .collect();
    (!av.is_empty() && !bv.is_empty()).then(|| Conflict {
        tag: "swap",
        reason: format!("Значения: {} ↔ {}", display(&av), display(&bv)),
        changed: av.len().max(bv.len()),
    })
}

/// Дата `2026-07-26`, `26-07-2026` или `26.07.2026` в пути или заголовке.
fn has_date(s: &str) -> bool {
    let b = s.as_bytes();
    let digits =
        |i: usize, n: usize| i + n <= b.len() && b[i..i + n].iter().all(u8::is_ascii_digit);
    (0..b.len()).any(|i| {
        (i == 0 || !b[i - 1].is_ascii_digit())
            && (digits(i, 4)
                && b.get(i + 4) == Some(&b'-')
                && digits(i + 5, 2)
                && b.get(i + 7) == Some(&b'-')
                && digits(i + 8, 2)
                || digits(i, 2)
                    && matches!(b.get(i + 2), Some(b'-' | b'.'))
                    && digits(i + 3, 2)
                    && b.get(i + 5) == b.get(i + 2)
                    && digits(i + 6, 4))
    })
}

/// История, а не текущее состояние: журналы `chats`, решения `adr`, датированные снимки
/// (дата в пути, выгрузки `*.out.md`) и разделы-записи с датой в заголовке (журналы прогонов, changelog).
fn history(c: &Chunk) -> bool {
    matches!(c.module.as_str(), "chats" | "adr")
        || c.path.ends_with(".out.md")
        || has_date(&c.path)
        || has_date(&c.heading)
}

fn strip_marker(line: &str) -> &str {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    let rest = &line[digits..];
    match rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
        Some(item) if (1..=3).contains(&digits) => item,
        _ => line,
    }
}

/// HTML-теги с атрибутами и адреса ссылок `](…)` — разметка, а не утверждения.
fn strip_markup(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        let tag = c == '<'
            && chars
                .peek()
                .is_some_and(|n| n.is_ascii_alphabetic() || matches!(n, '/' | '!'));
        let target = c == ']' && chars.peek() == Some(&'(');
        if tag || target {
            if target {
                out.push(']');
            }
            let close = if tag { '>' } else { ')' };
            for n in chars.by_ref() {
                if n == close {
                    break;
                }
            }
            out.push(' ');
            continue;
        }
        out.push(c);
    }
    out
}

/// Цифры заменены на `#`: строки одного шаблона с разными номерами совпадают.
fn mask(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_number = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            if !in_number {
                out.push('#');
            }
            in_number = true;
        } else if !(in_number && matches!(c, '.' | ',')) {
            out.push(c);
            in_number = false;
        }
    }
    out
}

/// Пределы защищают фоновую очередь; неполный расчёт не публикуется вместо полного.
pub fn find(chunks: &[Chunk], cancel: &AtomicBool) -> Result<Vec<NewFinding>, String> {
    let mut all = Vec::new();
    let mut df: HashMap<String, usize> = HashMap::new();
    for (ci, c) in chunks.iter().enumerate() {
        if history(c) {
            continue;
        }
        let body = c.text.split("\nother headers:").next().unwrap_or(&c.text);
        if body.split_whitespace().count() < 15 {
            continue;
        }
        let mut code = false;
        for raw_line in body.lines() {
            if cancel.load(Ordering::Relaxed) {
                return Err("Запуск остановлен".into());
            }
            let trimmed = raw_line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                code = !code;
                continue;
            }
            if code || trimmed.starts_with('#') {
                continue;
            }
            let clean = strip_markup(strip_marker(trimmed));
            let line = clean.as_str();
            // Сохраняем пунктуацию до разбора чисел: версии и идентификаторы не становятся числами.
            let mut start = 0;
            let mut pieces = Vec::new();
            for (pos, ch) in line.char_indices() {
                if ".!?;".contains(ch) {
                    let rest = &line[pos + ch.len_utf8()..];
                    if rest.starts_with(char::is_whitespace)
                        && rest
                            .trim_start()
                            .starts_with(|c: char| c.is_uppercase() || "«\"(".contains(c))
                    {
                        pieces.push(&line[start..pos + ch.len_utf8()]);
                        start = pos + ch.len_utf8();
                    }
                }
            }
            pieces.push(&line[start..]);
            for text in pieces {
                let tok = tokens(text);
                if tok.len() < 5 {
                    continue;
                }
                let words: BTreeSet<String> = tok
                    .iter()
                    .filter(|w| {
                        !w.chars().all(|c| c.is_ascii_digit())
                            && ![
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
                                "вне",
                            ]
                            .contains(&w.as_str())
                    })
                    .map(|w| stem(w))
                    .collect();
                for w in &words {
                    *df.entry(w.clone()).or_default() += 1;
                }
                all.push(Statement {
                    chunk: ci,
                    text: text.trim().to_owned(),
                    words,
                    ids: identifiers(text),
                    nums: numbers(text),
                });
                if all.len() > 100_000 {
                    return Err("Больше 100 000 утверждений: сузьте документацию".into());
                }
            }
        }
    }
    // Одна строка с разными номерами в трёх и более местах — шаблон («таблица 1…19»), а не утверждение.
    let masks: Vec<String> = all.iter().map(|s| mask(&s.text)).collect();
    let mut shapes: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for (i, s) in all.iter().enumerate().filter(|(_, s)| !s.nums.is_empty()) {
        shapes
            .entry(masks[i].as_str())
            .or_default()
            .insert(s.text.as_str());
    }
    let usable: Vec<bool> = all
        .iter()
        .enumerate()
        .map(|(i, s)| {
            !s.nums.is_empty()
                && !shapes
                    .get(masks[i].as_str())
                    .is_some_and(|texts| texts.len() >= 3)
        })
        .collect();
    let mut inv: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, s) in all.iter().enumerate().filter(|(i, _)| usable[*i]) {
        for w in &s.words {
            if df[w] <= 40 {
                inv.entry(w).or_default().push(i);
            }
        }
    }
    let mut pairs: BTreeMap<(usize, usize), u16> = BTreeMap::new();
    for (i, s) in all.iter().enumerate().filter(|(i, _)| usable[*i]) {
        for id in &s.ids {
            inv.entry(id).or_default().push(i);
        }
    }
    for list in inv.values() {
        if list.len() > 400 {
            continue;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err("Запуск остановлен".into());
        }
        for (p, &i) in list.iter().enumerate() {
            for &j in &list[p + 1..] {
                // Одна тема — один файл: внутри документа сравниваются разные записи и замеры, а не копии.
                if chunks[all[i].chunk].path == chunks[all[j].chunk].path {
                    continue;
                }
                *pairs.entry((i, j)).or_default() += 1;
                if pairs.len() > 2_000_000 {
                    return Err("Больше 2 000 000 кандидатных пар".into());
                }
            }
        }
    }
    let weight = |w: &String| (all.len() as f64 / df[w] as f64).ln();
    let mut findings = Vec::new();
    for ((i, j), shared) in pairs {
        if cancel.load(Ordering::Relaxed) {
            return Err("Запуск остановлен".into());
        }
        if shared < 2 {
            continue;
        }
        let (a, b) = (&all[i], &all[j]);
        let union: f64 = a.words.union(&b.words).map(weight).sum();
        let intersection: f64 = a.words.intersection(&b.words).map(weight).sum();
        let score = if union > 0.0 {
            intersection / union
        } else {
            0.0
        };
        if score < 0.4 {
            continue;
        }
        let Some(found) = conflict(&a.nums, &b.nums) else {
            continue;
        };
        // Без общей единицы и ключа — только почти копия, где разошлось одно значение.
        if found.tag == "swap" && (found.changed > 1 || score < 0.6) {
            continue;
        }
        let subjects = [a, b]
            .map(|s| {
                let c = &chunks[s.chunk];
                Subject {
                    path: c.path.clone(),
                    first_line: c.first_line,
                    last_line: c.last_line,
                    heading: c.heading.clone(),
                    role: document_role(&c.path).into(),
                }
            })
            .to_vec();
        // Оба утверждения включают значения; сдвиг строк не сбрасывает игнор, смена значения сбрасывает.
        let mut keys = [
            format!("{}#{}#{}", subjects[0].path, subjects[0].heading, a.text),
            format!("{}#{}#{}", subjects[1].path, subjects[1].heading, b.text),
        ];
        keys.sort();
        findings.push(NewFinding {
            fingerprint: fingerprint(&["contradictions", &keys[0], &keys[1]]),
            tag: found.tag.into(),
            group: "main".into(),
            reason: format!("Возможное расхождение — {}", found.reason),
            score: Some(score as f32),
            subjects,
            detail: format!("{}\n↔\n{}", a.text, b.text),
        });
        if findings.len() > 5000 {
            return Err("Больше 5000 возможных расхождений".into());
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(path: &str, module: &str, value: &str) -> Chunk {
        Chunk {
            path: path.into(),
            module: module.into(),
            heading: "Настройки".into(),
            first_line: 10,
            last_line: 20,
            text: format!(
                "# Настройки\nВремя ожидания ответа сервера в текущих настройках приложения составляет {value}. Это значение задано по умолчанию для всех проектов."
            ),
        }
    }
    #[test]
    fn numbers_exclude_versions_dates_ids_and_large_values() {
        let n = numbers(
            "Т-36 OQ-143 22-10-2026 1.2.3 file-100.md 12345678 задержка 30 секунд, порог силы 0,82",
        );
        assert_eq!(
            n.iter().map(|n| n.value).collect::<Vec<_>>(),
            vec![30.0, 0.82]
        );
        assert_eq!(n[0].unit, "время");
        assert_eq!(n[0].raw, "30 секунд");
        assert_eq!(numbers("Задержка `30с`")[0].unit, "время");
    }

    #[test]
    fn numbers_skip_ordinals_hashes_day_month_and_glued_letters() {
        // Номера пунктов, раундов и таблиц, хеши коммитов, даты «18.10» и «Jul 28», время «7pm» — не величины.
        let n = numbers(
            "раунд 16, ответ 5, table 3, №36, № 4, fixes 5+6, коммит 1b9dad6, 18.10, resets 7pm, Jul 28, модель 4B",
        );
        assert!(n.is_empty(), "{n:?}");
        // Единицы одного семейства приводятся к основной: часы и секунды сравнимы.
        let n = numbers("таймаут `8h`, бюджет 30000 s, порог 0,5");
        assert_eq!(
            n.iter().map(|n| n.value).collect::<Vec<_>>(),
            vec![28_800.0, 30_000.0, 0.5]
        );
        assert_eq!(n[0].unit, "время");
        assert_eq!(n[2].unit, "");
    }

    #[test]
    fn numeric_rules_keep_the_research_seeded_pairs_and_controls() {
        let pairs: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../../docs/knowledge/contradictions-2026-10-22/analysis/seeded.in.json"
        ))
        .unwrap();
        let mut positives = 0;
        let mut false_positives = 0;
        for pair in &pairs {
            let hit = conflict(
                &numbers(pair["a"].as_str().unwrap()),
                &numbers(pair["b"].as_str().unwrap()),
            )
            .is_some();
            if pair["label"] == "conflict" {
                positives += usize::from(hit);
            } else {
                false_positives += usize::from(hit);
            }
        }
        assert_eq!(
            false_positives, 0,
            "ложные на контролях исходного исследования"
        );
        assert_eq!(
            positives, 40,
            "числовые правила исходного исследования, до фильтра близости"
        );
    }
    #[test]
    fn numeric_conflict_has_sources_and_stable_identity() {
        let mut c = vec![
            chunk("a.md", "features", "30 с"),
            chunk("b.md", "manual", "60 секунд"),
        ];
        let f = find(&c, &AtomicBool::new(false)).unwrap();
        assert_eq!(f.len(), 1);
        assert!(
            f[0].reason.contains("время: 30 с ↔ 60 секунд"),
            "{}",
            f[0].reason
        );
        assert_eq!(
            (f[0].subjects[0].first_line, f[0].subjects[0].last_line),
            (10, 20)
        );
        c[0].first_line = 50;
        c[0].last_line = 60;
        assert_eq!(
            find(&c, &AtomicBool::new(false)).unwrap()[0].fingerprint,
            f[0].fingerprint
        );
        c[1].text = c[1].text.replace("60 секунд", "90 секунд");
        assert_ne!(
            find(&c, &AtomicBool::new(false)).unwrap()[0].fingerprint,
            f[0].fingerprint
        );
    }
    #[test]
    fn controls_history_rounding_equivalent_units_and_cancel() {
        for (path, module, value) in [
            ("b.md", "manual", "30 секунд"),
            ("b.md", "manual", "30,1 с"),
            ("b.md", "manual", "0,5 мин"),
            ("b.md", "manual", "30000 мс"),
            ("b.md", "chats", "60 с"),
            ("b.md", "adr", "60 с"),
            // Датированный снимок, выгрузка замера и тот же документ — не текущее состояние другой темы.
            ("knowledge/run-2026-11-05/report.md", "knowledge", "60 с"),
            ("knowledge/final.out.md", "knowledge", "60 с"),
            ("a.md", "features", "60 с"),
            // Значение только внутри HTML-атрибута — разметка, а не утверждение.
            ("b.md", "manual", "<span data-delay=\"60 с\"></span>30 с"),
        ] {
            assert!(
                find(
                    &[
                        chunk("a.md", "features", "30 с"),
                        chunk(path, module, value)
                    ],
                    &AtomicBool::new(false)
                )
                .unwrap()
                .is_empty(),
                "{path} {value}"
            );
        }
        assert!(find(&[chunk("a.md", "features", "30 с")], &AtomicBool::new(true)).is_err());
    }

    #[test]
    fn dated_log_sections_and_numbered_templates_are_not_candidates() {
        // Раздел журнала прогонов: дата в заголовке.
        let mut log = chunk("bot-runs/feed.md", "bot-runs", "60 с");
        log.heading = "Журнал прогонов/2026-07-26 — прогон 019f9e47".into();
        assert!(
            find(
                &[chunk("a.md", "features", "30 с"), log],
                &AtomicBool::new(false)
            )
            .unwrap()
            .is_empty()
        );
        // Одна строка с разными номерами в трёх документах — шаблон («таблица 1…3»).
        let template = [
            chunk("a.md", "features", "1 с"),
            chunk("b.md", "features", "2 с"),
            chunk("c.md", "features", "3 с"),
        ];
        assert!(find(&template, &AtomicBool::new(false)).unwrap().is_empty());
    }
}
