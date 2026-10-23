//! Изолированный индекс → настройки → поиск с внешними embed и rerank.
//! Аргументы: source embed-url embed-model rerank-url rerank-model evidence.json.
use bars_core::models::{EmbedError, Embedder, HttpEmbedder, HttpReranker, RemoteTarget, Reranker};
use bars_core::secrets::{KeyRef, MemorySecrets};
use bars_protocol::{Call, SettingsGetParams, SettingsOutput, SettingsSaveParams};
use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::ConfiguredProvider},
};
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 6 || !matches!(args[0].as_str(), "local-openai" | "tei") {
        return Err(
            "expected: source embed-url embed-model rerank-url rerank-model evidence.json".into(),
        );
    }
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(project.join("docs"))?;
    let quote = |s: &str| serde_json::to_string(s).unwrap();
    std::fs::write(
        home.join("settings.toml"),
        format!(
            "[embed]\nsource={}\nmodel={}\nbase_url={}\nbatch_size=2\n[rerank]\nenabled=true\nsource={}\nmodel={}\nbase_url={}\n",
            quote(&args[0]),
            quote(&args[2]),
            quote(&args[1]),
            quote(&args[0]),
            quote(&args[4]),
            quote(&args[3])
        ),
    )?;
    for (name, text) in [
        (
            "search",
            "Семантический поиск находит документацию по смыслу. Индексация строит векторы документов.",
        ),
        (
            "theme",
            "Окно приложения поддерживает светлую и тёмную тему оформления.",
        ),
        (
            "cooking",
            "Для борща нужны свёкла и капуста. Овощи варят в кастрюле.",
        ),
    ] {
        std::fs::write(
            project.join(format!("docs/{name}.md")),
            format!("---\ntitle: {name}\n---\n\n# {name}\n\n{text}\n"),
        )?;
    }
    let worker = Worker::start(
        WorkerConfig::new(home),
        Arc::new(ConfiguredProvider::new(temp.path().join("unused-cache"))),
    )?;
    let handler = WorkerHandler::new(worker.clone());
    // Завершить воркер и удалить только собственный профиль также при ошибке проверки.
    let result = async {
        worker.register(&project)?;
        let started = Instant::now();
        while !worker.is_idle() {
            if started.elapsed() > Duration::from_secs(180) { return Err::<_, Box<dyn std::error::Error>>("index timeout".into()); }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let status = worker.status(&project)?;
        if let Some(error) = &status.last_error { return Err(error.to_string().into()); }
        let expected = format!("{}:{}", args[0], args[2]);
        if status.model_index.as_deref() != Some(expected.as_str()) { return Err("wrong index model".into()); }
        let current: SettingsOutput = serde_json::from_value(handler.dispatch(Call::SettingsGet(SettingsGetParams {})).await.map_err(|e| e.to_string())?)?;
        let mut changed = current.settings;
        changed.embed.batch_size = 3;
        handler.dispatch(Call::SettingsSave(SettingsSaveParams { settings: changed.clone() })).await.map_err(|e| e.to_string())?;
        let saved: SettingsOutput = serde_json::from_value(handler.dispatch(Call::SettingsGet(SettingsGetParams {})).await.map_err(|e| e.to_string())?)?;
        if saved.settings != changed { return Err("settings roundtrip failed".into()); }
        let mut searches = Vec::new();
        for (query, expected) in [("Как найти документы по смыслу?", "docs/search.md"), ("Какие овощи нужны для борща?", "docs/cooking.md")] {
            let started = Instant::now();
            let output = handler.dispatch(Call::Search(serde_json::from_value(json!({
                "cwd":project, "project":project, "query":query, "min_score":0
            }))?)).await.map_err(|e| e.to_string())?;
            if !output["rerank_error"].is_null() || output["results"][0]["path"] != expected
                || !output["results"].as_array().is_some_and(|hits| !hits.is_empty() && hits.iter().all(|hit| hit["score_kind"] == "rerank")) {
                return Err(format!("search assertion failed: {output}").into());
            }
            searches.push(json!({"query":query,"elapsed_ms":started.elapsed().as_millis(),"output":output}));
        }
        let mut long_input_errors = Vec::new();
        if args[0] == "tei" {
            let target = |url: &str, model: &str| RemoteTarget {
                source: "tei", base_url: url.into(), model: model.into(),
                key: KeyRef { name: None, env: None, store: Arc::new(MemorySecrets::default()) },
            };
            let long = "документ ".repeat(2048);
            let mut embedder = HttpEmbedder::new(target(&args[1], &args[2]), 2);
            match embedder.embed(&[&long]) {
                Err(EmbedError::Rejected(error)) if error.contains("422") => {
                    long_input_errors.push(json!({"kind":"embed", "chars":long.chars().count(), "error":error}));
                }
                other => return Err(format!("TEI adapter did not reject long embed input: {other:?}").into()),
            }
            let mut reranker = HttpReranker::new(target(&args[3], &args[4]));
            match reranker.rerank("Как найти документ?", &[&long]) {
                Err(error) if error.to_string().contains("422") => {
                    long_input_errors.push(json!({"kind":"rerank", "chars":long.chars().count(), "error":error.to_string()}));
                }
                other => return Err(format!("TEI adapter did not reject long rerank input: {other:?}").into()),
            }
        }
        std::fs::write(&args[5], serde_json::to_vec_pretty(&json!({"source":args[0], "embed_url":args[1], "embed_model":args[2], "rerank_url":args[3], "rerank_model":args[4], "settings_roundtrip":true, "searches":searches, "long_input_errors":long_input_errors}))?)?;
        Ok(())
    }.await;
    worker.shutdown().await;
    drop(handler);
    drop(worker);
    temp.close()?;
    result
}
