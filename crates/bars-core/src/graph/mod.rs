//! Основа графового retrieval (GR-02, P0; `knowledge/graph-embedding-2026-10-18.md`).
//!
//! Markdown/явные отношения — bars-graph; существующие векторы — similarity.
//! Сходство не утверждает зависимость, направление факта, отрицание или условие.

mod build;
pub mod context;
pub mod retrieval;
pub mod similarity;
pub use bars_graph::*;
pub use build::build_index_graph;
