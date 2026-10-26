//! Изолированная проверка настоящего LM Studio: verify_lmstudio <base-url> <model> <evidence.json>.
//! Сервер и модель готовятся отдельно; пользовательские проекты/профиль не читаются.
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
    if args.len() != 3 {
        return Err("expected base-url, model, evidence.json".into());
    }
    let temp = tempfile::tempdir()?;
    let home = temp.path().join("profile");
    let project = temp.path().join("project");
    std::fs::create_dir_all(&home)?;
    std::fs::create_dir_all(project.join("docs"))?;
    // JSON strings also quote URLs/model names safely in TOML.
    std::fs::write(
        home.join("settings.toml"),
        format!(
            "[embed]\nsource='lmstudio'\nmodel={}\nbase_url={}\nbatch_size=2\n[rerank]\nenabled=false\n",
            serde_json::to_string(&args[1])?,
            serde_json::to_string(&args[0])?
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
        WorkerConfig::new(home.clone()),
        Arc::new(ConfiguredProvider::new(
            temp.path().join("unused-model-cache"),
        )),
    )?;
    let handler = WorkerHandler::new(worker.clone());
    let started = Instant::now();
    worker.register(&project)?;
    while !worker.is_idle() {
        if started.elapsed() > Duration::from_secs(180) {
            return Err("index timeout".into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let status = worker.status(&project)?;
    if let Some(error) = &status.last_error {
        return Err(error.to_string().into());
    }
    let expected_model = format!("lmstudio:{}", args[1]);
    assert_eq!(status.model_index.as_deref(), Some(expected_model.as_str()));
    let current: SettingsOutput = serde_json::from_value(
        handler
            .dispatch(Call::SettingsGet(SettingsGetParams {}))
            .await
            .map_err(|e| e.to_string())?,
    )?;
    assert_eq!(current.settings.embed.source, "lmstudio");
    let mut changed = current.settings.clone();
    changed.embed.batch_size = 3;
    handler
        .dispatch(Call::SettingsSave(SettingsSaveParams::between(
            current.clone(),
            changed.clone(),
        )))
        .await
        .map_err(|e| e.to_string())?;
    let saved: SettingsOutput = serde_json::from_value(
        handler
            .dispatch(Call::SettingsGet(SettingsGetParams {}))
            .await
            .map_err(|e| e.to_string())?,
    )?;
    assert_eq!(saved.settings, changed);
    let mut searches = Vec::new();
    for (query, expected) in [
        ("Как найти документы по смыслу?", "docs/search.md"),
        ("Какие овощи нужны для борща?", "docs/cooking.md"),
    ] {
        let start = Instant::now();
        let output = handler
            .dispatch(Call::Search(serde_json::from_value(json!({
                "cwd": project, "project": project, "query": query, "min_score": 0
            }))?))
            .await
            .map_err(|e| e.to_string())?;
        assert_eq!(output["results"][0]["path"], expected, "{output}");
        searches.push(
            json!({"query": query, "elapsed_ms": start.elapsed().as_millis(), "output": output}),
        );
    }
    let evidence = json!({"source":"lmstudio", "endpoint":args[0], "model":args[1],
        "index_status": {"state":format!("{:?}", status.state),
            "model_index":status.model_index, "model_settings":status.model_settings,
            "last_error":status.last_error}, "settings_roundtrip":true, "searches":searches});
    std::fs::write(&args[2], serde_json::to_vec_pretty(&evidence)?)?;
    worker.shutdown().await;
    drop(handler);
    drop(worker);
    temp.close()?;
    println!("LM Studio: index + settings roundtrip + 2 searches passed");
    Ok(())
}
