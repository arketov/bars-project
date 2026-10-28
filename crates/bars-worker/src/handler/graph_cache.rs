//! Общие проверенные снимки; замок каталога не удерживается во время SQLite/validate.
use bars_core::graph::{Dataset, GraphError, store};
use bars_core::storage::index_path;
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicU64, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

const RETAINED_BYTES: usize = 256 * 1024 * 1024;
const MAX_PROJECTS: usize = 8;

#[derive(Default)]
pub(crate) struct GraphCache {
    slots: Mutex<BTreeMap<PathBuf, Arc<Slot>>>,
    clock: AtomicU64,
}

#[derive(Default)]
struct Slot {
    entry: Mutex<Option<Entry>>,
    used: AtomicU64,
    bytes: AtomicUsize,
}

struct Entry {
    identity: FileIdentity,
    gate: Weak<RwLock<()>>,
    version: Option<String>,
    data: Option<Arc<Dataset>>,
}

#[derive(PartialEq, Eq)]
struct FileIdentity(u64, u64);

fn identity(path: &Path) -> Result<FileIdentity, GraphError> {
    let file = std::fs::File::open(path).map_err(|e| GraphError::InvalidData(e.to_string()))?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
        };
        let mut info = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
        // Валидный открытый handle и выходной буфер; владение handle остаётся у File.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
            return Err(GraphError::InvalidData(
                std::io::Error::last_os_error().to_string(),
            ));
        }
        let info = unsafe { info.assume_init() };
        Ok(FileIdentity(
            u64::from(info.dwVolumeSerialNumber),
            (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        ))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file
            .metadata()
            .map_err(|e| GraphError::InvalidData(e.to_string()))?;
        Ok(FileIdentity(meta.dev(), meta.ino()))
    }
}

pub(super) fn check(deadline: Instant) -> Result<(), GraphError> {
    if Instant::now() >= deadline {
        Err(GraphError::TimedOut)
    } else {
        Ok(())
    }
}

impl GraphCache {
    pub(crate) fn clear(&self) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
    pub(crate) fn remove(&self, project: &Path) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(project);
    }
    pub(crate) fn load(
        &self,
        project: &Path,
        gate: &Arc<RwLock<()>>,
        deadline: Instant,
    ) -> Result<Arc<Dataset>, GraphError> {
        check(deadline)?;
        let slot = {
            let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
            slots.entry(project.to_path_buf()).or_default().clone()
        };
        slot.used.store(
            self.clock.fetch_add(1, Ordering::Relaxed),
            Ordering::Relaxed,
        );
        let result = self.load_slot(&slot, project, gate, deadline);
        self.trim();
        result
    }

    fn load_slot(
        &self,
        slot: &Slot,
        project: &Path,
        gate: &Arc<RwLock<()>>,
        deadline: Instant,
    ) -> Result<Arc<Dataset>, GraphError> {
        let mut entry = loop {
            check(deadline)?;
            match slot.entry.try_lock() {
                Ok(entry) => break entry,
                Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
                Err(std::sync::TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(1))
                }
            }
        };
        let path = index_path(project);
        let file = identity(&path)?;
        if entry
            .as_ref()
            .is_none_or(|e| e.identity != file || !Weak::ptr_eq(&e.gate, &Arc::downgrade(gate)))
        {
            *entry = None;
            slot.bytes.store(0, Ordering::Relaxed);
            *entry = Some(Entry {
                identity: file,
                gate: Arc::downgrade(gate),
                version: None,
                data: None,
            });
        }
        let entry = entry.as_mut().expect("initialized cache slot");
        let db = Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.busy_timeout(deadline.saturating_duration_since(Instant::now()))?;
        let stamp = || -> Result<Option<String>, rusqlite::Error> {
            db.query_row("SELECT (SELECT value FROM graph_meta WHERE key='read_epoch') || ':' || (SELECT value FROM graph_meta WHERE key='read_revision') || ':' || (SELECT schema_version FROM pragma_schema_version)", [], |r| r.get(0))
        };
        // Старый индекс без метки не кэшируется; обычная загрузка проверяет его схему.
        let version: Option<String> = stamp().unwrap_or_default();
        if version.is_some()
            && version == entry.version
            && let Some(data) = &entry.data
        {
            check(deadline)?;
            return Ok(data.clone());
        }
        entry.data = None;
        slot.bytes.store(0, Ordering::Relaxed);
        let (data, bytes) = store::load_previous_sized_until(&db, deadline)?;
        let data = Arc::new(data);
        check(deadline)?;
        let after = stamp()?;
        // Изменения во время загрузки не привязываются к более новой ревизии.
        if version.is_some() && version == after && bytes <= RETAINED_BYTES {
            entry.version = version;
            entry.data = Some(data.clone());
            slot.bytes.store(bytes, Ordering::Relaxed);
        }
        Ok(data)
    }

    fn trim(&self) {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        while slots.len() > MAX_PROJECTS
            || slots
                .values()
                .map(|s| s.bytes.load(Ordering::Relaxed))
                .sum::<usize>()
                > RETAINED_BYTES
        {
            let Some(oldest) = slots
                .iter()
                // Активный Slot остаётся общим для загрузчика и ожидающих читателей.
                .filter(|(_, s)| Arc::strong_count(s) == 1)
                .min_by_key(|(_, s)| s.used.load(Ordering::Relaxed))
                .map(|(p, _)| p.clone())
            else {
                break;
            };
            slots.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bars_core::graph::source::{InputDocument, build};

    fn fixture(project: &Path, text: &str) -> Connection {
        std::fs::create_dir_all(project.join(".bars")).unwrap();
        let mut db = Connection::open(index_path(project)).unwrap();
        db.execute_batch("CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); INSERT INTO meta VALUES('schema_version','1'); CREATE TABLE documents(path TEXT PRIMARY KEY,hash TEXT);").unwrap();
        let data = build(
            "test",
            "docs",
            vec![InputDocument {
                path: "docs/a.md".into(),
                docs_path: "a.md".into(),
                module: ".".into(),
                title: "A".into(),
                text: text.into(),
                chunks: vec![],
            }],
        )
        .unwrap();
        for source in &data.sources {
            db.execute(
                "INSERT INTO documents VALUES(?1,?2)",
                [&source.meta.path, &source.meta.hash],
            )
            .unwrap();
        }
        store::ensure_schema(&mut db).unwrap();
        let manifest = store::document_manifest(&db).unwrap();
        store::publish(&mut db, &data, &manifest).unwrap();
        db
    }

    fn load(cache: &GraphCache, project: &Path, gate: &Arc<RwLock<()>>) -> Arc<Dataset> {
        cache
            .load(project, gate, Instant::now() + Duration::from_secs(5))
            .unwrap()
    }

    #[test]
    fn shared_snapshot_invalidates_on_status_sources_and_corruption() {
        let project = tempfile::tempdir().unwrap();
        let db = fixture(project.path(), "# A\n\nТ-1 зависит от Т-2.");
        let cache = GraphCache::default();
        let gate = Arc::new(RwLock::new(()));
        let first = load(&cache, project.path(), &gate);
        assert!(Arc::ptr_eq(&first, &load(&cache, project.path(), &gate)));
        store::set_state(&db, bars_core::graph::LayerState::Cancelled, None, None).unwrap();
        let stopped = load(&cache, project.path(), &gate);
        assert!(!Arc::ptr_eq(&first, &stopped));
        assert!(
            stopped
                .warnings
                .iter()
                .any(|s| s.contains("останов") || s.contains("Останов"))
        );
        db.execute("UPDATE documents SET hash='changed'", [])
            .unwrap();
        let stale = load(&cache, project.path(), &gate);
        assert!(stale.warnings.iter().any(|s| s.contains("устарел")));
        db.execute("UPDATE graph_evidence SET data='invalid json'", [])
            .unwrap();
        assert!(
            cache
                .load(
                    project.path(),
                    &gate,
                    Instant::now() + Duration::from_secs(5)
                )
                .is_err()
        );
    }

    #[test]
    fn replacement_and_registration_do_not_reuse_old_snapshot_or_lock_file() {
        let project = tempfile::tempdir().unwrap();
        drop(fixture(project.path(), "# Первый\n\nПервый текст."));
        let cache = GraphCache::default();
        let gate = Arc::new(RwLock::new(()));
        let first = load(&cache, project.path(), &gate);
        let next_gate = Arc::new(RwLock::new(()));
        assert!(!Arc::ptr_eq(
            &first,
            &load(&cache, project.path(), &next_gate)
        ));
        std::fs::rename(
            index_path(project.path()),
            project.path().join(".bars/old.sqlite"),
        )
        .unwrap();
        drop(fixture(project.path(), "# Второй\n\nВторой текст."));
        let second = load(&cache, project.path(), &next_gate);
        assert_ne!(first.snapshot, second.snapshot);
        assert!(second.sources[0].text.contains("Второй"));
        cache.remove(project.path());
        assert!(cache.slots.lock().unwrap().is_empty());
    }

    #[test]
    fn deadline_and_concurrent_readers_share_one_snapshot() {
        let project = tempfile::tempdir().unwrap();
        drop(fixture(project.path(), "# A\n\nТ-1 зависит от Т-2."));
        let cache = GraphCache::default();
        let gate = Arc::new(RwLock::new(()));
        assert!(matches!(
            cache.load(project.path(), &gate, Instant::now()),
            Err(GraphError::TimedOut)
        ));
        let snapshots = std::thread::scope(|scope| {
            let a = scope.spawn(|| load(&cache, project.path(), &gate));
            let b = scope.spawn(|| load(&cache, project.path(), &gate));
            (a.join().unwrap(), b.join().unwrap())
        });
        assert!(Arc::ptr_eq(&snapshots.0, &snapshots.1));
    }

    #[test]
    fn waiting_reader_times_out_without_discarding_shared_snapshot() {
        let project = tempfile::tempdir().unwrap();
        drop(fixture(project.path(), "# A\n\nТекст."));
        let cache = GraphCache::default();
        let gate = Arc::new(RwLock::new(()));
        let first = load(&cache, project.path(), &gate);
        let slot = cache.slots.lock().unwrap()[project.path()].clone();
        let held = slot.entry.lock().unwrap();
        assert!(matches!(
            cache.load(
                project.path(),
                &gate,
                Instant::now() + Duration::from_millis(10)
            ),
            Err(GraphError::TimedOut)
        ));
        drop(held);
        assert!(Arc::ptr_eq(&first, &load(&cache, project.path(), &gate)));
    }

    #[test]
    fn lru_evicts_idle_slots_without_splitting_active_loads() {
        let cache = GraphCache::default();
        let active = Arc::new(Slot::default());
        active.bytes.store(RETAINED_BYTES, Ordering::Relaxed);
        let idle = Arc::new(Slot::default());
        idle.used.store(1, Ordering::Relaxed);
        idle.bytes.store(1, Ordering::Relaxed);
        {
            let mut slots = cache.slots.lock().unwrap();
            slots.insert(PathBuf::from("active"), active.clone());
            slots.insert(PathBuf::from("idle"), idle);
        }
        cache.trim();
        assert!(Arc::ptr_eq(
            &cache.slots.lock().unwrap()[Path::new("active")],
            &active
        ));
        assert!(!cache.slots.lock().unwrap().contains_key(Path::new("idle")));
        // Превышение числа проектов снимается после освобождения активных записей.
        for i in 0..MAX_PROJECTS {
            let slot = Arc::new(Slot::default());
            slot.used.store(i as u64 + 2, Ordering::Relaxed);
            cache
                .slots
                .lock()
                .unwrap()
                .insert(PathBuf::from(format!("project-{i}")), slot);
        }
        cache.trim();
        assert_eq!(cache.slots.lock().unwrap().len(), MAX_PROJECTS);
        assert!(
            cache
                .slots
                .lock()
                .unwrap()
                .contains_key(Path::new("active"))
        );
        assert!(
            !cache
                .slots
                .lock()
                .unwrap()
                .contains_key(Path::new("project-0"))
        );
        drop(active);
        cache
            .slots
            .lock()
            .unwrap()
            .get(Path::new("project-1"))
            .unwrap()
            .bytes
            .store(1, Ordering::Relaxed);
        cache.trim();
        assert!(
            !cache
                .slots
                .lock()
                .unwrap()
                .contains_key(Path::new("active"))
        );
    }

    #[test]
    #[ignore = "явный замер на согласованном срезе индекса, без модели и рабочего профиля"]
    fn frozen_graph_resource_probe() {
        let source_path = std::env::var_os("BARS_RESOURCE_INDEX").expect("BARS_RESOURCE_INDEX");
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let output = workspace.join("docs/chats/2026-10-25-performance-audit");
        let fixture = tempfile::Builder::new()
            .prefix("resource-probe-")
            .tempdir_in(&output)
            .unwrap();
        std::fs::create_dir(fixture.path().join(".bars")).unwrap();
        let source =
            Connection::open_with_flags(source_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .unwrap();
        source
            .execute(
                "VACUUM INTO ?1",
                [index_path(fixture.path()).to_str().unwrap()],
            )
            .unwrap();
        drop(source);
        let mut db = Connection::open(index_path(fixture.path())).unwrap();
        store::ensure_schema(&mut db).unwrap();
        let mut direct = Vec::new();
        let mut expected = String::new();
        let mut counts = (0, 0, 0);
        for _ in 0..3 {
            let started = Instant::now();
            let data = store::load_previous(&db).unwrap();
            direct.push(started.elapsed().as_secs_f64() * 1000.0);
            let hash = blake3::hash(&serde_json::to_vec(&data).unwrap())
                .to_hex()
                .to_string();
            if expected.is_empty() {
                expected = hash.clone();
            } else {
                assert_eq!(expected, hash);
            }
            counts = (data.sources.len(), data.nodes.len(), data.edges.len());
        }
        drop(db);
        let cache = GraphCache::default();
        let gate = Arc::new(RwLock::new(()));
        let started = Instant::now();
        let first = cache
            .load(
                fixture.path(),
                &gate,
                Instant::now() + Duration::from_secs(60),
            )
            .unwrap();
        let cold_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(
            expected,
            blake3::hash(&serde_json::to_vec(&*first).unwrap())
                .to_hex()
                .as_str()
        );
        let mut cached = Vec::new();
        for _ in 0..10 {
            let started = Instant::now();
            let next = cache
                .load(
                    fixture.path(),
                    &gate,
                    Instant::now() + Duration::from_secs(5),
                )
                .unwrap();
            cached.push(started.elapsed().as_secs_f64() * 1000.0);
            assert!(Arc::ptr_eq(&first, &next));
        }
        let bytes: usize = cache
            .slots
            .lock()
            .unwrap()
            .values()
            .map(|s| s.bytes.load(Ordering::Relaxed))
            .sum();
        let report = serde_json::json!({ "scope":"frozen project graph; shared machine; no inference; elapsed time only", "source_documents":counts.0,"nodes":counts.1,"edges":counts.2,"direct_load_ms":direct,"cold_cache_ms":cold_ms,"repeated_cache_ms":cached,"estimated_retained_bytes":bytes,"dataset_blake3":expected,"identical_datasets":true });
        std::fs::write(
            output.join("graph-cache-resources.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .unwrap();
        println!("{report}");
    }
}
