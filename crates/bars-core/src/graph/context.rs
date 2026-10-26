use super::*;
use crate::{
    models::{EmbedError, Embedder, ModelFingerprint},
    search::{SearchError, SearchRequest, SearchSettings, search},
    storage::Index,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};
struct CachedQuery {
    fingerprint: ModelFingerprint,
    vector: Vec<f32>,
}
impl Embedder for CachedQuery {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
    fn embed(&mut self, _: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Err(EmbedError::Load("Контекст не эмбеддит документы".into()))
    }
    fn embed_query(&mut self, _: &str) -> Result<Vec<f32>, EmbedError> {
        Ok(self.vector.clone())
    }
}
/// Поиск отдельно по ролям; один вектор запроса существующей модели, прежние пороги cosine.
pub fn evidence_scores(
    index: &Index,
    data: &Dataset,
    embedder: &mut dyn Embedder,
    settings: &SearchSettings,
    request: &GraphRequest,
) -> Result<(BTreeMap<String, f32>, bool), SearchError> {
    evidence_scores_current(index, data, embedder, settings, request, None)
}

pub fn evidence_scores_current(
    index: &Index,
    data: &Dataset,
    embedder: &mut dyn Embedder,
    settings: &SearchSettings,
    request: &GraphRequest,
    current: Option<&BTreeMap<String, String>>,
) -> Result<(BTreeMap<String, f32>, bool), SearchError> {
    bars_graph::query::validate_request(request)
        .and_then(|_| bars_graph::query::validate_scope(data, &request.scope))
        .map_err(|error| SearchError::InvalidParams(error.to_string()))?;
    let start = Instant::now();
    let budget = request
        .budgets
        .resolve()
        .map_err(|e| SearchError::InvalidParams(e.to_string()))?
        .0;
    let query = request.query.as_deref().unwrap_or_default();
    let vector = embedder.embed_query(query)?;
    let mut cached = CachedQuery {
        fingerprint: embedder.fingerprint().clone(),
        vector,
    };
    let roles = request.context_roles();
    let mut scores = BTreeMap::<String, f32>::new();
    for role in roles {
        if start.elapsed().as_millis() >= u128::from(budget.timeout_ms) {
            return Ok((scores, true));
        }
        let ids: BTreeSet<_> = data
            .nodes
            .iter()
            .filter(|n| n.roles.contains(&role))
            .flat_map(|n| n.source_ids.iter())
            .collect();
        let files = data
            .sources
            .iter()
            .filter(|s| request.scope.allows(&s.meta) && ids.contains(&s.meta.reference.id))
            .filter(|s| current.is_none_or(|m| m.get(&s.meta.path) == Some(&s.meta.hash)))
            .map(|s| s.meta.docs_path.clone())
            .collect::<Vec<_>>();
        if files.is_empty() {
            continue;
        }
        let r = SearchRequest {
            query: query.into(),
            files,
            results: Some(3),
            candidates: Some(30),
            ..Default::default()
        };
        let result = search(index, &mut cached, None, settings, &r)?;
        for hit in result.hits {
            for ev in &data.evidence {
                if ev.span.last_line < hit.first_line || ev.span.first_line > hit.last_line {
                    continue;
                }
                if data
                    .sources
                    .iter()
                    .any(|s| s.meta.reference == ev.document && s.meta.path == hit.path)
                {
                    scores
                        .entry(ev.id.clone())
                        .and_modify(|s| *s = s.max(hit.score))
                        .or_insert(hit.score);
                }
            }
        }
    }
    Ok((scores, false))
}
