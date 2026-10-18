use crate::*;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

pub struct InputDocument {
    pub path: String,
    pub docs_path: String,
    pub module: String,
    pub title: String,
    pub text: String,
    pub chunks: Vec<InputChunk>,
}
pub struct InputChunk {
    pub key: String,
    pub label: String,
    pub spans: Vec<Range<usize>>,
}

/// Диапазон относится к исходной UTF-8 строке, включая исходные BOM/CRLF.
pub fn span(text: &str, range: Range<usize>) -> Result<Span, GraphError> {
    if range.start > range.end || text.get(range.clone()).is_none() {
        return Err(GraphError::InvalidData("неверный исходный диапазон".into()));
    }
    let line = |end: usize| {
        let bytes = text.as_bytes();
        let mut at = 0;
        let mut count = 1;
        while at < end {
            if bytes[at] == b'\r' {
                count += 1;
                if bytes.get(at + 1) == Some(&b'\n') && at + 1 < end {
                    at += 1;
                }
            } else if bytes[at] == b'\n' {
                count += 1;
            }
            at += 1;
        }
        count
    };
    let mut content_end = range.end;
    while content_end > range.start && matches!(text.as_bytes()[content_end - 1], b'\r' | b'\n') {
        content_end -= 1;
    }
    Ok(Span {
        start: range.start,
        end: range.end,
        first_line: line(range.start),
        last_line: line(if content_end == range.start {
            range.start
        } else {
            content_end - 1
        }),
    })
}

fn evidence(
    data: &mut Dataset,
    source: &SourceDocument,
    range: Range<usize>,
    method: &str,
) -> Result<String, GraphError> {
    let span = span(&source.text, range.clone())?;
    let key = serde_json::to_vec(&(&source.meta.reference, &span, method))?;
    let id = blake3::hash(&key).to_hex().to_string();
    if !data.evidence.iter().any(|e| e.id == id) {
        data.evidence.push(Evidence {
            id: id.clone(),
            document: source.meta.reference.clone(),
            span,
            quote: source.text[range].to_string(),
            source_hash: source.meta.hash.clone(),
            method: method.into(),
        });
    }
    Ok(id)
}

fn edge(
    data: &mut Dataset,
    from: &str,
    to: &str,
    predicate: Predicate,
    polarity: Polarity,
    qualifiers: BTreeMap<String, String>,
    origin: Origin,
    proof: &str,
) -> Result<(), GraphError> {
    let key = serde_json::to_vec(&(from, to, predicate, polarity, &qualifiers))?;
    let id = blake3::hash(&key).to_hex().to_string();
    if let Some(existing) = data.edges.iter_mut().find(|e| e.id == id) {
        if !existing.evidence.iter().any(|e| e == proof) {
            existing.evidence.push(proof.into());
        }
    } else {
        data.edges.push(Edge {
            id,
            from: from.into(),
            to: to.into(),
            predicate,
            polarity,
            qualifiers,
            origin,
            evidence: vec![proof.into()],
            score: None,
        });
    }
    Ok(())
}

fn roles(path: &str, heading: &str) -> Vec<Role> {
    let text = format!("{path} {heading}").to_lowercase();
    let mut roles = Vec::new();
    for (role, words) in [
        (
            Role::Requirement,
            &["requirements", "main_idea", "требования"][..],
        ),
        (Role::Decision, &["adr/", "решение", "принятое решение"][..]),
        (
            Role::Architecture,
            &["architecture/", "устройство", "архитектура"][..],
        ),
        (
            Role::Constraint,
            &["ограничения", "границы", "constraints"][..],
        ),
        (
            Role::Verification,
            &["verification", "проверки", "приёмка", "результаты проверки"][..],
        ),
        (
            Role::Behavior,
            &["поведение", "как работает", "действия пользователя"][..],
        ),
    ] {
        if words.iter().any(|w| text.contains(w)) {
            roles.push(role);
        }
    }
    roles
}

fn body_start(text: &str) -> usize {
    let bom = if text.starts_with('\u{feff}') { 3 } else { 0 };
    let mut offset = bom;
    let mut lines = text[bom..].split_inclusive(['\r', '\n']);
    if lines.next().is_none_or(|l| l.trim_end() != "---") {
        return bom;
    }
    offset += text[bom..]
        .split_inclusive(['\r', '\n'])
        .next()
        .map_or(0, str::len);
    for line in lines {
        offset += line.len();
        if matches!(line.trim_end(), "---" | "...") {
            return offset;
        }
    }
    bom
}

pub fn anchor(label: &str) -> String {
    label
        .to_lowercase()
        .chars()
        .filter_map(|c| {
            if c.is_whitespace() {
                Some('-')
            } else if c.is_alphanumeric() || matches!(c, '-' | '_') {
                Some(c)
            } else {
                None
            }
        })
        .collect()
}

fn clean(label: &str) -> String {
    label
        .trim()
        .trim_start_matches(['-', '*', '+', ' '])
        .trim()
        .trim_matches(['`', '«', '»', '"', '\'', '.', ';', ':'])
        .trim()
        .to_string()
}

fn entity(data: &mut Dataset, source: &SourceDocument, label: &str, proof: &str) -> String {
    let label = clean(label);
    let explicit_requirement = label
        .strip_prefix("Т-")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    let namespace = if explicit_requirement {
        format!("{}:requirement", data.namespace)
    } else {
        format!("{}:{}", data.namespace, source.meta.docs_path)
    };
    let reference = object_ref(
        &namespace,
        Kind::Entity,
        &label.to_lowercase(),
        METHOD_VERSION,
    );
    if let Some(node) = data
        .nodes
        .iter_mut()
        .find(|n| n.reference.id == reference.id)
    {
        if !node.source_ids.contains(&source.meta.reference.id) {
            node.source_ids.push(source.meta.reference.id.clone());
        }
        if !node.evidence.iter().any(|e| e == proof) {
            node.evidence.push(proof.into());
        }
    } else {
        data.nodes.push(Node {
            reference: reference.clone(),
            label,
            namespace,
            aliases: vec![],
            source_ids: vec![source.meta.reference.id.clone()],
            evidence: vec![proof.into()],
            roles: vec![],
            parent: None,
            anchor: None,
        });
    }
    reference.id
}

/// Только явно выраженные предикаты; правило не делает смыслового вывода из cosine.
fn assertions(
    data: &mut Dataset,
    source: &SourceDocument,
    owner: &str,
    range: Range<usize>,
) -> Result<(), GraphError> {
    let text = &source.text[range.clone()];
    let mut start = 0;
    let mut code = false;
    let mut nesting = 0usize;
    for (at, ch) in text.char_indices() {
        if ch == '`' {
            code = !code;
        }
        if !code {
            if matches!(ch, '(' | '[') {
                nesting += 1;
            } else if matches!(ch, ')' | ']') {
                nesting = nesting.saturating_sub(1);
            }
        }
        let end = at + ch.len_utf8();
        if !code
            && nesting == 0
            && matches!(ch, '.' | '!' | '?' | ';')
            && text[end..].chars().next().is_none_or(char::is_whitespace)
        {
            assertion_clause(data, source, owner, range.start + start..range.start + end)?;
            start = end;
        }
    }
    if start < text.len() {
        assertion_clause(data, source, owner, range.start + start..range.end)?;
    }
    Ok(())
}

fn assertion_clause(
    data: &mut Dataset,
    source: &SourceDocument,
    owner: &str,
    range: Range<usize>,
) -> Result<(), GraphError> {
    let raw = source.text[range.clone()].trim();
    if raw.contains('?') {
        return Ok(());
    }
    let mut conditions = BTreeMap::new();
    let mut clause = raw;
    if ["если ", "при ", "когда "]
        .iter()
        .any(|p| raw.to_lowercase().starts_with(p))
    {
        let Some((condition, rest)) = raw.split_once(", ") else {
            return Ok(());
        };
        conditions.insert("condition".into(), condition.into());
        clause = rest;
    }
    for (phrase, predicate) in [
        ("зависит от", Predicate::DependsOn),
        ("использует", Predicate::Uses),
        ("реализует", Predicate::Implements),
        ("проверяет", Predicate::Verifies),
        ("заменяет", Predicate::Supersedes),
        ("требует", Predicate::Requires),
        ("запускается только после", Predicate::Requires),
        ("выполняется только после", Predicate::Requires),
    ] {
        let lower = clause.to_lowercase();
        if lower.len() != clause.len() {
            continue;
        }
        let marker = format!(" {phrase} ");
        let Some(at) = lower.find(&marker) else {
            continue;
        };
        let mut left = clause[..at].trim();
        let mut right = clause[at + marker.len()..].trim();
        let polarity = if left.to_lowercase().ends_with(" не") {
            left = left[..left.len() - " не".len()].trim();
            Polarity::Negative
        } else {
            Polarity::Positive
        };
        let mut qualifiers = conditions.clone();
        if let Some((object, condition)) = right.split_once(", если ") {
            right = object;
            qualifiers.insert("condition".into(), format!("если {condition}"));
        }
        // Несколько предложений/предикатов требуют отдельного разбора; не поглощать соседний факт.
        if left.contains(['\n', '.', '!', ';', ':', ',']) || right.contains(['\n', '!', ';']) {
            continue;
        }
        // Местоимение не является именованным участником; порядок слов не задаёт направление.
        if !explicit_operand(left) || !explicit_operand(right) {
            continue;
        }
        right = right.trim_end_matches('.');
        if right.contains(". ")
            || left.is_empty()
            || right.is_empty()
            || left.chars().count() > 160
            || right.chars().count() > 160
        {
            continue;
        }
        let proof = evidence(data, source, range.clone(), METHOD_VERSION)?;
        let from = entity(data, source, left, &proof);
        let to = entity(data, source, right, &proof);
        edge(
            data,
            owner,
            &from,
            Predicate::Mentions,
            Polarity::Positive,
            BTreeMap::new(),
            Origin::Structural,
            &proof,
        )?;
        edge(
            data,
            owner,
            &to,
            Predicate::Mentions,
            Polarity::Positive,
            BTreeMap::new(),
            Origin::Structural,
            &proof,
        )?;
        edge(
            data,
            &from,
            &to,
            predicate,
            polarity,
            qualifiers,
            Origin::ExplicitRule,
            &proof,
        )?;
        break;
    }
    Ok(())
}

fn explicit_operand(text: &str) -> bool {
    // Без синтаксического разбора нельзя определить участника сложной фразы.
    if text.bytes().filter(|b| *b == b'`').count() % 2 != 0 {
        return false;
    }
    !text.split_whitespace().any(|word| {
        let lower = word
            .trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '_')
            .to_lowercase();
        matches!(
            lower.as_str(),
            "он" | "она"
                | "оно"
                | "они"
                | "это"
                | "его"
                | "её"
                | "ее"
                | "их"
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
                | "использует"
                | "проверяет"
                | "требует"
                | "зависит"
                | "заменяет"
                | "реализует"
        )
    })
}

struct Heading {
    start: usize,
    end: usize,
    level: u8,
    text: String,
    node: String,
}

pub fn build(
    namespace: &str,
    docs_root: &str,
    inputs: Vec<InputDocument>,
) -> Result<Dataset, GraphError> {
    build_cancellable(namespace, docs_root, inputs, || false)
}

pub fn build_cancellable(
    namespace: &str,
    docs_root: &str,
    mut inputs: Vec<InputDocument>,
    mut cancelled: impl FnMut() -> bool,
) -> Result<Dataset, GraphError> {
    inputs.sort_by(|a, b| a.docs_path.cmp(&b.docs_path));
    let mut data = Dataset::empty(namespace, docs_root);
    let mut pending = Vec::new();
    let mut seen = BTreeSet::new();
    for input in inputs {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        if input.docs_path.is_empty()
            || input.docs_path.starts_with('/')
            || input.docs_path.split('/').any(|s| s == "..")
            || !seen.insert(input.docs_path.clone())
        {
            return Err(GraphError::InvalidData(
                "неверный/повторный путь документа".into(),
            ));
        }
        let hash = blake3::hash(input.text.as_bytes()).to_hex().to_string();
        let reference = object_ref(namespace, Kind::Document, &input.docs_path, &hash);
        let source = SourceDocument {
            meta: SourceMeta {
                reference: reference.clone(),
                path: input.path,
                docs_path: input.docs_path,
                module: input.module,
                title: input.title.clone(),
                hash: hash.clone(),
            },
            text: input.text,
        };
        let doc_id = reference.id.clone();
        let start = body_start(&source.text);
        let mut headings: Vec<Heading> = Vec::new();
        let mut paragraphs = Vec::new();
        let mut links = Vec::new();
        let mut current: Option<(Range<usize>, u8, String)> = None;
        for (event, range) in Parser::new_ext(
            &source.text[start..],
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH,
        )
        .into_offset_iter()
        {
            if cancelled() {
                return Err(GraphError::Cancelled);
            }
            let range = range.start + start..range.end + start;
            match event {
                Event::Start(Tag::Heading { level, .. })
                    if source.text[range.clone()].trim_start().starts_with('#') =>
                {
                    current = Some((range, level as u8, String::new()));
                }
                Event::Text(text) | Event::Code(text) => {
                    if let Some((_, _, title)) = &mut current {
                        title.push_str(&text);
                    }
                }
                Event::End(TagEnd::Heading(_)) => {
                    if let Some((range, level, text)) = current.take() {
                        let key = format!("{}:{}", source.meta.docs_path, range.start);
                        let node = object_ref(namespace, Kind::Section, &key, &hash).id;
                        headings.push(Heading {
                            start: range.start,
                            end: range.end,
                            level,
                            text,
                            node,
                        });
                    }
                }
                Event::Start(Tag::Paragraph) => paragraphs.push(range),
                Event::Start(Tag::Link { dest_url, .. }) => {
                    links.push((range, dest_url.to_string()))
                }
                _ => {}
            }
        }
        let doc_range = headings.first().map_or(start..start, |h| h.start..h.end);
        let proof = evidence(&mut data, &source, doc_range, "markdown-source-v1")?;
        data.nodes.push(Node {
            reference: reference.clone(),
            label: input.title,
            namespace: namespace.into(),
            aliases: vec![],
            source_ids: vec![doc_id.clone()],
            evidence: vec![proof],
            roles: roles(&source.meta.docs_path, ""),
            parent: None,
            anchor: None,
        });
        let mut stack: Vec<&Heading> = vec![];
        let mut anchors = BTreeMap::<String, usize>::new();
        for heading in &headings {
            while stack.last().is_some_and(|h| h.level >= heading.level) {
                stack.pop();
            }
            let parent = stack.last().map_or(doc_id.as_str(), |h| h.node.as_str());
            let proof = evidence(
                &mut data,
                &source,
                heading.start..heading.end,
                "markdown-source-v1",
            )?;
            let base = anchor(&heading.text);
            let count = anchors.entry(base.clone()).or_default();
            let anchor = if *count == 0 {
                base
            } else {
                format!("{base}-{count}")
            };
            *count += 1;
            data.nodes.push(Node {
                reference: ObjectRef {
                    kind: Kind::Section,
                    id: heading.node.clone(),
                    revision: hash.clone(),
                },
                label: heading.text.clone(),
                namespace: source.meta.docs_path.clone(),
                aliases: vec![],
                source_ids: vec![doc_id.clone()],
                evidence: vec![proof.clone()],
                roles: roles(&source.meta.docs_path, &heading.text),
                parent: Some(parent.into()),
                anchor: Some(anchor),
            });
            edge(
                &mut data,
                parent,
                &heading.node,
                Predicate::Contains,
                Polarity::Positive,
                BTreeMap::new(),
                Origin::Structural,
                &proof,
            )?;
            stack.push(heading);
        }
        let owner = |offset: usize| {
            headings
                .iter()
                .rev()
                .find(|h| h.start <= offset)
                .map_or(doc_id.as_str(), |h| h.node.as_str())
        };
        for range in paragraphs {
            let proof = evidence(&mut data, &source, range.clone(), "markdown-source-v1")?;
            let owner_id = owner(range.start);
            if let Some(node) = data.nodes.iter_mut().find(|n| n.reference.id == owner_id) {
                node.evidence.push(proof);
            }
            assertions(&mut data, &source, owner_id, range)?;
        }
        for (range, target) in links {
            let proof = evidence(&mut data, &source, range.clone(), "markdown-link-v1")?;
            pending.push((
                source.meta.docs_path.clone(),
                owner(range.start).to_string(),
                proof,
                target,
            ));
        }
        let mut previous: Option<String> = None;
        for chunk in input.chunks {
            let reference = object_ref(
                namespace,
                Kind::Chunk,
                &format!("{}:{}", source.meta.docs_path, chunk.key),
                &hash,
            );
            let mut proofs = Vec::new();
            for range in &chunk.spans {
                proofs.push(evidence(
                    &mut data,
                    &source,
                    range.clone(),
                    "chunk-source-v1",
                )?);
            }
            data.nodes.push(Node {
                reference: reference.clone(),
                label: chunk.label,
                namespace: source.meta.docs_path.clone(),
                aliases: vec![],
                source_ids: vec![doc_id.clone()],
                evidence: proofs.clone(),
                roles: roles(&source.meta.docs_path, ""),
                parent: Some(doc_id.clone()),
                anchor: None,
            });
            for proof in &proofs {
                edge(
                    &mut data,
                    &doc_id,
                    &reference.id,
                    Predicate::Contains,
                    Polarity::Positive,
                    BTreeMap::new(),
                    Origin::Structural,
                    proof,
                )?;
                let ev = data
                    .evidence
                    .iter()
                    .find(|e| &e.id == proof)
                    .unwrap()
                    .span
                    .clone();
                for (i, h) in headings.iter().enumerate() {
                    let end = headings
                        .get(i + 1)
                        .map_or(source.text.len(), |next| next.start);
                    if ev.start < end && ev.end > h.start {
                        edge(
                            &mut data,
                            &reference.id,
                            &h.node,
                            Predicate::Covers,
                            Polarity::Positive,
                            BTreeMap::new(),
                            Origin::Structural,
                            proof,
                        )?;
                    }
                }
            }
            if let (Some(prev), Some(proof)) = (previous, proofs.first()) {
                edge(
                    &mut data,
                    &prev,
                    &reference.id,
                    Predicate::NextChunk,
                    Polarity::Positive,
                    BTreeMap::new(),
                    Origin::Structural,
                    proof,
                )?;
            }
            previous = Some(reference.id);
        }
        data.sources.push(source);
    }
    for (path, owner, proof, target) in pending {
        if cancelled() {
            return Err(GraphError::Cancelled);
        }
        if target.contains(':') || target.starts_with("//") {
            continue;
        }
        let (file, fragment) = target
            .split_once('#')
            .map_or((target.as_str(), None), |(p, a)| (p, Some(a)));
        let target_path = if file.is_empty() {
            Some(path.clone())
        } else {
            resolve_path(&path, file)
        };
        let target_source = target_path
            .as_ref()
            .and_then(|p| data.sources.iter().find(|s| &s.meta.docs_path == p));
        let resolved = target_source.and_then(|source| {
            if let Some(fragment) = fragment {
                let fragment = decode_percent(fragment)?;
                data.nodes
                    .iter()
                    .find(|n| {
                        n.reference.kind == Kind::Section
                            && n.source_ids.contains(&source.meta.reference.id)
                            && n.anchor.as_ref() == Some(&fragment)
                    })
                    .map(|n| n.reference.id.clone())
            } else {
                Some(source.meta.reference.id.clone())
            }
        });
        if let Some(id) = resolved {
            edge(
                &mut data,
                &owner,
                &id,
                Predicate::LinksTo,
                Polarity::Positive,
                BTreeMap::new(),
                Origin::Structural,
                &proof,
            )?;
        } else {
            data.broken_links.push(BrokenLink {
                source: owner,
                evidence: proof,
                target,
                reason: if target_path.is_none() {
                    "outside_docs_root"
                } else if target_source.is_some() {
                    "anchor_missing"
                } else {
                    "target_not_indexed"
                }
                .into(),
            });
        }
    }
    for node in data
        .nodes
        .iter_mut()
        .filter(|n| n.reference.kind == Kind::Entity)
    {
        let mut proof = node.evidence.clone();
        proof.sort();
        proof.dedup();
        node.reference.revision = blake3::hash(&serde_json::to_vec(&proof)?)
            .to_hex()
            .to_string();
    }
    data.warnings.push("Явные правила не гарантируют полноту смысловых отношений; отсутствие связи не доказывает отсутствие проверки или зависимости".into());
    data.fingerprint();
    Ok(data)
}

fn decode_percent(text: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text.as_bytes()[i] == b'%' {
            bytes.push(u8::from_str_radix(text.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            bytes.push(text.as_bytes()[i]);
            i += 1;
        }
    }
    String::from_utf8(bytes).ok()
}
fn resolve_path(source: &str, target: &str) -> Option<String> {
    let target = decode_percent(target)?.replace('\\', "/");
    if target.starts_with('/') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<_> = source.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(path: &str, text: &str) -> InputDocument {
        InputDocument {
            path: format!("docs/{path}"),
            docs_path: path.into(),
            module: "features".into(),
            title: path.into(),
            text: text.into(),
            chunks: vec![],
        }
    }
    #[test]
    fn source_offsets_reference_links_code_and_duplicate_anchors() {
        let text = "\u{feff}---\r\ntitle: А\r\n---\r\n# А\r\n[переход][r]\r\n\r\n`[код](bad.md)`\r\n\r\n```md\r\n[x](bad.md)\r\n```\r\n\r\n[r]: b.md#дубль-1\r\n";
        let data = build(
            "test",
            "docs",
            vec![
                input("a.md", text),
                input("b.md", "# Б\n## Дубль\nПервый\n## Дубль\nВторой"),
            ],
        )
        .unwrap();
        let links: Vec<_> = data
            .edges
            .iter()
            .filter(|e| e.predicate == Predicate::LinksTo)
            .collect();
        assert_eq!(links.len(), 1);
        assert!(data.broken_links.is_empty());
        let target = data
            .nodes
            .iter()
            .find(|n| n.reference.id == links[0].to)
            .unwrap();
        assert_eq!(target.anchor.as_deref(), Some("дубль-1"));
        for ev in &data.evidence {
            let source = data
                .sources
                .iter()
                .find(|s| s.meta.reference.id == ev.document.id)
                .unwrap();
            assert_eq!(source.text[ev.span.start..ev.span.end], ev.quote);
        }
    }
    #[test]
    fn polarity_condition_and_link_are_distinct() {
        let data = build(
            "test",
            "docs",
            vec![
                input(
                    "a.md",
                    "# А\n\nЕсли выбран CPU, `Реранкер` не требует `GPU`.\n\n[GPU](b.md)\n",
                ),
                input("b.md", "# GPU"),
            ],
        )
        .unwrap();
        let assertions: Vec<_> = data
            .edges
            .iter()
            .filter(|e| e.predicate.semantic())
            .collect();
        assert_eq!(assertions.len(), 1);
        assert_eq!(assertions[0].polarity, Polarity::Negative);
        assert_eq!(assertions[0].qualifiers["condition"], "Если выбран CPU");
        assert_eq!(
            data.edges
                .iter()
                .filter(|e| e.predicate == Predicate::LinksTo)
                .count(),
            1
        );
    }
    #[test]
    fn outside_paths_and_invalid_utf8_boundaries_are_not_facts() {
        let data = build(
            "test",
            "docs",
            vec![input(
                "a.md",
                "# А\n\n[вне](../secret.md) [нет](missing.md)\n",
            )],
        )
        .unwrap();
        assert_eq!(data.broken_links.len(), 2);
        assert_eq!(data.broken_links[0].reason, "outside_docs_root");
        assert!(span("А", 0..1).is_err());
    }
    #[test]
    fn edit_changes_revision_snapshot_and_does_not_reuse_another_heading() {
        let a = build("test", "docs", vec![input("a.md", "# Один\nТекст")]).unwrap();
        let b = build("test", "docs", vec![input("a.md", "# Другой\nТекст")]).unwrap();
        assert_ne!(a.snapshot, b.snapshot);
        assert_eq!(
            a.sources[0].meta.reference.id,
            b.sources[0].meta.reference.id
        );
        assert_ne!(
            a.sources[0].meta.reference.revision,
            b.sources[0].meta.reference.revision
        );
    }
}
