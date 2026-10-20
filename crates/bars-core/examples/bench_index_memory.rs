//! Контроль памяти индексатора без ONNX; не проверяет качество модели или пик worker.
#[cfg(windows)]
#[allow(dead_code)]
#[path = "bench_memory.rs"]
mod native;

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use bars_core::{
        indexing::{
            chunking::ChunkingParams,
            indexer::{IndexSettings, UpdateOutcome, update_index},
        },
        models::{EmbedError, Embedder, ModelFingerprint},
        storage::{Index, Journal},
    };
    use serde_json::json;
    use std::{fs, path::Path};

    struct Control {
        fp: ModelFingerprint,
        log: native::windows::Log,
        max_text_bytes: usize,
        max_vectors: usize,
    }
    impl Embedder for Control {
        fn fingerprint(&self) -> &ModelFingerprint {
            &self.fp
        }
        fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
            self.max_text_bytes = self.max_text_bytes.max(texts.iter().map(|t| t.len()).sum());
            self.max_vectors = self.max_vectors.max(texts.len());
            let vectors = vec![vec![1.0 / 32.0; 1024]; texts.len()];
            self.log.emit(
                "marker",
                "document_vectors",
                json!({"texts":texts.len(),"vector_bytes":texts.len()*4096}),
            );
            Ok(vectors)
        }
    }
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        return Err("bench_index_memory <project-with-docs> <new-output-dir>".into());
    }
    let project = Path::new(&args[0]);
    let out = Path::new(&args[1]);
    fs::create_dir(out)?;
    let log = native::windows::Log::create(&out.join("native-memory.jsonl"))?;
    log.reserve(2)?;
    log.emit(
        "config",
        "no_model_control",
        json!({"dim":1024,"model":false}),
    );
    let monitor = native::windows::Monitor::start(log.clone());
    let mut model = Control {
        fp: ModelFingerprint {
            source: "memory-control".into(),
            model: "constant-vector".into(),
            dim: 1024,
            doc_prefix: String::new(),
        },
        log: log.clone(),
        max_text_bytes: 0,
        max_vectors: 0,
    };
    let mut index = Index::open(&out.join("index.sqlite"), Journal::Delete)?;
    let settings = IndexSettings {
        chunking: ChunkingParams::default(),
        docs_root: "docs".into(),
    };
    let UpdateOutcome::Updated(report) = update_index(project, &mut index, &settings, &mut model)?
    else {
        return Err("unexpected incompatibility".into());
    };
    if !report.problems.is_empty() || index.chunk_count()? != index.vector_count()? {
        return Err(format!("invalid control: {report:?}").into());
    }
    let summary = json!({"documents":index.document_count()?,"chunks":index.chunk_count()?,"vectors":index.vector_count()?,"max_document_embed_bytes":model.max_text_bytes,"max_document_vector_bytes":model.max_vectors*4096,"report":format!("{report:?}")});
    log.plateau("index_finished");
    drop(index);
    drop(model);
    log.plateau("index_dropped");
    monitor.stop()?;
    log.emit("end", "complete", summary.clone());
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows memory harness");
}
