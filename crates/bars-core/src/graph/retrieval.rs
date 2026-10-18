//! Кандидаты и целые пути из готового графа; новые модели не вызываются.
use super::*;
use crate::{
    search::{SearchError, SearchRequest},
    storage::Index,
};
use std::collections::{BTreeMap, BTreeSet};

pub fn expand(
    index: &Index,
    data: &Dataset,
    request: &SearchRequest,
    seeds: &[i64],
    limit: usize,
    mut budgets: Budgets,
) -> Result<(Vec<i64>, GraphReply), SearchError> {
    let sources: BTreeMap<_, _> = data
        .sources
        .iter()
        .map(|s| (s.meta.path.as_str(), &s.meta))
        .collect();
    let mut statement=index.conn().prepare("SELECT c.id,d.path,c.ordinal,c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id ORDER BY c.id")?;
    let rows = statement.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u32>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut ids = BTreeMap::new();
    let mut refs = BTreeMap::new();
    for row in rows {
        let (id, path, ordinal, hash) = row?;
        let Some(source) = sources.get(path.as_str()) else {
            continue;
        };
        let key = format!("{}:{ordinal}:{hash}", source.docs_path);
        let reference = object_ref(&data.namespace, Kind::Chunk, &key, &hash);
        if let Some(node) = data.nodes.iter().find(|n| n.reference.id == reference.id) {
            ids.insert(node.reference.id.clone(), id);
            refs.insert(id, node.reference.clone());
        }
    }
    let roots = seeds
        .iter()
        .take(3)
        .filter_map(|id| refs.get(id).cloned())
        .collect::<Vec<_>>();
    if budgets.max_depth.is_none() {
        budgets.max_depth = Some(4);
    }
    if budgets.max_paths.is_none() {
        budgets.max_paths = Some(20);
    }
    let graph_request = GraphRequest {
        operation: Operation::Graph,
        scope: Scope {
            modules: request.modules.clone(),
            files: request.files.clone(),
        },
        roots,
        local: true,
        include_paths: true,
        view: View::Mixed,
        path_kind: PathKind::Mixed,
        budgets,
        limit: Some(limit.min(20) as u32),
        ..Default::default()
    };
    let reply = query::execute(data, &graph_request, || false)
        .map_err(|e| SearchError::InvalidParams(e.to_string()))?;
    let seeds: BTreeSet<_> = seeds.iter().copied().collect();
    let mut seen = BTreeSet::new();
    let candidates = reply
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Path { path } => path.nodes.last(),
            _ => None,
        })
        .filter_map(|r| ids.get(&r.id).copied())
        .filter(|id| !seeds.contains(id) && seen.insert(*id))
        .take(limit)
        .collect();
    Ok((candidates, reply))
}
