//! Общий текст графа для MCP/CLI. Нормализованные DTO остаются внутренними справочниками.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::graph_types::{
    Edge, Evidence, GraphPath, GraphReply, Item, Kind, LayerState, Node, Operation, Origin,
    Polarity, SourceMeta,
};
use crate::{GraphSearchOutput, SearchOutput};

pub const MAX_TEXT_CHARS: usize = 16_000;

#[cfg(test)]
#[path = "graph_text_tests.rs"]
mod tests;

fn name<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .expect("enum сериализуется")
        .as_str()
        .expect("имя enum")
        .into()
}

fn short(value: &str, limit: usize) -> String {
    let mut clean = String::new();
    let mut count = 0;
    let mut clipped = false;
    'words: for word in value.split_whitespace() {
        if !clean.is_empty() {
            clean.push(' ');
            count += 1;
        }
        for ch in word.chars() {
            if count >= limit.saturating_sub(1) {
                clipped = true;
                break 'words;
            }
            clean.push(ch);
            count += 1;
        }
    }
    if !clipped {
        return clean;
    }
    let end = clean.len();
    let cut = clean.as_str();
    let sentence = cut
        .rfind(['.', '!', '?'])
        .filter(|&i| i > end / 2)
        .map(|i| &cut[..i + 1]);
    format!("{}…", sentence.unwrap_or(cut).trim_end())
}

struct View<'a> {
    reply: &'a GraphReply,
    nodes: BTreeMap<&'a str, &'a Node>,
    sources: BTreeMap<&'a str, &'a SourceMeta>,
    evidence: BTreeMap<&'a str, &'a Evidence>,
    edges: BTreeMap<&'a str, &'a Edge>,
    quote_chars: usize,
    quote_groups: BTreeMap<&'a str, QuoteGroup<'a>>,
}
#[derive(Clone, Copy)]
struct QuoteGroup<'a> {
    proof: &'a Evidence,
    first: usize,
    last: usize,
}

fn quote_groups(reply: &GraphReply) -> BTreeMap<&str, QuoteGroup<'_>> {
    let nodes: BTreeMap<_, _> = reply
        .nodes
        .iter()
        .map(|n| (n.reference.id.as_str(), n))
        .collect();
    let edges: BTreeMap<_, _> = reply
        .assertions
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let mut ids: BTreeSet<_> = reply
        .assertions
        .iter()
        .filter(|e| e.origin == Origin::ExplicitRule)
        .flat_map(|e| e.evidence.iter().map(String::as_str))
        .collect();
    for item in &reply.items {
        match item {
            Item::RoleGroup { evidence, .. }
            | Item::Audit { evidence, .. }
            | Item::Conflict { evidence, .. } => ids.extend(evidence.iter().map(String::as_str)),
            Item::Node { node } => ids.extend(node.evidence.iter().map(String::as_str)),
            Item::Topic {
                members,
                representatives,
                ..
            } => {
                for reference in members.iter().chain(representatives) {
                    if let Some(node) = nodes.get(reference.id.as_str()) {
                        ids.extend(node.evidence.iter().map(String::as_str));
                    }
                }
            }
            Item::Path { path } | Item::Impact { path, .. } => {
                let step_proofs: BTreeSet<_> = path
                    .steps
                    .iter()
                    .filter_map(|s| edges.get(s.edge.as_str()))
                    .flat_map(|e| e.evidence.iter().map(String::as_str))
                    .collect();
                ids.extend(
                    path.evidence
                        .iter()
                        .map(String::as_str)
                        .filter(|id| !step_proofs.contains(id)),
                );
            }
            _ => {}
        }
    }
    let mut proofs: Vec<_> = reply
        .evidence
        .iter()
        .filter(|e| ids.contains(e.id.as_str()))
        .collect();
    proofs.sort_by_key(|e| {
        (
            &e.document.id,
            e.span.first_line,
            std::cmp::Reverse(e.span.last_line),
        )
    });
    let mut groups: Vec<(QuoteGroup<'_>, Vec<&str>)> = Vec::new();
    for proof in proofs {
        if let Some((group, members)) = groups
            .last_mut()
            .filter(|(g, _)| g.proof.document == proof.document && proof.span.first_line <= g.last)
        {
            group.last = group.last.max(proof.span.last_line);
            members.push(&proof.id);
            if proof.quote.len() > group.proof.quote.len() {
                group.proof = proof;
            }
        } else {
            groups.push((
                QuoteGroup {
                    proof,
                    first: proof.span.first_line,
                    last: proof.span.last_line,
                },
                vec![&proof.id],
            ));
        }
    }
    groups
        .into_iter()
        .flat_map(|(group, ids)| ids.into_iter().map(move |id| (id, group)))
        .collect()
}
impl<'a> View<'a> {
    fn new(reply: &'a GraphReply) -> Self {
        Self {
            reply,
            quote_groups: quote_groups(reply),
            quote_chars: if reply.limit_reasons.iter().any(|r| r == "quote_truncated") {
                30
            } else {
                300
            },
            nodes: reply
                .nodes
                .iter()
                .chain(&reply.candidates)
                .map(|n| (n.reference.id.as_str(), n))
                .collect(),
            sources: reply
                .sources
                .iter()
                .map(|s| (s.reference.id.as_str(), s))
                .collect(),
            evidence: reply.evidence.iter().map(|e| (e.id.as_str(), e)).collect(),
            edges: reply
                .assertions
                .iter()
                .map(|e| (e.id.as_str(), e))
                .collect(),
        }
    }
    fn label(&self, id: &str) -> String {
        let Some(n) = self.nodes.get(id) else {
            return "<unresolved object>".into();
        };
        if n.reference.kind == Kind::Entity {
            return short(&n.label, 180);
        }
        let source = self.sources.get(id).or_else(|| {
            n.source_ids
                .iter()
                .find_map(|id| self.sources.get(id.as_str()))
        });
        match (n.reference.kind, source) {
            (Kind::Document, Some(s)) => s.docs_path.clone(),
            (Kind::Section | Kind::Chunk, Some(s)) => {
                format!("{} › {}", s.docs_path, short(&n.label, 180))
            }
            _ => short(&n.label, 180),
        }
    }
    fn location(&self, proof: &Evidence) -> String {
        let path = self
            .sources
            .get(proof.document.id.as_str())
            .map_or("<source unavailable>", |s| s.docs_path.as_str());
        format!("{path}:{}-{}", proof.span.first_line, proof.span.last_line)
    }
    fn item_sources(&self, item: &Item) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        let mut add = |id: &str| {
            if let Some(node) = self.nodes.get(id) {
                ids.extend(node.source_ids.iter().cloned());
            }
        };
        match item {
            Item::Node { node } => add(&node.reference.id),
            Item::Edge { edge: e } | Item::Relation { assertion: e } => {
                add(&e.from);
                add(&e.to);
            }
            Item::Path { path } | Item::Impact { path, .. } => {
                for node in &path.nodes {
                    add(&node.id);
                }
            }
            Item::Topic { members, .. } => {
                for node in members {
                    add(&node.id);
                }
            }
            _ => {}
        }
        ids
    }
}

#[derive(Default, Clone)]
struct Citations {
    // Ссылка на первый факт вместо повторной цитаты в другом пути/роли.
    seen: BTreeMap<String, Vec<(usize, usize, usize, bool)>>,
}
impl Citations {
    fn render(&mut self, view: &View<'_>, ids: &[String], number: usize, quote: bool) -> String {
        self.render_quotes(view, ids, number, quote, 3)
    }
    fn render_quotes(
        &mut self,
        view: &View<'_>,
        ids: &[String],
        number: usize,
        quote: bool,
        max_quotes: usize,
    ) -> String {
        let mut proofs: Vec<_> = ids
            .iter()
            .filter_map(|id| view.evidence.get(id.as_str()).copied())
            .map(|proof| {
                if quote {
                    view.quote_groups
                        .get(proof.id.as_str())
                        .copied()
                        .unwrap_or(QuoteGroup {
                            proof,
                            first: proof.span.first_line,
                            last: proof.span.last_line,
                        })
                } else {
                    QuoteGroup {
                        proof,
                        first: proof.span.first_line,
                        last: proof.span.last_line,
                    }
                }
            })
            .collect();
        proofs.sort_by_key(|g| (&g.proof.document.id, g.first, std::cmp::Reverse(g.last)));
        let mut merged: Vec<(&Evidence, usize, usize)> = Vec::new();
        for group in proofs {
            let proof = group.proof;
            if let Some((previous, first, last)) = merged
                .last_mut()
                .filter(|(p, _, last)| p.document == proof.document && group.first <= *last)
            {
                *first = (*first).min(group.first);
                *last = (*last).max(group.last);
                if proof.quote.len() > previous.quote.len() {
                    *previous = proof;
                }
            } else {
                merged.push((proof, group.first, group.last));
            }
        }
        let total = merged.len();
        let mut text = String::new();
        let mut printed = 0;
        for (proof, first, last) in merged {
            let source = view
                .sources
                .get(proof.document.id.as_str())
                .map_or("<source unavailable>", |s| s.docs_path.as_str());
            let location = format!("{source}:{first}-{last}");
            let rows = self.seen.entry(proof.document.id.clone()).or_default();
            let previous = rows
                .iter()
                .find(|(a, b, _, quoted)| *a <= first && *b >= last && (!quote || *quoted));
            if let Some((_, _, n, _)) = previous {
                if *n != number {
                    let _ = writeln!(text, "   {location} (see fact {n})");
                }
                continue;
            }
            let _ = write!(text, "   {location}");
            let quoted = quote && printed < max_quotes;
            if quoted {
                let _ = write!(text, " «{}»", short(&proof.quote, view.quote_chars));
            }
            text.push('\n');
            rows.push((first, last, number, quoted));
            printed += 1;
        }
        if quote && total > max_quotes {
            let _ = writeln!(
                text,
                "   +{} more sources; full text: bars_read",
                total - max_quotes
            );
        }
        text
    }
}

fn edge_text(view: &View<'_>, edge: &Edge) -> String {
    let mut text = format!(
        "{} —{}→ {}",
        view.label(&edge.from),
        name(edge.predicate),
        view.label(&edge.to)
    );
    if edge.polarity == Polarity::Negative {
        text.push_str(" [negative]");
    }
    match edge.origin {
        Origin::Structural => text.push_str(" [structural]"),
        Origin::Similarity => {
            if let Some(score) = edge.score {
                let _ = write!(text, " [similarity {score:.2}]");
            } else {
                text.push_str(" [similarity]");
            }
        }
        Origin::ExplicitRule => {}
    }
    for (key, value) in &edge.qualifiers {
        let _ = write!(text, " [{}={}]", short(key, 80), short(value, 180));
    }
    text
}

fn path_text(view: &View<'_>, path: &GraphPath, n: usize, citations: &mut Citations) -> String {
    let mut text = format!(
        "path {n} ({} hops, {}{})\n   {}\n",
        path.steps.len(),
        name(view.reply.requested.path_kind),
        if path.complete { "" } else { ", incomplete" },
        path.nodes
            .iter()
            .map(|r| view.label(&r.id))
            .collect::<Vec<_>>()
            .join(" → ")
    );
    for (i, step) in path.steps.iter().enumerate() {
        if let Some(edge) = view.edges.get(step.edge.as_str()) {
            let _ = writeln!(
                text,
                "   step {}: {}{}",
                i + 1,
                edge_text(view, edge),
                if step.reversed {
                    " [reverse traversal]"
                } else {
                    ""
                }
            );
            text.push_str(&citations.render(
                view,
                &edge.evidence,
                n,
                edge.origin == Origin::ExplicitRule,
            ));
        } else {
            let _ = writeln!(text, "   step {}: assertion unavailable", i + 1);
        }
    }
    let step_proofs: BTreeSet<_> = path
        .steps
        .iter()
        .filter_map(|step| view.edges.get(step.edge.as_str()))
        .flat_map(|edge| edge.evidence.iter())
        .collect();
    let extra: Vec<_> = path
        .evidence
        .iter()
        .filter(|id| !step_proofs.contains(id))
        .cloned()
        .collect();
    text.push_str(&citations.render(view, &extra, n, true));
    text
}

fn unit(view: &View<'_>, item: &Item, n: usize, citations: &mut Citations) -> String {
    match item {
        Item::Relation { assertion: edge } | Item::Edge { edge } => format!(
            "{n}. {}\n{}",
            edge_text(view, edge),
            citations.render(view, &edge.evidence, n, edge.origin == Origin::ExplicitRule)
        ),
        Item::Path { path } => path_text(view, path, n, citations),
        Item::Impact { node, direct, path } => format!(
            "{n}. {}: {} (проверить описание)\n{}",
            if *direct { "direct" } else { "indirect" },
            view.label(&node.id),
            path_text(view, path, n, citations)
        ),
        Item::RoleGroup { role, evidence } => format!(
            "{n}. role: {}\n{}",
            name(role),
            citations.render_quotes(view, evidence, n, true, usize::MAX)
        ),
        Item::Node { node } => format!(
            "{n}. {} ({})\n{}",
            view.label(&node.reference.id),
            name(node.reference.kind),
            citations.render(view, &node.evidence, n, true)
        ),
        Item::Topic {
            label,
            members,
            representatives,
            ..
        } => {
            let mut text = format!(
                "{n}. topic: {} ({} members)\n",
                short(label, 180),
                members.len()
            );
            for r in representatives {
                let _ = writeln!(text, "   representative: {}", view.label(&r.id));
                if let Some(node) = view.nodes.get(r.id.as_str()) {
                    text.push_str(&citations.render(view, &node.evidence, n, true));
                }
            }
            for r in members.iter().filter(|r| !representatives.contains(r)) {
                let _ = writeln!(text, "   member: {}", view.label(&r.id));
                if let Some(node) = view.nodes.get(r.id.as_str()) {
                    text.push_str(&citations.render(view, &node.evidence, n, true));
                }
            }
            text
        }
        Item::Audit {
            kind,
            node,
            evidence,
            detail,
        } => format!(
            "{n}. {}{}: {}\n{}",
            short(kind, 100),
            node.as_ref()
                .map_or(String::new(), |r| format!(" · {}", view.label(&r.id))),
            short(detail, 500),
            citations.render(view, evidence, n, true)
        ),
        Item::Conflict {
            assertions,
            evidence,
            ..
        } => {
            let mut text = format!("{n}. conflict candidate, not a verdict\n");
            for id in assertions {
                if let Some(e) = view.edges.get(id.as_str()) {
                    let _ = writeln!(text, "   {}", edge_text(view, e));
                    text.push_str(&citations.render(view, &e.evidence, n, true));
                }
            }
            text.push_str(&citations.render(view, evidence, n, true));
            text
        }
    }
}

fn status(reply: &GraphReply) -> String {
    let mut text = String::new();
    for (layer, state) in &reply.layer_states {
        if *state != LayerState::Ready {
            let _ = writeln!(
                text,
                "state: {layer} layer {} ({} of {} files)",
                name(state),
                reply.coverage.processed,
                reply.coverage.indexed
            );
        }
    }
    if reply.layer_states.is_empty() && reply.layer_state != LayerState::Ready {
        let _ = writeln!(text, "state: graph {}", name(reply.layer_state));
    }
    if !reply.missing_roles.is_empty() {
        let _ = writeln!(
            text,
            "state: missing roles: {}",
            reply
                .missing_roles
                .iter()
                .map(name)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for warning in &reply.warnings {
        let _ = writeln!(
            text,
            "state: {}",
            short(&crate::graph_types::humanize_warning(warning.clone()), 500)
        );
    }
    if !reply.scope.modules.is_empty() {
        let _ = writeln!(
            text,
            "state: scope modules={}",
            short(&reply.scope.modules.join(", "), 400)
        );
    }
    if !reply.scope.files.is_empty() {
        let _ = writeln!(
            text,
            "state: scope files={}",
            short(&reply.scope.files.join(", "), 600)
        );
    }
    let defaults = crate::graph_types::Budgets::default()
        .resolve()
        .expect("умолчания")
        .0;
    macro_rules! budget {
        ($field:ident) => {
            if reply.applied.$field != defaults.$field
                || reply
                    .requested
                    .budgets
                    .$field
                    .is_some_and(|v| v != reply.applied.$field)
            {
                let _ = write!(
                    text,
                    "state: applied: {}={}",
                    stringify!($field),
                    reply.applied.$field
                );
                if let Some(v) = reply
                    .requested
                    .budgets
                    .$field
                    .filter(|v| *v != reply.applied.$field)
                {
                    let _ = write!(text, " (requested {v}, clamped)");
                }
                text.push('\n');
            }
        };
    }
    budget!(max_depth);
    budget!(max_nodes);
    budget!(max_edges);
    budget!(max_fanout);
    budget!(max_paths);
    budget!(timeout_ms);
    budget!(max_bytes);
    text
}

/// Число целых элементов, которые помещаются в текст. Вызывается воркером до публикации курсора.
pub fn fitting_items(reply: &GraphReply, max_chars: usize) -> usize {
    let view = View::new(reply);
    let mut citations = Citations::default();
    let mut size = status(reply).chars().count() + 1800;
    let mut count = 0;
    let mut extra_sources = BTreeSet::new();
    for (i, item) in reply.items.iter().enumerate() {
        let text = unit(&view, item, i + 1, &mut citations);
        size += text.chars().count();
        for id in view.item_sources(item) {
            if !citations.seen.contains_key(&id)
                && extra_sources.insert(id.clone())
                && let Some(source) = view.sources.get(id.as_str())
            {
                size += source.docs_path.chars().count()
                    + short(&source.title, 180).chars().count()
                    + 20;
            }
        }
        if size > max_chars && i > 0 {
            break;
        }
        count += 1;
        if size > max_chars {
            break;
        }
    }
    count
}

/// Ограничивает DTO только для текстового вызова. JSON-вызов эту функцию не использует.
pub fn prepare_graph_text(
    reply: &mut GraphReply,
    max_chars: usize,
) -> Result<(), crate::graph_types::GraphError> {
    // Роль — контейнер, единица ограничения контекста — отдельный фрагмент.
    if reply.operation == Operation::Context {
        let mut remaining = max_chars.saturating_sub(status(reply).chars().count() + 1800);
        let view = View::new(reply);
        let mut keep = BTreeSet::new();
        for item in &reply.items {
            if let Item::RoleGroup { evidence, .. } = item {
                for id in evidence {
                    if keep.contains(id) {
                        continue;
                    }
                    if let Some(proof) = view.evidence.get(id.as_str()) {
                        let size = view.location(proof).chars().count()
                            + short(&proof.quote, 300).chars().count()
                            + 100;
                        if size <= remaining || keep.is_empty() {
                            remaining = remaining.saturating_sub(size);
                            keep.insert(id.clone());
                        }
                    }
                }
            }
        }
        let mut changed = false;
        for item in &mut reply.items {
            if let Item::RoleGroup { evidence, .. } = item {
                let before = evidence.len();
                evidence.retain(|id| keep.contains(id));
                changed |= before != evidence.len();
            }
        }
        reply
            .items
            .retain(|item| !matches!(item,Item::RoleGroup{evidence,..} if evidence.is_empty()));
        if changed {
            mark_text_limit(reply);
            reply.next_cursor = None;
        }
        reply.missing_roles = reply
            .requested
            .context_roles()
            .into_iter()
            .filter(|r| {
                !reply
                    .items
                    .iter()
                    .any(|i| matches!(i,Item::RoleGroup{role,..} if role==r))
            })
            .collect();
    }
    let keep = fitting_items(reply, max_chars);
    if keep < reply.items.len() {
        crate::graph_types::query::truncate_text_page(reply, keep)?;
    }
    crate::graph_types::query::rematerialize_text_reply(reply);
    if render_graph(reply, false).chars().count() > max_chars {
        mark_text_limit(reply);
        reply.limit_reasons.push("quote_truncated".into());
        if render_graph(reply, false).chars().count() > max_chars {
            return Err(crate::graph_types::GraphError::InvalidParams("Первый факт или путь с обязательными основаниями не помещается в текстовый предел; используйте format=json / --json или сузьте files/predicates".into()));
        }
    }
    crate::graph_types::query::rematerialize_text_reply(reply);
    Ok(())
}
fn mark_text_limit(reply: &mut GraphReply) {
    reply.truncated = true;
    if !reply.limit_reasons.iter().any(|r| r == "text_limit") {
        reply.limit_reasons.push("text_limit".into());
    }
}

pub fn render_graph_text(reply: &GraphReply) -> String {
    render_graph(reply, false)
}
fn render_graph(reply: &GraphReply, related: bool) -> String {
    let view = View::new(reply);
    let operation = name(reply.operation);
    let mut text = if related {
        "=== RELATED ===\n".into()
    } else {
        format!("=== GRAPH {operation} ===\n")
    };
    if !reply.candidates.is_empty() {
        let _ = writeln!(text, "resolved: {}", short(&reply.resolution, 100));
        for (i, node) in reply.candidates.iter().take(5).enumerate() {
            let matches_from =
                reply
                    .requested
                    .from
                    .as_ref()
                    .is_some_and(|endpoint| match endpoint {
                        crate::graph_types::Endpoint::Ref(reference) => {
                            reference == &node.reference
                        }
                        crate::graph_types::Endpoint::Name {
                            label,
                            kind,
                            namespace,
                        } => {
                            let label = label.to_lowercase();
                            kind.is_none_or(|kind| kind == node.reference.kind)
                                && namespace
                                    .as_ref()
                                    .is_none_or(|namespace| namespace == &node.namespace)
                                && std::iter::once(&node.label).chain(node.aliases.iter()).any(
                                    |value| {
                                        let value = value.to_lowercase();
                                        value.contains(&label) || label.contains(&value)
                                    },
                                )
                        }
                        crate::graph_types::Endpoint::Document { .. } => {
                            node.reference.kind == Kind::Document
                        }
                    });
            let field = if reply.operation == Operation::Paths {
                if matches_from { "from" } else { "to" }
            } else {
                "entity_ref"
            };
            let _ = writeln!(
                text,
                "{}. «{}» ({}, {} documents) → {}:{}",
                i + 1,
                view.label(&node.reference.id),
                name(node.reference.kind),
                node.source_ids.len(),
                field,
                serde_json::to_string(&node.reference).expect("ObjectRef сериализуется")
            );
        }
    }
    if !reply.requested.documents.is_empty() || !reply.requested.roots.is_empty() {
        for source in &reply.sources {
            if reply.requested.roots.iter().any(|r| r == &source.reference)
                || reply.requested.documents.iter().any(|p| {
                    let normalize = crate::graph_types::hints::normalize_document_input;
                    normalize(p).is_some_and(|path| {
                        Some(&path) == normalize(&source.docs_path).as_ref()
                            || Some(&path) == normalize(&source.path).as_ref()
                            || normalize(&source.path)
                                .is_some_and(|saved| path.ends_with(&format!("/{saved}")))
                    })
                })
            {
                let _ = writeln!(text, "resolved: document {}", source.docs_path);
            }
        }
    }
    if reply.operation == Operation::Graph {
        let _ = writeln!(
            text,
            "counts: {} nodes, {} edges",
            reply.nodes.len(),
            reply.assertions.len()
        );
    }
    let mut citations = Citations::default();
    for (i, item) in reply.items.iter().enumerate() {
        if related {
            text.push_str(if matches!(item, Item::RoleGroup { .. }) {
                "why: query\n"
            } else if matches!(item, Item::Node { .. }) {
                "why: root\n"
            } else {
                "why: path\n"
            });
        }
        text.push_str(&unit(&view, item, i + 1, &mut citations));
    }
    for source in &reply.sources {
        if !citations.seen.contains_key(&source.reference.id) {
            let _ = writeln!(
                text,
                "source: {} · {}",
                source.docs_path,
                short(&source.title, 180)
            );
        }
    }
    text.push_str(&status(reply));
    if reply.truncated {
        if reply.limit_reasons.iter().any(|r| r == "timeout_ms") {
            let _ = writeln!(
                text,
                "state: timeout after {} ms; found {} items; missing roles: {}",
                reply.elapsed_ms,
                reply.items.len(),
                reply
                    .missing_roles
                    .iter()
                    .map(name)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        let _ = writeln!(
            text,
            "truncated: shown {} items; limits: {}",
            reply.items.len(),
            reply.limit_reasons.join(", ")
        );
        if let Some(cursor) = &reply.next_cursor {
            let _ = writeln!(text, "next: continue: cursor=\"{cursor}\"");
        } else {
            text.push_str(
                "next: narrow: сузьте modules/files, predicates или roles; уменьшите limit\n",
            );
        }
    }
    if reply.items.is_empty() {
        let _ = writeln!(
            text,
            "state: {}: в выбранной области и пределах результата нет; покрыто {} из {} документов, полнота смысловых связей неизвестна; отсутствие зависимости не доказано",
            reply.resolution, reply.coverage.processed, reply.coverage.indexed
        );
        if !reply.truncated {
            text.push_str(if reply.operation == Operation::Impact { "next: bars_links с direction=incoming для документа и bars_rag_search по его названию\n" } else if reply.resolution == "ambiguous" { "next: повторите вызов с ObjectRef выбранного кандидата\n" } else { "next: bars_rag_search без mode или bars_text_search по точной фразе\n" });
        }
    }
    text.push_str(if related {
        "=== END RELATED ==="
    } else {
        "=== END GRAPH ==="
    });
    text
}

/// Прежние RESULT-блоки: общий вид для расширенного поиска MCP и CLI.
pub fn render_search_text(output: &SearchOutput) -> String {
    search_text(output, usize::MAX, true).0
}
fn search_text(output: &SearchOutput, max_chars: usize, primary_search: bool) -> (String, usize) {
    let scope_summary = crate::search_scope_text(output, primary_search);
    let scope_summary = if scope_summary.chars().count() <= 1200 {
        scope_summary
    } else {
        "scope/by_module: truncated; full scope and counts: format=json\n".into()
    };
    let mut text = format!("=== SEARCH: results {} ===\n", output.results.len());
    let mut shown = 0;
    for (i, hit) in output.results.iter().enumerate() {
        let mut block = format!("=== RESULT {} of {} ===\n", i + 1, output.results.len());
        if !hit.chunk_id.is_empty() {
            let _ = writeln!(block, "id: {}", hit.chunk_id);
        }
        let _ = writeln!(
            block,
            "path: {}\nlines: {}-{}\nmodule: {}\ntitle: {}\nheading: {}",
            hit.path, hit.first_line, hit.last_line, hit.module, hit.title, hit.heading_chain
        );
        if !hit.sections.is_empty() {
            let _ = writeln!(block, "sections: {}", hit.sections.join("; "));
        }
        let _ = writeln!(
            block,
            "score: {:.3} ({})\n--- text ---\n{}\n=== END RESULT {} ===",
            hit.score,
            name(hit.score_kind),
            hit.text.trim_end(),
            i + 1
        );
        if text.chars().count() + block.chars().count() + 700 + scope_summary.chars().count()
            > max_chars
        {
            break;
        }
        text.push_str(&block);
        shown += 1;
    }
    let a = &output.applied;
    let _ = writeln!(
        text,
        "=== APPLIED PARAMETERS ===\nlimit: {}\nreturned: {shown}\ncandidates: {}\nmin_score: {} ({})\nclamped: {}\nindexing: {}\npending: {}",
        a.results,
        a.candidates,
        a.min_score,
        name(a.score_kind),
        a.clamped,
        output.indexing,
        output.pending
    );
    text.push_str(&scope_summary);
    if let Some(error) = &output.rerank_error {
        let _ = writeln!(text, "rerank_error: {}", short(error, 300));
    }
    if output.indexing {
        text.push_str("Indexing in progress: results may be incomplete.\n");
    }
    if shown < output.results.len() {
        let _ = writeln!(
            text,
            "truncated: shown {shown} of {} results; text_limit\nnext: narrow: уменьшите limit или сузьте modules/files; полный ответ: format=json",
            output.results.len()
        );
    }
    text.push_str("=== END SEARCH ===");
    (text, shown)
}

pub fn prepare_graph_search_text(
    output: &mut GraphSearchOutput,
) -> Result<(), crate::graph_types::GraphError> {
    let primary = search_text(
        &output.search,
        MAX_TEXT_CHARS / 2,
        output.applied_mode != crate::graph_types::RetrievalMode::Graph,
    )
    .0
    .chars()
    .count();
    let count = usize::from(output.related.is_some()) + usize::from(output.graph_results.is_some());
    for graph in [&mut output.related, &mut output.graph_results]
        .into_iter()
        .flatten()
    {
        prepare_graph_text(graph, MAX_TEXT_CHARS.saturating_sub(primary + 1000) / count)?;
    }
    if render_graph_search_text(output).chars().count() > MAX_TEXT_CHARS {
        return Err(crate::graph_types::GraphError::InvalidParams("Текст поиска с обязательными источниками не помещается в предел; сузьте files/limit или используйте format=json / --json".into()));
    }
    Ok(())
}
pub fn render_graph_search_text(output: &GraphSearchOutput) -> String {
    let has_graph = output.related.is_some() || output.graph_results.is_some();
    let (mut text, _) = search_text(
        &output.search,
        if has_graph {
            MAX_TEXT_CHARS / 2
        } else {
            MAX_TEXT_CHARS - 800
        },
        output.applied_mode != crate::graph_types::RetrievalMode::Graph,
    );
    if output.requested_mode != output.applied_mode || output.fallback_reason.is_some() {
        let _ = write!(
            text,
            "\nstate: fallback: {}→{}",
            name(output.requested_mode),
            name(output.applied_mode)
        );
        if let Some(reason) = &output.fallback_reason {
            let _ = write!(text, ", {}", short(reason, 500));
        }
    }
    if let Some(graph) = &output.related {
        text.push('\n');
        text.push_str(&render_graph(graph, true));
    }
    if let Some(graph) = &output.graph_results {
        text.push('\n');
        text.push_str(&render_graph(graph, false));
    }
    text
}
