//! Каталог встроенных embed-моделей (`docs/features/models/local-models.md`, Т-17).

use super::embedder::ModelFingerprint;

/// Чем считается встроенная модель.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// ONNX Runtime через fastembed (процессор или видеокарта).
    Onnx,
    /// candle, фича fastembed `qwen3`; только процессор.
    Qwen3,
    /// candle, фича fastembed `nomic-v2-moe`; только процессор.
    NomicV2Moe,
}

/// Одна встроенная embed-модель.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinModel {
    /// Идентификатор: он же в `embed.model` и в отпечатке.
    pub id: &'static str,
    pub dim: usize,
    /// Префикс, который bars добавляет к тексту документа; входит в отпечаток.
    pub doc_prefix: &'static str,
    /// Префикс или инструкция, которую bars добавляет к запросу.
    pub query_prefix: &'static str,
    /// Длина входа в токенах, которую bars задаёт модели явно.
    pub max_length: usize,
    pub backend: Backend,
    /// Предупреждения для окна при выборе модели: скорость, память, обрезка входа.
    pub warnings: &'static [&'static str],
}

pub const BGE_M3_ID: &str = "BAAI/bge-m3";
pub const BGE_M3_DIM: usize = 1024;
pub const QWEN3_0_6B_ID: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const QWEN3_4B_ID: &str = "Qwen/Qwen3-Embedding-4B";
pub const NOMIC_V2_MOE_ID: &str = "nomic-ai/nomic-embed-text-v2-moe";

/// Инструкция к запросу Qwen3-Embedding: часть того, как модель обучена.
pub const QWEN3_QUERY_PREFIX: &str =
    "Instruct: Given a search query, retrieve relevant passages from documentation\nQuery: ";

const QWEN3_SPEED: &str = "считается на процессоре через candle и заметно медленнее bge-m3";
const NOMIC_INPUT: &str = "длина входа 512 токенов: чанк длиннее примерно 1500 символов попадает в вектор не целиком; уменьшите `chunking.max_chars`";

const CATALOG: &[BuiltinModel] = &[
    BuiltinModel {
        id: BGE_M3_ID,
        dim: BGE_M3_DIM,
        doc_prefix: "",
        query_prefix: "",
        max_length: 8192,
        backend: Backend::Onnx,
        warnings: &[],
    },
    BuiltinModel {
        id: QWEN3_0_6B_ID,
        dim: 1024,
        doc_prefix: "",
        query_prefix: QWEN3_QUERY_PREFIX,
        max_length: 8192,
        backend: Backend::Qwen3,
        warnings: &[QWEN3_SPEED],
    },
    BuiltinModel {
        id: QWEN3_4B_ID,
        dim: 2560,
        doc_prefix: "",
        query_prefix: QWEN3_QUERY_PREFIX,
        max_length: 8192,
        backend: Backend::Qwen3,
        warnings: &[
            QWEN3_SPEED,
            "модель весит около 8 ГБ на диске и держит в памяти около 16 ГБ: на CPU первая индексация проекта идёт часами",
        ],
    },
    BuiltinModel {
        id: NOMIC_V2_MOE_ID,
        dim: 768,
        doc_prefix: "search_document: ",
        query_prefix: "search_query: ",
        max_length: 512,
        backend: Backend::NomicV2Moe,
        warnings: &[
            "считается на процессоре через candle и медленнее bge-m3",
            NOMIC_INPUT,
        ],
    },
];

/// Все встроенные embed-модели; bge-m3 первая.
pub fn builtin_models() -> &'static [BuiltinModel] {
    CATALOG
}

/// Модель каталога по идентификатору.
pub fn builtin_model(id: &str) -> Option<&'static BuiltinModel> {
    CATALOG.iter().find(|model| model.id == id)
}

impl BuiltinModel {
    /// Отпечаток известен без загрузки модели: воркер сверяет его с индексом до эмбеддинга (Т-25).
    pub fn fingerprint(&self) -> ModelFingerprint {
        ModelFingerprint {
            source: "builtin".to_string(),
            model: self.id.to_string(),
            dim: self.dim,
            doc_prefix: self.doc_prefix.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_the_four_models_and_bge_m3_comes_first() {
        let ids: Vec<_> = builtin_models().iter().map(|model| model.id).collect();
        assert_eq!(
            ids,
            [BGE_M3_ID, QWEN3_0_6B_ID, QWEN3_4B_ID, NOMIC_V2_MOE_ID]
        );
        assert!(builtin_model("нет такой").is_none());
    }

    #[test]
    fn fingerprints_differ_between_models_and_nomic_carries_its_document_prefix() {
        let all: Vec<_> = builtin_models().iter().map(|m| m.fingerprint()).collect();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a, b);
            }
        }
        let nomic = builtin_model(NOMIC_V2_MOE_ID).unwrap().fingerprint();
        assert_eq!(nomic.doc_prefix, "search_document: ");
        assert_eq!(nomic.source, "builtin");
    }

    #[test]
    fn slow_and_short_input_models_warn() {
        assert!(builtin_model(BGE_M3_ID).unwrap().warnings.is_empty());
        for id in [QWEN3_0_6B_ID, QWEN3_4B_ID, NOMIC_V2_MOE_ID] {
            assert!(!builtin_model(id).unwrap().warnings.is_empty(), "{id}");
        }
    }
}
