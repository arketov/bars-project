//! Закреплённый GPU-экспорт этапа 11; CPU использует штатный FP32 fastembed.
use super::{Accelerator, BGE_RERANKER_V2_M3_ID, RerankError};
use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct VerifiedFile {
    file: String,
    bytes: u64,
    modified_ns: u128,
    blake3: String,
}

fn ready_path(dir: &Path) -> PathBuf {
    dir.join("bars-verified")
        .join(format!("rerank-fp16-{REVISION}.json"))
}

fn cache_signature(dir: &Path) -> Option<Vec<VerifiedFile>> {
    let cache = Cache::new(dir.into());
    let files = std::iter::once((REPO, REVISION, MODEL, MODEL_HASH)).chain(
        TOKENIZERS
            .iter()
            .map(|(file, hash)| (TOKENIZER_REPO, TOKENIZER_REVISION, *file, *hash)),
    );
    files
        .map(|(id, revision, file, hash)| {
            let path = cache.repo(repo(id, revision)).get(file)?;
            let metadata = fs::metadata(&path).ok()?;
            if !metadata.is_file()
                || metadata.len() == 0
                || (file == MODEL && metadata.len() != MODEL_BYTES)
            {
                return None;
            }
            Some(VerifiedFile {
                file: path.strip_prefix(dir).ok()?.to_string_lossy().into_owned(),
                bytes: metadata.len(),
                modified_ns: metadata
                    .modified()
                    .ok()?
                    .duration_since(std::time::UNIX_EPOCH)
                    .ok()?
                    .as_nanos(),
                blake3: hash.into(),
            })
        })
        .collect()
}

fn mark_verified(dir: &Path, signature: &[VerifiedFile]) -> Result<(), RerankError> {
    use std::io::Write;
    let target = ready_path(dir);
    fs::create_dir_all(target.parent().unwrap()).map_err(|e| RerankError::Load(e.to_string()))?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temporary = target.with_extension(format!("{}.{}.tmp", std::process::id(), nonce));
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(signature)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, &target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| RerankError::Load(e.to_string()))
}

const REPO: &str = "onnx-community/bge-reranker-v2-m3-ONNX";
pub(super) const REVISION: &str = "6f5ff65298512715a1e669753bc754d2bc8f367b";
const MODEL: &str = "onnx/model_fp16.onnx";
const MODEL_BYTES: u64 = 1_136_209_678;
const MODEL_HASH: &str = "c2d3b38ad447d974485ab00b3d38ae91394fc5fc2f7946d173252ee13b9c966f";
const TOKENIZER_REPO: &str = "rozgo/bge-reranker-v2-m3";
const TOKENIZER_REVISION: &str = "fbd57b17b4db111a9d16813bb08b4c804fac18e9";
const TOKENIZERS: [(&str, &str); 4] = [
    (
        "config.json",
        "8787637040f354a66fc3c88c15077fb3ce7def77fca6c326926d8df869a50ff6",
    ),
    (
        "tokenizer.json",
        "6b125367faacadc927af65361f5d64bbe95c2befe6e3c337b72830193f59b34b",
    ),
    (
        "special_tokens_map.json",
        "7928b0181fe837ea946ad566a4c8c3421b414988e65c61af6cedf79586971810",
    ),
    (
        "tokenizer_config.json",
        "54693e6b3ab433a83416028e923cc1a3e48ac4b3e901413cce59c1b219e5bd34",
    ),
];

fn repo(id: &str, revision: &str) -> Repo {
    Repo::with_revision(id.into(), RepoType::Model, revision.into())
}

fn gpu_preferred(accelerator: Accelerator) -> bool {
    cfg!(windows) && accelerator == Accelerator::Auto
}

/// Все файлы есть; содержимое ещё не обязательно проверено загрузчиком.
pub fn reranker_files_present(dir: &Path, accelerator: Accelerator) -> bool {
    if !gpu_preferred(accelerator) {
        return super::is_downloaded(dir, BGE_RERANKER_V2_M3_ID);
    }
    cache_signature(dir).is_some()
}

/// Проверяет кэш выбранного варианта без сети и без загрузки модели.
pub fn reranker_is_downloaded(dir: &Path, accelerator: Accelerator) -> bool {
    if !gpu_preferred(accelerator) {
        return super::is_downloaded(dir, BGE_RERANKER_V2_M3_ID);
    }
    let Some(current) = cache_signature(dir) else {
        return false;
    };
    // UI polling must not hash a gigabyte on every request. Only an atomic marker
    // written after verification counts; changed metadata invalidates it.
    fs::read(ready_path(dir))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<VerifiedFile>>(&bytes).ok())
        .is_some_and(|verified| verified == current)
}

/// При Auto это прогресс предпочитаемого GPU-варианта; 100% не означает готовую сессию.
pub fn reranker_download_percent(dir: &Path, accelerator: Accelerator) -> u8 {
    if !gpu_preferred(accelerator) {
        return super::download_percent(dir, BGE_RERANKER_V2_M3_ID).unwrap_or(0);
    }
    if reranker_is_downloaded(dir, accelerator) {
        return 100;
    }
    let blobs = dir.join(format!("models--{}/blobs", REPO.replace('/', "--")));
    let have: u64 = fs::read_dir(blobs)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    (u128::from(have) * 100 / u128::from(MODEL_BYTES)).min(99) as u8
}

fn verify(path: &Path, expected: &str) -> Result<(), RerankError> {
    super::artifact::verify(path, expected).map_err(RerankError::Load)
}

pub(super) fn fp16(dir: &Path) -> Result<(PathBuf, fastembed::TokenizerFiles), RerankError> {
    // from_cache не читает HF_HOME; публичные модели не требуют пользовательского токена.
    let api = ApiBuilder::from_cache(Cache::new(dir.into()))
        .with_token(None)
        .with_progress(false)
        .with_retries(3)
        .build()
        .map_err(|e| RerankError::Load(e.to_string()))?;
    let model = api
        .repo(repo(REPO, REVISION))
        .get(MODEL)
        .map_err(|e| RerankError::Load(e.to_string()))?;
    verify(&model, MODEL_HASH)?;
    let tokenizer = api.repo(repo(TOKENIZER_REPO, TOKENIZER_REVISION));
    let mut files = Vec::new();
    for (name, hash) in TOKENIZERS {
        let path = tokenizer
            .get(name)
            .map_err(|e| RerankError::Load(e.to_string()))?;
        verify(&path, hash)?;
        files.push(fs::read(path).map_err(|e| RerankError::Load(e.to_string()))?);
    }
    let signature = cache_signature(dir)
        .ok_or_else(|| RerankError::Load("кэш FP16 изменился во время проверки".into()))?;
    mark_verified(dir, &signature)?;
    let mut files = files.into_iter();
    Ok((
        model,
        fastembed::TokenizerFiles {
            config_file: files.next().unwrap(),
            tokenizer_file: files.next().unwrap(),
            special_tokens_map_file: files.next().unwrap(),
            tokenizer_config_file: files.next().unwrap(),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_files_need_verified_marker_and_modification_invalidates_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().into());
        for (id, revision, name) in std::iter::once((REPO, REVISION, MODEL)).chain(
            TOKENIZERS
                .iter()
                .map(|(name, _)| (TOKENIZER_REPO, TOKENIZER_REVISION, *name)),
        ) {
            let repo = cache.repo(repo(id, revision));
            repo.create_ref(revision).unwrap();
            let path = repo.pointer_path(revision).join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let file = fs::File::create(path).unwrap();
            file.set_len(if name == MODEL { MODEL_BYTES } else { 1 })
                .unwrap();
        }
        let signature = cache_signature(dir.path()).unwrap();
        if cfg!(windows) {
            assert!(reranker_files_present(dir.path(), Accelerator::Auto));
            assert!(!reranker_is_downloaded(dir.path(), Accelerator::Auto));
        }
        mark_verified(dir.path(), &signature).unwrap();
        // Atomic replacement also works when the previous marker already exists.
        mark_verified(dir.path(), &signature).unwrap();
        if cfg!(windows) {
            assert!(reranker_is_downloaded(dir.path(), Accelerator::Auto));
        }
        let tokenizer = cache
            .repo(repo(TOKENIZER_REPO, TOKENIZER_REVISION))
            .get("config.json")
            .unwrap();
        fs::write(tokenizer, b"changed").unwrap();
        assert_ne!(cache_signature(dir.path()).unwrap(), signature);
        if cfg!(windows) {
            assert!(!reranker_is_downloaded(dir.path(), Accelerator::Auto));
        }
    }
    #[test]
    fn corrupt_artifact_is_rejected_before_session_creation() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model.onnx");
        fs::write(&file, "incomplete").unwrap();
        assert!(verify(&file, MODEL_HASH).is_err());
        let expected = blake3::hash(b"incomplete").to_hex().to_string();
        assert!(verify(&file, &expected).is_ok());
    }
    #[test]
    fn cpu_does_not_require_gpu_export_and_other_revisions_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let old = dir
            .path()
            .join("models--rozgo--bge-reranker-v2-m3/snapshots/cpu");
        for name in [
            "config.json",
            "tokenizer.json",
            "special_tokens_map.json",
            "tokenizer_config.json",
            "model.onnx",
            "model.onnx.data",
        ] {
            fs::create_dir_all(&old).unwrap();
            fs::write(old.join(name), "w").unwrap();
        }
        assert!(reranker_is_downloaded(dir.path(), Accelerator::Cpu));
        assert_eq!(reranker_download_percent(dir.path(), Accelerator::Cpu), 100);
        if cfg!(windows) {
            assert!(!reranker_is_downloaded(dir.path(), Accelerator::Auto));
            assert_eq!(reranker_download_percent(dir.path(), Accelerator::Auto), 0);
        }
    }
}
