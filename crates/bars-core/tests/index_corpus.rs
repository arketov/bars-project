//! Индексация корпуса `docs/` самого bars: индекс сходится с диском, повторная проверка ничего не
//! трогает, полнотекстовый поиск по стеммингу находит документы. Модель — подделка из хешей.

use std::path::Path;

use bars_core::indexing::chunking::{ChunkingParams, chunk_document};
use bars_core::indexing::indexer::{IndexSettings, UpdateOutcome, UpdateReport, update_index};
use bars_core::indexing::walk::{read_utf8, walk_docs};
use bars_core::models::{EmbedError, Embedder, ModelFingerprint};
use bars_core::storage::stem::TextStemmer;
use bars_core::storage::{Index, Journal};

const DIM: usize = 8;

struct HashEmbedder {
    fingerprint: ModelFingerprint,
    computed: usize,
}

impl Embedder for HashEmbedder {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }

    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.computed += texts.len();
        Ok(texts
            .iter()
            .map(|text| {
                let hash = blake3::hash(text.as_bytes());
                (0..DIM)
                    .map(|i| f32::from(hash.as_bytes()[i]) + 1.0)
                    .collect()
            })
            .collect())
    }
}

fn run(project: &Path, index: &mut Index, model: &mut HashEmbedder) -> UpdateReport {
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    match update_index(project, index, &settings, model).expect("индексация") {
        UpdateOutcome::Updated(report) => report,
        other => panic!("{other:?}"),
    }
}

#[test]
fn indexes_the_bars_docs() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let walk = walk_docs(&project, Path::new("docs")).expect("корень docs/ обходится");
    let expected_chunks: usize = walk
        .files
        .iter()
        .map(|file| {
            let source = read_utf8(file).unwrap();
            chunk_document(&file.path, &source, &ChunkingParams::default())
                .chunks
                .len()
        })
        .sum();

    let dir = tempfile::tempdir().unwrap();
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
    let mut model = HashEmbedder {
        fingerprint: ModelFingerprint {
            source: "builtin".into(),
            model: "hash".into(),
            dim: DIM,
            doc_prefix: String::new(),
        },
        computed: 0,
    };

    let report = run(&project, &mut index, &mut model);

    assert!(report.problems.is_empty(), "{:?}", report.problems);
    assert_eq!(report.added, walk.files.len());
    assert_eq!(index.document_count().unwrap(), walk.files.len());
    assert_eq!(index.chunk_count().unwrap(), expected_chunks);
    // Каждый вектор посчитан один раз, лишних в индексе нет.
    assert_eq!(model.computed, index.vector_count().unwrap());
    assert!(index.vector_count().unwrap() <= expected_chunks);

    let modules: Vec<String> = index
        .conn()
        .prepare("SELECT DISTINCT module FROM documents ORDER BY module")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(modules.iter().any(|m| m == "."), "{modules:?}");
    assert!(modules.iter().any(|m| m == "architecture"), "{modules:?}");

    // Стемминг: запрос «стемминга» находит документы, где написано «стемминг» и «стеммер».
    let query = TextStemmer::new().stem_text("стемминга");
    let hits: i64 = index
        .conn()
        .query_row(
            "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
            [&query],
            |row| row.get(0),
        )
        .unwrap();
    assert!(hits > 0, "по запросу `{query}` ничего не нашлось");

    let again = run(&project, &mut index, &mut model);
    assert_eq!(again.unchanged, walk.files.len());
    assert_eq!(again.added + again.changed + again.removed, 0);
    assert_eq!(model.computed, index.vector_count().unwrap());
}
