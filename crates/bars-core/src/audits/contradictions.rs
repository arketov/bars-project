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
    value: f64,
    unit: String,
    key: String,
}
struct Statement {
    chunk: usize,
    text: String,
    words: BTreeSet<String>,
    ids: BTreeSet<String>,
    nums: Vec<Number>,
}

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
fn unit(word: &str) -> &str {
    match word {
        "с" | "сек" => "сек",
        "мс" => "мс",
        "мин" => "мин",
        "ч" => "час",
        "день" | "сутки" | "суток" => "дн",
        "мб" | "mb" => "мб",
        "гб" | "gb" => "гб",
        "кб" | "kb" => "кб",
        "%" => "%",
        "шт" => "шт",
        "раз" | "раза" => "раз",
        _ => [
            "миллисекунд",
            "секунд",
            "минут",
            "час",
            "дн",
            "мбайт",
            "мегабайт",
            "гбайт",
            "гигабайт",
            "кбайт",
            "слов",
            "знак",
            "символ",
            "байт",
            "строк",
            "штук",
            "чанк",
            "документ",
            "пар",
            "измерен",
        ]
        .into_iter()
        .zip([
            "мс",
            "сек",
            "мин",
            "час",
            "дн",
            "мб",
            "мб",
            "гб",
            "гб",
            "кб",
            "слов",
            "знак",
            "знак",
            "байт",
            "строк",
            "шт",
            "чанк",
            "док",
            "пар",
            "изм",
        ])
        .find(|(prefix, _)| word.starts_with(prefix))
        .map_or("", |(_, u)| u),
    }
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
        let forbidden = |c: char| c.is_alphanumeric() || "_.-:/#`\\".contains(c);
        if start > 0 && forbidden(chars[start - 1].1)
            || i + 1 < chars.len()
                && "-:/.,".contains(chars[i].1)
                && chars[i + 1].1.is_ascii_digit()
        {
            continue;
        }
        let byte = chars[start].0;
        let end_byte = chars.get(end).map_or(text.len(), |c| c.0);
        let Ok(value) = text[byte..end_byte].replace(',', ".").parse::<f64>() else {
            continue;
        };
        if value >= 10_000_000.0 {
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
        let after = text[end_byte..].trim_start();
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphabetic() || *c == '%')
            .collect();
        result.push(Number {
            value,
            key,
            unit: unit(&name).to_owned(),
        });
    }
    result
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 0.02 * a.abs().max(b.abs())
}
fn disjoint(a: &[f64], b: &[f64]) -> bool {
    !a.is_empty() && !b.is_empty() && a.iter().all(|x| b.iter().all(|y| !close(*x, *y)))
}
fn values(nums: &[Number], key: impl Fn(&Number) -> bool) -> Vec<f64> {
    nums.iter().filter(|n| key(n)).map(|n| n.value).collect()
}
fn display(values: &[f64]) -> String {
    values
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(", ")
        .replace('.', ",")
}

fn conflict(a: &[Number], b: &[Number]) -> Option<(String, String)> {
    for (tag, by_unit) in [("num", true), ("slot", false)] {
        let keys: BTreeSet<_> = a
            .iter()
            .map(|n| if by_unit { &n.unit } else { &n.key })
            .filter(|k| !k.is_empty())
            .collect();
        for key in keys {
            let matches = |n: &Number| {
                if by_unit {
                    &n.unit == key
                } else {
                    &n.key == key
                }
            };
            let av = values(a, matches);
            let bv = values(b, matches);
            if disjoint(&av, &bv) {
                return Some((
                    tag.into(),
                    format!("{key}: {} ↔ {}", display(&av), display(&bv)),
                ));
            }
        }
    }
    let av = values(a, |n| b.iter().all(|m| !close(n.value, m.value)));
    let bv = values(b, |n| a.iter().all(|m| !close(n.value, m.value)));
    if !av.is_empty() && !bv.is_empty() {
        Some((
            "swap".into(),
            format!("Значения: {} ↔ {}", display(&av), display(&bv)),
        ))
    } else {
        None
    }
}

/// Пределы защищают фоновую очередь; неполный расчёт не публикуется вместо полного.
pub fn find(chunks: &[Chunk], cancel: &AtomicBool) -> Result<Vec<NewFinding>, String> {
    let mut all = Vec::new();
    let mut df: HashMap<String, usize> = HashMap::new();
    for (ci, c) in chunks.iter().enumerate() {
        if matches!(c.module.as_str(), "chats" | "adr") {
            continue;
        }
        let body = c.text.split("\nother headers:").next().unwrap_or(&c.text);
        if body.split_whitespace().count() < 15 {
            continue;
        }
        for line in body.lines() {
            if cancel.load(Ordering::Relaxed) {
                return Err("Запуск остановлен".into());
            }
            if line.trim_start().starts_with('#') {
                continue;
            }
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
    let mut inv: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, s) in all.iter().enumerate().filter(|(_, s)| !s.nums.is_empty()) {
        for w in &s.words {
            if df[w] <= 40 {
                inv.entry(w).or_default().push(i);
            }
        }
    }
    let mut pairs: BTreeMap<(usize, usize), u16> = BTreeMap::new();
    for (i, s) in all.iter().enumerate().filter(|(_, s)| !s.nums.is_empty()) {
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
                if all[i].chunk == all[j].chunk {
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
        let Some((tag, reason)) = conflict(&a.nums, &b.nums) else {
            continue;
        };
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
            tag,
            group: "main".into(),
            reason: format!("Возможное расхождение — {reason}"),
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
        assert_eq!(n[0].unit, "сек");
        assert_eq!(numbers("Задержка `30с`")[0].unit, "сек");
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
        assert!(f[0].reason.contains("30 ↔ 60"));
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
        for (value, module) in [
            ("30 секунд", "manual"),
            ("30,1 с", "manual"),
            ("60 с", "chats"),
            ("60 с", "adr"),
        ] {
            assert!(
                find(
                    &[
                        chunk("a.md", "features", "30 с"),
                        chunk("b.md", module, value)
                    ],
                    &AtomicBool::new(false)
                )
                .unwrap()
                .is_empty()
            );
        }
        assert!(find(&[chunk("a.md", "features", "30 с")], &AtomicBool::new(true)).is_err());
    }
}
