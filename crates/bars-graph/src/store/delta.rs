//! Частичное обновление графа по изменённым документам (схема v3, раздел «Проект частичного
//! обновления графа» в docs/architecture/graph.md).
//!
//! Вызывающий разбирает только изменённые и новые документы (`source::parse_document`) и считает
//! близость чанков инкрементально; здесь одна транзакция заменяет в хранилище строки этих
//! документов и их связей. Результат построчно равен полной публикации тех же входов.
use super::*;
use crate::source::{
    self, DocParse, LinkContext, LinkOutcome, PART_KEY_SEPARATOR, PendingLink, part_seq,
};
use std::cell::RefCell;
use std::collections::HashSet;

/// Ссылки документа с порядковым номером части, в которой они стоят.
type NumberedLinks = Vec<(usize, PendingLink)>;

/// Близость чанков после инкрементального расчёта.
pub struct SimilarityDelta {
    /// Строки kNN всех чанков с вектором: идентификатор чанка → (сосед, cosine).
    pub rows: BTreeMap<String, Vec<(String, f32)>>,
    /// Чанки, чей список соседей изменился или появился.
    pub changed: BTreeSet<String>,
    pub state: LayerState,
    /// Покрытие, посчитанное вызывающим (как у полной сборки).
    pub layer: GraphLayerStatus,
    /// Хеш идентификаторов и векторов расчёта: входит в generation и snapshot.
    pub digest: String,
}

pub struct DeltaInput {
    /// Манифест индекса после изменений: путь → хеш.
    pub expected: BTreeMap<String, String>,
    /// `RevisionStamp::input` на момент чтения входов расчёта.
    pub input_stamp: String,
    /// Исчезнувшие документы: (путь индекса, путь от корня документации).
    pub removed: Vec<(String, String)>,
    /// Разбор изменённых и новых документов.
    pub parses: Vec<DocParse>,
    /// Путь от корня документации → id документа, для всех документов графа.
    pub docs: BTreeMap<String, String>,
    /// Путь от корня документации → путь индекса, для всех документов графа.
    pub paths: BTreeMap<String, String>,
    pub similarity: SimilarityDelta,
    pub snapshot: String,
}

/// Манифест опубликованного графа, если на нём допустимо частичное обновление: схема v3,
/// признак `delta_base`, тот же метод, нет файлов с ошибками, есть все три слоя.
pub fn delta_base(db: &Connection) -> Result<Option<BTreeMap<String, String>>, GraphError> {
    if schema::version(db)? != Some(3)
        || meta(db, "delta_base")?.as_deref() != Some("1")
        || meta(db, "method")?.as_deref() != Some(METHOD_VERSION)
        || meta(db, "file_errors_total")?.is_some_and(|v| v != "0")
    {
        return Ok(None);
    }
    let layers: BTreeMap<String, GraphLayerStatus> = match meta(db, "layers")? {
        Some(v) => serde_json::from_str(&v)?,
        None => return Ok(None),
    };
    if layers.len() != 3
        || ["structure", "semantic", "similarity"]
            .iter()
            .any(|n| layers.get(*n).is_none_or(|l| l.generation.is_none()))
    {
        return Ok(None);
    }
    Ok(meta(db, "manifest")?
        .map(|v| serde_json::from_str(&v))
        .transpose()?)
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, GraphError> {
    Ok(serde_json::to_string(value)?)
}

fn node_row(tx: &Connection, id: &str) -> Result<Option<Node>, GraphError> {
    tx.query_row("SELECT data FROM graph_nodes WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()?
    .map(|data| Ok(serde_json::from_str(&data)?))
    .transpose()
}

fn insert_edge(tx: &Connection, edge: &Edge) -> Result<(), GraphError> {
    tx.prepare_cached("INSERT INTO graph_edges VALUES(?1,?2,?3,?4,?5)")?
        .execute(params![
            edge.id,
            edge.from,
            edge.to,
            json(&edge.predicate)?,
            json(edge)?
        ])?;
    let mut insert = tx.prepare_cached("INSERT INTO graph_edge_evidence VALUES(?1,?2)")?;
    for proof in &edge.evidence {
        insert.execute(params![edge.id, proof])?;
    }
    Ok(())
}

fn delete_edge(tx: &Connection, id: &str) -> Result<(), GraphError> {
    tx.prepare_cached("DELETE FROM graph_edge_evidence WHERE edge_id=?1")?
        .execute([id])?;
    tx.prepare_cached("DELETE FROM graph_edges WHERE id=?1")?
        .execute([id])?;
    Ok(())
}

fn docs_path_of<'a>(reverse: &'a BTreeMap<&str, &str>, path: &str) -> Option<&'a str> {
    reverse.get(path).copied()
}

/// Применяет дельту в открытой транзакции `BEGIN IMMEDIATE`; коммит — за владельцем.
/// `Ok(false)` — база не подходит (манифест графа не согласован с дельтой), транзакцию нужно
/// откатить и выполнить полную публикацию. `StaleRef` — входы ушли вперёд за время расчёта.
pub fn apply_delta(tx: &rusqlite::Transaction<'_>, input: &DeltaInput) -> Result<bool, GraphError> {
    let Some(base) = delta_base(tx)? else {
        return Ok(false);
    };
    if document_manifest(tx)? != input.expected {
        return Err(GraphError::StaleRef(
            "исходники изменились во время построения".into(),
        ));
    }
    if revision_stamp(tx)?.map(|s| s.input).as_deref() != Some(input.input_stamp.as_str()) {
        return Err(GraphError::StaleRef("входы kNN изменились".into()));
    }
    let changed: BTreeSet<&str> = input
        .parses
        .iter()
        .map(|p| p.source.meta.path.as_str())
        .collect();
    let removed: BTreeSet<&str> = input.removed.iter().map(|(p, _)| p.as_str()).collect();
    let stored_sources: usize = tx.query_row("SELECT count(*) FROM graph_sources", [], |r| {
        r.get::<_, i64>(0)
    })? as usize;
    if stored_sources != base.len()
        || input.parses.iter().any(|p| {
            input.expected.get(&p.source.meta.path) != Some(&p.source.meta.hash)
                || input.paths.get(&p.source.meta.docs_path) != Some(&p.source.meta.path)
        })
        || input
            .expected
            .iter()
            .any(|(path, hash)| !changed.contains(path.as_str()) && base.get(path) != Some(hash))
        || base
            .keys()
            .any(|path| !input.expected.contains_key(path) && !removed.contains(path.as_str()))
        || removed.iter().any(|p| !base.contains_key(*p))
    {
        return Ok(false);
    }
    let affected: Vec<(&str, &str)> = input
        .removed
        .iter()
        .map(|(p, d)| (p.as_str(), d.as_str()))
        .chain(input.parses.iter().map(|p| {
            (
                p.source.meta.path.as_str(),
                p.source.meta.docs_path.as_str(),
            )
        }))
        .collect();
    let affected_paths: BTreeSet<&str> = affected.iter().map(|(p, _)| *p).collect();
    let reverse: BTreeMap<&str, &str> = input
        .paths
        .iter()
        .map(|(d, p)| (p.as_str(), d.as_str()))
        .collect();

    // Триггеры ревизии молчат до конца транзакции; ключ откатывается вместе с ней.
    tx.execute(
        "INSERT INTO graph_meta VALUES(?1,'1') ON CONFLICT(key) DO UPDATE SET value='1'",
        [schema::PUBLISHING],
    )?;

    // 1. Что принадлежало изменённым документам до правки.
    let mut old_evidence: HashSet<String> = HashSet::new();
    let mut old_nodes: Vec<(String, String)> = Vec::new();
    for path in &affected_paths {
        let mut stmt = tx.prepare_cached("SELECT id FROM graph_evidence WHERE source_path=?1")?;
        for id in stmt.query_map([path], |r| r.get::<_, String>(0))? {
            old_evidence.insert(id?);
        }
        let mut stmt = tx.prepare_cached(
            "SELECT n.id,n.kind FROM graph_node_sources s JOIN graph_nodes n ON n.id=s.node_id WHERE s.source_path=?1",
        )?;
        for row in stmt.query_map([path], |r| Ok((r.get(0)?, r.get(1)?)))? {
            old_nodes.push(row?);
        }
    }
    let entity_kind = json(&Kind::Entity)?;
    let chunk_kind = json(&Kind::Chunk)?;
    let old_chunks: BTreeSet<String> = old_nodes
        .iter()
        .filter(|(_, kind)| *kind == chunk_kind)
        .map(|(id, _)| id.clone())
        .collect();
    let mut old_entities: BTreeSet<String> = old_nodes
        .iter()
        .filter(|(_, kind)| *kind == entity_kind)
        .map(|(id, _)| id.clone())
        .collect();
    let mut touched: BTreeMap<String, Edge> = BTreeMap::new();
    {
        let mut by_evidence =
            tx.prepare_cached("SELECT edge_id FROM graph_edge_evidence WHERE evidence_id=?1")?;
        let mut load = tx.prepare_cached("SELECT data FROM graph_edges WHERE id=?1")?;
        for proof in &old_evidence {
            let ids = by_evidence
                .query_map([proof], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for id in ids {
                if touched.contains_key(&id) {
                    continue;
                }
                let data: String = load.query_row([&id], |r| r.get(0))?;
                touched.insert(id, serde_json::from_str(&data)?);
            }
        }
    }

    // 2. Документы, чьи ссылки нужно разрешить заново: ведущие в изменённые пути и вики-ссылки.
    let mut relink: BTreeSet<String> = BTreeSet::new();
    for (_, docs_path) in &affected {
        let low = format!("{docs_path}{PART_KEY_SEPARATOR}");
        let high = format!("{docs_path}\u{20}");
        let mut stmt = tx.prepare_cached(
            "SELECT DISTINCT source_path FROM graph_parts WHERE kind='link' AND key>=?1 AND key<?2",
        )?;
        for path in stmt.query_map([&low, &high], |r| r.get::<_, String>(0))? {
            relink.insert(path?);
        }
    }
    {
        let mut stmt =
            tx.prepare_cached("SELECT DISTINCT source_path FROM graph_parts WHERE kind='wiki'")?;
        for path in stmt.query_map([], |r| r.get::<_, String>(0))? {
            relink.insert(path?);
        }
    }
    relink.retain(|p| !affected_paths.contains(p.as_str()));
    let mut relinked: Vec<(String, String, NumberedLinks)> = Vec::new();
    let links_to = json(&Predicate::LinksTo)?;
    for path in &relink {
        let Some(docs_path) = docs_path_of(&reverse, path) else {
            return Ok(false);
        };
        let mut links = Vec::new();
        let mut stmt = tx.prepare_cached(
            "SELECT key,data FROM graph_parts WHERE source_path=?1 AND kind IN ('link','wiki')",
        )?;
        for row in stmt.query_map([path], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })? {
            let (key, data) = row?;
            links.push((part_seq(&key), serde_json::from_str::<PendingLink>(&data)?));
        }
        links.sort_by_key(|(seq, _)| *seq);
        // Прежние рёбра `links_to` этого документа: у владельцев ссылок.
        let owners: BTreeSet<&str> = links.iter().map(|(_, l)| l.owner.as_str()).collect();
        for owner in owners {
            let mut stmt =
                tx.prepare_cached("SELECT id FROM graph_edges WHERE from_id=?1 AND predicate=?2")?;
            let ids = stmt
                .query_map(params![owner, links_to], |r| r.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            for id in ids {
                delete_edge(tx, &id)?;
            }
        }
        tx.prepare_cached("DELETE FROM graph_parts WHERE source_path=?1 AND kind='broken'")?
            .execute([path])?;
        relinked.push((path.clone(), docs_path.to_string(), links));
    }

    // 3. Близость: чанки, чьи рёбра пересчитываются.
    let new_chunks: BTreeSet<String> = input
        .parses
        .iter()
        .flat_map(|p| p.data.nodes.iter())
        .filter(|n| n.reference.kind == Kind::Chunk)
        .map(|n| n.reference.id.clone())
        .collect();
    let mut dirty: BTreeSet<String> = old_chunks
        .iter()
        .chain(&new_chunks)
        .chain(&input.similarity.changed)
        .cloned()
        .collect();
    let similar_to = json(&Predicate::SimilarTo)?;
    {
        let mut out =
            tx.prepare_cached("SELECT id FROM graph_edges WHERE from_id=?1 AND predicate=?2")?;
        let mut into =
            tx.prepare_cached("SELECT id FROM graph_edges WHERE to_id=?1 AND predicate=?2")?;
        let mut doomed: BTreeSet<String> = BTreeSet::new();
        for id in &dirty {
            for stmt in [&mut out, &mut into] {
                for edge in stmt.query_map(params![id, similar_to], |r| r.get::<_, String>(0))? {
                    doomed.insert(edge?);
                }
            }
        }
        for id in doomed {
            delete_edge(tx, &id)?;
        }
    }
    // Рёбра, затронутые по доказательствам: правила и ссылки сливаются заново, близость — нет.
    let mut merged: BTreeMap<String, Edge> = BTreeMap::new();
    for (id, mut edge) in touched {
        delete_edge(tx, &id)?;
        if edge.origin == Origin::Similarity {
            continue;
        }
        edge.evidence.retain(|proof| !old_evidence.contains(proof));
        if !edge.evidence.is_empty() {
            merged.insert(id, edge);
        }
    }

    // 4. Удаление строк изменённых документов.
    for (id, kind) in &old_nodes {
        if *kind != entity_kind {
            tx.prepare_cached("DELETE FROM graph_nodes WHERE id=?1")?
                .execute([id])?;
        }
    }
    for path in &affected_paths {
        tx.prepare_cached("DELETE FROM graph_node_sources WHERE source_path=?1")?
            .execute([path])?;
        tx.prepare_cached("DELETE FROM graph_parts WHERE source_path=?1")?
            .execute([path])?;
        tx.prepare_cached(
            "DELETE FROM graph_edge_evidence WHERE evidence_id IN (SELECT id FROM graph_evidence WHERE source_path=?1)",
        )?
        .execute([path])?;
        tx.prepare_cached("DELETE FROM graph_evidence WHERE source_path=?1")?
            .execute([path])?;
        tx.prepare_cached("DELETE FROM graph_sources WHERE path=?1")?
            .execute([path])?;
    }

    // 5. Вставка частей новых и изменённых документов.
    let mut sources: Vec<&DocParse> = input.parses.iter().collect();
    sources.sort_by(|a, b| a.source.meta.path.cmp(&b.source.meta.path));
    let mut new_entities: BTreeSet<String> = BTreeSet::new();
    let mut new_nodes: Vec<&str> = Vec::new();
    for parse in &sources {
        let meta = &parse.source.meta;
        tx.prepare_cached("INSERT INTO graph_sources VALUES(?1,?2,?3)")?
            .execute(params![meta.path, meta.hash, json(&parse.source)?])?;
        let mut parts = source::parts_for(parse);
        parts.sort_by(|a, b| (&a.kind, &a.key).cmp(&(&b.kind, &b.key)));
        for part in &parts {
            tx.prepare_cached("INSERT INTO graph_parts VALUES(?1,?2,?3,?4)")?
                .execute(params![part.source_path, part.kind, part.key, part.data])?;
        }
        let mut evidence: Vec<_> = parse.data.evidence.iter().collect();
        evidence.sort_by(|a, b| a.id.cmp(&b.id));
        for ev in evidence {
            tx.prepare_cached("INSERT INTO graph_evidence VALUES(?1,?2,?3)")?
                .execute(params![ev.id, meta.path, json(ev)?])?;
        }
        for node in &parse.data.nodes {
            if node.reference.kind == Kind::Entity {
                new_entities.insert(node.reference.id.clone());
                continue;
            }
            tx.prepare_cached("INSERT INTO graph_nodes VALUES(?1,?2,?3,?4)")?
                .execute(params![
                    node.reference.id,
                    json(&node.reference.kind)?,
                    node.reference.revision,
                    json(node)?
                ])?;
            tx.prepare_cached("INSERT INTO graph_node_sources VALUES(?1,?2)")?
                .execute(params![node.reference.id, meta.path])?;
            new_nodes.push(node.reference.id.as_str());
        }
    }

    // 6. Общие узлы сущностей собираются из вкладов документов.
    old_entities.extend(new_entities);
    for id in &old_entities {
        let mut contributions: Vec<(String, Node)> = Vec::new();
        {
            let mut stmt = tx.prepare_cached(
                "SELECT source_path,data FROM graph_parts WHERE kind='entity' AND key=?1 ORDER BY source_path",
            )?;
            for row in stmt.query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })? {
                let (path, data) = row?;
                contributions.push((path, serde_json::from_str(&data)?));
            }
        }
        let nodes: Vec<Node> = contributions.iter().map(|(_, n)| n.clone()).collect();
        let exists = tx
            .prepare_cached("SELECT 1 FROM graph_nodes WHERE id=?1")?
            .exists([id])?;
        let Some(node) = source::merge_entity(&nodes) else {
            if exists {
                tx.prepare_cached("DELETE FROM graph_nodes WHERE id=?1")?
                    .execute([id])?;
            }
            continue;
        };
        if exists {
            tx.prepare_cached("UPDATE graph_nodes SET revision=?2,data=?3 WHERE id=?1")?
                .execute(params![id, node.reference.revision, json(&node)?])?;
            tx.prepare_cached("DELETE FROM graph_node_sources WHERE node_id=?1")?
                .execute([id])?;
        } else {
            tx.prepare_cached("INSERT INTO graph_nodes VALUES(?1,?2,?3,?4)")?
                .execute(params![
                    id,
                    json(&node.reference.kind)?,
                    node.reference.revision,
                    json(&node)?
                ])?;
        }
        for (path, _) in &contributions {
            tx.prepare_cached("INSERT INTO graph_node_sources VALUES(?1,?2)")?
                .execute(params![id, path])?;
        }
    }

    // 7. Ссылки: новые документы и документы, чьи цели изменились.
    let failure: RefCell<Option<GraphError>> = RefCell::new(None);
    let anchor_of = |docs_path: &str, fragment: &str| -> Option<String> {
        let path = input.paths.get(docs_path)?;
        let found = tx
            .prepare_cached(
                "SELECT data FROM graph_parts WHERE source_path=?1 AND kind='anchor' AND key=?2",
            )
            .and_then(|mut stmt| {
                stmt.query_row(params![path, fragment], |r| r.get::<_, String>(0))
                    .optional()
            });
        match found {
            Ok(value) => value,
            Err(error) => {
                *failure.borrow_mut() = Some(error.into());
                None
            }
        }
    };
    let context = LinkContext {
        docs: &input.docs,
        anchor: &anchor_of,
    };
    let mut documents: Vec<(&str, &str, NumberedLinks)> = Vec::new();
    for parse in &sources {
        documents.push((
            parse.source.meta.path.as_str(),
            parse.source.meta.docs_path.as_str(),
            parse.links.iter().cloned().enumerate().collect(),
        ));
    }
    for (path, docs_path, links) in &relinked {
        documents.push((path.as_str(), docs_path.as_str(), links.clone()));
    }
    for (path, docs_path, links) in &documents {
        for (seq, link) in links {
            match source::resolve_link(&context, docs_path, link) {
                LinkOutcome::Nothing => {}
                LinkOutcome::Edge(to) => {
                    let edge = source::links_to_edge(&link.owner, &to, &link.proof);
                    match merged.get_mut(&edge.id) {
                        Some(existing) => {
                            if !existing.evidence.contains(&link.proof) {
                                existing.evidence.push(link.proof.clone());
                            }
                        }
                        None => {
                            merged.insert(edge.id.clone(), edge);
                        }
                    }
                }
                LinkOutcome::Broken(broken) => {
                    let part = source::broken_part(path, *seq, &broken);
                    tx.prepare_cached("INSERT INTO graph_parts VALUES(?1,?2,?3,?4)")?
                        .execute(params![part.source_path, part.kind, part.key, part.data])?;
                }
            }
        }
    }
    if let Some(error) = failure.into_inner() {
        return Err(error);
    }

    // 8. Рёбра правил и структуры: прежние доказательства других документов плюс новые.
    for parse in &sources {
        for edge in &parse.data.edges {
            match merged.get_mut(&edge.id) {
                Some(existing) => {
                    for proof in &edge.evidence {
                        if !existing.evidence.contains(proof) {
                            existing.evidence.push(proof.clone());
                        }
                    }
                }
                None => {
                    merged.insert(edge.id.clone(), edge.clone());
                }
            }
        }
    }
    for edge in merged.values_mut() {
        if edge.origin == Origin::ExplicitRule {
            edge.evidence.sort();
            edge.evidence.dedup();
        }
    }
    for edge in merged.values() {
        insert_edge(tx, edge)?;
    }

    // 9. Рёбра близости: пары, где хотя бы один чанк пересчитан.
    dirty.extend(input.similarity.changed.iter().cloned());
    let mut cache: HashMap<String, Vec<String>> = HashMap::new();
    let mut evidence_of = |id: &str| -> Result<Vec<String>, GraphError> {
        if let Some(found) = cache.get(id) {
            return Ok(found.clone());
        }
        let node = node_row(tx, id)?
            .ok_or_else(|| GraphError::InvalidData("нет узла чанка для kNN".into()))?;
        cache.insert(id.to_string(), node.evidence.clone());
        Ok(node.evidence)
    };
    for (from, neighbors) in &input.similarity.rows {
        for (to, cosine) in neighbors {
            if !dirty.contains(from) && !dirty.contains(to) {
                continue;
            }
            let mut evidence = evidence_of(from)?;
            evidence.extend(evidence_of(to)?);
            evidence.sort();
            evidence.dedup();
            if evidence.is_empty() {
                continue;
            }
            insert_edge(
                tx,
                &Edge {
                    id: blake3::hash(format!("similar:{from}:{to}").as_bytes())
                        .to_hex()
                        .to_string(),
                    from: from.clone(),
                    to: to.clone(),
                    predicate: Predicate::SimilarTo,
                    polarity: Polarity::Positive,
                    qualifiers: Default::default(),
                    origin: Origin::Similarity,
                    evidence,
                    score: Some(*cosine),
                },
            )?;
        }
    }

    // 10. Проверка дельты: у новых узлов есть источник, внешние ключи не нарушены на затронутом.
    validate_delta(tx, &new_nodes, &old_entities)?;

    // 11. Метаданные и ревизия.
    let broken: Vec<BrokenLink> = {
        let mut stmt = tx
            .prepare("SELECT data FROM graph_parts WHERE kind='broken' ORDER BY source_path,key")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?
            .map(|row| Ok(serde_json::from_str(&row?)?))
            .collect::<Result<_, GraphError>>()?
    };
    tx.execute(
        "UPDATE graph_aux SET data=?1 WHERE key='broken_links'",
        [json(&broken)?],
    )?;
    tx.execute(
        "UPDATE graph_aux SET data=?1 WHERE key='similarity_state'",
        [json(&input.similarity.state)?],
    )?;
    set_meta(tx, "snapshot", &input.snapshot)?;
    set_meta(tx, "manifest", &json(&input.expected)?)?;
    set_meta(tx, "similarity_inputs", &similarity_inputs(tx)?)?;
    let semantic_state: LayerState = tx
        .query_row(
            "SELECT data FROM graph_aux WHERE key='semantic_state'",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(|s| serde_json::from_str(&s))
        .transpose()?
        .unwrap_or(LayerState::Partial);
    let mut layers: BTreeMap<String, GraphLayerStatus> = serde_json::from_str(
        &meta(tx, "layers")?.ok_or_else(|| GraphError::InvalidData("нет слоёв графа".into()))?,
    )?;
    let digest = |name: &str, fingerprint: &str, extra: &str| -> Result<String, GraphError> {
        Ok(blake3::hash(&serde_json::to_vec(&(
            "delta-v1",
            name,
            fingerprint,
            &input.expected,
            extra,
        ))?)
        .to_hex()
        .to_string())
    };
    for (name, layer) in &mut layers {
        let extra = if name == "similarity" {
            input.similarity.digest.as_str()
        } else {
            ""
        };
        if name == "similarity" {
            let mut fresh = input.similarity.layer.clone();
            fresh.generation = Some(digest(name, &fresh.fingerprint, extra)?);
            fresh.source_revisions = input.expected.clone();
            *layer = fresh;
            continue;
        }
        layer.source_revisions = input.expected.clone();
        layer.generation = Some(digest(name, &layer.fingerprint, extra)?);
        layer.processed = input.expected.len();
        layer.total = input.expected.len();
        layer.coverage.processed = input.expected.len();
        layer.coverage.total = input.expected.len();
        layer.freshness = Freshness::Current;
        layer.stale_reason = None;
    }
    set_meta(tx, "layers", &json(&layers)?)?;
    set_meta(
        tx,
        "state",
        if semantic_state == LayerState::Ready {
            "ready"
        } else {
            "partial"
        },
    )?;
    set_meta(tx, "active_job", "")?;
    set_meta(tx, "job_state", "\"idle\"")?;
    set_meta(tx, "error", "")?;
    if let Some(stamp) = revision_stamp(tx)? {
        set_meta(tx, "published_input_stamp", &stamp.input)?;
    }
    tx.execute("DELETE FROM graph_meta WHERE key=?1", [schema::PUBLISHING])?;
    let bumped = tx.execute(
        "UPDATE graph_revisions SET read_revision=read_revision+1 WHERE id=1",
        [],
    )?;
    if bumped != 1 {
        return Err(GraphError::InvalidData("отсутствуют метки графа".into()));
    }
    Ok(true)
}

/// Проверка дельты без загрузки набора: каждый новый узел и сущность имеют источник.
fn validate_delta(
    tx: &Connection,
    new_nodes: &[&str],
    entities: &BTreeSet<String>,
) -> Result<(), GraphError> {
    let mut sourced = tx.prepare_cached("SELECT 1 FROM graph_node_sources WHERE node_id=?1")?;
    for id in new_nodes {
        if !sourced.exists([id])? {
            return Err(GraphError::InvalidData("у узла нет источника".into()));
        }
    }
    let mut exists = tx.prepare_cached("SELECT 1 FROM graph_nodes WHERE id=?1")?;
    for id in entities {
        if exists.exists([id])? && !sourced.exists([id])? {
            return Err(GraphError::InvalidData("у сущности нет источника".into()));
        }
    }
    Ok(())
}
