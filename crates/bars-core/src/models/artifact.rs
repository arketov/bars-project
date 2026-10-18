//! Общая потоковая проверка закреплённых файлов моделей.
use std::{fs::File, path::Path};

#[derive(Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Stamp {
    path: std::path::PathBuf,
    bytes: u64,
    modified_ns: u128,
    blake3: String,
}

pub(super) fn stamp(path: &Path, expected: &str, size: Option<u64>) -> Option<Stamp> {
    let meta = path.metadata().ok()?;
    if !meta.is_file() || meta.len() == 0 || size.is_some_and(|n| n != meta.len()) {
        return None;
    }
    Some(Stamp {
        path: path.into(),
        bytes: meta.len(),
        modified_ns: meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos(),
        blake3: expected.into(),
    })
}

pub(super) fn marked(marker: &Path, stamps: &[Stamp]) -> bool {
    std::fs::read(marker)
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Stamp>>(&b).ok())
        .is_some_and(|old| old == stamps)
}

pub(super) fn mark(marker: &Path, stamps: &[Stamp]) -> Result<(), String> {
    use std::io::Write;
    let parent = marker.parent().ok_or("нет каталога отметки кэша")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = marker.with_extension(format!("{}.{nonce}.tmp", std::process::id()));
    let result = (|| -> Result<(), String> {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(stamps).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        std::fs::rename(&temp, marker).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

pub(super) fn verify(path: &Path, expected: &str) -> Result<(), String> {
    let mut hash = blake3::Hasher::new();
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut file, &mut hash).map_err(|e| e.to_string())?;
    if hash.finalize().to_hex().as_str() != expected {
        return Err(format!("BLAKE3 не совпал: {}", path.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn marker_requires_matching_files_hash_size_and_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("model");
        let marker = dir.path().join("ready.json");
        std::fs::write(&file, b"weights").unwrap();
        let stamps = [stamp(&file, "hash", Some(7)).unwrap()];
        assert!(!marked(&marker, &stamps));
        mark(&marker, &stamps).unwrap();
        assert!(marked(&marker, &stamps));
        assert!(!marked(
            &marker,
            &[stamp(&file, "other hash", None).unwrap()]
        ));
        assert!(stamp(&file, "hash", Some(8)).is_none());
        std::fs::write(&file, b"modified weights").unwrap();
        assert!(!marked(&marker, &[stamp(&file, "hash", None).unwrap()]));
        std::fs::write(&marker, b"broken").unwrap();
        assert!(!marked(&marker, &stamps));
    }
    #[test]
    fn rejects_changed_and_missing_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("artifact");
        std::fs::write(&file, b"weights").unwrap();
        let expected = blake3::hash(b"weights").to_hex().to_string();
        verify(&file, &expected).unwrap();
        std::fs::write(&file, b"changed").unwrap();
        assert!(verify(&file, &expected).unwrap_err().contains("BLAKE3"));
        assert!(verify(&dir.path().join("missing"), &expected).is_err());
    }
}
