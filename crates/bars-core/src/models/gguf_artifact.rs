//! Закреплённые Q4_K_M: ADR-0002, OQ-60; готовность не равна наличию первого файла.
use super::{EmbedError, QWEN3_0_6B_ID, QWEN3_4B_ID};
use hf_hub::{Cache, Repo, RepoType, api::sync::ApiBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

pub(super) struct Artifact {
    pub repo: &'static str,
    pub revision: &'static str,
    pub file: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}
pub(super) fn artifact(id: &str) -> Option<&'static Artifact> {
    static SMALL: Artifact = Artifact {
        repo: "enacimie/Qwen3-Embedding-0.6B-Q4_K_M-GGUF",
        revision: "51fe2a65af23d8cfd3c9c1d89846cf9073f8902b",
        file: "qwen3-embedding-0.6b-q4_k_m.gguf",
        bytes: 396_474_560,
        sha256: "17c3e3f2eaabc6e321702b4a13680d042e72afc5d602f359f27a670c3e54718c",
    };
    static LARGE: Artifact = Artifact {
        repo: "Qwen/Qwen3-Embedding-4B-GGUF",
        revision: "f4602530db1d980e16da9d7d3a70294cf5c190be",
        file: "Qwen3-Embedding-4B-Q4_K_M.gguf",
        bytes: 2_496_703_776,
        sha256: "2b0cf8f17b4c723c27303015383c27ec4bf2d8314bb677d05e920dd70bb0f16b",
    };
    match id {
        QWEN3_0_6B_ID => Some(&SMALL),
        QWEN3_4B_ID => Some(&LARGE),
        _ => None,
    }
}
impl Artifact {
    fn repo(&self) -> Repo {
        Repo::with_revision(self.repo.into(), RepoType::Model, self.revision.into())
    }
    fn path(&self, dir: &Path) -> Option<PathBuf> {
        Cache::new(dir.into()).repo(self.repo()).get(self.file)
    }
    fn marker(&self, dir: &Path) -> PathBuf {
        dir.join("bars-verified")
            .join(format!("qwen-q4-{}.json", self.revision))
    }
}
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Signature {
    bytes: u64,
    modified_ns: u128,
    sha256: String,
}
fn signature(a: &Artifact, path: &Path) -> Option<Signature> {
    let m = fs::metadata(path).ok()?;
    if !m.is_file() || m.len() != a.bytes {
        return None;
    }
    Some(Signature {
        bytes: m.len(),
        modified_ns: m
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
        sha256: a.sha256.into(),
    })
}
fn ready(a: &Artifact, dir: &Path) -> bool {
    let Some(path) = a.path(dir) else {
        return false;
    };
    let Some(current) = signature(a, &path) else {
        return false;
    };
    fs::read(a.marker(dir))
        .ok()
        .and_then(|b| serde_json::from_slice::<Signature>(&b).ok())
        .is_some_and(|s| s == current)
}
pub(super) fn is_downloaded(dir: &Path, id: &str) -> bool {
    artifact(id).is_some_and(|a| ready(a, dir))
}
pub(super) fn percent(dir: &Path, id: &str) -> Option<u8> {
    let a = artifact(id)?;
    if ready(a, dir) {
        return Some(100);
    }
    let blobs = dir.join(format!("models--{}/blobs", a.repo.replace('/', "--")));
    let have: u64 = fs::read_dir(blobs)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum();
    Some((u128::from(have) * 100 / u128::from(a.bytes)).min(99) as u8)
}
fn verify(a: &Artifact, path: &Path) -> Result<(), EmbedError> {
    if signature(a, path).is_none() {
        return Err(EmbedError::Files("GGUF: неверная длина файла".into()));
    }
    let mut file = fs::File::open(path).map_err(load_error)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer).map_err(load_error)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    if format!("{:x}", hash.finalize()) != a.sha256 {
        return Err(EmbedError::Files("GGUF: SHA-256 не совпал".into()));
    }
    Ok(())
}
fn load_error(e: impl std::fmt::Display) -> EmbedError {
    EmbedError::Load(e.to_string())
}
fn mark(a: &Artifact, dir: &Path, path: &Path) -> Result<(), EmbedError> {
    use std::io::Write;
    let sig = signature(a, path).ok_or_else(|| load_error("GGUF изменился во время проверки"))?;
    let target = a.marker(dir);
    fs::create_dir_all(target.parent().unwrap()).map_err(load_error)?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(load_error)?
        .as_nanos();
    let temp = target.with_extension(format!("{}-{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(load_error)?;
        file.write_all(&serde_json::to_vec(&sig).map_err(load_error)?)
            .map_err(load_error)?;
        file.sync_all().map_err(load_error)?;
        drop(file);
        fs::rename(&temp, &target).map_err(load_error)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
pub(super) fn ensure(dir: &Path, id: &str) -> Result<PathBuf, EmbedError> {
    let a = artifact(id).ok_or_else(|| load_error("неизвестная GGUF-модель"))?;
    if ready(a, dir) {
        return a
            .path(dir)
            .ok_or_else(|| EmbedError::Files("GGUF исчез из кэша".into()));
    }
    let path = if let Some(path) = a.path(dir) {
        path
    } else {
        ApiBuilder::from_cache(Cache::new(dir.into()))
            .with_token(None)
            .with_progress(false)
            .with_retries(3)
            .build()
            .map_err(load_error)?
            .repo(a.repo())
            .get(a.file)
            .map_err(super::failure::hub_embed_error)?
    };
    // Даже полный файл не считается готовым до проверки опубликованного хеша.
    let before = signature(a, &path);
    verify(a, &path)?;
    if before != signature(a, &path) {
        return Err(load_error("GGUF изменился во время хеширования"));
    }
    mark(a, dir, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOY: Artifact = Artifact {
        repo: "test/gguf",
        revision: "pinned",
        file: "model.gguf",
        bytes: 3,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    };
    #[test]
    fn readiness_needs_hash_and_changed_file_invalidates_marker() {
        let dir = tempfile::tempdir().unwrap();
        let repo = Cache::new(dir.path().into()).repo(TOY.repo());
        repo.create_ref(TOY.revision).unwrap();
        let path = repo.pointer_path(TOY.revision).join(TOY.file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"abc").unwrap();
        assert!(!ready(&TOY, dir.path()));
        verify(&TOY, &path).unwrap();
        mark(&TOY, dir.path(), &path).unwrap();
        assert!(ready(&TOY, dir.path()));
        mark(&TOY, dir.path(), &path).unwrap();
        fs::write(&path, b"truncated").unwrap();
        assert!(!ready(&TOY, dir.path()));
        assert!(verify(&TOY, &path).is_err());
        fs::write(&path, b"bad").unwrap();
        assert!(verify(&TOY, &path).is_err());
    }
}
