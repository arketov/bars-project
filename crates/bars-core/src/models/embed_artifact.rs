//! Закреплённые кандидаты embed; заводской FP32-профиль не изменяется.
use super::{EmbedError, ModelFingerprint, bge_m3_fingerprint};
use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) const REVISION: &str = "4de13258303883538bd53b696b452bf8099f0858";
const REPO: &str = "Xenova/bge-m3";
#[derive(Clone, Copy)]
pub(super) enum Profile {
    Fp16,
    Int8,
}
impl Profile {
    pub(super) fn precision(self) -> &'static str {
        match self {
            Self::Fp16 => "fp16",
            Self::Int8 => "int8",
        }
    }
    fn model(self) -> (&'static str, u64, &'static str) {
        match self {
            Self::Fp16 => (
                "onnx/model_fp16.onnx",
                1_133_992_936,
                "7e7b4768c043cd18d1a2590bb057a4623018e4fe9e40ece87aad6102c6970531",
            ),
            Self::Int8 => (
                "onnx/model_quantized.onnx",
                569_694_530,
                "8833e8f5d30267c0bc3c81aeef01605983b67a0eee0701edc7ea827e4bdb3411",
            ),
        }
    }
    pub(super) fn fingerprint(self) -> ModelFingerprint {
        let mut fp = bge_m3_fingerprint();
        fp.model = format!("{}@{}:{REVISION}", fp.model, self.precision());
        fp
    }
}

fn repo() -> Repo {
    Repo::with_revision(REPO.into(), RepoType::Model, REVISION.into())
}

fn marker(dir: &Path, profile: Profile) -> PathBuf {
    dir.join("bars-verified")
        .join(format!("embed-{}-{REVISION}.json", profile.precision()))
}

fn signature(dir: &Path, profile: Profile) -> Option<Vec<super::artifact::Stamp>> {
    let cache = Cache::new(dir.into()).repo(repo());
    let (name, bytes, hash) = profile.model();
    std::iter::once((name, hash, Some(bytes)))
        .chain(TOKENIZERS.iter().map(|(n, h)| (*n, *h, None)))
        .map(|(name, hash, size)| super::artifact::stamp(&cache.get(name)?, hash, size))
        .collect()
}

pub(super) fn is_downloaded(dir: &Path, profile: Profile) -> bool {
    signature(dir, profile).is_some_and(|s| super::artifact::marked(&marker(dir, profile), &s))
}

pub(super) fn percent(dir: &Path, profile: Profile) -> u8 {
    if is_downloaded(dir, profile) {
        return 100;
    }
    let have: u64 = fs::read_dir(dir.join(format!("models--{}/blobs", REPO.replace('/', "--"))))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    (u128::from(have) * 100 / u128::from(profile.model().1)).min(99) as u8
}
const TOKENIZERS: [(&str, &str); 4] = [
    (
        "config.json",
        "b77268be781e85d1a2a106a977845bb3f0268063ac8d760ca5794e51d450c41f",
    ),
    (
        "tokenizer.json",
        "3b9e10283ee7e9591fa8ced7f38658d371da10171397470dac091f1f6ba36fc3",
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

pub(super) fn ensure(dir: &Path, profile: Profile) -> Result<PathBuf, EmbedError> {
    let load_error = EmbedError::Load;
    let api = ApiBuilder::from_cache(Cache::new(dir.into()))
        .with_token(None)
        .with_progress(false)
        .with_retries(3)
        .build()
        .map_err(|e| load_error(e.to_string()))?;
    let repo = api.repo(repo());
    let (name, _, _) = profile.model();
    let model = repo.get(name).map_err(|e| load_error(e.to_string()))?;
    for (name, _) in TOKENIZERS {
        repo.get(name).map_err(|e| load_error(e.to_string()))?;
    }
    let before =
        signature(dir, profile).ok_or_else(|| load_error("неполный кэш quantized embed".into()))?;
    if !super::artifact::marked(&marker(dir, profile), &before) {
        super::artifact::verify(&model, profile.model().2).map_err(load_error)?;
        for (name, hash) in TOKENIZERS {
            super::artifact::verify(
                &repo.get(name).map_err(|e| load_error(e.to_string()))?,
                hash,
            )
            .map_err(load_error)?;
        }
        if signature(dir, profile).as_ref() != Some(&before) {
            return Err(load_error("кэш embed изменился во время проверки".into()));
        }
        super::artifact::mark(&marker(dir, profile), &before).map_err(load_error)?;
    }
    Ok(model)
}

pub(super) fn fetch(
    dir: &Path,
    profile: Profile,
) -> Result<(PathBuf, fastembed::TokenizerFiles), EmbedError> {
    let model = ensure(dir, profile)?;
    let cache = Cache::new(dir.into()).repo(repo());
    let files: Result<Vec<_>, _> = TOKENIZERS
        .iter()
        .map(|(name, _)| {
            let path = cache
                .get(name)
                .ok_or_else(|| EmbedError::Load(format!("нет файла {name}")))?;
            fs::read(path).map_err(|e| EmbedError::Load(e.to_string()))
        })
        .collect();
    let files = files?;
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
    fn complete_pinned_cache_needs_marker_and_changed_files_invalidate_it() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::new(dir.path().into()).repo(repo());
        cache.create_ref(REVISION).unwrap();
        let profile = Profile::Fp16;
        assert!(!is_downloaded(dir.path(), profile));
        for (name, bytes) in std::iter::once((profile.model().0, profile.model().1))
            .chain(TOKENIZERS.iter().map(|(n, _)| (*n, 1)))
        {
            let path = cache.pointer_path(REVISION).join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::File::create(path).unwrap().set_len(bytes).unwrap();
        }
        assert!(!is_downloaded(dir.path(), profile));
        let sig = signature(dir.path(), profile).unwrap();
        super::super::artifact::mark(&marker(dir.path(), profile), &sig).unwrap();
        assert!(is_downloaded(dir.path(), profile));
        assert_eq!(percent(dir.path(), profile), 100);
        assert!(!is_downloaded(dir.path(), Profile::Int8));
        fs::write(cache.pointer_path(REVISION).join("config.json"), b"changed").unwrap();
        assert!(!is_downloaded(dir.path(), profile));
        assert!(percent(dir.path(), profile) < 100);
        fs::File::options()
            .write(true)
            .open(cache.pointer_path(REVISION).join(profile.model().0))
            .unwrap()
            .set_len(1)
            .unwrap();
        assert!(
            ensure(dir.path(), profile)
                .unwrap_err()
                .to_string()
                .contains("неполный")
        );
    }
    #[test]
    fn precision_and_revision_are_part_of_index_identity() {
        let legacy = bge_m3_fingerprint();
        let fp16 = Profile::Fp16.fingerprint();
        let int8 = Profile::Int8.fingerprint();
        assert!(!legacy.matches(&fp16));
        assert!(!legacy.matches(&int8));
        assert!(!fp16.matches(&int8));
        assert!(fp16.model.contains(REVISION));
        assert!(int8.model.contains(REVISION));
        assert_eq!(fp16.dim, legacy.dim);
        assert_eq!(fp16.doc_prefix, legacy.doc_prefix);
    }
}
