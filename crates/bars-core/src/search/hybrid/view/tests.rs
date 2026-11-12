//! Поиск по виду worktree: владелец и дельта в одном пуле (`docs/features/projects/worktrees.md`,
//! «Поиск по worktree», «Проверка»). Тесты не запускались: сборка запрещена в этом шаге.

use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::{IndexSettings, NoObserver, update_index};
use crate::indexing::worktree_delta::{
    IndexOwner, SyncRequest, SyncScope, excluded_paths, open_delta, sync,
};
use crate::models::{EmbedError, Embedder, ModelFingerprint};
use crate::search::{
    DELTA_ID_OFFSET, IndexView, SearchError, SearchRequest, SearchResponse, SearchSettings, search,
    search_in_view,
};
use crate::storage::{Index, Journal, index_path};

const DIM: usize = 8;

struct FakeEmbedder {
    fingerprint: ModelFingerprint,
}

impl FakeEmbedder {
    fn new() -> Self {
        Self {
            fingerprint: ModelFingerprint {
                source: "builtin".into(),
                model: "m".into(),
                dim: DIM,
                doc_prefix: String::new(),
            },
        }
    }
}

impl Embedder for FakeEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
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

/// Документ из разделов по одному слову на раздел: слова и ищутся.
fn doc(title: &str, words: &[&str]) -> String {
    let mut text = format!("# {title}\n\n");
    for (i, word) in words.iter().enumerate() {
        text.push_str(&format!(
            "## Раздел {i}\n\n{}\n\n",
            format!("{word} ").repeat(60)
        ));
    }
    text
}

/// Владелец с готовым индексом, копия его документов в виде и готовая дельта.
struct Fixture {
    _dir: tempfile::TempDir,
    view: PathBuf,
    index: Index,
    delta: Index,
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
    update_index(&owner, &mut index, &settings(), &mut FakeEmbedder::new()).unwrap();
    let delta = open_delta(&owner, "wt").unwrap();
    let mut fixture = Fixture {
        _dir: dir,
        view,
        index,
        delta,
    };
    resync(&mut fixture);
    fixture
}

fn resync(fixture: &mut Fixture) {
    let settings = settings();
    sync(
        &mut fixture.delta,
        &IndexOwner(&fixture.index),
        &mut FakeEmbedder::new(),
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
}

fn edit(fixture: &mut Fixture, path: &str, text: &str) {
    write(&fixture.view, path, text);
    resync(fixture);
}

fn remove(fixture: &mut Fixture, path: &str) {
    fs::remove_file(fixture.view.join(path)).unwrap();
    resync(fixture);
}

fn request(query: &str) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        results: Some(50),
        candidates: Some(50),
        ..SearchRequest::default()
    }
}

fn in_view(fixture: &Fixture, request: &SearchRequest) -> Result<SearchResponse, SearchError> {
    let excluded = excluded_paths(&fixture.index, &fixture.delta).unwrap();
    let view = IndexView::worktree(&fixture.index, &fixture.delta, &excluded.paths).unwrap();
    search_in_view(
        &view,
        &mut FakeEmbedder::new(),
        None,
        &SearchSettings::default(),
        request,
        None,
        true,
        None,
    )
}

fn in_owner(fixture: &Fixture, request: &SearchRequest) -> Result<SearchResponse, SearchError> {
    search(
        &fixture.index,
        &mut FakeEmbedder::new(),
        None,
        &SearchSettings::default(),
        request,
    )
}

fn paths_of(response: &SearchResponse) -> Vec<&str> {
    response.hits.iter().map(|hit| hit.path.as_str()).collect()
}

fn base() -> Vec<(&'static str, String)> {
    vec![
        ("docs/a.md", doc("A", &["alphaword", "betaword"])),
        ("docs/b.md", doc("B", &["gammaword"])),
        ("docs/c.md", doc("C", &["deltaword"])),
    ]
}

#[test]
fn document_created_only_in_worktree_is_found_only_with_wt() {
    let mut fixture = fixture(&base());
    edit(&mut fixture, "docs/new.md", &doc("Новый", &["zqnovelword"]));

    let found = in_view(&fixture, &request("zqnovelword")).unwrap();
    assert_eq!(found.hits[0].path, "docs/new.md");
    let main = in_owner(&fixture, &request("zqnovelword")).unwrap();
    assert!(!paths_of(&main).contains(&"docs/new.md"));
}

#[test]
fn document_deleted_in_worktree_is_not_found_with_wt() {
    let mut fixture = fixture(&base());
    remove(&mut fixture, "docs/b.md");

    let view = in_view(&fixture, &request("gammaword")).unwrap();
    assert!(!paths_of(&view).contains(&"docs/b.md"), "{:?}", paths_of(&view));
    let main = in_owner(&fixture, &request("gammaword")).unwrap();
    assert!(paths_of(&main).contains(&"docs/b.md"));
}

#[test]
fn worktree_without_edits_gives_owner_results_bitwise() {
    let fixture = fixture(&base());
    for query in ["alphaword", "gammaword deltaword", "раздел"] {
        let view = in_view(&fixture, &request(query)).unwrap();
        let main = in_owner(&fixture, &request(query)).unwrap();
        assert_eq!(view.hits, main.hits, "запрос {query}");
        assert_eq!(view.weak, main.weak);
        assert_eq!(view.candidate_modules, main.candidate_modules);
    }
}

#[test]
fn unrelated_edit_does_not_change_scores_of_other_documents() {
    let mut fixture = fixture(&base());
    edit(&mut fixture, "docs/c.md", &doc("C", &["epsilonword"]));

    let view = in_view(&fixture, &request("alphaword")).unwrap();
    let main = in_owner(&fixture, &request("alphaword")).unwrap();
    // Правленый документ не содержит слов запроса: у дельты нет полнотекстового списка, `f` других
    // документов не меняется.
    for before in main.hits.iter().filter(|hit| hit.path != "docs/c.md") {
        let after = view
            .hits
            .iter()
            .find(|hit| hit.path == before.path && hit.first_line == before.first_line)
            .unwrap_or_else(|| panic!("{} пропал из выдачи вида", before.path));
        assert_eq!(after.score.to_bits(), before.score.to_bits(), "{}", before.path);
        assert_eq!(after.signals, before.signals, "{}", before.path);
    }
}

#[test]
fn merged_pool_has_no_chunk_id_collision() {
    let mut fixture = fixture(&base());
    edit(
        &mut fixture,
        "docs/a.md",
        &doc("A", &["alphaword", "omegaword"]),
    );

    // Локальные номера чанков в обеих базах начинаются с единицы и пересекаются.
    let view = in_view(&fixture, &request("alphaword omegaword")).unwrap();
    let mut ids: Vec<i64> = view.hits.iter().map(|hit| hit.chunk_id).collect();
    let total = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), total, "номера чанков в выдаче уникальны");
    let from_delta = view
        .hits
        .iter()
        .find(|hit| hit.chunk_id >= DELTA_ID_OFFSET)
        .expect("в выдаче есть чанк дельты");
    assert_eq!(from_delta.path, "docs/a.md");
    assert!(from_delta.text.contains("omegaword") || from_delta.text.contains("alphaword"));
    let owner_hit = view
        .hits
        .iter()
        .find(|hit| hit.chunk_id < DELTA_ID_OFFSET)
        .expect("в выдаче есть чанк владельца");
    assert_ne!(owner_hit.path, "docs/a.md", "версию a.md даёт только дельта");

    // Слой отдаёт чанк из нужной базы.
    let excluded = excluded_paths(&fixture.index, &fixture.delta).unwrap();
    let layer = IndexView::worktree(&fixture.index, &fixture.delta, &excluded.paths).unwrap();
    let loaded = layer.load_hits(&[from_delta.chunk_id, owner_hit.chunk_id]).unwrap();
    assert_eq!(loaded[0].path, "docs/a.md");
    assert_eq!(loaded[0].text, from_delta.text);
    assert_eq!(loaded[1].path, owner_hit.path);
    assert_eq!(layer.source(from_delta.chunk_id).1, from_delta.chunk_id - DELTA_ID_OFFSET);
}

#[test]
fn in_modules_files_resolve_over_union_of_owner_and_delta() {
    let mut fixture = fixture(&base());
    edit(
        &mut fixture,
        "docs/fresh/x.md",
        &doc("Икс", &["zqfreshword"]),
    );

    // Модуль, файл и префикс `in` существуют только в дельте.
    let mut by_module = request("zqfreshword");
    by_module.modules = vec!["fresh".into()];
    assert_eq!(in_view(&fixture, &by_module).unwrap().hits[0].path, "docs/fresh/x.md");
    assert!(matches!(
        in_owner(&fixture, &by_module),
        Err(SearchError::InvalidParams(_))
    ));
    let mut by_file = request("zqfreshword");
    by_file.files = vec!["fresh/x.md".into()];
    assert_eq!(in_view(&fixture, &by_file).unwrap().hits[0].path, "docs/fresh/x.md");
    let mut by_path = request("zqfreshword");
    by_path.paths = vec!["docs/fresh".into()];
    assert_eq!(in_view(&fixture, &by_path).unwrap().hits[0].path, "docs/fresh/x.md");

    // Модуль владельца остаётся доступным, а исключённый документ владельца в область не входит.
    let mut owner_module = request("alphaword");
    owner_module.modules = vec!["fresh".into(), ".".into()];
    assert!(in_view(&fixture, &owner_module).is_ok());
}

#[test]
fn excluded_paths_cover_deleted_and_replaced_owner_documents() {
    let mut fixture = fixture(&base());
    edit(&mut fixture, "docs/a.md", &doc("A", &["zzword"]));
    remove(&mut fixture, "docs/b.md");

    let excluded = excluded_paths(&fixture.index, &fixture.delta).unwrap();
    assert_eq!(excluded.paths, vec!["docs/a.md", "docs/b.md"]);
    assert_eq!(excluded.delta_documents, 1);
    assert_eq!(excluded.pending, 0);
}
