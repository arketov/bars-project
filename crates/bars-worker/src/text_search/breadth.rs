//! Узкий запрос дискового поиска вхождений (ISS-109, ADR-0007): режим occurrences ищет все вхождения одной
//! конкретной строки на диске. Широкий шаблон отклоняется ошибкой `too_broad` с отсылкой
//! к единому поиску `bars_search`. Проверяется только агентский путь `disk_text`.
use bars_protocol::{ErrorKind, RpcError};
use regex_syntax::ast::{Ast, RepetitionKind, RepetitionRange, parse::Parser};
use serde_json::{Map, json};

/// Длина регулярного выражения: шаблон одной строки короткий, длинный — набор ключевых слов.
pub const MAX_REGEX_BYTES: usize = 256;
/// Вариантов через `|` во всём шаблоне; `|` внутри `[...]` не считается.
pub const MAX_BRANCHES: usize = 4;
/// Вложенность групп: глубже точная строка не нужна.
pub const MAX_GROUP_DEPTH: usize = 2;
/// Наибольший счётчик `{n}`, `{n,m}`, `{n,}`: 64 знака — самый длинный реальный токен (хеш).
pub const MAX_REPEAT: u32 = 64;
/// Неограниченных повторов `*`, `+`, `{n,}`: хватает на `foo.*bar`.
pub const MAX_UNBOUNDED: usize = 2;
/// Обязательный кусок подряд идущих знаков в каждой ветке, с буквой или цифрой.
pub const MIN_ANCHOR_CHARS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooBroad {
    RegexTooLong(usize),
    TooManyBranches(usize),
    GroupsTooDeep(usize),
    NestedRepetition,
    RepeatTooLarge(u32),
    TooManyUnbounded(usize),
    NoAnchor,
    LiteralTooShort,
}

impl TooBroad {
    pub fn code(self) -> &'static str {
        match self {
            TooBroad::RegexTooLong(_) => "regex_too_long",
            TooBroad::TooManyBranches(_) => "too_many_branches",
            TooBroad::GroupsTooDeep(_) => "groups_too_deep",
            TooBroad::NestedRepetition => "nested_repetition",
            TooBroad::RepeatTooLarge(_) => "repeat_too_large",
            TooBroad::TooManyUnbounded(_) => "too_many_unbounded",
            TooBroad::NoAnchor => "no_anchor",
            TooBroad::LiteralTooShort => "literal_too_short",
        }
    }

    fn limit(self) -> u64 {
        match self {
            TooBroad::RegexTooLong(_) => MAX_REGEX_BYTES as u64,
            TooBroad::TooManyBranches(_) => MAX_BRANCHES as u64,
            TooBroad::GroupsTooDeep(_) => MAX_GROUP_DEPTH as u64,
            TooBroad::NestedRepetition => 1,
            TooBroad::RepeatTooLarge(_) => u64::from(MAX_REPEAT),
            TooBroad::TooManyUnbounded(_) => MAX_UNBOUNDED as u64,
            TooBroad::NoAnchor | TooBroad::LiteralTooShort => MIN_ANCHOR_CHARS as u64,
        }
    }

    fn actual(self) -> Option<u64> {
        match self {
            TooBroad::RegexTooLong(n)
            | TooBroad::TooManyBranches(n)
            | TooBroad::GroupsTooDeep(n)
            | TooBroad::TooManyUnbounded(n) => Some(n as u64),
            TooBroad::RepeatTooLarge(n) => Some(u64::from(n)),
            TooBroad::NestedRepetition | TooBroad::NoAnchor | TooBroad::LiteralTooShort => None,
        }
    }

    fn reason(self) -> String {
        match self {
            TooBroad::RegexTooLong(n) => {
                format!("выражение длиннее {MAX_REGEX_BYTES} байт: {n}")
            }
            TooBroad::TooManyBranches(n) => {
                format!("вариантов через «|»: {n}, допустимо не больше {MAX_BRANCHES}")
            }
            TooBroad::GroupsTooDeep(n) => {
                format!("вложенность групп: {n}, допустимо не больше {MAX_GROUP_DEPTH}")
            }
            TooBroad::NestedRepetition => {
                r"повтор внутри повтора, например (a+)+ или (\w+\s)*".to_owned()
            }
            TooBroad::RepeatTooLarge(n) => {
                format!("счётчик повтора: {n}, допустимо не больше {MAX_REPEAT}")
            }
            TooBroad::TooManyUnbounded(n) => format!(
                "неограниченных повторов (*, +, {{n,}}): {n}, допустимо не больше {MAX_UNBOUNDED}"
            ),
            TooBroad::NoAnchor => format!(
                "нет обязательного куска от {MIN_ANCHOR_CHARS} знаков с буквой или цифрой в каждой ветке — выражение совпадает почти с любой строкой"
            ),
            TooBroad::LiteralTooShort => {
                format!("строка короче {MIN_ANCHOR_CHARS} знаков или без букв и цифр")
            }
        }
    }
}

/// Причина отказа или `None`, если запрос узкий. Пустой, слишком длинный и ошибочный
/// запрос пропускается: их по-прежнему отклоняет `matcher` (`invalid_regex`).
pub fn classify(query: &str, regex: bool) -> Option<TooBroad> {
    if query.is_empty() || query.len() > 8192 {
        return None;
    }
    if !regex {
        return (!qualifies(query.trim())).then_some(TooBroad::LiteralTooShort);
    }
    if query.len() > MAX_REGEX_BYTES {
        return Some(TooBroad::RegexTooLong(query.len()));
    }
    if regex_syntax::parse(query).is_err() {
        return None;
    }
    let Ok(ast) = Parser::new().parse(query) else {
        return None;
    };
    let mut stats = Stats::default();
    stats.walk(&ast, 0, false);
    let branches = stats.bars + 1;
    if branches > MAX_BRANCHES {
        Some(TooBroad::TooManyBranches(branches))
    } else if stats.depth > MAX_GROUP_DEPTH {
        Some(TooBroad::GroupsTooDeep(stats.depth))
    } else if stats.nested {
        Some(TooBroad::NestedRepetition)
    } else if stats.repeat > MAX_REPEAT {
        Some(TooBroad::RepeatTooLarge(stats.repeat))
    } else if stats.unbounded > MAX_UNBOUNDED {
        Some(TooBroad::TooManyUnbounded(stats.unbounded))
    } else if !anchored(&ast) {
        Some(TooBroad::NoAnchor)
    } else {
        None
    }
}

/// Ошибка `too_broad` для агента: код `invalid_params`, в `data` — причина, предел и куда идти.
pub fn check(query: &str, regex: bool) -> Result<(), RpcError> {
    let Some(broad) = classify(query, regex) else {
        return Ok(());
    };
    let mut extra = Map::new();
    extra.insert("kind".into(), json!("too_broad"));
    extra.insert("reason".into(), json!(broad.code()));
    extra.insert("limit".into(), json!(broad.limit()));
    if let Some(actual) = broad.actual() {
        extra.insert("actual".into(), json!(actual));
    }
    extra.insert("use".into(), json!("bars_search"));
    Err(RpcError::bars_with(
        ErrorKind::InvalidParams,
        format!(
            "Слишком широкий запрос для точного поиска: {}. Режим occurrences находит все вхождения одной конкретной строки на диске: переименование, полный список мест, только что записанный текст. Для широкого поиска используйте bars_search — смысл, слова, имена и номера вместе, операторы \"фраза\", -слово, path:папка. Не дробите запрос на серию узких вызовов.",
            broad.reason()
        ),
        extra,
    ))
}

#[derive(Default)]
struct Stats {
    bars: usize,
    depth: usize,
    unbounded: usize,
    repeat: u32,
    nested: bool,
}

impl Stats {
    fn walk(&mut self, ast: &Ast, depth: usize, in_repetition: bool) {
        match ast {
            Ast::Alternation(a) => {
                self.bars += a.asts.len().saturating_sub(1);
                for branch in &a.asts {
                    self.walk(branch, depth, in_repetition);
                }
            }
            Ast::Concat(c) => {
                for part in &c.asts {
                    self.walk(part, depth, in_repetition);
                }
            }
            Ast::Group(g) => {
                self.depth = self.depth.max(depth + 1);
                self.walk(&g.ast, depth + 1, in_repetition);
            }
            Ast::Repetition(r) => {
                self.nested |= in_repetition;
                match &r.op.kind {
                    RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore => self.unbounded += 1,
                    RepetitionKind::ZeroOrOne => {}
                    RepetitionKind::Range(range) => {
                        let count = match *range {
                            RepetitionRange::Exactly(n) | RepetitionRange::AtLeast(n) => n,
                            RepetitionRange::Bounded(_, m) => m,
                        };
                        self.repeat = self.repeat.max(count);
                        if matches!(range, RepetitionRange::AtLeast(_)) {
                            self.unbounded += 1;
                        }
                    }
                }
                self.walk(&r.ast, depth, true);
            }
            _ => {}
        }
    }
}

fn minimum(kind: &RepetitionKind) -> u32 {
    match kind {
        RepetitionKind::ZeroOrOne | RepetitionKind::ZeroOrMore => 0,
        RepetitionKind::OneOrMore => 1,
        RepetitionKind::Range(
            RepetitionRange::Exactly(n)
            | RepetitionRange::AtLeast(n)
            | RepetitionRange::Bounded(n, _),
        ) => *n,
    }
}

/// Узел обязательно содержит подряд идущий литерал, который проходит `qualifies`.
fn anchored(ast: &Ast) -> bool {
    match ast {
        Ast::Concat(c) => {
            let mut run = String::new();
            for part in &c.asts {
                match part {
                    Ast::Literal(l) => run.push(l.c),
                    Ast::Flags(_) | Ast::Empty(_) | Ast::Assertion(_) => {}
                    other => {
                        if qualifies(&run) || anchored(other) {
                            return true;
                        }
                        run.clear();
                    }
                }
            }
            qualifies(&run)
        }
        Ast::Alternation(a) => a.asts.iter().all(anchored),
        Ast::Group(g) => anchored(&g.ast),
        Ast::Repetition(r) => minimum(&r.op.kind) >= 1 && anchored(&r.ast),
        _ => false,
    }
}

fn qualifies(run: &str) -> bool {
    run.chars().count() >= MIN_ANCHOR_CHARS && run.chars().any(char::is_alphanumeric)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regex(q: &str) -> Option<TooBroad> {
        classify(q, true)
    }

    #[test]
    fn breadth_accepts_specific_strings() {
        assert_eq!(classify("some_option", false), None);
        assert_eq!(classify("Т-39", false), None);
        for q in [
            "some_option",
            "ISS-1[0-9]{2}",
            r"Т-3\d",
            "foo|bar|baz|qux",
            "bars_(text|rag)_search",
            r"рекомендаци\w*",
            "(?i)ошибка",
            "foo.*bar",
            "^#{1,3} Ошибки",
            r"OQ-\d{3}",
        ] {
            assert_eq!(regex(q), None, "{q}");
        }
    }

    #[test]
    fn breadth_rejects_too_many_branches() {
        assert_eq!(
            regex("foo|bar|baz|qux|quux"),
            Some(TooBroad::TooManyBranches(5))
        );
        assert_eq!(regex("(a|b|c)(d|e|f)"), Some(TooBroad::TooManyBranches(5)));
        // `|` внутри класса символов — не ветка.
        assert_eq!(regex("[a|b]foo"), None);
    }

    #[test]
    fn breadth_rejects_deep_groups() {
        assert_eq!(regex("((foo(bar)))"), Some(TooBroad::GroupsTooDeep(3)));
        assert_eq!(regex("((foo)bar)"), None);
    }

    #[test]
    fn breadth_rejects_nested_repetition() {
        for q in ["(a+)+", r"(\w+\s)*", "(?:abc?)+"] {
            assert_eq!(regex(q), Some(TooBroad::NestedRepetition), "{q}");
        }
    }

    #[test]
    fn breadth_rejects_large_counts_and_many_unbounded() {
        assert_eq!(regex(r"foo\d{65}"), Some(TooBroad::RepeatTooLarge(65)));
        assert_eq!(regex("o{1000}"), Some(TooBroad::RepeatTooLarge(1000)));
        assert_eq!(
            regex("foo.*bar.*baz.*"),
            Some(TooBroad::TooManyUnbounded(3))
        );
    }

    #[test]
    fn breadth_requires_anchor_in_every_branch() {
        for q in [
            ".*",
            r"\w+",
            "^",
            "(a|b)*c",
            r"T-\d+",
            "foo|.*",
            "foo|",
            r"\[\[.*\]\]",
            "[0-9a-f]{64}",
        ] {
            assert_eq!(regex(q), Some(TooBroad::NoAnchor), "{q}");
        }
    }

    #[test]
    fn breadth_literal_mode() {
        for q in ["x", "ab", "---", "```", ".*", "  a "] {
            assert_eq!(classify(q, false), Some(TooBroad::LiteralTooShort), "{q}");
        }
        assert_eq!(classify("a.*b", false), None);
    }

    #[test]
    fn breadth_defers_invalid_and_bounds_to_matcher() {
        assert_eq!(regex("["), None);
        assert_eq!(regex(r"\p{Nope}x"), None);
        assert_eq!(regex(""), None);
        assert_eq!(classify("", false), None);
        let long = "a".repeat(300);
        assert_eq!(regex(&long), Some(TooBroad::RegexTooLong(300)));
    }

    #[test]
    fn breadth_error_contract() {
        assert!(check("some_option", true).is_ok());
        let error = check("foo|bar|baz|qux|quux", true).unwrap_err();
        assert_eq!(error.code, ErrorKind::InvalidParams.code());
        let data = error.data.unwrap();
        assert_eq!(data["kind"], "too_broad");
        assert_eq!(data["reason"], "too_many_branches");
        assert_eq!(data["limit"], 4);
        assert_eq!(data["actual"], 5);
        assert_eq!(data["use"], "bars_search");
        for part in ["bars_search", "\"фраза\"", "path:"] {
            assert!(error.message.contains(part), "{}", error.message);
        }
        let no_actual = check(".*", true).unwrap_err().data.unwrap();
        assert_eq!(no_actual["reason"], "no_anchor");
        assert!(no_actual.get("actual").is_none());
    }
}
