//! Поиск по индексу проекта: гибридный, со слиянием RRF, реранком и порогом
//! (`docs/search/hybrid-search.md`).

mod hybrid;
mod params;

pub use hybrid::{SearchError, SearchHit, SearchResponse, search};
pub use params::{AppliedParams, ScoreKind, SearchRequest, SearchSettings};
