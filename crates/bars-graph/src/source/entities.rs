use super::*;

type AliasTargets = BTreeMap<String, Vec<Range<usize>>>;

#[derive(Default)]
pub(super) struct Entities {
    // Alias identity belongs to its declaring document, never to a bare project-wide name.
    aliases: BTreeMap<(String, String), AliasTargets>,
}

pub(super) fn clean(text: &str) -> String {
    let mut text = text.trim().trim_end_matches(['.', ';', ':']).trim();
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = text.strip_prefix(marker) {
            text = rest.trim();
            break;
        }
    }
    // Strip paired wrappers only. Mixed prose + inline code must retain both backticks.
    for (open, close) in [('`', '`'), ('«', '»'), ('"', '"'), ('\'', '\'')] {
        if text.starts_with(open)
            && text.ends_with(close)
            && text.len() > open.len_utf8() + close.len_utf8()
        {
            let inner = &text[open.len_utf8()..text.len() - close.len_utf8()];
            if !inner.contains([open, close]) {
                text = inner.trim();
            }
        }
    }
    text.into()
}

fn qualified(text: &str) -> bool {
    (text.contains("::") || text.contains('.'))
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_:.-/".contains(c))
        && text.split([':', '.']).filter(|s| !s.is_empty()).count() >= 2
        && text
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && !text.ends_with(['.', ':', '/'])
}

pub(crate) fn declaration(text: &str) -> Option<(String, String)> {
    let text = text.trim().trim_end_matches('.');
    let (id, alias) = text.split_once(" (алиас: ")?;
    let alias = operand(alias.strip_suffix(')')?)?;
    let key = clean(id);
    if !id.starts_with('`')
        || !id.ends_with('`')
        || !qualified(&key)
        || qualified(&alias)
        || alias.starts_with("Т-")
    {
        return None;
    }
    Some((key, alias))
}

pub(super) fn operand(text: &str) -> Option<String> {
    let label = clean(text);
    if label.is_empty() || label.chars().count() > 160 || label.contains(['\n', '\r']) {
        return None;
    }
    let mut code = false;
    let mut prose = String::new();
    for c in label.chars() {
        if c == '`' {
            code = !code;
            prose.push(' ');
        } else if !code {
            prose.push(c);
        }
    }
    if code || prose.contains([',', '!', '?', ';', '(', ')', '[', ']', '«', '»', '"', '\'']) {
        return None;
    }
    // Dots/colons are meaningful inside a whole qualified code identifier only.
    if prose.contains(['.', ':'])
        && !(text.trim().trim_end_matches('.').ends_with('`') && qualified(&label))
    {
        return None;
    }
    if prose.split_whitespace().any(|word| {
        matches!(
            word.to_lowercase().as_str(),
            "не" | "без"
                | "он"
                | "она"
                | "оно"
                | "они"
                | "это"
                | "его"
                | "её"
                | "ее"
                | "их"
                | "этот"
                | "эта"
                | "эту"
                | "этого"
                | "тот"
                | "та"
                | "ту"
                | "той"
                | "такой"
                | "который"
                | "которая"
                | "которое"
                | "которые"
                | "которой"
                | "и"
                | "или"
                | "но"
                | "а"
                | "если"
                | "когда"
                | "при"
                | "заново"
                | "снова"
                | "только"
                | "использует"
                | "проверяет"
                | "требует"
                | "зависит"
                | "заменяет"
                | "реализует"
        )
    }) {
        return None;
    }
    Some(label)
}

impl Entities {
    pub(super) fn ambiguous(&self, path: &str, label: &str) -> bool {
        self.aliases
            .get(&(path.into(), clean(label).to_lowercase()))
            .is_some_and(|candidates| candidates.len() != 1)
    }

    pub(super) fn collect(
        inputs: &[InputDocument],
        cancelled: &mut impl FnMut() -> bool,
    ) -> Result<Self, GraphError> {
        let mut result = Self::default();
        for input in inputs {
            if cancelled() {
                return Err(GraphError::Cancelled);
            }
            let start = links::body_start(&input.text);
            for (event, range) in
                Parser::new_ext(&input.text[start..], Options::empty()).into_offset_iter()
            {
                if cancelled() {
                    return Err(GraphError::Cancelled);
                }
                if !matches!(event, Event::Start(Tag::Paragraph)) {
                    continue;
                }
                let range = range.start + start..range.end + start;
                let Some((key, alias)) = declaration(&input.text[range.clone()]) else {
                    continue;
                };
                result
                    .aliases
                    .entry((input.docs_path.clone(), alias.to_lowercase()))
                    .or_default()
                    .entry(key)
                    .or_default()
                    .push(range);
            }
        }
        Ok(result)
    }

    pub(super) fn resolve(
        &self,
        data: &mut Dataset,
        source: &SourceDocument,
        raw: &str,
        proof: &str,
    ) -> Result<Option<(String, Vec<String>)>, GraphError> {
        let Some(mut label) = operand(raw) else {
            return Ok(None);
        };
        let alias = self
            .aliases
            .get(&(source.meta.docs_path.clone(), label.to_lowercase()));
        let mut proofs = vec![proof.to_string()];
        let mut aliases = Vec::new();
        let mut technical = raw.trim().trim_end_matches('.').ends_with('`')
            && raw.trim().starts_with('`')
            && qualified(&label);
        if let Some(candidates) = alias {
            if candidates.len() != 1 {
                return Ok(None);
            }
            let (id, ranges) = candidates.first_key_value().unwrap();
            aliases.push(label);
            label = id.clone();
            technical = true;
            for range in ranges {
                proofs.push(evidence(data, source, range.clone(), METHOD_VERSION)?);
            }
        }
        let requirement = label
            .strip_prefix("Т-")
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        let namespace = if requirement {
            format!("{}:requirement", data.namespace)
        } else if technical {
            format!("{}:technical", data.namespace)
        } else {
            format!("{}:{}", data.namespace, source.meta.docs_path)
        };
        let key = if technical {
            label.clone()
        } else {
            label.to_lowercase()
        };
        let reference = object_ref(&namespace, Kind::Entity, &key, METHOD_VERSION);
        let identity_proofs = proofs
            .iter()
            .filter(|p| p.as_str() != proof)
            .cloned()
            .collect();
        if let Some(node) = data
            .nodes
            .iter_mut()
            .find(|n| n.reference.id == reference.id)
        {
            node.source_ids.push(source.meta.reference.id.clone());
            node.evidence.extend(proofs);
            node.aliases.extend(aliases);
            node.source_ids.sort();
            node.source_ids.dedup();
            node.evidence.sort();
            node.evidence.dedup();
            node.aliases.sort();
            node.aliases.dedup();
        } else {
            data.nodes.push(Node {
                reference: reference.clone(),
                label,
                namespace,
                aliases,
                source_ids: vec![source.meta.reference.id.clone()],
                evidence: proofs,
                roles: vec![],
                parent: None,
                anchor: None,
            });
        }
        Ok(Some((reference.id, identity_proofs)))
    }
}
