//! Гибридный поиск: полнотекстовый и векторный кандидаты, слияние RRF, реранк, порог
//! (`docs/features/search/hybrid-search.md`, Т-12…Т-14, Т-18, Т-24, Т-25).

use std::collections::{HashMap, HashSet};

use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::params::{AppliedParams, ScoreKind, SearchRequest, SearchSettings};
use super::vector_top::VectorTop;
use crate::models::{EmbedError, Embedder, RerankError, Reranker};
use crate::storage::stem::TextStemmer;
use crate::storage::{Compatibility, Index, StorageError};

/// Константа Reciprocal Rank Fusion: оценка кандидата — сумма `1 / (RRF_K + место)` по спискам.
const RRF_K: f32 = 60.0;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Запрос или отбор не годятся (`invalid_params` в протоколе).
    #[error("{0}")]
    InvalidParams(String),
    /// Индекс построен другой моделью или старой схемой (Т-25): искать в нём нельзя.
    #[error("нужна переиндексация: индекс построен другой моделью или старой схемой")]
    ReindexRequired,
    #[error(transparent)]
    Embed(#[from] EmbedError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error("ошибка SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// ID чанка неизвестен или устарел после обновления документа (`chunk_not_found`).
    #[error(
        "ID чанка `{0}` неизвестен или устарел после обновления индекса; найди чанк поиском заново"
    )]
    ChunkNotFound(String),
    #[error("у чанка `{0}` нет вектора в индексе: индекс повреждён, нужна переиндексация")]
    MissingVector(String),
    #[error("вектор в индексе длиной {got}, а у запроса {expected}")]
    VectorSize { expected: usize, got: usize },
}

/// Один результат (Т-13).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub source_hash: String,
    pub chunk_id: i64,
    /// Непрозрачный ID чанка для `get_similar`; действует, пока не изменился документ чанка.
    pub public_id: String,
    /// Путь документа от корня проекта.
    pub path: String,
    pub module: String,
    pub title: String,
    pub heading_chain: String,
    pub first_line: usize,
    pub last_line: usize,
    pub text: String,
    pub score: f32,
    pub score_kind: ScoreKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    pub applied: AppliedParams,
    /// Причина, по которой rerank включён, но выдача вернулась без него.
    pub rerank_error: Option<String>,
    /// Нормализованный вектор запроса, если он вычислялся (при пустом корпусе — `None`). Внутренний
    /// сигнал для воркера («Новые темы»), в публичную выдачу не попадает.
    pub query_vector: Option<Vec<f32>>,
}

/// Ищет в индексе одного проекта. Реранк идёт, если передан `reranker`; если он не отработал,
/// выдача возвращается без него, а причина — в `rerank_error`.
pub fn search(
    index: &Index,
    embedder: &mut dyn Embedder,
    reranker: Option<&mut dyn Reranker>,
    settings: &SearchSettings,
    request: &SearchRequest,
) -> Result<SearchResponse, SearchError> {
    search_expanded(index, embedder, reranker, settings, request, None)
}

/// Внутреннее расширение до rerank/top-K; оценки и область проверяет тот же поисковый путь.
pub type CandidateExpansion<'a> =
    dyn FnMut(&Index, &[i64], usize) -> Result<Vec<i64>, SearchError> + 'a;
pub fn search_expanded(
    index: &Index,
    embedder: &mut dyn Embedder,
    reranker: Option<&mut dyn Reranker>,
    settings: &SearchSettings,
    request: &SearchRequest,
    mut expansion: Option<&mut CandidateExpansion<'_>>,
) -> Result<SearchResponse, SearchError> {
    // FTS открывает служебные таблицы лениво: смена схемы между чтениями ломает xConnect.
    // Сохраняем транзакцию вызывающего кода, если поиск уже работает в его снимке.
    let _snapshot = if index.conn().is_autocommit() {
        Some(index.conn().unchecked_transaction()?)
    } else {
        None
    };
    let match_query = fts_query(&request.query)
        .ok_or_else(|| SearchError::InvalidParams("в запросе нет ни одного слова".to_string()))?;
    if matches!(
        index.compatibility(embedder.fingerprint())?,
        Compatibility::ModelChanged | Compatibility::SchemaOutdated
    ) {
        return Err(SearchError::ReindexRequired);
    }
    let scope = Scope::build(index, request)?;

    let planned = settings.resolve(request, reranker.is_some());
    if scope.chunk_count(index)? == 0 {
        return Ok(SearchResponse {
            hits: Vec::new(),
            applied: planned,
            rerank_error: None,
            query_vector: None,
        });
    }

    let mut query_vector = embedder.embed_query(&request.query)?;
    let expected = embedder.fingerprint().dim;
    if query_vector.len() != expected {
        return Err(EmbedError::Dimension {
            expected,
            got: query_vector.len(),
        }
        .into());
    }
    normalize(&mut query_vector);

    let candidates = planned.candidates;
    let text_ids = scope.text_candidates(index, &match_query.all, candidates)?;
    let exact_ids = match &match_query.identifiers {
        Some(identifiers) => scope.text_candidates(index, identifiers, candidates)?,
        None => Vec::new(),
    };
    let mut cosines = scope.cosines(index, &query_vector, candidates, &text_ids, &exact_ids)?;
    let vector_ids = top_by_score(&cosines, candidates);
    let mut fused = exact_first(&exact_ids, fuse(&text_ids, &vector_ids));
    if let Some(expansion) = expansion.as_mut().filter(|_| !fused.is_empty()) {
        let additional = expansion(index, &fused, candidates)?
            .into_iter()
            .take(candidates)
            .collect::<Vec<_>>();
        // Принудительно посчитать cosine добавленных кандидатов, сохранив исходный scope.
        let scores = scope.cosines(index, &query_vector, 0, &additional, &[])?;
        let additional = additional
            .into_iter()
            .filter(|id| scores.contains_key(id))
            .collect::<Vec<_>>();
        cosines.extend(scores);
        let original = fused.clone();
        let mut seen = HashSet::new();
        fused.clear();
        // Чередование до окончательного rerank; исходные точные identifiers остаются первыми.
        for i in 0..original.len().max(additional.len()) {
            for id in [original.get(i), additional.get(i)].into_iter().flatten() {
                if seen.insert(*id) {
                    fused.push(*id);
                }
            }
        }
        fused = exact_first(&exact_ids, fused);
    }

    let mut rerank_error = None;
    if let Some(reranker) = reranker {
        match rerank(index, reranker, request, &planned, &fused) {
            Ok(hits) => {
                return Ok(SearchResponse {
                    hits: threshold_and_cut(hits, &planned),
                    applied: planned,
                    rerank_error: None,
                    query_vector: Some(query_vector),
                });
            }
            Err(error) => rerank_error = Some(error.to_string()),
        }
    }

    let applied = if rerank_error.is_some() {
        settings.resolve(request, false)
    } else {
        planned
    };
    let ids: Vec<i64> = fused
        .iter()
        .copied()
        .filter(|id| cosine_of(&cosines, *id) >= applied.min_score)
        .take(applied.results)
        .collect();
    let mut hits = load_hits(index, &ids)?;
    for hit in &mut hits {
        hit.score = cosine_of(&cosines, hit.chunk_id);
        hit.score_kind = ScoreKind::Cosine;
    }
    Ok(SearchResponse {
        hits,
        applied,
        rerank_error,
        query_vector: Some(query_vector),
    })
}

/// Диагностика первого этапа для бенча IDEA-06: FTS, векторы, слияние.
/// Использует те же операции, что `search`, до порогов и отсечения результатов.
#[doc(hidden)]
pub fn candidate_trace(
    index: &Index,
    query_vector: &[f32],
    request: &SearchRequest,
    candidates: usize,
) -> Result<[Vec<SearchHit>; 3], SearchError> {
    let query = fts_query(&request.query)
        .ok_or_else(|| SearchError::InvalidParams("в запросе нет ни одного слова".into()))?;
    let scope = Scope::build(index, request)?;
    let mut vector = query_vector.to_vec();
    normalize(&mut vector);
    let text = scope.text_candidates(index, &query.all, candidates)?;
    let exact = match query.identifiers {
        Some(ref identifiers) => scope.text_candidates(index, identifiers, candidates)?,
        None => Vec::new(),
    };
    let cosines = scope.cosines(index, &vector, candidates, &text, &exact)?;
    let vector = top_by_score(&cosines, candidates);
    let fused = exact_first(&exact, fuse(&text, &vector));
    let mut lists = [
        load_hits(index, &text)?,
        load_hits(index, &vector)?,
        load_hits(index, &fused)?,
    ];
    for hits in &mut lists {
        for hit in hits {
            hit.score = cosine_of(&cosines, hit.chunk_id);
        }
    }
    Ok(lists)
}

/// Запросы к FTS5 по одному запросу пользователя.
#[derive(Debug, PartialEq, Eq)]
struct FtsQuery {
    /// Слова запроса после стемминга, каждое в кавычках, через `OR` (`bm25` сам поднимает чанки
    /// с большим числом совпавших слов). Слово запроса, которое стемминг разрезал на несколько
    /// (`agent_min_score`, `по-русски`), идёт ещё и фразой.
    all: String,
    /// Только фразы-идентификаторы: слова запроса с `_`, `.` или `/` внутри. `None`, если таких нет.
    identifiers: Option<String>,
}

/// `None`, если в запросе нет ни одного слова.
fn fts_query(query: &str) -> Option<FtsQuery> {
    let stemmer = TextStemmer::new();
    let mut phrases: Vec<String> = Vec::new();
    let mut identifiers: Vec<String> = Vec::new();
    for raw in query.split_whitespace() {
        let stemmed = stemmer.stem_text(raw);
        if stemmed.split_whitespace().count() < 2 {
            continue;
        }
        let phrase = format!("\"{stemmed}\"");
        if is_identifier(raw) && !identifiers.contains(&phrase) {
            identifiers.push(phrase.clone());
        }
        if !phrases.contains(&phrase) {
            phrases.push(phrase);
        }
    }
    let mut parts = phrases;
    for term in stemmer.stem_text(query).split_whitespace() {
        let term = format!("\"{term}\"");
        if !parts.contains(&term) {
            parts.push(term);
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(FtsQuery {
        all: parts.join(" OR "),
        identifiers: (!identifiers.is_empty()).then(|| identifiers.join(" OR ")),
    })
}

/// Слово запроса похоже на идентификатор или имя файла: `agent_min_score`, `settings.toml`,
/// `a/x.md`. Знаки по краям (точка в конце предложения, скобки) не в счёт.
fn is_identifier(raw: &str) -> bool {
    raw.trim_matches(|c: char| !c.is_alphanumeric())
        .contains(['_', '.', '/'])
}

/// Чанки, в которых есть идентификатор из запроса, идут первыми: точное совпадение идентификатора
/// должно находиться первым без реранка (Т-12), а в слиянии RRF его легко обгоняет чанк, который
/// просто второй в обоих списках. Внутри групп порядок слияния сохраняется.
fn exact_first(exact: &[i64], fused: Vec<i64>) -> Vec<i64> {
    let position: HashMap<i64, usize> = fused.iter().enumerate().map(|(p, id)| (*id, p)).collect();
    let mut head = exact.to_vec();
    head.sort_by_key(|id| position.get(id).copied().unwrap_or(usize::MAX));
    let exact_set: HashSet<i64> = head.iter().copied().collect();
    head.extend(fused.into_iter().filter(|id| !exact_set.contains(id)));
    head
}

fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in vector {
            *x /= norm;
        }
    }
}

/// Косинус для чанка без вектора считается нулевым: такого чанка быть не должно.
fn cosine_of(cosines: &HashMap<i64, f32>, id: i64) -> f32 {
    cosines.get(&id).copied().unwrap_or(0.0).clamp(0.0, 1.0)
}

/// Первые `limit` чанков по убыванию оценки; при равенстве — по номеру, чтобы порядок был устойчив.
pub(super) fn top_by_score(scores: &HashMap<i64, f32>, limit: usize) -> Vec<i64> {
    let mut all: Vec<(i64, f32)> = scores.iter().map(|(id, score)| (*id, *score)).collect();
    all.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    all.into_iter().take(limit).map(|(id, _)| id).collect()
}

/// Reciprocal Rank Fusion двух списков; лучшие — первыми. При равной оценке выше тот, кто стоит
/// выше в полнотекстовом списке: точное совпадение идентификатора поднимает именно он (Т-12),
/// а симметричные места «первый и второй» в двух списках дают ровно равные суммы.
fn fuse(text_ids: &[i64], vector_ids: &[i64]) -> Vec<i64> {
    let mut scores: HashMap<i64, f32> = HashMap::new();
    for list in [text_ids, vector_ids] {
        for (place, id) in list.iter().enumerate() {
            *scores.entry(*id).or_insert(0.0) += 1.0 / (RRF_K + place as f32 + 1.0);
        }
    }
    let text_place: HashMap<i64, usize> = text_ids
        .iter()
        .enumerate()
        .map(|(place, id)| (*id, place))
        .collect();
    let place_of = |id: &i64| text_place.get(id).copied().unwrap_or(usize::MAX);
    let mut fused: Vec<i64> = scores.keys().copied().collect();
    fused.sort_by(|a, b| {
        scores[b]
            .total_cmp(&scores[a])
            .then_with(|| place_of(a).cmp(&place_of(b)))
            .then(a.cmp(b))
    });
    fused
}

/// Переоценивает лучших слитых кандидатов; результат упорядочен по оценке реранка.
fn rerank(
    index: &Index,
    reranker: &mut dyn Reranker,
    request: &SearchRequest,
    applied: &AppliedParams,
    fused: &[i64],
) -> Result<Vec<SearchHit>, RerankError> {
    let pool: Vec<i64> = fused
        .iter()
        .copied()
        .take(applied.rerank_candidates)
        .collect();
    let mut hits =
        load_hits(index, &pool).map_err(|error| RerankError::Rerank(error.to_string()))?;
    if hits.is_empty() {
        return Ok(hits);
    }
    // Rerank-модель видит путь, цепочку заголовков и текст: они входят в начало документа.
    let documents: Vec<String> = hits
        .iter()
        .map(|hit| format!("{}\n{}\n{}", hit.path, hit.heading_chain, hit.text))
        .collect();
    let refs: Vec<&str> = documents.iter().map(String::as_str).collect();
    let scores = reranker.rerank(&request.query, &refs)?;
    if scores.len() != hits.len() {
        return Err(RerankError::Count {
            expected: hits.len(),
            got: scores.len(),
        });
    }
    for (hit, score) in hits.iter_mut().zip(scores) {
        hit.score = score.clamp(0.0, 1.0);
        hit.score_kind = ScoreKind::Rerank;
    }
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    Ok(hits)
}

fn threshold_and_cut(hits: Vec<SearchHit>, applied: &AppliedParams) -> Vec<SearchHit> {
    hits.into_iter()
        .filter(|hit| hit.score >= applied.min_score)
        .take(applied.results)
        .collect()
}

/// Чанки с документами; порядок — как в `ids`. Оценку проставляет вызывающий.
pub(super) fn load_hits(index: &Index, ids: &[i64]) -> Result<Vec<SearchHit>, SearchError> {
    let mut statement = index.conn().prepare_cached(
        "SELECT d.path, d.module, d.title, c.heading_chain, c.first_line, c.last_line, c.text,
                d.hash, c.ordinal, c.embed_hash
         FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.id = ?1",
    )?;
    let mut hits = Vec::with_capacity(ids.len());
    for &id in ids {
        let hit = statement.query_row([id], |row| {
            let path: String = row.get(0)?;
            let document_hash: String = row.get(7)?;
            let ordinal: i64 = row.get(8)?;
            let embed_hash: String = row.get(9)?;
            Ok(SearchHit {
                chunk_id: id,
                public_id: super::similar::public_id(
                    id,
                    &document_hash,
                    &path,
                    ordinal,
                    &embed_hash,
                ),
                source_hash: document_hash,
                path,
                module: row.get(1)?,
                title: row.get(2)?,
                heading_chain: row.get(3)?,
                first_line: row.get::<_, i64>(4)? as usize,
                last_line: row.get::<_, i64>(5)? as usize,
                text: row.get(6)?,
                score: 0.0,
                score_kind: ScoreKind::Cosine,
            })
        })?;
        hits.push(hit);
    }
    Ok(hits)
}

/// Текст ошибки неизвестного модуля со списком существующих (OQ-169).
fn unknown_module(index: &Index, module: &str) -> Result<String, SearchError> {
    let docs_root = index
        .stored_layout()?
        .map(|layout| layout.docs_root)
        .unwrap_or_default();
    let mut statement = index.conn().prepare("SELECT module, path FROM documents")?;
    let docs = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(crate::doc_path::unknown_module_message(
        &docs_root, module, &docs,
    ))
}

/// Отбор по модулям и файлам (Т-14): условие для SQL и его параметры.
pub(super) struct Scope {
    clause: String,
    params: Vec<Value>,
}

impl Scope {
    /// Строит отбор и проверяет, что каждый модуль и файл есть в индексе. Модули и файлы
    /// объединяются; пустой отбор — весь проект.
    pub(super) fn build(index: &Index, request: &SearchRequest) -> Result<Self, SearchError> {
        let mut parts = Vec::new();
        let mut params = Vec::new();

        for module in &request.modules {
            let known = index
                .conn()
                .prepare_cached("SELECT EXISTS(SELECT 1 FROM documents WHERE module = ?1)")?
                .query_row([module], |row| row.get::<_, bool>(0))?;
            if !known {
                return Err(SearchError::InvalidParams(unknown_module(index, module)?));
            }
            params.push(Value::from(module.clone()));
        }
        if !request.modules.is_empty() {
            parts.push(format!(
                "d.module IN ({})",
                placeholders(request.modules.len())
            ));
        }

        if !request.files.is_empty() {
            // Файл задаётся от корня документации или от корня проекта (OQ-169); в индексе путь —
            // от корня проекта.
            let docs_root = index
                .stored_layout()?
                .map(|layout| layout.docs_root)
                .unwrap_or_default();
            let mut stored_paths = Vec::new();
            for file in &request.files {
                let mut found = None;
                for candidate in crate::doc_path::stored_candidates(&docs_root, file) {
                    let known = index
                        .conn()
                        .prepare_cached("SELECT EXISTS(SELECT 1 FROM documents WHERE path = ?1)")?
                        .query_row([&candidate], |row| row.get::<_, bool>(0))?;
                    if known {
                        found = Some(candidate);
                        break;
                    }
                }
                let Some(stored) = found else {
                    let mut statement = index.conn().prepare("SELECT path FROM documents")?;
                    let known = statement
                        .query_map([], |row| row.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    return Err(SearchError::InvalidParams(
                        crate::doc_path::unknown_file_message(
                            &docs_root,
                            known.iter().map(String::as_str),
                            file,
                        ),
                    ));
                };
                stored_paths.push(stored);
            }
            parts.push(format!("d.path IN ({})", placeholders(stored_paths.len())));
            params.extend(stored_paths.into_iter().map(Value::from));
        }

        let clause = if parts.is_empty() {
            "1".to_string()
        } else {
            format!("({})", parts.join(" OR "))
        };
        Ok(Self { clause, params })
    }

    fn chunk_count(&self, index: &Index) -> Result<i64, SearchError> {
        let sql = format!(
            "SELECT count(*) FROM chunks c JOIN documents d ON d.id = c.document_id WHERE {}",
            self.clause
        );
        Ok(index
            .conn()
            .query_row(&sql, params_from_iter(&self.params), |row| row.get(0))?)
    }

    /// Первые `limit` чанков по `bm25`. Слова запроса и текст чанков прошли один стемминг.
    fn text_candidates(
        &self,
        index: &Index,
        match_query: &str,
        limit: usize,
    ) -> Result<Vec<i64>, SearchError> {
        let sql = format!(
            "SELECT chunks_fts.rowid
             FROM chunks_fts
             JOIN chunks c ON c.id = chunks_fts.rowid
             JOIN documents d ON d.id = c.document_id
             WHERE chunks_fts MATCH ? AND {}
             ORDER BY bm25(chunks_fts)
             LIMIT ?",
            self.clause
        );
        let mut params = vec![Value::from(match_query.to_string())];
        params.extend(self.params.iter().cloned());
        params.push(Value::Integer(limit as i64));
        let mut statement = index.conn().prepare(&sql)?;
        let ids = statement
            .query_map(params_from_iter(params), |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }

    /// Косинусная близость запроса ко всем чанкам области: точный перебор. Векторы в индексе и
    /// вектор запроса нормализованы, поэтому близость — скалярное произведение.
    pub(super) fn cosines(
        &self,
        index: &Index,
        query_vector: &[f32],
        candidates: usize,
        text: &[i64],
        exact: &[i64],
    ) -> Result<HashMap<i64, f32>, SearchError> {
        // Random BLOB lookups reread overflow pages; a full search streams vectors once.
        // Scoped requests keep their selective lookup instead of scanning unrelated vectors.
        let from = if self.params.is_empty() {
            "vectors v CROSS JOIN chunks c ON c.embed_hash = v.embed_hash
             CROSS JOIN documents d ON d.id = c.document_id"
        } else {
            "chunks c JOIN documents d ON d.id = c.document_id
             JOIN vectors v ON v.embed_hash = c.embed_hash"
        };
        let sql = format!(
            "SELECT c.id, v.vector
             FROM {from}
             WHERE {}",
            self.clause
        );
        let mut statement = index.conn().prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(&self.params))?;
        let mut scores = VectorTop::new(candidates, text.iter().chain(exact).copied());
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let value = row.get_ref(1)?;
            let blob = match value {
                rusqlite::types::ValueRef::Blob(blob) => blob,
                other => {
                    return Err(rusqlite::Error::InvalidColumnType(
                        1,
                        "vector".into(),
                        other.data_type(),
                    )
                    .into());
                }
            };
            let dim = blob.len() / size_of::<f32>();
            if dim != query_vector.len() {
                return Err(SearchError::VectorSize {
                    expected: query_vector.len(),
                    got: dim,
                });
            }
            let dot: f32 = blob
                .as_chunks::<4>()
                .0
                .iter()
                .zip(query_vector)
                .map(|(bytes, b)| f32::from_le_bytes(*bytes) * b)
                .sum();
            scores.push(id, dot);
        }
        Ok(scores.finish())
    }
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

#[cfg(test)]
mod tests;
