//! Stage 17 acceptance through the real worker and its window handlers.
//! verify_devices <isolated-home> <existing-model-cache> <embed:auto|cpu> <rerank:auto|cpu>
use bars_protocol::{
    Call, ModelsInfoParams, SearchOutput, SettingsGetParams, SettingsOutput, SettingsSaveParams,
};
use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::ConfiguredProvider},
};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("expected home, cache, embed device, rerank device".into());
    }
    let home = PathBuf::from(&args[0]);
    let cache = PathBuf::from(&args[1]);
    if home.exists() {
        return Err("acceptance home must be new".into());
    }
    for device in &args[2..] {
        if !["auto", "cpu"].contains(&device.as_str()) {
            return Err("invalid device".into());
        }
    }
    assert!(bars_core::models::is_downloaded(
        &cache,
        bars_core::models::BGE_M3_ID
    ));
    assert!(bars_core::models::is_downloaded(
        &cache,
        bars_core::models::BGE_RERANKER_V2_M3_ID
    ));
    let project = home.join("fixture");
    std::fs::create_dir_all(project.join("docs"))?;
    std::fs::write(
        project.join("docs/search.md"),
        "---\ntitle: Search\n---\n# Search\n\nBars combines semantic vector search with lexical full text search. Rerank scores candidate documents.\n",
    )?;
    std::fs::write(
        project.join("docs/other.md"),
        "---\ntitle: Theme\n---\n# Theme\n\nThe application supports light and dark themes.\n",
    )?;
    std::fs::write(
        home.join("settings.toml"),
        format!(
            "[embed]\naccelerator='{}'\n[rerank]\nenabled=true\naccelerator='{}'\n",
            args[2], args[3]
        ),
    )?;
    let worker = Worker::start(
        WorkerConfig::new(home.clone()),
        Arc::new(ConfiguredProvider::new(cache)),
    )?;
    let handler = WorkerHandler::new(worker.clone());
    worker.register(&project)?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let mut progress = Vec::new();
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if let Some(p) = worker.progress() {
            progress.push(json!({"done":p.docs_done,"total":p.docs_total,"device":p.embed_device,"attempt":p.attempt}));
        }
        if worker.is_idle() {
            break;
        }
        if Instant::now() > deadline {
            return Err("index timeout".into());
        }
    }
    assert!(worker.status(&project)?.last_error.is_none());
    let search = || {
        Call::Search(serde_json::from_value(json!({"cwd": project, "project": project, "query":"semantic lexical hybrid search", "min_score":0})) .unwrap())
    };
    let mut searches = Vec::new();
    let output = loop {
        match handler.dispatch(search()).await {
            Ok(value) => {
                let runtime = handler
                    .dispatch(Call::ModelsInfo(ModelsInfoParams {}))
                    .await
                    .map_err(|e| e.to_string())?;
                searches.push(json!({"search":value,"runtime":runtime}));
                std::fs::write(
                    home.join("searches.json"),
                    serde_json::to_vec_pretty(&searches)?,
                )?;
                if runtime["runtime"]["rerank"]["state"] == "failed" {
                    return Err("rerank load failed; see searches.json".into());
                }
                if runtime["runtime"]["rerank"]["state"] == "ready"
                    && value["applied"]["score_kind"] == "rerank"
                    && value.get("rerank_error").is_none()
                {
                    break value;
                }
                if Instant::now() >= deadline {
                    return Err("rerank timeout; see searches.json".into());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(error) if error.to_string().contains("model") && Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            Err(error) => return Err(error.to_string().into()),
        }
    };
    let typed: SearchOutput = serde_json::from_value(output.clone())?;
    assert!(!typed.results.is_empty());
    let runtime = handler
        .dispatch(Call::ModelsInfo(ModelsInfoParams {}))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        runtime["runtime"]["embed"]["actual"],
        if args[2] == "cpu" { "cpu" } else { "directml" }
    );
    assert_eq!(
        runtime["runtime"]["rerank"]["actual"],
        if args[3] == "cpu" { "cpu" } else { "directml" }
    );
    // Same preferences survive the real save/get path without invalidating the index.
    let current: SettingsOutput = serde_json::from_value(
        handler
            .dispatch(Call::SettingsGet(SettingsGetParams {}))
            .await
            .map_err(|e| e.to_string())?,
    )?;
    handler
        .dispatch(Call::SettingsSave(SettingsSaveParams {
            settings: current.settings.clone(),
        }))
        .await
        .map_err(|e| e.to_string())?;
    let saved: SettingsOutput = serde_json::from_value(
        handler
            .dispatch(Call::SettingsGet(SettingsGetParams {}))
            .await
            .map_err(|e| e.to_string())?,
    )?;
    assert_eq!(current.settings, saved.settings);
    let repeated = handler
        .dispatch(search())
        .await
        .map_err(|e| e.to_string())?;
    assert!(!repeated["results"].as_array().unwrap().is_empty());
    assert_eq!(repeated["applied"]["score_kind"], "rerank");
    assert!(repeated.get("rerank_error").is_none());
    let mut changed = saved.settings.clone();
    changed.rerank.accelerator = match changed.rerank.accelerator {
        bars_protocol::AcceleratorDoc::Auto => bars_protocol::AcceleratorDoc::Cpu,
        bars_protocol::AcceleratorDoc::Cpu => bars_protocol::AcceleratorDoc::Auto,
    };
    let changed_expected = if changed.rerank.accelerator == bars_protocol::AcceleratorDoc::Cpu {
        "cpu"
    } else {
        "directml"
    };
    handler
        .dispatch(Call::SettingsSave(SettingsSaveParams { settings: changed }))
        .await
        .map_err(|e| e.to_string())?;
    let after_save = handler
        .dispatch(Call::ModelsInfo(ModelsInfoParams {}))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        after_save["runtime"]["embed"]["actual"],
        runtime["runtime"]["embed"]["actual"]
    );
    assert_eq!(after_save["runtime"]["rerank"]["state"], "unloaded");
    let change_deadline = Instant::now() + Duration::from_secs(180);
    let changed_search = loop {
        if let Ok(value) = handler.dispatch(search()).await {
            if value["applied"]["score_kind"] == "rerank" && value.get("rerank_error").is_none() {
                break value;
            }
        }
        if Instant::now() > change_deadline {
            return Err("search after device change timed out".into());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let changed_runtime = handler
        .dispatch(Call::ModelsInfo(ModelsInfoParams {}))
        .await
        .map_err(|e| e.to_string())?;
    assert_eq!(
        changed_runtime["runtime"]["rerank"]["actual"],
        changed_expected
    );
    assert_eq!(
        worker.status(&project)?.model_index,
        worker.status(&project)?.model_settings
    );
    let evidence = json!({"devices":[args[2],args[3]],"runtime":runtime,"search":output,"repeat":repeated,"progress":progress,"saved":saved,"after_device_change":{"runtime":changed_runtime["runtime"],"search":changed_search}});
    std::fs::write(
        home.join("result.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    println!(
        "{}",
        json!({"home":home,"runtime":evidence["runtime"]["runtime"],"results":typed.results.len(),"saved":true})
    );
    worker.shutdown().await;
    Ok(())
}
