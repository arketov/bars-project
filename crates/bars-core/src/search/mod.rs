//! Поиск по индексу проекта: гибридный, со слиянием RRF, реранком и порогом
//! (`docs/features/search/hybrid-search.md`).

mod duplicates;
mod hybrid;
mod params;
mod similar;
mod vector_top;

pub use duplicates::{
    Change, DuplicateChunk, DuplicateCounts, DuplicateItem, DuplicateKind, Differences, DuplicatesRequest,
    DuplicatesResponse, duplicates,
};
pub use hybrid::{
    SearchError, SearchHit, SearchResponse, candidate_trace, search, search_expanded,
};
pub use params::{AppliedParams, ScoreKind, SearchRequest, SearchSettings};
pub use similar::{SimilarRequest, SimilarResponse, similar};
