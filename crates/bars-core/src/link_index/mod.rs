//! Индекс ссылок проекта: исходящие и входящие ссылки документов
//! (`docs/features/knowledge-base/link-index.md`, Т-30, Т-31).
//!
//! Отдельная БД `.bars/links.sqlite3`, строится из файлов на диске без чанков, векторов и модели.
//! Адресат ссылки не хранится: он зависит от набора документов и разрешается при чтении.

mod store;
pub use bars_graph::links::{DocSet, moves};

#[cfg(test)]
mod tests;

pub use store::{
    BrokenLink, IncomingLink, LINKS_PARSER_VERSION, LinkIndex, LinkIndexError, LinkInto, LinkPage,
    LinksOf, OutgoingLink, RefreshStats, SourceInto, store_path,
};
