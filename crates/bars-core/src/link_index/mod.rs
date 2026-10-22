//! Индекс ссылок проекта: исходящие и входящие ссылки документов
//! (`docs/features/knowledge-base/link-index.md`, Т-30, Т-31).
//!
//! Отдельная БД `.bars/links.sqlite3`, строится из файлов на диске без чанков, векторов и модели.
//! Адресат ссылки не хранится: он зависит от набора документов и разрешается при чтении.

mod store;

#[cfg(test)]
mod tests;

pub use store::{
    IncomingLink, LINKS_PARSER_VERSION, LinkIndex, LinkIndexError, LinksOf,
    OutgoingLink, RefreshStats, store_path,
};
