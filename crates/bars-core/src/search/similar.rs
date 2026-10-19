//! Похожие чанки по ID чанка (`docs/features/search/maintenance-tools.md`, IDEA-12).

use rusqlite::OptionalExtension;

use super::hybrid::{Scope, SearchError, SearchHit, load_hits, top_by_score};
use super::params::{ScoreKind, SearchRequest, SearchSettings};
use crate::storage::Index;

/// Сколько знаков контрольной суммы в ID чанка.
const CHECK_LEN: usize = 8;

/// Запрос похожих чанков.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimilarRequest {
    /// ID чанка из выдачи поиска.
    pub id: String,
    /// Модули (корневой — `.`), из которых брать кандидатов; пусто — весь проект.
    pub modules: Vec<String>,
    pub results: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SimilarResponse {
    /// Исходный чанк; в `hits` его нет.
    pub source: SearchHit,
    pub hits: Vec<SearchHit>,
    /// Сколько результатов применено после ограничения пределом.
    pub results: usize,
    pub clamped: bool,
}

/// ID чанка: номер строки `chunks.id` и контрольная сумма документа, пути, `ordinal` и
/// `embed_hash`. Перезапись документа удаляет чанки и вставляет заново, поэтому освободившийся
/// номер может достаться чужому чанку; контрольная сумма делает такой ID устаревшим.
pub(super) fn public_id(
    chunk_id: i64,
    document_hash: &str,
    path: &str,
    ordinal: i64,
    embed_hash: &str,
) -> String {
    format!(
        "{chunk_id}-{}",
        check(document_hash, path, ordinal, embed_hash)
    )
}

fn check(document_hash: &str, path: &str, ordinal: i64, embed_hash: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [document_hash, path, &ordinal.to_string(), embed_hash] {
        hasher.update(part.as_bytes());
        hasher.update(&[0]);
    }
    hasher.finalize().to_hex()[..CHECK_LEN].to_string()
}

/// Номер строки из ID; `None`, если строка не формата `<номер>-<контроль>`.
fn parse(id: &str) -> Option<i64> {
    let (number, check) = id.split_once('-')?;
    let well_formed = check.len() == CHECK_LEN
        && check
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !well_formed || number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse::<i64>().ok().filter(|number| *number > 0)
}

/// Самые близкие по косинусу к чанку `request.id` чанки выбранной области, без самого чанка.
/// Точный перебор готовых векторов; новый embed не нужен.
pub fn similar(
    index: &Index,
    settings: &SearchSettings,
    request: &SimilarRequest,
) -> Result<SimilarResponse, SearchError> {
    let number = parse(&request.id).ok_or_else(|| {
        SearchError::InvalidParams(format!(
            "неверный ID чанка `{}`: ожидается `<номер>-<8 знаков>` из выдачи поиска",
            request.id
        ))
    })?;
    let embed_hash: Option<String> = index
        .conn()
        .query_row(
            "SELECT embed_hash FROM chunks WHERE id = ?1",
            [number],
            |row| row.get(0),
        )
        .optional()?;
    let Some(embed_hash) = embed_hash else {
        return Err(SearchError::ChunkNotFound(request.id.clone()));
    };
    let mut source = load_hits(index, &[number])?.remove(0);
    if source.public_id != request.id {
        return Err(SearchError::ChunkNotFound(request.id.clone()));
    }
    source.score = 1.0;
    let vector = index
        .vector(&embed_hash)?
        .ok_or_else(|| SearchError::MissingVector(request.id.clone()))?;

    let scope = Scope::build(
        index,
        &SearchRequest {
            modules: request.modules.clone(),
            ..SearchRequest::default()
        },
    )?;
    let max = settings.max_results.max(1);
    let results = request
        .results
        .map_or(settings.results.clamp(1, max), |value| value.clamp(1, max));
    let clamped = request.results.is_some_and(|value| value != results);
    // Исходный чанк сам входит в область с косинусом 1: берём на один больше.
    let cosines = scope.cosines(index, &vector, results + 1, &[], &[])?;
    let ids: Vec<i64> = top_by_score(&cosines, results + 1)
        .into_iter()
        .filter(|id| *id != number)
        .take(results)
        .collect();
    let mut hits = load_hits(index, &ids)?;
    for hit in &mut hits {
        hit.score = cosines
            .get(&hit.chunk_id)
            .copied()
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        hit.score_kind = ScoreKind::Cosine;
    }
    Ok(SimilarResponse {
        source,
        hits,
        results,
        clamped,
    })
}
