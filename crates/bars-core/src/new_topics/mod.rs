//! «Новые темы»: вопросы, для которых поиск не нашёл материалов (`docs/features/new-topics/`).
//!
//! Здесь — хранилище отдельной БД `.bars/new-topics.sqlite3` и дедупликация. Сбор в воркере и окно —
//! в `bars-worker`. Тексты вопросов и векторы в логи не попадают.

mod guard;
mod store;

#[cfg(test)]
mod tests;

pub use guard::{differs, is_special, normalize_question};
pub use store::{
    Change, CheckResultRow, CheckRow, EmbeddingSpace, MAX_TOPICS, MAX_VARIANTS_PER_TOPIC, Match,
    NewCheck, NewCheckResult, Observation, Reason, Recorded, Source, StoreError, TopicRow,
    TopicStatus, TopicStore, VariantOutcome, VariantRow, store_path,
};
