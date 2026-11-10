//! Один поиск на баллах сигналов: полнотекстовые и векторные кандидаты, сложение баллов
//! `S = a + w_c·c + w_f·f + w_p·p + w_l·l`, rerank, единый порог по опоре, операторы запроса,
//! свёртка кусков раздела (`docs/features/search/hybrid-search.md`, ADR-0007, Т-12…Т-14, Т-18, Т-24,
//! Т-25).

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::params_from_iter;
use rusqlite::types::Value;

use super::fulltext::{BM25, ExactProbe, FtsQuery, f_signal, fts_plan_for, p_signal};
use super::operators::{OperatorFilter, ParsedQuery};
use super::params::{AppliedParams, ScoreKind, ScoreWeights, SearchRequest, SearchSettings};
use super::vector_top::VectorTop;
use crate::models::{EmbedError, Embedder, RerankError, Reranker};
use crate::storage::{Compatibility, Index, StorageError};

/// Во сколько раз полный перебор векторов берёт больше кандидатов, когда в запросе есть фразы или
/// исключения: часть векторных кандидатов отсеют операторы.
const OPERATOR_OVERFETCH: usize = 3;

#[derive(Debug, thiserror::Error)]
pub enum SearchError {
    /// Запрос или отбор не годятся (`invalid_params` в протоколе).
    #[error("{0}")]
    InvalidParams(String),
    /// Индекс построен другой моделью или старой схемой (Т-25): искать в нём нельзя.
    #[error("нужна переиндексация: индекс построен другой моделью или старой схемой")]
    ReindexRequired,
    /// Полнотекстовая таблица записана прежним разбором: её пересоздаёт первое задание обновления.
    #[error("полнотекстовый индекс перестраивается после обновления bars: повторите запрос позже")]
    FtsRebuilding,
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
    /// `min(1, S)` раздела: итоговая оценка лучшего куска плюс бонус свёртки.
    pub score: f32,
    /// Вид опоры оценки.
    pub score_kind: ScoreKind,
    /// Баллы лучшего куска по сигналам.
    pub signals: HitSignals,
    /// Сколько кусков в разделе (в `chunks` документа с той же цепочкой заголовков).
    pub parts: usize,
    /// Сколько из них нашлось (дошло до выдачи свёрнутым).
    pub parts_found: usize,
}

/// Баллы результата по сигналам схемы баллов (`None` — сигнала не было).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HitSignals {
    /// `r` — оценка rerank-модели.
    pub rerank: Option<f32>,
    /// `c` — косинус; `None`, когда embed-модель не успела загрузиться.
    pub cosine: Option<f32>,
    /// `f` — слова запроса по полям, 0…1.
    pub fulltext: f32,
    /// `p` — точное совпадение, 0…1.
    pub exact: f32,
    /// `l` — связи, 0 или 1; `None`, когда сигнал не считался (вес `w_l` равен 0, источника ссылок
    /// нет или он не ответил).
    pub links: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    /// Разделы пула ниже порога оценки: показываются строками «слабые совпадения» без текста,
    /// когда основная выдача пуста (ступенчатая выдача агентского слоя).
    pub weak: Vec<WeakHit>,
    pub applied: AppliedParams,
    /// Уникальный пул до rerank и порога, без загрузки текстов кандидатов.
    pub candidate_modules: BTreeMap<String, usize>,
    /// Причина, по которой rerank включён, но выдача вернулась без него.
    pub rerank_error: Option<String>,
    /// Нормализованный вектор запроса, если он вычислялся (при пустом корпусе — `None`). Внутренний
    /// сигнал для воркера («Новые темы»), в публичную выдачу не попадает.
    pub query_vector: Option<Vec<f32>>,
    /// Запрос после разбора операторов («понято как»).
    pub understood: ParsedQuery,
}

/// Раздел пула ниже порога оценки (ступенчатая выдача агентского слоя): строка «слабые совпадения»
/// без текста. Тексты не загружаются — нужны только путь, строки и заголовок.
#[derive(Debug, Clone, PartialEq)]
pub struct WeakHit {
    pub path: String,
    pub heading_chain: String,
    pub first_line: usize,
    pub last_line: usize,
    /// Опора оценки (rerank или косинус); ниже порога `min_score`.
    pub score: f32,
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
    expansion: Option<&mut CandidateExpansion<'_>>,
) -> Result<SearchResponse, SearchError> {
    search_with_signals(
        index, embedder, reranker, settings, request, expansion, true,
    )
}

/// То же, но с выбором смысловой части. При `semantic = false` embed-модель не вызывается
/// (воркер не дождался её загрузки): выдача строится по полнотекстовому списку, косинуса нет,
/// порог косинуса не применяется, расширение кандидатов не вызывается. Отпечаток модели нужен
/// только для проверки совместимости индекса (Т-25) и загрузки не требует.
pub fn search_with_signals(
    index: &Index,
    embedder: &mut dyn Embedder,
    reranker: Option<&mut dyn Reranker>,
    settings: &SearchSettings,
    request: &SearchRequest,
    expansion: Option<&mut CandidateExpansion<'_>>,
    semantic: bool,
) -> Result<SearchResponse, SearchError> {
    search_with_links(
        index, embedder, reranker, settings, request, expansion, semantic, None,
    )
}

/// Источник сигнала `l`: по путям якорей (документы трёх лучших результатов) и путям документов
/// выдачи отвечает, какие из вторых связаны ссылкой с якорями (`LinkIndex::linked_documents`).
/// Ошибка источника не ломает поиск: сигнал просто не считается.
pub type LinkedDocuments<'a> =
    dyn FnMut(&[String], &[String]) -> Result<HashSet<String>, SearchError> + 'a;

/// Количество лучших результатов, документы которых служат якорями сигнала `l`.
const LINK_ANCHORS: usize = 3;

/// То же, что `search_with_signals`, плюс сигнал связей `l`. Он считается, только если передан
/// `links` и вес `w_l` больше нуля.
#[allow(clippy::too_many_arguments)]
pub fn search_with_links(
    index: &Index,
    embedder: &mut dyn Embedder,
    reranker: Option<&mut dyn Reranker>,
    settings: &SearchSettings,
    request: &SearchRequest,
    expansion: Option<&mut CandidateExpansion<'_>>,
    semantic: bool,
    links: Option<&mut LinkedDocuments<'_>>,
) -> Result<SearchResponse, SearchError> {
    // FTS открывает служебные таблицы лениво: смена схемы между чтениями ломает xConnect.
    // Сохраняем транзакцию вызывающего кода, если поиск уже работает в его снимке.
    let _snapshot = if index.conn().is_autocommit() {
        Some(index.conn().unchecked_transaction()?)
    } else {
        None
    };
    if matches!(
        index.compatibility(embedder.fingerprint())?,
        Compatibility::ModelChanged | Compatibility::SchemaOutdated
    ) {
        return Err(SearchError::ReindexRequired);
    }
    // Колонки `chunks_fts` пересоздаёт первый писатель при открытии индекса; читатель их не чинит
    // и старую таблицу с другими колонками не читает.
    if index.stem_outdated()? {
        return Err(SearchError::FtsRebuilding);
    }
    let plan = QueryPlan::build(index, &request.query)?;
    let scope = Scope::build_with_paths(index, request, &plan.parsed.paths)?;

    let planned = settings.resolve(request, reranker.is_some());
    if scope.chunk_count(index)? == 0 {
        return Ok(SearchResponse {
            hits: Vec::new(),
            weak: Vec::new(),
            applied: planned,
            candidate_modules: BTreeMap::new(),
            rerank_error: None,
            query_vector: None,
            understood: plan.parsed,
        });
    }

    let query_vector = if semantic {
        let mut vector = embedder.embed_query(&plan.parsed.text)?;
        let expected = embedder.fingerprint().dim;
        if vector.len() != expected {
            return Err(EmbedError::Dimension {
                expected,
                got: vector.len(),
            }
            .into());
        }
        normalize(&mut vector);
        Some(vector)
    } else {
        None
    };

    let gathered = gather(
        index,
        &scope,
        &plan,
        query_vector.as_deref(),
        planned.candidates,
        &planned.weights,
        expansion,
    )?;
    let candidate_modules = count_candidate_modules(index, &gathered.ids())?;

    let mut rerank_error = None;
    let mut rerank_scores = None;
    if let Some(reranker) = reranker {
        match rerank_scores_of(
            index,
            reranker,
            &plan.parsed.text,
            &gathered,
            planned.rerank_candidates,
        ) {
            Ok(scores) => rerank_scores = Some(scores),
            Err(error) => rerank_error = Some(error.to_string()),
        }
    }

    let mut applied = if rerank_error.is_some() {
        settings.resolve(request, false)
    } else {
        planned
    };
    applied.score_kind = if rerank_scores.is_some() {
        ScoreKind::Rerank
    } else if query_vector.is_some() {
        ScoreKind::Cosine
    } else {
        ScoreKind::None
    };
    // Опоры нет (ни rerank, ни косинуса): порог применять не к чему, порядок остаётся словесным.
    if applied.score_kind == ScoreKind::None {
        applied.min_score = 0.0;
    }

    let mut ranked = rank(&gathered, rerank_scores.as_ref(), &applied);
    load_documents(index, &mut ranked)?;
    let linked = match links {
        Some(links) if applied.weights.links > 0.0 => {
            apply_links(&mut ranked, applied.weights.links, links)
        }
        _ => false,
    };
    applied.signals = used_signals(&applied, query_vector.is_some(), linked);
    let hits = fold_sections(index, ranked, &applied)?;
    // «Слабые совпадения» — только когда порог отрезал всё: агент видит, что было рядом.
    // Ниже порога считаем лениво, чтобы обычная выдача не платила за проход по пулу.
    let weak = if hits.is_empty() {
        let mut below = below_threshold(&gathered, rerank_scores.as_ref(), &applied);
        fold_weak(index, &mut below)?
    } else {
        Vec::new()
    };
    Ok(SearchResponse {
        hits,
        weak,
        applied,
        candidate_modules,
        rerank_error,
        query_vector,
        understood: plan.parsed,
    })
}

/// Сигналы, посчитанные для этого ответа (в `applied.signals`).
fn used_signals(applied: &AppliedParams, cosine: bool, linked: bool) -> Vec<&'static str> {
    let mut signals = Vec::new();
    if applied.score_kind == ScoreKind::Rerank {
        signals.push("rerank");
    }
    if cosine {
        signals.push("cosine");
    }
    signals.push("fulltext");
    signals.push("exact");
    if linked {
        signals.push("links");
    }
    signals
}

/// Запрос после разбора: слова для FTS5 и сигнала `p`, условия операторов.
pub(super) struct QueryPlan {
    pub(super) parsed: ParsedQuery,
    match_query: FtsQuery,
    probe: ExactProbe,
    filter: OperatorFilter,
}

impl QueryPlan {
    /// Разбирает операторы и собирает выражения FTS5 с исправлением опечаток. Запрос, в котором
    /// после снятия операторов не осталось слов, — `invalid_params`.
    pub(super) fn build(index: &Index, query: &str) -> Result<Self, SearchError> {
        let parsed = ParsedQuery::parse(query);
        let (match_query, probe) = fts_plan_for(index.conn(), &parsed.text)?.ok_or_else(|| {
            SearchError::InvalidParams("в запросе нет ни одного слова".to_string())
        })?;
        let filter = parsed.filter();
        Ok(Self {
            parsed,
            match_query,
            probe,
            filter,
        })
    }
}

/// Кандидат с баллами сигналов; опорных `r` и `a` у него ещё нет.
struct Candidate {
    id: i64,
    /// Косинус; `None`, если запрос без вектора.
    cosine: Option<f32>,
    fulltext: f32,
    exact: f32,
}

impl Candidate {
    /// Предварительная оценка `S0 = c + w_f·f + w_p·p` (без embed — `f + w_p·p`): по ней кандидаты
    /// уходят в rerank.
    fn preliminary(&self, weights: &ScoreWeights) -> f32 {
        match self.cosine {
            Some(cosine) => cosine + weights.fulltext * self.fulltext + weights.exact * self.exact,
            None => self.fulltext + weights.exact * self.exact,
        }
    }
}

/// Пул кандидатов запроса, по убыванию предварительной оценки.
struct Gathered {
    pool: Vec<Candidate>,
    /// Первые кандидаты полнотекстового списка и векторного (для диагностики бенча).
    text_ids: Vec<i64>,
    vector_ids: Vec<i64>,
}

impl Gathered {
    fn ids(&self) -> Vec<i64> {
        self.pool.iter().map(|candidate| candidate.id).collect()
    }
}

/// Собирает пул: первые `candidates` по составному `bm25`, первые `candidates` по косинусу и
/// слова-идентификаторы; операторы отсекают лишнее; считает `f` и `p`; упорядочивает по `S0`.
fn gather(
    index: &Index,
    scope: &Scope,
    plan: &QueryPlan,
    query_vector: Option<&[f32]>,
    candidates: usize,
    weights: &ScoreWeights,
    mut expansion: Option<&mut CandidateExpansion<'_>>,
) -> Result<Gathered, SearchError> {
    let conn = index.conn();
    let text_scored =
        scope.text_scored(index, &plan.filter.apply(&plan.match_query.all), candidates)?;
    let text_ids: Vec<i64> = text_scored.iter().map(|(id, _)| *id).collect();
    let exact_ids = match &plan.match_query.identifiers {
        Some(identifiers) => {
            scope.text_candidates(index, &plan.filter.apply(identifiers), candidates)?
        }
        None => Vec::new(),
    };
    let overfetch = if plan.filter.is_empty() {
        candidates
    } else {
        candidates * OPERATOR_OVERFETCH
    };
    let mut cosines = match query_vector {
        Some(vector) => scope.cosines(index, vector, overfetch, &text_ids, &exact_ids)?,
        None => HashMap::new(),
    };
    let mut vector_ids = top_by_score(&cosines, overfetch);
    plan.filter.retain(conn, &mut vector_ids)?;
    vector_ids.truncate(candidates);

    let mut pool: Vec<i64> = Vec::new();
    let mut seen: HashSet<i64> = HashSet::new();
    for id in text_ids.iter().chain(&exact_ids).chain(&vector_ids) {
        if seen.insert(*id) {
            pool.push(*id);
        }
    }
    if let (Some(expansion), Some(query_vector)) = (
        expansion.as_mut().filter(|_| !pool.is_empty()),
        query_vector,
    ) {
        let mut additional: Vec<i64> = expansion(index, &pool, candidates)?
            .into_iter()
            .take(candidates)
            .collect();
        // Косинус добавленных кандидатов считается принудительно, исходная область сохраняется.
        let scores = scope.cosines(index, query_vector, 0, &additional, &[])?;
        additional.retain(|id| scores.contains_key(id));
        plan.filter.retain(conn, &mut additional)?;
        cosines.extend(scores);
        for id in additional {
            if seen.insert(id) {
                pool.push(id);
            }
        }
    }

    let fulltext = f_signal(conn, &plan.match_query.all, &text_scored, &pool)?;
    let exact = p_signal(conn, &plan.probe, &pool)?;
    let mut candidates: Vec<Candidate> = pool
        .into_iter()
        .map(|id| Candidate {
            id,
            cosine: query_vector.map(|_| cosine_of(&cosines, id)),
            fulltext: fulltext.get(&id).copied().unwrap_or(0.0),
            exact: exact.get(&id).copied().unwrap_or(0.0),
        })
        .collect();
    candidates.sort_by(|a, b| {
        b.preliminary(weights)
            .total_cmp(&a.preliminary(weights))
            .then(b.fulltext.total_cmp(&a.fulltext))
            .then(a.id.cmp(&b.id))
    });
    Ok(Gathered {
        pool: candidates,
        text_ids,
        vector_ids,
    })
}

/// Оценки rerank первых `rerank_candidates` кандидатов пула (по `S0`), по id чанка. Ей уходят путь,
/// цепочка заголовков и текст; модель читает первые 512 токенов вместе с запросом.
fn rerank_scores_of(
    index: &Index,
    reranker: &mut dyn Reranker,
    query: &str,
    gathered: &Gathered,
    rerank_candidates: usize,
) -> Result<HashMap<i64, f32>, RerankError> {
    let ids: Vec<i64> = gathered
        .pool
        .iter()
        .take(rerank_candidates)
        .map(|candidate| candidate.id)
        .collect();
    let hits = load_hits(index, &ids).map_err(|error| RerankError::Rerank(error.to_string()))?;
    if hits.is_empty() {
        return Ok(HashMap::new());
    }
    let documents: Vec<String> = hits
        .iter()
        .map(|hit| format!("{}\n{}\n{}", hit.path, hit.heading_chain, hit.text))
        .collect();
    let refs: Vec<&str> = documents.iter().map(String::as_str).collect();
    let scores = reranker.rerank(query, &refs)?;
    if scores.len() != hits.len() {
        return Err(RerankError::Count {
            expected: hits.len(),
            got: scores.len(),
        });
    }
    Ok(ids
        .into_iter()
        .zip(scores)
        .map(|(id, score)| (id, score.clamp(0.0, 1.0)))
        .collect())
}

/// Кусок с итоговой оценкой, до свёртки раздела.
struct Ranked {
    id: i64,
    /// `S` куска.
    score: f32,
    signals: HitSignals,
    document_id: i64,
    path: String,
    heading_chain: String,
    ordinal: i64,
}

/// Итоговая оценка `S = a + w_c·c + w_f·f + w_p·p` каждого кусочка пула. Опора `a` — оценка rerank,
/// без неё — косинус, без обоих — нет (порядок `f + w_p·p`). Порог относится к опоре, а не к `S`;
/// кандидаты ниже порога отсекает `below_threshold` (они идут в «слабые совпадения» пустой выдачи).
/// С rerank остаются только кандидаты, ушедшие в него.
fn rank(
    gathered: &Gathered,
    rerank: Option<&HashMap<i64, f32>>,
    applied: &AppliedParams,
) -> Vec<Ranked> {
    let weights = &applied.weights;
    let mut ranked = Vec::new();
    for candidate in &gathered.pool {
        let (anchor, score) = match rerank {
            Some(scores) => {
                let Some(&r) = scores.get(&candidate.id) else {
                    continue;
                };
                let cosine = weights.cosine * candidate.cosine.unwrap_or(0.0);
                (
                    Some(r),
                    r + cosine
                        + weights.fulltext * candidate.fulltext
                        + weights.exact * candidate.exact,
                )
            }
            None => match candidate.cosine {
                Some(c) => (
                    Some(c),
                    c + weights.fulltext * candidate.fulltext + weights.exact * candidate.exact,
                ),
                None => (None, candidate.fulltext + weights.exact * candidate.exact),
            },
        };
        if anchor.is_some_and(|anchor| anchor < applied.min_score) {
            continue;
        }
        ranked.push(Ranked {
            id: candidate.id,
            score,
            signals: HitSignals {
                rerank: rerank.and(anchor),
                cosine: candidate.cosine,
                fulltext: candidate.fulltext,
                exact: candidate.exact,
                links: None,
            },
            document_id: 0,
            path: String::new(),
            heading_chain: String::new(),
            ordinal: 0,
        });
    }
    ranked
}

/// Кандидаты с опорой ниже порога `min_score` (те, что `rank` отсекает): «слабые совпадения» для
/// пустой выдачи. Кандидат без опоры сюда не попадает — порог к нему не применяется и слабым он
/// не считается. Порядок — как в пуле, по `S0`.
fn below_threshold(
    gathered: &Gathered,
    rerank: Option<&HashMap<i64, f32>>,
    applied: &AppliedParams,
) -> Vec<Ranked> {
    let mut below = Vec::new();
    for candidate in &gathered.pool {
        let anchor = match rerank {
            Some(scores) => match scores.get(&candidate.id) {
                Some(&r) => Some(r),
                None => continue,
            },
            None => match candidate.cosine {
                Some(cosine) => Some(cosine),
                None => continue,
            },
        };
        if anchor.is_some_and(|anchor| anchor >= applied.min_score) {
            continue;
        }
        below.push(Ranked {
            id: candidate.id,
            score: candidate.preliminary(&applied.weights),
            signals: HitSignals {
                rerank: rerank.and(anchor),
                cosine: candidate.cosine,
                fulltext: candidate.fulltext,
                exact: candidate.exact,
                links: None,
            },
            document_id: 0,
            path: String::new(),
            heading_chain: String::new(),
            ordinal: 0,
        });
    }
    below
}

/// Сколько «слабых» разделов отдаётся при пустой выдаче (ступенчатая выдача агентского слоя).
const WEAK_LIMIT: usize = 3;

/// Сворачивает кандидатов ниже порога в разделы и берёт лучшие по опоре — без загрузки текстов.
fn fold_weak(index: &Index, below: &mut [Ranked]) -> Result<Vec<WeakHit>, SearchError> {
    if below.is_empty() {
        return Ok(Vec::new());
    }
    load_documents(index, below)?;
    below.sort_by(|a, b| {
        anchor_of(b)
            .unwrap_or(0.0)
            .total_cmp(&anchor_of(a).unwrap_or(0.0))
            .then(b.signals.fulltext.total_cmp(&a.signals.fulltext))
            .then_with(|| a.path.cmp(&b.path))
            .then(a.ordinal.cmp(&b.ordinal))
    });
    let mut seen: HashSet<(i64, &str)> = HashSet::new();
    let mut weak = Vec::new();
    for item in below.iter() {
        if weak.len() == WEAK_LIMIT {
            break;
        }
        if !seen.insert((item.document_id, item.heading_chain.as_str())) {
            continue;
        }
        let (first_line, last_line): (i64, i64) = index.conn().query_row(
            "SELECT min(first_line), max(last_line) FROM chunks
             WHERE document_id = ?1 AND heading_chain = ?2",
            rusqlite::params![item.document_id, item.heading_chain],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        weak.push(WeakHit {
            path: item.path.clone(),
            heading_chain: item.heading_chain.clone(),
            first_line: first_line as usize,
            last_line: last_line as usize,
            score: anchor_of(item).unwrap_or(0.0),
        });
    }
    Ok(weak)
}

/// Опора кандидата: rerank, а без него — косинус.
fn anchor_of(item: &Ranked) -> Option<f32> {
    item.signals.rerank.or(item.signals.cosine)
}

/// Дописывает кускам документ, цепочку заголовков и номер: по ним идут свёртка и сигнал `l`.
fn load_documents(index: &Index, ranked: &mut [Ranked]) -> Result<(), SearchError> {
    let mut statement = index.conn().prepare_cached(
        "SELECT c.document_id, d.path, c.heading_chain, c.ordinal
         FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.id = ?1",
    )?;
    for item in ranked {
        let (document_id, path, heading_chain, ordinal) =
            statement.query_row([item.id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
        item.document_id = document_id;
        item.path = path;
        item.heading_chain = heading_chain;
        item.ordinal = ordinal;
    }
    Ok(())
}

/// Сигнал `l`: 1, если документ куска связан ссылкой (в любую сторону) с документом одного из трёх
/// лучших результатов — разделов по `S` без `l` — и это не он сам; иначе 0. К `S` добавляется
/// `w_l·l`. Один запрос к источнику ссылок на весь пул. Возвращает, посчитан ли сигнал: когда
/// источник не ответил, оценки и `signals.links` не меняются.
fn apply_links(ranked: &mut [Ranked], weight: f32, links: &mut LinkedDocuments<'_>) -> bool {
    let mut order: Vec<usize> = (0..ranked.len()).collect();
    order.sort_by(|&a, &b| {
        let (a, b) = (&ranked[a], &ranked[b]);
        b.score
            .total_cmp(&a.score)
            .then(b.signals.fulltext.total_cmp(&a.signals.fulltext))
            .then_with(|| a.path.cmp(&b.path))
            .then(a.ordinal.cmp(&b.ordinal))
    });
    let mut anchors: Vec<String> = Vec::new();
    {
        let mut sections: HashSet<(i64, &str)> = HashSet::new();
        for &position in &order {
            let item = &ranked[position];
            if sections.len() == LINK_ANCHORS {
                break;
            }
            if sections.insert((item.document_id, item.heading_chain.as_str()))
                && !anchors.contains(&item.path)
            {
                anchors.push(item.path.clone());
            }
        }
    }
    let mut paths: Vec<String> = ranked.iter().map(|item| item.path.clone()).collect();
    paths.sort_unstable();
    paths.dedup();
    let Ok(linked) = links(&anchors, &paths) else {
        return false;
    };
    for item in ranked {
        let l = if linked.contains(&item.path) {
            1.0
        } else {
            0.0
        };
        item.score += weight * l;
        item.signals.links = Some(l);
    }
    true
}

/// Куски одного раздела (один документ, одна цепочка заголовков) дают один результат: `S` раздела —
/// `S` лучшего куска плюс `fold_bonus` за каждый найденный кусок сверх лучшего. Порог уже применён к
/// кускам; результатов — `applied.results` разделов. Равные `S` — по `f`, пути документа и номеру
/// куска.
fn fold_sections(
    index: &Index,
    mut ranked: Vec<Ranked>,
    applied: &AppliedParams,
) -> Result<Vec<SearchHit>, SearchError> {
    ranked.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(b.signals.fulltext.total_cmp(&a.signals.fulltext))
            .then_with(|| a.path.cmp(&b.path))
            .then(a.ordinal.cmp(&b.ordinal))
    });

    // Раздел -> (позиция лучшего куска, сколько кусков нашлось); порядок — по убыванию `S`.
    let mut order: Vec<(i64, String)> = Vec::new();
    let mut sections: HashMap<(i64, String), (usize, usize)> = HashMap::new();
    for (position, item) in ranked.iter().enumerate() {
        let key = (item.document_id, item.heading_chain.clone());
        match sections.get_mut(&key) {
            Some((_, found)) => *found += 1,
            None => {
                order.push(key.clone());
                sections.insert(key, (position, 1));
            }
        }
    }
    let bonus = applied.weights.fold_bonus;
    let mut folded: Vec<(f32, usize, usize)> = order
        .iter()
        .map(|key| {
            let (best, found) = sections[key];
            let extra = found.saturating_sub(1) as f32;
            (ranked[best].score + bonus * extra, best, found)
        })
        .collect();
    if bonus != 0.0 {
        // Бонус может переставить разделы; позиция лучшего куска — запасной порядок.
        folded.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    }
    folded.truncate(applied.results);

    let mut hits = Vec::with_capacity(folded.len());
    for (score, best, found) in folded {
        let item = &ranked[best];
        let mut hit = load_hits(index, &[item.id])?
            .pop()
            .ok_or_else(|| SearchError::ChunkNotFound(item.id.to_string()))?;
        let (parts, first_line, last_line): (i64, i64, i64) = index.conn().query_row(
            "SELECT count(*), min(first_line), max(last_line) FROM chunks
             WHERE document_id = ?1 AND heading_chain = ?2",
            rusqlite::params![item.document_id, item.heading_chain],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        hit.first_line = first_line as usize;
        hit.last_line = last_line as usize;
        hit.score = score.min(1.0);
        hit.score_kind = applied.score_kind;
        hit.signals = item.signals;
        hit.parts = parts as usize;
        hit.parts_found = found;
        hits.push(hit);
    }
    Ok(hits)
}

fn count_candidate_modules(
    index: &Index,
    ids: &[i64],
) -> Result<BTreeMap<String, usize>, SearchError> {
    if ids.is_empty() {
        return Ok(BTreeMap::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let mut statement = index.conn().prepare(&format!(
        "SELECT d.module, count(*) FROM chunks c JOIN documents d ON d.id = c.document_id \
         WHERE c.id IN ({placeholders}) GROUP BY d.module"
    ))?;
    let rows = statement.query_map(params_from_iter(ids), |row| {
        Ok((row.get(0)?, row.get::<_, i64>(1)? as usize))
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Диагностика первого этапа для бенча IDEA-06: полнотекстовый список, векторный, пул по `S0`.
/// Использует те же операции, что `search`, до rerank, порога и свёртки.
#[doc(hidden)]
pub fn candidate_trace(
    index: &Index,
    query_vector: &[f32],
    request: &SearchRequest,
    candidates: usize,
) -> Result<[Vec<SearchHit>; 3], SearchError> {
    let plan = QueryPlan::build(index, &request.query)?;
    let scope = Scope::build_with_paths(index, request, &plan.parsed.paths)?;
    let mut vector = query_vector.to_vec();
    normalize(&mut vector);
    let weights = ScoreWeights::default();
    let gathered = gather(
        index,
        &scope,
        &plan,
        Some(&vector),
        candidates,
        &weights,
        None,
    )?;
    let cosines: HashMap<i64, f32> = gathered
        .pool
        .iter()
        .filter_map(|candidate| candidate.cosine.map(|cosine| (candidate.id, cosine)))
        .collect();
    let mut lists = [
        load_hits(index, &gathered.text_ids)?,
        load_hits(index, &gathered.vector_ids)?,
        load_hits(index, &gathered.ids())?,
    ];
    for hits in &mut lists {
        for hit in hits {
            hit.score = cosine_of(&cosines, hit.chunk_id);
        }
    }
    Ok(lists)
}

/// Кандидат пула с сырыми баллами сигналов: вход подбора весов схемы баллов (шаг 2.3.7).
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct PoolCandidate {
    /// Чанк с документом; `score` и `signals` не заполнены.
    pub hit: SearchHit,
    /// `c`; `None`, когда вектора запроса нет.
    pub cosine: Option<f32>,
    /// `f`.
    pub fulltext: f32,
    /// `p`.
    pub exact: f32,
    /// `r`: rerank оценивает весь пул, а не первые `rerank_candidates`; `None` — без rerank или
    /// при его ошибке.
    pub rerank: Option<f32>,
    /// Место (с нуля) в полнотекстовом списке пула; `None` — кандидата в нём не было.
    pub text_rank: Option<usize>,
    /// Место (с нуля) в векторном списке пула.
    pub vector_rank: Option<usize>,
}

/// Пул кандидатов запроса со всеми сырыми сигналами, до порога, свёртки и сложения баллов.
#[doc(hidden)]
#[derive(Debug, Clone)]
pub struct PoolDump {
    /// По убыванию предварительной оценки `S0` с весами из настроек.
    pub candidates: Vec<PoolCandidate>,
    pub rerank_error: Option<String>,
    pub understood: ParsedQuery,
}

/// Те же шаги, что у `search_with_links` до сложения баллов, но rerank получает весь пул: по этим
/// данным бенч подбирает веса вне поиска и воспроизводит выбор первых `rerank_candidates` при любых
/// весах. Используется только бенчем.
#[doc(hidden)]
pub fn scored_pool(
    index: &Index,
    embedder: &mut dyn Embedder,
    reranker: Option<&mut dyn Reranker>,
    settings: &SearchSettings,
    request: &SearchRequest,
) -> Result<PoolDump, SearchError> {
    let _snapshot = if index.conn().is_autocommit() {
        Some(index.conn().unchecked_transaction()?)
    } else {
        None
    };
    if matches!(
        index.compatibility(embedder.fingerprint())?,
        Compatibility::ModelChanged | Compatibility::SchemaOutdated
    ) {
        return Err(SearchError::ReindexRequired);
    }
    if index.stem_outdated()? {
        return Err(SearchError::FtsRebuilding);
    }
    let plan = QueryPlan::build(index, &request.query)?;
    let scope = Scope::build_with_paths(index, request, &plan.parsed.paths)?;
    let planned = settings.resolve(request, reranker.is_some());
    if scope.chunk_count(index)? == 0 {
        return Ok(PoolDump {
            candidates: Vec::new(),
            rerank_error: None,
            understood: plan.parsed,
        });
    }
    let mut vector = embedder.embed_query(&plan.parsed.text)?;
    let expected = embedder.fingerprint().dim;
    if vector.len() != expected {
        return Err(EmbedError::Dimension {
            expected,
            got: vector.len(),
        }
        .into());
    }
    normalize(&mut vector);
    let gathered = gather(
        index,
        &scope,
        &plan,
        Some(&vector),
        planned.candidates,
        &planned.weights,
        None,
    )?;
    let mut rerank_error = None;
    let mut rerank_scores = HashMap::new();
    if let Some(reranker) = reranker {
        match rerank_scores_of(
            index,
            reranker,
            &plan.parsed.text,
            &gathered,
            gathered.pool.len(),
        ) {
            Ok(scores) => rerank_scores = scores,
            Err(error) => rerank_error = Some(error.to_string()),
        }
    }
    let rank_in = |ids: &[i64]| -> HashMap<i64, usize> {
        ids.iter()
            .enumerate()
            .map(|(place, id)| (*id, place))
            .collect()
    };
    let (text_rank, vector_rank) = (rank_in(&gathered.text_ids), rank_in(&gathered.vector_ids));
    let hits = load_hits(index, &gathered.ids())?;
    let candidates = gathered
        .pool
        .iter()
        .zip(hits)
        .map(|(candidate, hit)| PoolCandidate {
            cosine: candidate.cosine,
            fulltext: candidate.fulltext,
            exact: candidate.exact,
            rerank: rerank_scores.get(&candidate.id).copied(),
            text_rank: text_rank.get(&candidate.id).copied(),
            vector_rank: vector_rank.get(&candidate.id).copied(),
            hit,
        })
        .collect();
    Ok(PoolDump {
        candidates,
        rerank_error,
        understood: plan.parsed,
    })
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
                signals: HitSignals::default(),
                parts: 1,
                parts_found: 1,
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
    /// Строка отбора без операторов `path:`.
    pub(super) fn build(index: &Index, request: &SearchRequest) -> Result<Self, SearchError> {
        Self::build_with_paths(index, request, &[])
    }

    /// Исключает из области документ по пути (от корня проекта) — агентский `like` без разделов
    /// исходного документа (ISS-26).
    pub(super) fn exclude_path(&mut self, path: &str) {
        self.clause = format!("({}) AND d.path != ?", self.clause);
        self.params.push(Value::from(path));
    }

    /// Строит отбор и проверяет, что каждый модуль и файл есть в индексе. Модули и файлы
    /// объединяются; пустой отбор — весь проект. Префиксы `path:` из запроса **сужают** этот отбор
    /// (пересечение, а не объединение): документ должен лежать под одним из префиксов.
    pub(super) fn build_with_paths(
        index: &Index,
        request: &SearchRequest,
        paths: &[String],
    ) -> Result<Self, SearchError> {
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

        if !request.paths.is_empty() {
            // Агентская область `in`: префиксы папок и файлов любой глубины, объединены между собой.
            let stored = path_prefixes(index, &request.paths)?;
            let conditions =
                vec!["(d.path = ? OR d.path LIKE ? ESCAPE '\\')"; stored.len()].join(" OR ");
            parts.push(format!("({conditions})"));
            for prefix in stored {
                params.push(Value::from(prefix.clone()));
                params.push(Value::from(format!("{}/%", escape_like(&prefix))));
            }
        }

        let mut clause = if parts.is_empty() {
            "1".to_string()
        } else {
            format!("({})", parts.join(" OR "))
        };
        if !paths.is_empty() {
            let stored = path_prefixes(index, paths)?;
            let conditions =
                vec!["(d.path = ? OR d.path LIKE ? ESCAPE '\\')"; stored.len()].join(" OR ");
            clause = format!("{clause} AND ({conditions})");
            for prefix in stored {
                params.push(Value::from(prefix.clone()));
                params.push(Value::from(format!("{}/%", escape_like(&prefix))));
            }
        }
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

    /// Первые `limit` чанков по составному `bm25`. Слова запроса и текст чанков прошли один разбор.
    fn text_candidates(
        &self,
        index: &Index,
        match_query: &str,
        limit: usize,
    ) -> Result<Vec<i64>, SearchError> {
        Ok(self
            .text_scored(index, match_query, limit)?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// То же с сырым баллом `-bm25` (больше — лучше), по убыванию: вход сигнала `f`
    /// (`fulltext::f_signal`).
    pub(super) fn text_scored(
        &self,
        index: &Index,
        match_query: &str,
        limit: usize,
    ) -> Result<Vec<(i64, f32)>, SearchError> {
        let sql = format!(
            "SELECT chunks_fts.rowid, {BM25}
             FROM chunks_fts
             JOIN chunks c ON c.id = chunks_fts.rowid
             JOIN documents d ON d.id = c.document_id
             WHERE chunks_fts MATCH ? AND {}
             ORDER BY {BM25}
             LIMIT ?",
            self.clause
        );
        let mut params = vec![Value::from(match_query.to_string())];
        params.extend(self.params.iter().cloned());
        params.push(Value::Integer(limit as i64));
        let mut statement = index.conn().prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(params), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    super::fulltext::raw_score(row.get(1)?),
                ))
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
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

/// Экранирует `%`, `_` и обратную косую черту в префиксе для `LIKE ... ESCAPE`: в именах папок бывает `_`.
fn escape_like(prefix: &str) -> String {
    crate::doc_path::like_escape(prefix)
}

/// Префиксы `path:` в виде путей индекса (от корня проекта). Префикс задаётся от корня документации
/// или проекта, как у `files`; подходит тот вариант, под которым в индексе есть документ (сам файл
/// или папка с документами). Путь, под которым документов нет, — `invalid_params` с подсказкой.
fn path_prefixes(index: &Index, paths: &[String]) -> Result<Vec<String>, SearchError> {
    let docs_root = index
        .stored_layout()?
        .map(|layout| layout.docs_root)
        .unwrap_or_default();
    let mut found = Vec::new();
    for path in paths {
        let mut chosen = None;
        for candidate in crate::doc_path::stored_candidates(&docs_root, path) {
            let known = index
                .conn()
                .prepare_cached(
                    "SELECT EXISTS(SELECT 1 FROM documents
                     WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\')",
                )?
                .query_row(
                    [candidate.clone(), format!("{}/%", escape_like(&candidate))],
                    |row| row.get::<_, bool>(0),
                )?;
            if known {
                chosen = Some(candidate);
                break;
            }
        }
        let Some(stored) = chosen else {
            let mut statement = index.conn().prepare("SELECT path FROM documents")?;
            let known = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            return Err(SearchError::InvalidParams(format!(
                "path:{path} — в индексе нет документов под этим путём. {}",
                crate::doc_path::unknown_file_message(
                    &docs_root,
                    known.iter().map(String::as_str),
                    path,
                )
            )));
        };
        push_path(&mut found, stored);
    }
    Ok(found)
}

fn push_path(list: &mut Vec<String>, item: String) {
    if !list.contains(&item) {
        list.push(item);
    }
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(", ")
}

#[cfg(test)]
mod tests;
