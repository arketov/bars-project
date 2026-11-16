//! Похожие чанки по ID чанка (`docs/features/search/maintenance-tools.md`, IDEA-12).

use rusqlite::OptionalExtension;

use super::hybrid::{
    Catalog, DELTA_ID_OFFSET, IndexView, Scope, SearchError, SearchHit, top_by_score,
};
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
    /// Пути-префиксы области кандидатов (папка любой глубины или файл); пусто — весь проект.
    pub paths: Vec<String>,
    /// Не возвращать чанки исходного документа (агентский `like`; ISS-26).
    pub exclude_document: bool,
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

/// Публичный ID чанка документа, содержащего строку: разрешение агентского `путь:строки`
/// (источник `like`) без поискового вызова. Путь — от корня документации или проекта.
pub fn chunk_public_at_line(
    index: &Index,
    docs_root: &str,
    path: &str,
    line: i64,
) -> Result<String, SearchError> {
    chunk_public_at_line_in_view(&IndexView::single(index), docs_root, path, line)
}

/// То же по виду проекта: путь проверяется по каталогу вида, документ из дельты worktree читается
/// из дельты (у владельца его версия исключена), остальные — у владельца. ID чанка дельты несёт
/// номер со смещением `DELTA_ID_OFFSET`, как в выдаче поиска по виду.
pub fn chunk_public_at_line_in_view(
    view: &IndexView,
    docs_root: &str,
    path: &str,
    line: i64,
) -> Result<String, SearchError> {
    let mut stored = None;
    for candidate in crate::doc_path::stored_candidates(docs_root, path) {
        if view.has_path(&candidate)? {
            stored = Some(candidate);
            break;
        }
    }
    let Some(stored) = stored else {
        let known: Vec<String> = view
            .documents()?
            .into_iter()
            .map(|(_, known_path)| known_path)
            .collect();
        return Err(SearchError::InvalidParams(
            crate::doc_path::unknown_file_message(
                docs_root,
                known.iter().map(String::as_str),
                path,
            ),
        ));
    };
    let (index, offset) = match view.delta() {
        Some(delta) if delta.has_path(&stored)? => (delta, DELTA_ID_OFFSET),
        _ => (view.owner(), 0),
    };
    let row = index
        .conn()
        .query_row(
            "SELECT c.id, d.hash, d.path, c.ordinal, c.embed_hash
             FROM chunks c JOIN documents d ON d.id = c.document_id
             WHERE d.path = ?1 AND c.first_line <= ?2 AND c.last_line >= ?2
             ORDER BY c.ordinal LIMIT 1",
            rusqlite::params![stored, line],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((id, hash, path, ordinal, embed_hash)) = row else {
        let last: i64 = index.conn().query_row(
            "SELECT COALESCE(MAX(c.last_line), 0)
             FROM chunks c JOIN documents d ON d.id = c.document_id WHERE d.path = ?1",
            [&stored],
            |row| row.get(0),
        )?;
        return Err(SearchError::InvalidParams(format!(
            "в документе {path} нет строки {line}: в нём {last} строк"
        )));
    };
    Ok(public_id(id + offset, &hash, &path, ordinal, &embed_hash))
}

/// Самые близкие по косинусу к чанку `request.id` чанки выбранной области, без самого чанка.
/// Точный перебор готовых векторов; новый embed не нужен.
pub fn similar(
    index: &Index,
    settings: &SearchSettings,
    request: &SimilarRequest,
) -> Result<SimilarResponse, SearchError> {
    similar_in_view(&IndexView::single(index), settings, request)
}

/// То же по виду проекта: чанк источника и кандидаты берутся из владельца (без исключённых
/// путей) и дельты worktree, номера чанков дельты — со смещением `DELTA_ID_OFFSET`. Косинус
/// абсолютный и от базы не зависит, поэтому слияние баз точное.
pub fn similar_in_view(
    view: &IndexView,
    settings: &SearchSettings,
    request: &SimilarRequest,
) -> Result<SimilarResponse, SearchError> {
    let number = parse(&request.id).ok_or_else(|| {
        SearchError::InvalidParams(format!(
            "неверный ID чанка `{}`: ожидается `<номер>-<8 знаков>` из выдачи поиска",
            request.id
        ))
    })?;
    let (source_index, local) = view.source(number);
    let embed_hash: Option<String> = source_index
        .conn()
        .query_row(
            "SELECT embed_hash FROM chunks WHERE id = ?1",
            [local],
            |row| row.get(0),
        )
        .optional()?;
    let Some(embed_hash) = embed_hash else {
        return Err(SearchError::ChunkNotFound(request.id.clone()));
    };
    let mut source = view.load_hits(&[number])?.remove(0);
    if source.public_id != request.id {
        return Err(SearchError::ChunkNotFound(request.id.clone()));
    }
    source.score = 1.0;
    let vector = source_index
        .vector(&embed_hash)?
        .ok_or_else(|| SearchError::MissingVector(request.id.clone()))?;

    let mut scope = Scope::build_with_paths(
        view,
        &SearchRequest {
            modules: request.modules.clone(),
            paths: request.paths.clone(),
            ..SearchRequest::default()
        },
        &[],
    )?;
    if request.exclude_document {
        scope.exclude_path(&source.path);
    }
    let max = settings.max_results.max(1);
    let results = request
        .results
        .map_or(settings.results.clamp(1, max), |value| value.clamp(1, max));
    let clamped = request.results.is_some_and(|value| value != results);
    // Исходный чанк сам входит в область с косинусом 1: берём на один больше.
    let cosines = view.cosines(&scope, &vector, results + 1, &[], &[])?;
    let ids: Vec<i64> = top_by_score(&cosines, results + 1)
        .into_iter()
        .filter(|id| *id != number)
        .take(results)
        .collect();
    let mut hits = view.load_hits(&ids)?;
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
