//! `links_of`: ссылки документа читаются с диска без модели и без готового индекса поиска.

use bars_core::secrets::MemorySecrets;
use bars_protocol::{LinksIntoOutput, LinksIntoParams, LinksOfOutput, LinksOfParams};

use super::*;
use crate::autostart::MemoryAutostart;
use crate::handler::WorkerHandler;
use crate::worker::Worker;

fn handler() -> (TempDir, Worker, WorkerHandler) {
    let home = TempDir::new().unwrap();
    let mut config = WorkerConfig::new(home.path().to_path_buf()).without_graph_pause();
    config.secrets = Arc::new(MemorySecrets::default());
    config.autostart = Arc::new(MemoryAutostart::new("\"/opt/bars/bars\"", None));
    let worker = Worker::start(config, FakeProvider::new("m1")).unwrap();
    let handler = WorkerHandler::new(worker.clone());
    (home, worker, handler)
}

async fn links_of(handler: &WorkerHandler, root: &Path, path: &str) -> LinksOfOutput {
    let params = LinksOfParams {
        project: root.display().to_string(),
        path: path.to_string(),
    };
    serde_json::from_value(handler.dispatch(Call::LinksOf(params)).await.unwrap()).unwrap()
}

#[tokio::test]
async fn links_of_reads_fresh_files_and_rejects_relative_project() {
    let (_home, worker, handler) = handler();
    let projects = TempDir::new().unwrap();
    let root = projects.path().join("proj");
    fs::create_dir_all(root.join("docs")).unwrap();
    fs::write(
        root.join("docs/a.md"),
        "---\ntitle: Альфа\n---\n[бета](b.md) и [[нет]]\n",
    )
    .unwrap();
    fs::write(root.join("docs/b.md"), "# Бета\n\n[[a]]\n").unwrap();
    worker.register(&root).unwrap();

    let a = links_of(&handler, &root, "docs/a.md").await;
    assert!(a.known);
    assert_eq!(a.outgoing.len(), 2);
    assert_eq!(a.outgoing[0].target.as_deref(), Some("docs/b.md"));
    assert!(a.outgoing[1].broken);
    assert_eq!(a.incoming.len(), 1);
    assert_eq!(a.incoming[0].path, "docs/b.md");

    // Правка на диске видна в следующем ответе, без ожидания слежения и индексации.
    fs::write(root.join("docs/нет.md"), "# Нет\n").unwrap();
    let a = links_of(&handler, &root, "docs/a.md").await;
    assert!(a.outgoing.iter().all(|link| !link.broken));

    let error = handler
        .dispatch(Call::LinksOf(LinksOfParams {
            project: "proj".into(),
            path: "docs/a.md".into(),
        }))
        .await
        .unwrap_err();
    assert!(error.message.contains("абсолютный путь"));
    worker.shutdown().await;
}

#[tokio::test]
async fn links_into_lists_sources_outside_the_set_without_model() {
    let (_home, worker, handler) = handler();
    let projects = TempDir::new().unwrap();
    let root = projects.path().join("proj");
    fs::create_dir_all(root.join("docs/dir")).unwrap();
    fs::write(
        root.join("docs/a.md"),
        "---\ntitle: Альфа\n---\n[бета](dir/b.md)\n",
    )
    .unwrap();
    fs::write(root.join("docs/dir/b.md"), "# Бета\n\n[[c]]\n").unwrap();
    fs::write(root.join("docs/dir/c.md"), "# Це\n").unwrap();
    worker.register(&root).unwrap();

    let params = |paths: &[&str]| LinksIntoParams {
        project: root.display().to_string(),
        paths: paths.iter().map(|p| p.to_string()).collect(),
    };
    let both = ["docs/dir/b.md", "docs/dir/c.md"];
    let out: LinksIntoOutput = serde_json::from_value(
        handler
            .dispatch(Call::LinksInto(params(&both)))
            .await
            .unwrap(),
    )
    .unwrap();
    // b → c внутри набора не считается; a → b снаружи считается.
    assert_eq!(out.sources.len(), 1);
    assert_eq!(out.sources[0].path, "docs/a.md");
    assert_eq!(out.sources[0].title.as_deref(), Some("Альфа"));
    assert_eq!(
        (
            out.sources[0].links[0].line,
            out.sources[0].links[0].text.as_str()
        ),
        (4, "бета")
    );

    let error = handler
        .dispatch(Call::LinksInto(LinksIntoParams {
            project: "proj".into(),
            paths: vec![],
        }))
        .await
        .unwrap_err();
    assert!(error.message.contains("абсолютный путь"));
}
