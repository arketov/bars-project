//! НФТ-2 через настоящий воркер и IPC: готовый индекс естественных 5000 чанков, CPU, без rerank.
//! Протокол: docs/knowledge/inference-speed.md, «Естественные 5 000 чанков».
#[cfg(windows)]
#[allow(dead_code)]
#[path = "../../bars-core/examples/bench_memory.rs"]
mod standalone;

#[cfg(windows)]
mod bench {
    use bars_core::{
        indexing::chunking::ChunkingParams,
        models::{EmbedError, Embedder, ModelFingerprint, RerankError, Reranker},
        settings::Settings,
        storage::{Index, Journal, Layout},
    };
    use bars_protocol::{
        Client, ClientKind, Endpoint, RegisterParams, ScoreKind, SearchParams, Target,
    };
    use bars_worker::{
        serve::serve_with,
        worker::{ConfiguredProvider, ModelProvider, WorkerConfig},
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::{
        collections::HashMap,
        fs::{self, OpenOptions},
        io::Write,
        path::{Path, PathBuf},
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::{Duration, Instant},
    };

    use super::standalone::windows::{Log, Monitor};

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

    #[derive(Deserialize)]
    struct Inputs {
        queries: Vec<String>,
    }
    #[derive(Deserialize)]
    struct Document {
        path: String,
        source: String,
    }
    #[derive(Deserialize)]
    struct Corpus {
        documents: Vec<Document>,
    }

    /// Счётчики embed внутри воркера: документы должны остаться нетронутыми, запрос — один.
    #[derive(Default)]
    struct Counters {
        documents: AtomicUsize,
        queries: AtomicUsize,
        query_seconds: Mutex<f64>,
    }
    struct Counted {
        inner: Box<dyn Embedder + Send>,
        counters: Arc<Counters>,
    }
    impl Embedder for Counted {
        fn fingerprint(&self) -> &ModelFingerprint {
            self.inner.fingerprint()
        }
        fn embed(&mut self, texts: &[&str]) -> std::result::Result<Vec<Vec<f32>>, EmbedError> {
            self.counters
                .documents
                .fetch_add(texts.len(), Ordering::SeqCst);
            self.inner.embed(texts)
        }
        fn embed_query(&mut self, text: &str) -> std::result::Result<Vec<f32>, EmbedError> {
            let start = Instant::now();
            let result = self.inner.embed_query(text);
            *self.counters.query_seconds.lock().unwrap() = start.elapsed().as_secs_f64();
            self.counters.queries.fetch_add(1, Ordering::SeqCst);
            result
        }
    }
    struct Provider {
        base: ConfiguredProvider,
        counters: Arc<Counters>,
    }
    impl ModelProvider for Provider {
        fn fingerprint(&self, s: &Settings) -> std::result::Result<ModelFingerprint, EmbedError> {
            self.base.fingerprint(s)
        }
        fn load(&self, s: &Settings) -> std::result::Result<Box<dyn Embedder + Send>, EmbedError> {
            Ok(Box::new(Counted {
                inner: self.base.load(s)?,
                counters: self.counters.clone(),
            }))
        }
        fn batch_size(&self, s: &Settings) -> usize {
            self.base.batch_size(s)
        }
        fn batch_chars(&self, s: &Settings) -> Option<usize> {
            self.base.batch_chars(s)
        }
        fn load_reranker(
            &self,
            _: &Settings,
        ) -> std::result::Result<Box<dyn Reranker + Send>, RerankError> {
            Err(RerankError::Load("rerank выключен стендом".into()))
        }
    }

    fn digest(bytes: &[u8]) -> String {
        blake3::hash(bytes).to_hex().to_string()
    }
    fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
        Ok(serde_json::from_slice(&fs::read(path)?)?)
    }
    fn quantile(sorted: &[f64], p: f64) -> f64 {
        sorted[(sorted.len() as f64 * p).ceil() as usize - 1]
    }
    fn request(project: &str, query: &str) -> SearchParams {
        SearchParams {
            target: Target {
                cwd: project.into(),
                project: Some(project.into()),
            },
            query: query.into(),
            modules: vec![],
            files: vec![],
            limit: None,
            candidates: None,
            min_score: None,
        }
    }
    fn counts(path: &Path) -> Result<(usize, usize)> {
        let index = Index::open_read_only(path)?;
        Ok((index.chunk_count()?, index.vector_count()?))
    }

    /// Копия проекта: документы корпуса как есть и готовый индекс с параметрами нарезки.
    fn materialize(corpus: &Corpus, database: &Path, project: &Path) -> Result<()> {
        for doc in &corpus.documents {
            let relative = doc
                .path
                .strip_prefix("docs/")
                .ok_or("путь корпуса вне docs/")?;
            let target = project.join("docs").join(relative);
            fs::create_dir_all(target.parent().unwrap())?;
            fs::write(target, &doc.source)?;
        }
        fs::create_dir_all(project.join(".bars"))?;
        let index_path = project.join(".bars/index.sqlite");
        fs::copy(database, &index_path)?;
        let mut index = Index::open(&index_path, Journal::Delete)?;
        index.write_layout(&Layout {
            chunking: ChunkingParams::default(),
            docs_root: "docs".into(),
        })?;
        Ok(())
    }

    async fn series(
        inputs: &Inputs,
        corpus_path: &Path,
        database: &Path,
        models: &Path,
        out: &Path,
    ) -> Result<Value> {
        let complete: Value = read(Path::new(&format!("{}.complete.json", database.display())))?;
        if complete["index_blake3"] != digest(&fs::read(database)?) {
            return Err("индекс изменился после подготовки".into());
        }
        let core_rows = out.with_file_name("core-rows.jsonl");
        let mut core_hits = HashMap::new();
        for line in fs::read_to_string(&core_rows)?.lines() {
            let row: Value = serde_json::from_str(line)?;
            core_hits.insert(
                row["query"].as_str().unwrap_or_default().to_owned(),
                row["hits"].clone(),
            );
        }
        if core_hits.len() != inputs.queries.len() {
            return Err("нет полного core-прогона для сверки выдачи".into());
        }
        let work = out.join("work");
        let project = work.join("project");
        materialize(&read(corpus_path)?, database, &project)?;
        let home = work.join("home");
        fs::create_dir_all(&home)?;
        fs::write(
            home.join("settings.toml"),
            "[embed]\naccelerator = \"cpu\"\n[rerank]\nenabled = false\n",
        )?;
        let mut config = WorkerConfig::new(home);
        config.install_env = None;
        config.periodic_interval = Duration::from_secs(86400);
        let counters = Arc::new(Counters::default());
        let provider = Arc::new(Provider {
            base: ConfiguredProvider::new(models.to_path_buf()),
            counters: counters.clone(),
        });
        let endpoint = Endpoint::named(&format!("search-cpu-owned-{}", std::process::id()));
        let ep = endpoint.clone();
        let (send, receive) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            serve_with(config, &ep, provider, |h| {
                let _ = send.send(h);
            })
            .await
        });
        let handler = receive.await?;
        let project = fs::canonicalize(project)?;
        let index_path = project.join(".bars/index.sqlite");
        let target = project.to_string_lossy().into_owned();
        let mut client = Client::connect(&endpoint, ClientKind::Cli).await?;
        let register = Instant::now();
        client
            .call(&RegisterParams {
                path: target.clone(),
            })
            .await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        while !handler.worker().is_idle() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let settle_seconds = register.elapsed().as_secs_f64();
        let documents_embedded = counters.documents.load(Ordering::SeqCst);
        if documents_embedded != 0 || counts(&index_path)? != (5000, 5000) {
            return Err(format!(
                "воркер изменил готовый индекс: embed документов {documents_embedded}, чанки/векторы {:?}",
                counts(&index_path)?
            )
            .into());
        }
        let mut rows = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join("worker-rows.jsonl"))?;
        let start = Instant::now();
        let first = client.call(&request(&target, &inputs.queries[0])).await?;
        let first_seconds = start.elapsed().as_secs_f64();
        let first_embed_seconds = *counters.query_seconds.lock().unwrap();
        if first.results.is_empty() {
            return Err("пустая выдача первого запроса".into());
        }
        let (mut times, mut embeds, mut matches) = (Vec::new(), Vec::new(), 0);
        for round in 0..4 {
            for offset in 0..inputs.queries.len() {
                let query = &inputs.queries[(offset + round * 7) % inputs.queries.len()];
                let before = counters.queries.load(Ordering::SeqCst);
                let start = Instant::now();
                let response = client.call(&request(&target, query)).await?;
                let seconds = start.elapsed().as_secs_f64();
                let embed_seconds = *counters.query_seconds.lock().unwrap();
                if response.applied.score_kind != ScoreKind::Cosine
                    || response.rerank_error.is_some()
                    || response.indexing
                    || counters.queries.load(Ordering::SeqCst) != before + 1
                    || response.results.is_empty()
                {
                    return Err(format!(
                        "неожиданный путь поиска воркера: {query}: {:?}, rerank_error {:?}, indexing {}, embed запросов {}, результатов {}",
                        response.applied.score_kind,
                        response.rerank_error,
                        response.indexing,
                        counters.queries.load(Ordering::SeqCst) - before,
                        response.results.len()
                    )
                    .into());
                }
                let hits = response.results.iter().map(|h| json!({"path":h.path,"heading":h.heading_chain,"line":h.first_line,"score":h.score})).collect::<Vec<_>>();
                // Сверка через тот же текст JSON: f32 в Value становится f64, сравнивать надо разобранное.
                let parsed: Value = serde_json::from_str(&serde_json::to_string(&hits)?)?;
                let same = core_hits.get(query) == Some(&parsed);
                matches += usize::from(same);
                writeln!(
                    rows,
                    "{}",
                    json!({"query":query,"round":round,"seconds":seconds,"embed_seconds":embed_seconds,"same_as_core":same,"hits":hits})
                )?;
                rows.flush()?;
                times.push(seconds);
                embeds.push(embed_seconds);
            }
        }
        let documents_embedded = counters.documents.load(Ordering::SeqCst);
        handler.worker().shutdown().await;
        server.abort();
        let _ = server.await;
        drop(client);
        fs::remove_dir_all(&work)?;
        if documents_embedded != 0 {
            return Err("воркер эмбеддил документы во время серии".into());
        }
        times.sort_by(f64::total_cmp);
        embeds.sort_by(f64::total_cmp);
        Ok(
            json!({"status":"complete-worker-ipc","chunks":5000,"unique_queries":inputs.queries.len(),"observations":times.len(),"settle_seconds":settle_seconds,"first_search_seconds":first_seconds,"first_embed_seconds":first_embed_seconds,"median_seconds":quantile(&times,0.5),"p95_seconds":quantile(&times,0.95),"max_seconds":times.last(),"under_one_second":times.iter().filter(|t| **t < 1.0).count(),"embed_median_seconds":quantile(&embeds,0.5),"embed_p95_seconds":quantile(&embeds,0.95),"same_as_core":matches,"documents_embedded":documents_embedded,"index_blake3":complete["index_blake3"],"inputs_blake3":digest(&serde_json::to_vec(&inputs.queries)?),"boundary":"client-measured IPC to real worker, CPU embed, rerank off, model load in first search, OS cache uncontrolled"}),
        )
    }

    #[tokio::main(flavor = "multi_thread", worker_threads = 4)]
    pub async fn main() -> Result<()> {
        let a: Vec<_> = std::env::args().skip(1).collect();
        if a.len() != 5 {
            return Err("<corpus.json> <inputs.json> <index.sqlite> <models> <new-out-dir with core-rows.jsonl beside>".into());
        }
        let inputs: Inputs = read(Path::new(&a[1]))?;
        if inputs.queries.len() < 25 {
            return Err("нужно не менее 25 вопросов".into());
        }
        let out = PathBuf::from(&a[4]);
        fs::create_dir(&out)?;
        let log = Log::create(&out.join("memory.jsonl"))?;
        // Машина общая (OQ-195): ждать запаса, а не снижать порог.
        log.wait_reserve(6, Duration::from_secs(1800))?;
        log.emit(
            "config",
            "search_cpu_worker",
            json!({"startup_commit_gib":6,"stop_commit_gib":1,"interval_ms":100}),
        );
        let monitor = Monitor::start(log.clone());
        let result = series(
            &inputs,
            Path::new(&a[0]),
            Path::new(&a[2]),
            Path::new(&a[3]),
            &out,
        )
        .await;
        log.emit(
            "end",
            "search_cpu_worker",
            json!({"ok":result.is_ok(),"error":result.as_ref().err().map(ToString::to_string)}),
        );
        monitor.stop()?;
        let summary = result?;
        fs::write(
            out.join("summary.json"),
            serde_json::to_vec_pretty(&summary)?,
        )?;
        println!("{summary}");
        Ok(())
    }
}

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    bench::main()
}
#[cfg(not(windows))]
fn main() {
    eprintln!("This experimental harness requires Windows.");
}
