//! Ядро bars: индекс, нарезка, индексация, поиск, модели.
//!
//! Готово: нарезка документов ([`indexing::chunking`]), обход файлов ([`indexing::walk`]), индекс SQLite
//! ([`storage`]), индексация ([`indexing::indexer`]), встроенная embed-модель ([`models`]) и гибридный
//! поиск ([`search`]), чтение настроек ([`settings`]), структура документа ([`outline`]) и путь
//! документа ([`doc_path`]). Остальное — каркас.

pub mod doc_path;
pub mod indexing;
pub mod instruction;
pub mod models;
pub mod outline;
pub mod search;
pub mod settings;
pub mod storage;

use fastembed::EmbeddingModel;

/// Встроенная embed-модель по умолчанию (Т-17).
pub fn default_embedding_model() -> EmbeddingModel {
    EmbeddingModel::BGEM3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_model_is_bge_m3() {
        assert!(matches!(default_embedding_model(), EmbeddingModel::BGEM3));
    }

    #[test]
    fn bundled_sqlite_has_fts5() {
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE VIRTUAL TABLE t USING fts5(body);")
            .unwrap();
    }

    #[test]
    fn russian_stemmer_works() {
        let stemmer = rust_stemmers::Stemmer::create(rust_stemmers::Algorithm::Russian);
        assert_eq!(stemmer.stem("документами"), stemmer.stem("документ"));
    }

    #[test]
    fn blake3_hash_is_stable() {
        assert_eq!(blake3::hash(b"bars"), blake3::hash(b"bars"));
    }
}
