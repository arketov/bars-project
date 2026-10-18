//! Поиск по индексу проекта: гибридный, со слиянием RRF, реранком и порогом
//! (`docs/features/search/hybrid-search.md`).

mod hybrid;
mod params;

pub use hybrid::{SearchError, SearchHit, SearchResponse, candidate_trace, search};
pub use params::{AppliedParams, ScoreKind, SearchRequest, SearchSettings};
