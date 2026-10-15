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

/// Скачаны ли файлы встроенной модели в `models_dir` (`~/.bars/models`, кэш hf-hub:
/// `models--<организация>--<имя>/snapshots/<ревизия>/…`). Нужен окну для меток «скачана» и кнопки «Скачать»;
/// целостность файлов не проверяется. Файлы снимка в кэше hf-hub могут быть символическими ссылками
/// на `blobs/`, поэтому размер берётся у цели ссылки, а не у самой ссылки. Встроенный rerank лежит
/// в кэше под именем ONNX-экспорта `rozgo/bge-reranker-v2-m3`, а не под своим `id`
/// (`docs/features/models/local-models.md`).
pub fn is_downloaded(models_dir: &std::path::Path, id: &str) -> bool {
    let repo = if id == super::BGE_RERANKER_V2_M3_ID {
        "rozgo/bge-reranker-v2-m3"
    } else {
        id
    };
    fn has_weights(dir: &std::path::Path, depth: usize) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                return depth > 0 && has_weights(&path, depth - 1);
            }
            let weights = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension == "onnx" || extension == "safetensors");
            weights && std::fs::metadata(&path).is_ok_and(|meta| meta.len() > 0)
        })
    }
    let snapshots = models_dir
        .join(format!("models--{}", repo.replace('/', "--")))
        .join("snapshots");
    std::fs::read_dir(snapshots)
        .map(|entries| {
            entries
                .flatten()
                .any(|snapshot| has_weights(&snapshot.path(), 2))
        })
        .unwrap_or(false)
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
    fn downloaded_means_weights_in_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_downloaded(dir.path(), BGE_M3_ID));
        let snapshot = dir.path().join("models--BAAI--bge-m3/snapshots/abc/onnx");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("config.json"), "{}").unwrap();
        assert!(
            !is_downloaded(dir.path(), BGE_M3_ID),
            "без весов — не скачана"
        );
        std::fs::write(snapshot.join("model.onnx"), "w").unwrap();
        assert!(is_downloaded(dir.path(), BGE_M3_ID));
        assert!(!is_downloaded(dir.path(), QWEN3_0_6B_ID));
    }

    #[test]
    fn weights_behind_a_symlink_count_as_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("models--BAAI--bge-m3");
        let snapshot = model.join("snapshots/abc");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::create_dir_all(model.join("blobs")).unwrap();
        let blob = model.join("blobs/deadbeef");
        std::fs::write(&blob, "weights").unwrap();
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&blob, snapshot.join("model.onnx"));
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&blob, snapshot.join("model.onnx"));
        if linked.is_err() {
            eprintln!("символические ссылки недоступны: проверка пропущена");
            return;
        }
        assert!(is_downloaded(dir.path(), BGE_M3_ID));
        std::fs::remove_file(&blob).unwrap();
        assert!(
            !is_downloaded(dir.path(), BGE_M3_ID),
            "ссылка в пустоту — не скачана"
        );
    }

    #[test]
    fn reranker_is_found_under_the_onnx_export_repo() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_downloaded(dir.path(), super::super::BGE_RERANKER_V2_M3_ID));
        let snapshot = dir
            .path()
            .join("models--rozgo--bge-reranker-v2-m3/snapshots/abc");
        std::fs::create_dir_all(&snapshot).unwrap();
        std::fs::write(snapshot.join("model.onnx"), "w").unwrap();
        assert!(is_downloaded(dir.path(), super::super::BGE_RERANKER_V2_M3_ID));
    }

    #[test]
    fn slow_and_short_input_models_warn() {
        assert!(builtin_model(BGE_M3_ID).unwrap().warnings.is_empty());
        for id in [QWEN3_0_6B_ID, QWEN3_4B_ID, NOMIC_V2_MOE_ID] {
            assert!(!builtin_model(id).unwrap().warnings.is_empty(), "{id}");
        }
    }
}
