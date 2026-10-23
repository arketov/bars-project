//! Этап 11: настоящий worker, IPC, shared slots; только копия корпуса и отдельный профиль.
#[path = "../../../bars-core/examples/inference/mod.rs"]
mod experiment;
use bars_core::{
    models::{EmbedError, Embedder, ModelFingerprint, RerankError, Reranker},
    settings::Settings,
};
use bars_protocol::{Client, ClientKind, Endpoint, RegisterParams, SearchParams, Target};
use bars_worker::{
    serve::serve_with,
    worker::{ConfiguredProvider, ModelProvider, WorkerConfig},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Provider {
    base: ConfiguredProvider,
    models: PathBuf,
    cfg: experiment::Config,
}
impl ModelProvider for Provider {
    fn fingerprint(&self, s: &Settings) -> std::result::Result<ModelFingerprint, EmbedError> {
        self.base.fingerprint(s)
    }
    fn load(&self, s: &Settings) -> std::result::Result<Box<dyn Embedder + Send>, EmbedError> {
        self.base.load(s)
    }
    fn batch_size(&self, s: &Settings) -> usize {
        self.base.batch_size(s)
    }
    fn batch_chars(&self, s: &Settings) -> Option<usize> {
        self.base.batch_chars(s)
    }
    fn load_reranker(
        &self,
        s: &Settings,
    ) -> std::result::Result<Box<dyn Reranker + Send>, RerankError> {
        if self.cfg.name == "production" {
            return self.base.load_reranker(s);
        }
        experiment::ExperimentalReranker::load(&self.models, self.cfg.clone())
            .map(|r| Box::new(r) as Box<dyn Reranker + Send>)
            .map_err(|e| RerankError::Load(e.to_string()))
    }
}
fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
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
        limit: Some(15),
        candidates: Some(30),
        min_score: Some(0.),
    }
}
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
pub async fn main() -> Result<()> {
    let a: Vec<_> = std::env::args().skip(1).collect();
    if a.len() != 5 {
        return Err("<frozen-snapshot> <input-data> <models> <config> <new-out>".into());
    }
    let source = Path::new(&a[0]);
    let data = Path::new(&a[1]);
    let models = PathBuf::from(&a[2]);
    let cfg: experiment::Config = serde_json::from_slice(&fs::read(&a[3])?)?;
    let out = PathBuf::from(&a[4]);
    if out.exists() {
        return Err("new output directory required".into());
    }
    fs::create_dir_all(&out)?;
    let project = out.join("project");
    copy_tree(&source.join("project/docs"), &project.join("docs"))?;
    fs::create_dir_all(project.join(".bars"))?;
    fs::copy(
        data.join("index.sqlite"),
        project.join(".bars/index.sqlite"),
    )?;
    let home = out.join("home");
    fs::create_dir_all(&home)?;
    fs::write(
        home.join("settings.toml"),
        "[rerank]\nenabled = true\n[search]\nmin_score_vector = 0.0\nmin_score_rerank = 0.0\n",
    )?;
    let mut config = WorkerConfig::new(home);
    config.install_env = None;
    config.periodic_interval = Duration::from_secs(86400);
    let provider = Arc::new(Provider {
        base: ConfiguredProvider::new(models.clone()),
        models,
        cfg: cfg.clone(),
    });
    let endpoint = Endpoint::named(&format!("inference-owned-{}", std::process::id()));
    let ep = endpoint.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        serve_with(config, &ep, provider, |h| {
            let _ = send.send(h);
        })
        .await
    });
    let handler = receive.await?;
    let project = fs::canonicalize(project)?.to_string_lossy().into_owned();
    let mut client = Client::connect(&endpoint, ClientKind::Cli).await?;
    client
        .call(&RegisterParams {
            path: project.clone(),
        })
        .await?;
    while !handler.worker().is_idle() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let inputs: Vec<Value> = serde_json::from_slice(&fs::read(data.join("inputs.json"))?)?;
    let queries: Vec<_> = inputs
        .iter()
        .take(8)
        .map(|c| c["query"].as_str().unwrap().to_owned())
        .collect();
    let warm = client.call(&request(&project, &queries[0])).await?;
    if warm.rerank_error.is_some() {
        return Err("warmup rerank fallback".into());
    }
    let mut file = File::create(out.join("rows.jsonl"))?;
    for query in &queries {
        let start = Instant::now();
        let response = client.call(&request(&project, query)).await?;
        if response.rerank_error.is_some() {
            return Err("serial rerank fallback".into());
        }
        writeln!(
            file,
            "{}",
            json!({"mode":"serial","seconds":start.elapsed().as_secs_f64(),"query":query,"response":response})
        )?;
    }
    for concurrency in [2, 4] {
        let mut tasks = Vec::new();
        for query in queries.iter().take(concurrency) {
            let endpoint = endpoint.clone();
            let query = query.clone();
            let project = project.clone();
            tasks.push(tokio::spawn(async move{
                let mut client=Client::connect(&endpoint,ClientKind::Cli).await.map_err(|e|e.to_string())?;
                let start=Instant::now();let response=client.call(&request(&project,&query)).await.map_err(|e|e.to_string())?;
                if response.rerank_error.is_some(){return Err("concurrent rerank fallback".to_owned());}
                Ok::<_,String>(json!({"mode":format!("concurrent-{concurrency}"),"seconds":start.elapsed().as_secs_f64(),"query":query,"response":response}))
            }));
        }
        for task in tasks {
            writeln!(file, "{}", task.await??)?;
        }
    }
    // Real worker indexing overlaps queries; copied documents only, rebuild forces actual embeddings.
    handler.worker().reindex(Path::new(&project), true, false)?;
    let wait = Instant::now();
    while handler.worker().progress().is_none() {
        if wait.elapsed() > Duration::from_secs(10) {
            return Err("index job did not become active; overlap unproven".into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    for query in &queries {
        let active = handler.worker().progress().is_some();
        let start = Instant::now();
        let response = client.call(&request(&project, query)).await?;
        if response.rerank_error.is_some() {
            return Err("during-index rerank fallback".into());
        }
        writeln!(
            file,
            "{}",
            json!({"mode":"during-index","active_at_start":active,"seconds":start.elapsed().as_secs_f64(),"query":query,"response":response})
        )?;
    }
    handler.worker().cancel(Path::new(&project))?;
    handler.worker().shutdown().await;
    server.abort();
    let _ = server.await;
    fs::write(
        out.join("complete.json"),
        serde_json::to_vec_pretty(
            &json!({"config":cfg,"serial":8,"concurrent":6,"during_index":8,"owned_endpoint":format!("inference-owned-{}",std::process::id())}),
        )?,
    )?;
    Ok(())
}
