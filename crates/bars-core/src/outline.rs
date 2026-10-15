//! Структура документа: заголовки с номерами строк (Т-26, `docs/features/agent-tools/mcp-tools.md`).
//!
//! Чистая функция над текстом файла. Заголовки распознаются теми же примитивами, что при нарезке
//! ([`crate::indexing::chunking`]): ATX, без строк внутри блоков кода, номера строк по исходному
//! файлу вместе с frontmatter.

use std::collections::BTreeSet;
use std::fmt;

use crate::indexing::chunking::{Fence, normalize, parse_heading, split_frontmatter};

/// Один заголовок документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// Строка заголовка в файле, с единицы.
    pub line: usize,
    /// Число решёток, 1…6.
    pub level: usize,
    pub text: String,
}

impl fmt::Display for Heading {
    /// `7 ## Стадия` — одинаково для MCP и CLI (Т-26).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {} {}", self.line, "#".repeat(self.level), self.text)
    }
}

/// Заголовок, внутри которого нужны заголовки.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Under {
    /// Полная цепочка от верхнего заголовка документа, как `heading_chain` в выдаче поиска.
    Chain(String),
    /// Строка самого заголовка.
    Line(usize),
}

/// Что вернуть из структуры документа.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutlineQuery {
    /// Уровни заголовков; пусто — все.
    pub levels: Vec<usize>,
    /// Только заголовки внутри заданного; сам он в ответ не входит.
    pub under: Option<Under>,
}

/// Запрос структуры отклонён: параметр вне допустимого (`invalid_params`).
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum OutlineError {
    #[error("уровень заголовка {0} вне 1…6")]
    InvalidLevel(usize),
    #[error("заголовок не найден: {0}")]
    HeadingNotFound(String),
    #[error("в строке {0} нет заголовка")]
    NotAHeading(usize),
}

/// Заголовок вместе с цепочкой от корня.
struct Node {
    heading: Heading,
    chain: String,
}

/// Структура документа по запросу, в порядке документа.
pub fn outline(source: &str, query: &OutlineQuery) -> Result<Vec<Heading>, OutlineError> {
    if let Some(level) = query.levels.iter().find(|l| !(1..=6).contains(*l)) {
        return Err(OutlineError::InvalidLevel(*level));
    }
    let nodes = headings(source);
    let selected: Vec<usize> = match &query.under {
        None => (0..nodes.len()).collect(),
        Some(under) => descendants(&nodes, under)?.into_iter().collect(),
    };
    Ok(selected
        .into_iter()
        .map(|index| &nodes[index].heading)
        .filter(|h| query.levels.is_empty() || query.levels.contains(&h.level))
        .cloned()
        .collect())
}

/// Все заголовки документа с цепочками; правила — как в `chunking::parse_sections`.
fn headings(source: &str) -> Vec<Node> {
    let normalized = normalize(source);
    let lines: Vec<&str> = normalized.split('\n').collect();
    let (_, body_start) = split_frontmatter(&lines);

    // Открытые заголовки: уровень и текст.
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut fence = Fence::default();
    let mut nodes = Vec::new();
    for (index, line) in lines.iter().enumerate().skip(body_start) {
        if fence.step(line) {
            continue;
        }
        let Some((level, text)) = parse_heading(line) else {
            continue;
        };
        while stack.last().is_some_and(|(open, _)| *open >= level) {
            stack.pop();
        }
        let chain = stack
            .iter()
            .map(|(_, open_text)| open_text.as_str())
            .chain([text.as_str()])
            .collect::<Vec<_>>()
            .join("/");
        stack.push((level, text.clone()));
        nodes.push(Node {
            heading: Heading {
                line: index + 1,
                level,
                text,
            },
            chain,
        });
    }
    nodes
}

/// Индексы заголовков внутри каждого заголовка, подошедшего под `under` (OQ-36, OQ-37).
fn descendants(nodes: &[Node], under: &Under) -> Result<BTreeSet<usize>, OutlineError> {
    let targets: Vec<usize> = match under {
        Under::Chain(chain) => {
            let chain = chain.trim();
            let found: Vec<usize> = (0..nodes.len())
                .filter(|&i| nodes[i].chain == chain)
                .collect();
            if found.is_empty() {
                return Err(OutlineError::HeadingNotFound(chain.to_string()));
            }
            found
        }
        Under::Line(line) => {
            let found = nodes.iter().position(|n| n.heading.line == *line);
            vec![found.ok_or(OutlineError::NotAHeading(*line))?]
        }
    };
    let mut inside = BTreeSet::new();
    for target in targets {
        let level = nodes[target].heading.level;
        inside.extend((target + 1..nodes.len()).take_while(|&i| nodes[i].heading.level > level));
    }
    Ok(inside)
}

#[cfg(test)]
mod tests;
