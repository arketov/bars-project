//! Каталог встроенных embed-моделей (`docs/features/models/local-models.md`, Т-17).

use super::embedder::ModelFingerprint;

/// Чем считается встроенная модель.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// ONNX Runtime через fastembed (процессор или видеокарта).
    Onnx,
    /// GGUF Q4_K_M через встроенный llama.cpp.
    Gguf,
    /// candle, фича fastembed `nomic-v2-moe`; только процессор.
    NomicV2Moe,
}

/// Одна встроенная embed-модель.
#[derive(Debug, Clone, Copy)]
pub struct BuiltinModel {
    /// Идентификатор в `embed.model`; отпечаток также закрепляет представление модели.
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
pub const BGE_M3_FP16_ID: &str = "BAAI/bge-m3:fp16";
pub const BGE_M3_DIM: usize = 1024;
pub const QWEN3_0_6B_ID: &str = "Qwen/Qwen3-Embedding-0.6B";
pub const QWEN3_4B_ID: &str = "Qwen/Qwen3-Embedding-4B";
pub const NOMIC_V2_MOE_ID: &str = "nomic-ai/nomic-embed-text-v2-moe";

/// Инструкция к запросу Qwen3-Embedding: часть того, как модель обучена.
pub const QWEN3_QUERY_PREFIX: &str =
    "Instruct: Given a search query, retrieve relevant passages from documentation\nQuery: ";

const QWEN3_SPEED: &str = "GGUF Q4_K_M через llama.cpp; GPU при доступном Vulkan/Metal, иначе CPU; после прежнего candle-индекса нужна переиндексация";
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
    #[cfg(windows)]
    BuiltinModel {
        id: BGE_M3_FP16_ID,
        dim: BGE_M3_DIM,
        doc_prefix: "",
        query_prefix: "",
        max_length: 8192,
        backend: Backend::Onnx,
        warnings: &[
            "FP16: только DirectML; при недоступной GPU выберите обычную bge-m3; нужна переиндексация",
        ],
    },
    BuiltinModel {
        id: QWEN3_0_6B_ID,
        dim: 1024,
        doc_prefix: "",
        query_prefix: QWEN3_QUERY_PREFIX,
        max_length: 8192,
        backend: Backend::Gguf,
        warnings: &[
            QWEN3_SPEED,
            "0,6B: сторонняя GGUF-конверсия enacimie, закреплённая ревизией и SHA-256",
        ],
    },
    BuiltinModel {
        id: QWEN3_4B_ID,
        dim: 2560,
        doc_prefix: "",
        query_prefix: QWEN3_QUERY_PREFIX,
        max_length: 8192,
        backend: Backend::Gguf,
        warnings: &[
            QWEN3_SPEED,
            "4B: около 2,5 ГБ на диске; память контекста и вычислений добавляется к весам, CPU может работать медленно",
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
        if self.id == BGE_M3_FP16_ID {
            return super::bge_m3_fp16_fingerprint();
        }
        if self.backend == Backend::Gguf {
            return super::gguf::fingerprint(self);
        }
        ModelFingerprint {
            source: "builtin".to_string(),
            model: self.id.to_string(),
            dim: self.dim,
            doc_prefix: self.doc_prefix.to_string(),
        }
    }
}

/// Какие файлы нужны модели и сколько они весят: по ним модель считается скачанной и считается
/// ход скачивания.
struct Layout {
    /// Репозиторий в кэше hf-hub. Встроенный rerank лежит под именем ONNX-экспорта
    /// `rozgo/bge-reranker-v2-m3`, а не под своим `id`.
    repo: &'static str,
    /// Файлы снимка, которые должны лежать целиком.
    files: &'static [&'static str],
    /// Веса — `safetensors`: один файл или шарды по `model.safetensors.index.json`.
    safetensors: bool,
    /// Ожидаемый размер всех файлов, байт; приблизительный.
    size_bytes: u64,
}

const TOKENIZER_FILES: [&str; 3] = [
    "special_tokens_map.json",
    "tokenizer.json",
    "tokenizer_config.json",
];

const BGE_M3_FILES: &[&str] = &[
    "config.json",
    TOKENIZER_FILES[0],
    TOKENIZER_FILES[1],
    TOKENIZER_FILES[2],
    "onnx/model.onnx",
    "onnx/model.onnx_data",
    "onnx/Constant_7_attr__value",
];

const RERANKER_FILES: &[&str] = &[
    "config.json",
    TOKENIZER_FILES[0],
    TOKENIZER_FILES[1],
    TOKENIZER_FILES[2],
    "model.onnx",
    "model.onnx.data",
];

const CANDLE_FILES: &[&str] = &["config.json", "tokenizer.json"];

fn layout(id: &str) -> Option<Layout> {
    let candle = |repo: &'static str, size_bytes: u64| Layout {
        repo,
        files: CANDLE_FILES,
        safetensors: true,
        size_bytes,
    };
    Some(match id {
        BGE_M3_ID => Layout {
            repo: BGE_M3_ID,
            files: BGE_M3_FILES,
            safetensors: false,
            size_bytes: 2_284_711_286,
        },
        super::BGE_RERANKER_V2_M3_ID => Layout {
            repo: "rozgo/bge-reranker-v2-m3",
            files: RERANKER_FILES,
            safetensors: false,
            size_bytes: 2_288_282_930,
        },
        NOMIC_V2_MOE_ID => candle(NOMIC_V2_MOE_ID, 960_000_000),
        _ => return None,
    })
}

fn model_dir(models_dir: &std::path::Path, layout: &Layout) -> std::path::PathBuf {
    models_dir.join(format!("models--{}", layout.repo.replace('/', "--")))
}

/// Файл есть и не пуст. Файлы снимка в кэше hf-hub могут быть символическими ссылками на `blobs/`,
/// поэтому размер берётся у цели ссылки, а не у самой ссылки.
fn is_present(path: &std::path::Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() > 0)
}

/// Получает файлы ONNX-каталога без создания сессии и чтения весов целиком в память.
pub fn download_onnx_artifacts(
    models_dir: &std::path::Path,
    id: &str,
) -> Result<(), super::ModelFailure> {
    use super::{ModelErrorKind, ModelFailure};
    if id == BGE_M3_FP16_ID {
        return super::embed_artifact::ensure(models_dir, super::embed_artifact::Profile::Fp16)
            .map(|_| ())
            .map_err(|e| e.failure());
    }
    let info = layout(id).filter(|l| !l.safetensors).ok_or_else(|| {
        ModelFailure::new(
            ModelErrorKind::Runtime,
            format!("для модели `{id}` нет ONNX-списка файлов"),
        )
    })?;
    let api = hf_hub::api::sync::ApiBuilder::from_cache(hf_hub::Cache::new(models_dir.into()))
        .with_token(None)
        .with_progress(false)
        .with_retries(3)
        .build()
        .map_err(|e| ModelFailure::new(ModelErrorKind::Runtime, e.to_string()))?;
    let repo = api.repo(hf_hub::Repo::model(info.repo.into()));
    for file in info.files {
        let path = repo
            .get(file)
            .map_err(|e| ModelFailure::new(super::failure::hub_kind(&e), e.to_string()))?;
        if !is_present(&path) {
            return Err(ModelFailure::new(
                ModelErrorKind::FilesMissing,
                format!("файл модели пуст или отсутствует: {file}"),
            ));
        }
    }
    Ok(())
}

/// Все ли файлы с весами `safetensors` в снимке: либо `model.safetensors`, либо все шарды из
/// `model.safetensors.index.json`.
fn safetensors_ready(snapshot: &std::path::Path) -> bool {
    if is_present(&snapshot.join("model.safetensors")) {
        return true;
    }
    let Ok(index) = std::fs::read(snapshot.join("model.safetensors.index.json")) else {
        return false;
    };
    let Ok(index) = serde_json::from_slice::<serde_json::Value>(&index) else {
        return false;
    };
    let Some(map) = index["weight_map"].as_object() else {
        return false;
    };
    !map.is_empty()
        && map
            .values()
            .filter_map(|shard| shard.as_str())
            .all(|shard| is_present(&snapshot.join(shard)))
}

/// Скачаны ли все файлы встроенной модели в `models_dir` (`~/.bars/models`, кэш hf-hub:
/// `models--<организация>--<имя>/snapshots/<ревизия>/…`). Нужен окну для меток «скачана» и кнопки
/// «Скачать» и воркеру, чтобы знать, что поиск не начнёт скачивание. Проверяется наличие и
/// непустота каждого файла каталога; содержимое не проверяется (`docs/features/models/local-models.md`,
/// «Когда модель считается скачанной»).
pub fn is_downloaded(models_dir: &std::path::Path, id: &str) -> bool {
    if id == BGE_M3_FP16_ID {
        return super::embed_artifact::is_downloaded(
            models_dir,
            super::embed_artifact::Profile::Fp16,
        );
    }
    if super::gguf_artifact::artifact(id).is_some() {
        return super::gguf_artifact::is_downloaded(models_dir, id);
    }
    let Some(layout) = layout(id) else {
        return false;
    };
    let Ok(snapshots) = std::fs::read_dir(model_dir(models_dir, &layout).join("snapshots")) else {
        return false;
    };
    snapshots.flatten().any(|snapshot| {
        let snapshot = snapshot.path();
        layout
            .files
            .iter()
            .all(|file| is_present(&snapshot.join(file)))
            && (!layout.safetensors || safetensors_ready(&snapshot))
    })
}

/// Ход скачивания модели в процентах: размер файлов в `blobs/` кэша (вместе с недокачанными
/// `.incomplete`) к ожидаемому размеру. Пока файлы не все на месте, не больше 99; у скачанной — 100.
/// Неизвестная модель — `None`.
pub fn download_percent(models_dir: &std::path::Path, id: &str) -> Option<u8> {
    if id == BGE_M3_FP16_ID {
        return Some(super::embed_artifact::percent(
            models_dir,
            super::embed_artifact::Profile::Fp16,
        ));
    }
    if super::gguf_artifact::artifact(id).is_some() {
        return super::gguf_artifact::percent(models_dir, id);
    }
    let layout = layout(id)?;
    if is_downloaded(models_dir, id) {
        return Some(100);
    }
    let have: u64 = std::fs::read_dir(model_dir(models_dir, &layout).join("blobs"))
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.metadata().ok())
                .filter(|meta| meta.is_file())
                .map(|meta| meta.len())
                .sum()
        })
        .unwrap_or(0);
    let percent = (u128::from(have) * 100 / u128::from(layout.size_bytes.max(1))).min(99);
    Some(percent as u8)
}

/// Файлы последнего снимка модели с размерами для лога: `model.onnx 1.2 МБ; model.onnx.data нет`.
/// Нужна диагностике, когда модель отвечает странно: видно неполные и отсутствующие файлы.
/// Модели нет в каталоге или в кэше нет снимка — `None`.
pub fn model_files_report(models_dir: &std::path::Path, id: &str) -> Option<String> {
    let layout = layout(id)?;
    let snapshot = std::fs::read_dir(model_dir(models_dir, &layout).join("snapshots"))
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .next()?;
    let parts: Vec<String> = layout
        .files
        .iter()
        .map(|file| match std::fs::metadata(snapshot.join(file)) {
            Ok(meta) if meta.is_file() => {
                format!("{file} {:.1} МБ", meta.len() as f64 / 1_048_576.0)
            }
            _ => format!("{file} нет"),
        })
        .collect();
    Some(parts.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_download_gets_artifacts_without_parsing_an_onnx_session() {
        let dir = tempfile::tempdir().unwrap();
        let cached =
            hf_hub::Cache::new(dir.path().into()).repo(hf_hub::Repo::model(BGE_M3_ID.into()));
        cached.create_ref("fixture").unwrap();
        for file in BGE_M3_FILES {
            let path = cached.pointer_path("fixture").join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "not an ONNX model").unwrap();
            assert!(cached.get(file).is_some());
        }
        download_onnx_artifacts(dir.path(), BGE_M3_ID).unwrap();
        assert!(is_downloaded(dir.path(), BGE_M3_ID));
        assert!(download_onnx_artifacts(dir.path(), QWEN3_4B_ID).is_err());
        assert!(download_onnx_artifacts(dir.path(), "unknown").is_err());
    }

    #[test]
    fn catalog_has_expected_models_and_fp32_bge_m3_comes_first() {
        let ids: Vec<_> = builtin_models().iter().map(|model| model.id).collect();
        let expected = if cfg!(windows) {
            vec![
                BGE_M3_ID,
                BGE_M3_FP16_ID,
                QWEN3_0_6B_ID,
                QWEN3_4B_ID,
                NOMIC_V2_MOE_ID,
            ]
        } else {
            vec![BGE_M3_ID, QWEN3_0_6B_ID, QWEN3_4B_ID, NOMIC_V2_MOE_ID]
        };
        assert_eq!(ids, expected);
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

    /// Кладёт в снимок все файлы модели, кроме `skip`.
    fn write_files(snapshot: &std::path::Path, id: &str, skip: &[&str]) {
        for file in layout(id).unwrap().files {
            if skip.contains(file) {
                continue;
            }
            let path = snapshot.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "w").unwrap();
        }
    }

    #[test]
    fn downloaded_means_every_file_of_the_model_is_in_a_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!is_downloaded(dir.path(), BGE_M3_ID));
        let snapshot = dir.path().join("models--BAAI--bge-m3/snapshots/abc");
        write_files(&snapshot, BGE_M3_ID, &["onnx/model.onnx_data"]);
        assert!(
            !is_downloaded(dir.path(), BGE_M3_ID),
            "малый model.onnx есть, а model.onnx_data ещё нет — не скачана"
        );
        std::fs::write(snapshot.join("onnx/model.onnx_data"), "").unwrap();
        assert!(
            !is_downloaded(dir.path(), BGE_M3_ID),
            "пустой файл не считается"
        );
        std::fs::write(snapshot.join("onnx/model.onnx_data"), "w").unwrap();
        assert!(is_downloaded(dir.path(), BGE_M3_ID));
        assert!(!is_downloaded(dir.path(), QWEN3_0_6B_ID));
        assert!(!is_downloaded(dir.path(), "нет такой"));
    }

    #[test]
    fn weights_behind_a_symlink_count_as_downloaded() {
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("models--BAAI--bge-m3");
        let snapshot = model.join("snapshots/abc");
        write_files(&snapshot, BGE_M3_ID, &["onnx/model.onnx_data"]);
        std::fs::create_dir_all(model.join("blobs")).unwrap();
        let blob = model.join("blobs/deadbeef");
        std::fs::write(&blob, "weights").unwrap();
        let link = snapshot.join("onnx/model.onnx_data");
        #[cfg(windows)]
        let linked = std::os::windows::fs::symlink_file(&blob, &link);
        #[cfg(unix)]
        let linked = std::os::unix::fs::symlink(&blob, &link);
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
        let id = super::super::BGE_RERANKER_V2_M3_ID;
        assert!(!is_downloaded(dir.path(), id));
        let snapshot = dir
            .path()
            .join("models--rozgo--bge-reranker-v2-m3/snapshots/abc");
        write_files(&snapshot, id, &["model.onnx.data"]);
        assert!(!is_downloaded(dir.path(), id));
        std::fs::write(snapshot.join("model.onnx.data"), "w").unwrap();
        assert!(is_downloaded(dir.path(), id));
    }

    #[test]
    fn files_report_names_missing_files_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let id = super::super::BGE_RERANKER_V2_M3_ID;
        assert_eq!(model_files_report(dir.path(), id), None);
        let snapshot = dir
            .path()
            .join("models--rozgo--bge-reranker-v2-m3/snapshots/abc");
        write_files(&snapshot, id, &["model.onnx.data"]);
        let report = model_files_report(dir.path(), id).unwrap();
        assert!(report.contains("model.onnx 0.0 МБ"));
        assert!(report.contains("model.onnx.data нет"));
        assert_eq!(model_files_report(dir.path(), "нет такой"), None);
    }

    #[test]
    fn candle_model_needs_single_weights_or_every_shard() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = dir
            .path()
            .join("models--nomic-ai--nomic-embed-text-v2-moe/snapshots/abc");
        write_files(&snapshot, NOMIC_V2_MOE_ID, &[]);
        assert!(!is_downloaded(dir.path(), NOMIC_V2_MOE_ID), "весов нет");

        std::fs::write(
            snapshot.join("model.safetensors.index.json"),
            r#"{"weight_map":{"a":"model-1.safetensors","b":"model-2.safetensors"}}"#,
        )
        .unwrap();
        std::fs::write(snapshot.join("model-1.safetensors"), "w").unwrap();
        assert!(
            !is_downloaded(dir.path(), NOMIC_V2_MOE_ID),
            "второго шарда нет"
        );
        std::fs::write(snapshot.join("model-2.safetensors"), "w").unwrap();
        assert!(is_downloaded(dir.path(), NOMIC_V2_MOE_ID));

        let dir2 = tempfile::tempdir().unwrap();
        let single = dir2
            .path()
            .join("models--nomic-ai--nomic-embed-text-v2-moe/snapshots/abc");
        write_files(&single, NOMIC_V2_MOE_ID, &[]);
        assert!(!is_downloaded(dir2.path(), NOMIC_V2_MOE_ID));
        std::fs::write(single.join("model.safetensors"), "w").unwrap();
        assert!(is_downloaded(dir2.path(), NOMIC_V2_MOE_ID));
    }

    #[test]
    fn download_percent_follows_blob_sizes_and_stops_short_of_100_until_done() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(download_percent(dir.path(), BGE_M3_ID), Some(0));
        assert_eq!(download_percent(dir.path(), "нет такой"), None);

        let model = dir.path().join("models--BAAI--bge-m3");
        std::fs::create_dir_all(model.join("blobs")).unwrap();
        let half = layout(BGE_M3_ID).unwrap().size_bytes / 2;
        let file = std::fs::File::create(model.join("blobs/x.incomplete")).unwrap();
        // Only logical length matters; NTFS otherwise allocates gigabytes of zeros.
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let status = std::process::Command::new("fsutil")
                .args(["sparse", "setflag"])
                .arg(model.join("blobs/x.incomplete"))
                .creation_flags(0x08000000)
                .status()
                .unwrap();
            assert!(status.success(), "не удалось создать разреженную фикстуру");
        }
        file.set_len(half).unwrap();
        assert_eq!(download_percent(dir.path(), BGE_M3_ID), Some(50));

        file.set_len(layout(BGE_M3_ID).unwrap().size_bytes * 2)
            .unwrap();
        assert_eq!(download_percent(dir.path(), BGE_M3_ID), Some(99));

        write_files(&model.join("snapshots/abc"), BGE_M3_ID, &[]);
        assert_eq!(download_percent(dir.path(), BGE_M3_ID), Some(100));
    }

    #[test]
    fn slow_and_short_input_models_warn() {
        assert!(builtin_model(BGE_M3_ID).unwrap().warnings.is_empty());
        for id in [QWEN3_0_6B_ID, QWEN3_4B_ID, NOMIC_V2_MOE_ID] {
            assert!(!builtin_model(id).unwrap().warnings.is_empty(), "{id}");
        }
    }
}
