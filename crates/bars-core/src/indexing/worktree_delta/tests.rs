use std::cell::Cell;
use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use super::*;
use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::{NoObserver, update_index};
use crate::storage::index_path;

const DIM: usize = 8;

/// Детерминированная подделка модели со счётчиками вызовов.
struct FakeEmbedder {
    fingerprint: ModelFingerprint,
    calls: usize,
    vectors_computed: usize,
}

impl FakeEmbedder {
    fn new(model: &str) -> Self {
        Self {
            fingerprint: ModelFingerprint {
                source: "builtin".into(),
                model: model.into(),
                dim: DIM,
                doc_prefix: String::new(),
            },
            calls: 0,
            vectors_computed: 0,
        }
    }
}

impl Embedder for FakeEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.calls += 1;
        self.vectors_computed += texts.len();
        Ok(texts
            .iter()
            .map(|text| {
                let hash = blake3::hash(text.as_bytes());
                (0..DIM)
                    .map(|i| f32::from(hash.as_bytes()[i % 32]) + 1.0)
                    .collect()
            })
            .collect())
    }
}

fn settings() -> IndexSettings {
    IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    }
}

thread_local! {
    static TICKS: Cell<u64> = const { Cell::new(0) };
}

fn write(root: &Path, relative: &str, content: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, content).unwrap();
    // Время изменения задаётся явно: правки в одну долю секунды не должны пройти по размеру и stat.
    let file = fs::File::options().write(true).open(&path).unwrap();
    let ticks = TICKS.with(|ticks| {
        ticks.set(ticks.get() + 1);
        ticks.get()
    });
    file.set_modified(UNIX_EPOCH + Duration::from_secs(1_000_000 + ticks))
        .unwrap();
}

fn section(title: &str, word: &str) -> String {
    format!("## {title}\n\n{}\n\n", format!("{word} ").repeat(60))
}

fn doc(title: &str, words: &[&str]) -> String {
    let mut text = format!("# {title}\n\n");
    for (i, word) in words.iter().enumerate() {
        text.push_str(&section(&format!("Раздел {i}"), word));
    }
    text
}

/// Владелец с готовым индексом и копия его документов в `wt` (корень вида).
struct Fixture {
    _dir: tempfile::TempDir,
    owner: PathBuf,
    view: PathBuf,
    index: Index,
}

fn fixture(files: &[(&str, String)]) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let owner = dir.path().join("owner");
    let view = dir.path().join("wt");
    for (path, text) in files {
        write(&owner, path, text);
        write(&view, path, text);
    }
    let mut index = Index::open(&index_path(&owner), Journal::Wal).unwrap();
    let mut model = FakeEmbedder::new("m");
    update_index(&owner, &mut index, &settings(), &mut model).unwrap();
    Fixture {
        _dir: dir,
        owner,
        view,
        index,
    }
}

fn run(
    fixture: &Fixture,
    delta: &mut Index,
    model: &mut FakeEmbedder,
    scope: SyncScope<'_>,
) -> SyncReport {
    let settings = settings();
    sync(
        delta,
        &IndexOwner(&fixture.index),
        model,
        &SyncRequest {
            view_root: &fixture.view,
            settings: &settings,
            excluded: &[],
            scope,
            name: "wt",
        },
        &NoObserver,
    )
    .unwrap()
}

fn paths(delta: &Index) -> Vec<String> {
    delta
        .documents()
        .unwrap()
        .into_iter()
        .map(|document| document.path)
        .collect()
}

fn view_paths(delta: &Index) -> Vec<String> {
    view_files(delta)
        .unwrap()
        .into_iter()
        .map(|document| document.path)
        .collect()
}

#[test]
fn delta_of_identical_worktree_has_no_documents_and_no_embed() {
    let fixture = fixture(&[
        ("docs/a.md", doc("A", &["alpha", "beta"])),
        ("docs/b.md", doc("B", &["gamma"])),
    ]);
    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    let mut model = FakeEmbedder::new("m");
    let report = run(&fixture, &mut delta, &mut model, SyncScope::All);

    assert_eq!(delta.document_count().unwrap(), 0);
    assert_eq!(delta.chunk_count().unwrap(), 0);
    assert_eq!(delta.vector_count().unwrap(), 0);
    assert_eq!(model.calls, 0);
    assert_eq!(report.written, 0);
    assert_eq!(report.view_documents, 2);
    assert_eq!(view_files(&delta).unwrap().len(), 2);
    assert!(is_ready(&delta).unwrap());
    let (digest, synced_at) = stored_sync(&delta).unwrap();
    assert_eq!(digest.unwrap(), owner_digest(&fixture.index).unwrap());
    assert!(synced_at.is_some());
    assert!(
        delta_path(&fixture.owner, "wt").is_file(),
        "файл дельты лежит в .bars/worktrees/<имя>"
    );

    // Повторная сверка не читает файлы: размер и время совпали с `wt_files`.
    let again = run(&fixture, &mut delta, &mut model, SyncScope::All);
    assert_eq!(again.hashed, 0);
    assert_eq!(again.skipped_by_stat, 2);
}

#[test]
fn one_section_edit_embeds_only_that_section() {
    let old = doc("A", &["alpha", "beta", "gamma"]);
    let new = doc("A", &["alpha", "omega", "gamma"]);
    let fixture = fixture(&[("docs/a.md", old.clone()), ("docs/b.md", doc("B", &["x"]))]);
    write(&fixture.view, "docs/a.md", &new);

    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    let mut model = FakeEmbedder::new("m");
    let report = run(&fixture, &mut delta, &mut model, SyncScope::All);

    assert_eq!(paths(&delta), vec!["docs/a.md".to_string()]);
    let params = ChunkingParams::default();
    let old_hashes: HashSet<String> = chunk_document("docs/a.md", &old, &params)
        .chunks
        .into_iter()
        .map(|chunk| chunk.embed_hash)
        .collect();
    let new_hashes: HashSet<String> = chunk_document("docs/a.md", &new, &params)
        .chunks
        .into_iter()
        .map(|chunk| chunk.embed_hash)
        .collect();
    let changed = new_hashes.difference(&old_hashes).count();
    assert!(changed >= 1 && changed < new_hashes.len());
    assert_eq!(report.chunks_embedded, changed);
    assert_eq!(model.vectors_computed, changed);
    assert_eq!(report.chunks_copied, new_hashes.len() - changed);
    // Вектор неизменившегося раздела — тот же, что у владельца.
    let shared = new_hashes.intersection(&old_hashes).next().unwrap();
    assert_eq!(
        delta.vector(shared).unwrap(),
        fixture.index.vector(shared).unwrap()
    );
    assert_eq!(report.removed, 0);
}

#[test]
fn delta_hash_matches_indexer_hash() {
    // BOM и CRLF: хеш считается по тексту так же, как у индексатора, и вид совпадает с владельцем.
    let text = "\u{feff}# T\r\n\r\nтекст раздела\r\n".to_string();
    let fixture = fixture(&[("docs/b.md", text.clone())]);
    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    let mut model = FakeEmbedder::new("m");
    run(&fixture, &mut delta, &mut model, SyncScope::All);

    let owner_hash = fixture.index.documents().unwrap().remove(0).hash;
    let view_hash = view_files(&delta).unwrap().remove(0).hash;
    assert_eq!(view_hash, owner_hash);
    assert_eq!(view_hash, content_hash(&text));
    assert_eq!(delta.document_count().unwrap(), 0);
}

#[test]
fn edit_created_and_removed_documents_and_return_to_owner_version() {
    let a = doc("A", &["alpha"]);
    let fixture = fixture(&[("docs/a.md", a.clone()), ("docs/b.md", doc("B", &["beta"]))]);
    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    let mut model = FakeEmbedder::new("m");

    write(&fixture.view, "docs/a.md", &doc("A", &["alpha", "more"]));
    write(&fixture.view, "docs/new.md", &doc("New", &["fresh"]));
    fs::remove_file(fixture.view.join("docs/b.md")).unwrap();
    let report = run(&fixture, &mut delta, &mut model, SyncScope::All);
    assert_eq!(
        paths(&delta),
        vec!["docs/a.md".to_string(), "docs/new.md".to_string()]
    );
    assert_eq!(report.written, 2);
    assert_eq!(view_paths(&delta), vec!["docs/a.md", "docs/new.md"]);

    // Документ снова равен владельцу: из дельты уходит, остаётся в `wt_files`.
    write(&fixture.view, "docs/a.md", &a);
    let only: HashSet<String> = ["docs/a.md".to_string()].into();
    let report = run(&fixture, &mut delta, &mut model, SyncScope::Paths(&only));
    assert_eq!(report.removed, 1);
    assert_eq!(paths(&delta), vec!["docs/new.md".to_string()]);
    // Сверка по путям не трогает остальное: `new.md` на месте, `b.md` по-прежнему не в виде.
    assert_eq!(view_paths(&delta), vec!["docs/a.md", "docs/new.md"]);
}

/// Владелец, который «подменился» между снимками: после первого снимка он уже другой.
struct SwappingOwner<'a> {
    inner: IndexOwner<'a>,
    snapshots: Cell<usize>,
    swapped_path: String,
}

impl OwnerView for SwappingOwner<'_> {
    fn snapshot(&self) -> Result<OwnerSnapshot, DeltaError> {
        let call = self.snapshots.get();
        self.snapshots.set(call + 1);
        let mut snapshot = self.inner.snapshot()?;
        if call >= 1 {
            snapshot
                .hashes
                .insert(self.swapped_path.clone(), "новый хеш".into());
            let mut pairs: Vec<_> = snapshot.hashes.clone().into_iter().collect();
            pairs.sort();
            snapshot.digest = digest_of(&pairs);
        }
        Ok(snapshot)
    }

    fn vectors(&self, hashes: &[&str]) -> Result<HashMap<String, Vec<f32>>, DeltaError> {
        self.inner.vectors(hashes)
    }
}

#[test]
fn owner_swap_triggers_recheck_without_embed() {
    let fixture = fixture(&[("docs/a.md", doc("A", &["alpha", "beta"]))]);
    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    let mut model = FakeEmbedder::new("m");
    let owner = SwappingOwner {
        inner: IndexOwner(&fixture.index),
        snapshots: Cell::new(0),
        swapped_path: "docs/a.md".into(),
    };
    let settings = settings();
    let report = sync(
        &mut delta,
        &owner,
        &mut model,
        &SyncRequest {
            view_root: &fixture.view,
            settings: &settings,
            excluded: &[],
            scope: SyncScope::All,
            name: "wt",
        },
        &NoObserver,
    )
    .unwrap();

    assert_eq!(report.restarts, 1, "подмена владельца — сверка заново");
    // Владелец ушёл вперёд: документ теперь отличается и пишется в дельту, но векторы чанков
    // берутся у владельца: модель не вызывалась.
    assert_eq!(paths(&delta), vec!["docs/a.md".to_string()]);
    assert_eq!(model.calls, 0);
    assert!(report.chunks_copied > 0);
    let (digest, _) = stored_sync(&delta).unwrap();
    assert_eq!(digest.unwrap(), report.owner_digest);
}

#[test]
fn check_delta_tells_fresh_compatible_and_mismatch() {
    let fixture = fixture(&[("docs/a.md", doc("A", &["alpha"]))]);
    let snapshot = OwnerSnapshot::read(&fixture.index).unwrap();
    let mut delta = open_delta(&fixture.owner, "wt").unwrap();
    assert_eq!(
        check_delta(&delta, &snapshot, &settings()).unwrap(),
        DeltaCheck::Fresh
    );
    let mut model = FakeEmbedder::new("m");
    run(&fixture, &mut delta, &mut model, SyncScope::All);
    assert_eq!(
        check_delta(&delta, &snapshot, &settings()).unwrap(),
        DeltaCheck::Compatible
    );

    // Другая модель владельца.
    let mut other = snapshot.clone();
    other.fingerprint.as_mut().unwrap().model = "другая".into();
    assert!(matches!(
        check_delta(&delta, &other, &settings()).unwrap(),
        DeltaCheck::Mismatch(_)
    ));
    // Другая нарезка и другой корень документации.
    let mut chunking = settings();
    chunking.chunking.max_chars += 1;
    assert!(matches!(
        check_delta(&delta, &snapshot, &chunking).unwrap(),
        DeltaCheck::Mismatch(_)
    ));
    let mut docs_root = settings();
    docs_root.docs_root = "other".into();
    assert!(matches!(
        check_delta(&delta, &snapshot, &docs_root).unwrap(),
        DeltaCheck::Mismatch(_)
    ));
    // Формат новее известного.
    delta.set_meta("delta_version", "999").unwrap();
    assert!(matches!(
        check_delta(&delta, &snapshot, &settings()).unwrap(),
        DeltaCheck::Mismatch(_)
    ));
}
