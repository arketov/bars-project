//! Поиск по индексу проекта: один поиск на баллах сигналов, с операторами запроса, реранком, порогом
//! и свёрткой кусков раздела (`docs/features/search/hybrid-search.md`, ADR-0007).

mod duplicates;
pub mod fulltext;
mod hybrid;
pub mod operators;
mod params;
mod similar;
mod vector_top;

pub use duplicates::{
    Change, Differences, DuplicateChunk, DuplicateCounts, DuplicateItem, DuplicateKind,
    DuplicatesRequest, DuplicatesResponse, DuplicatesSnapshot, duplicates,
    duplicates_from_snapshot, duplicates_snapshot,
};
pub use hybrid::{
    HitSignals, LinkedDocuments, SearchError, SearchHit, SearchResponse, candidate_trace, search,
    search_expanded, search_with_links, search_with_signals,
};
pub use operators::ParsedQuery;
pub use params::{AppliedParams, ScoreKind, ScoreWeights, SearchRequest, SearchSettings};
pub use similar::{SimilarRequest, SimilarResponse, similar};
