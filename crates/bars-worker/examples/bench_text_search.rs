//! НФТ-6: настоящий обработчик text_search, копия корпуса, FakeProvider без индексации.
use bars_core::indexing::walk::walk_docs;
use bars_protocol::{Call, TextSearchParams};
use bars_worker::{
    handler::WorkerHandler,
    worker::{Worker, WorkerConfig, models::fake::FakeProvider},
};
use std::{
    path::{Path, PathBuf},
    time::Instant,
};
#[tokio::main]
async fn main() {
    let source = PathBuf::from(std::env::args_os().nth(1).expect("корень проекта"));
    let docs = PathBuf::from(std::env::args_os().nth(2).unwrap_or_else(|| "docs".into()));
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("project");
    let inventory = walk_docs(&source, &docs).unwrap();
    let mut bytes = 0;
    for file in &inventory.files {
        let target = root.join(&file.path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        bytes += std::fs::copy(&file.fs_path, target).unwrap();
    }
    let worker = Worker::start(
        WorkerConfig::new(temp.path().join("home")),
        FakeProvider::new("text-bench"),
    )
    .unwrap();
    worker.pause();
    worker.register(&root).unwrap();
    if docs != Path::new("docs") {
        panic!("этот замер использует docs/; другой корень требует настройки фикстуры");
    }
    let handler = WorkerHandler::new(worker.clone());
    let mut rows = vec![];
    for (query, regex, case_sensitive) in [
        ("индекс", false, false),
        ("Т-39", false, true),
        ("NO_MATCH_TEXT_SEARCH_23817", false, false),
        (r"^.*Т-39.*$", true, false),
    ] {
        let mut times = vec![];
        let mut last = serde_json::Value::Null;
        for n in 0..41 {
            let start = Instant::now();
            last = handler
                .dispatch(Call::TextSearch(TextSearchParams {
                    project: root.display().to_string(),
                    query: query.into(),
                    regex,
                    case_sensitive,
                    window_id: "bench".into(),
                    request_id: n.to_string(),
                    ..Default::default()
                }))
                .await
                .unwrap();
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if n > 0 {
                times.push(elapsed);
            }
        }
        let mut sorted = times.clone();
        sorted.sort_by(f64::total_cmp);
        rows.push(serde_json::json!({"query":query,"regex":regex,"case_sensitive":case_sensitive,"samples_ms":times,"p50_ms":sorted[19],"p95_ms":sorted[37],"docs_total":last["docs_total"],"hits_total":last["hits_total"],"truncated":last["truncated"]}));
    }
    worker.shutdown().await;
    println!("{}",serde_json::to_string_pretty(&serde_json::json!({"cache":"warm OS cache; copied snapshot", "documents":inventory.files.len(),"bytes":bytes,"worker":"real dispatch, paused indexing, FakeProvider", "rows":rows})).unwrap());
}
