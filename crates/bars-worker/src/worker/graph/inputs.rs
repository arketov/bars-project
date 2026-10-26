//! Устойчивый ключ входов и отметки остановки/повтора; не часть публичного статуса.
use super::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::io::Read;

pub(super) fn get(index: &Index, key: &str) -> Result<Option<String>, GraphError> {
    Ok(index
        .conn()
        .query_row("SELECT value FROM graph_meta WHERE key=?1", [key], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
        .filter(|v| !v.is_empty()))
}
pub(super) fn put(index: &Index, key: &str, value: &str) -> Result<(), GraphError> {
    index.conn().execute("INSERT INTO graph_meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE value<>excluded.value", [key, value])?;
    Ok(())
}
pub(super) fn key(project: &Path, index: &Index) -> Result<String, GraphError> {
    let tx = index.conn().unchecked_transaction()?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"graph-auto-v1");
    hash.update(bars_core::graph::METHOD_VERSION.as_bytes());
    hash.update(bars_core::graph::similarity_job::METHOD.as_bytes());
    hash.update(&serde_json::to_vec(&store::document_manifest(&tx)?)?);
    let mut stmt = tx.prepare(
        "SELECT key,value FROM meta WHERE key LIKE 'model_%' OR key IN ('docs_root','chunk_min_chars','chunk_max_chars') ORDER BY key",
    )?;
    let meta = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    hash.update(&serde_json::to_vec(&meta)?);
    drop(stmt);
    let mut stmt = tx.prepare("SELECT DISTINCT v.embed_hash FROM vectors v CROSS JOIN chunks c ON c.embed_hash=v.embed_hash ORDER BY v.embed_hash")?;
    let vectors = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    hash.update(&serde_json::to_vec(&vectors)?);
    drop(stmt);
    let exists: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='graph_aux')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let failed: Option<String> = tx
            .query_row(
                "SELECT data FROM graph_aux WHERE key='failed_sources'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        let root: Option<String> = tx
            .query_row(
                "SELECT data FROM graph_aux WHERE key='docs_root'",
                [],
                |r| r.get(0),
            )
            .optional()?;
        if let (Some(failed), Some(root)) = (failed, root) {
            let root: String = serde_json::from_str(&root)?;
            let mut sources: Vec<bars_core::graph::SourceMeta> = serde_json::from_str(&failed)?;
            sources.sort_by(|a, b| a.docs_path.cmp(&b.docs_path));
            for source in sources {
                hash.update(source.docs_path.as_bytes());
                let path = project.join(&root).join(&source.docs_path);
                hash.update(&serde_json::to_vec(&signature(
                    &path,
                    &project.join(&root),
                ))?);
            }
        }
    }
    tx.commit()?;
    Ok(hash.finalize().to_hex().to_string())
}
// Читаются только ранее исключённые файлы, с тем же пределом, что у построения.
fn signature(path: &Path, docs_root: &Path) -> String {
    let read = || -> Result<String, io::Error> {
        let physical = path.canonicalize()?;
        if !physical.starts_with(docs_root.canonicalize()?) {
            return Ok("outside_docs_root".into());
        }
        let file = fs::File::open(physical)?;
        let meta = file.metadata()?;
        if meta.len() > 16 * 1024 * 1024 {
            return Ok(format!(
                "too_large:{}:{:?}",
                meta.len(),
                meta.modified().ok()
            ));
        }
        let mut bytes = Vec::new();
        file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
        Ok(blake3::hash(&bytes).to_hex().to_string())
    };
    read().unwrap_or_else(|e| format!("unreadable:{:?}", e.kind()))
}
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Retry {
    failures: u32,
    after_ms: u64,
}
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
impl Retry {
    pub(super) fn remaining(&self) -> Option<Duration> {
        let ms = self.after_ms.saturating_sub(now_ms());
        (ms > 0).then(|| Duration::from_millis(ms.min(300_000)))
    }
    pub(super) fn failed(&mut self) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let delay = match self.failures {
            1 => 30,
            2 => 60,
            _ => 300,
        };
        self.after_ms = now_ms() + delay * 1000;
        Duration::from_secs(delay)
    }
}
pub(super) fn retry(index: &Index, key: &str) -> Result<Retry, GraphError> {
    if let Some(value) = get(index, "auto_retry")? {
        let (target, retry): (String, Retry) = serde_json::from_str(&value)?;
        if target == key {
            return Ok(retry);
        }
    }
    Ok(Retry::default())
}
pub(super) fn save_retry(index: &Index, key: &str, retry: &Retry) -> Result<(), GraphError> {
    put(index, "auto_retry", &serde_json::to_string(&(key, retry))?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_file_probe_stays_inside_document_root() {
        let folder = tempfile::tempdir().unwrap();
        let docs = folder.path().join("docs");
        fs::create_dir(&docs).unwrap();
        let outside = folder.path().join("outside.md");
        fs::write(&outside, "Вне области графа").unwrap();
        assert_eq!(signature(&outside, &docs), "outside_docs_root");
        let inside = docs.join("inside.md");
        fs::write(&inside, [0xff, 0xfe]).unwrap();
        assert_eq!(
            signature(&inside, &docs),
            blake3::hash(&[0xff, 0xfe]).to_hex().to_string()
        );
    }
    #[test]
    fn retry_is_bounded_and_grows() {
        let mut retry = Retry::default();
        assert!(retry.remaining().is_none());
        assert_eq!(retry.failed(), Duration::from_secs(30));
        assert_eq!(retry.failed(), Duration::from_secs(60));
        for _ in 0..5 {
            assert_eq!(retry.failed(), Duration::from_secs(300));
        }
        assert!(retry.remaining().unwrap() <= Duration::from_secs(300));
    }
}
