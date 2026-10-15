//! Т-16, Т-17: поиск на внешних embed и rerank. Провайдер — локальный фальшивый сервер,
//! ключ — выдуманная строка в памяти; настоящие провайдеры и ключи не используются.

use bars_core::models::testing::{FakeServer, Request, Response};
use bars_core::secrets::{MemorySecrets, SecretStore};
use serde_json::json;

use super::*;
use crate::worker::ConfiguredProvider;

const KEY: &str = "sk-test-not-a-real-key-123456";

/// Embed: четырёхмерный «мешок букв»; rerank: «Борщ» нравится больше остального.
fn provider_server(rerank_status: u16) -> FakeServer {
    FakeServer::start(move |request: &Request| match request.path.as_str() {
        "/v1/embeddings" => {
            let input = request.json()["input"].clone();
            let data: Vec<_> = input
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    let text = text.as_str().unwrap();
                    let count = |letter: char| text.chars().filter(|c| *c == letter).count();
                    json!({"index": index, "embedding": [
                        1.0 + (text.len() % 5) as f64,
                        1.0 + count('о') as f64,
                        1.0 + count('а') as f64,
                        1.0,
                    ]})
                })
                .collect();
            Response::ok(json!({ "data": data }))
        }
        "/rerank" if rerank_status != 200 => Response::status(rerank_status, "сломалось"),
        "/rerank" => {
            let body = request.json();
            let results: Vec<_> = body["documents"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, doc)| {
                    let score = if doc.as_str().unwrap().contains("Борщ") {
                        0.9
                    } else {
                        0.2
                    };
                    json!({"index": index, "relevance_score": score})
                })
                .collect();
            Response::ok(json!({ "results": results }))
        }
        _ => Response::status(404, "not found"),
    })
}

fn secrets() -> Arc<dyn SecretStore> {
    let secrets = MemorySecrets::default();
    secrets.set("bars/test", KEY).unwrap();
    Arc::new(secrets)
}

async fn harness(name: &str) -> Harness {
    let provider = ConfiguredProvider::with_secrets(PathBuf::from("models-not-used"), secrets());
    Harness::start(name, Arc::new(provider)).await
}

fn write_settings(harness: &Harness, text: &str) {
    fs::write(harness.home.path().join("settings.toml"), text).unwrap();
}

fn docs(harness: &Harness) -> PathBuf {
    harness.project(
        "ext",
        &[
            (
                "cooking.md",
                "# Борщ\n\nСвёкла, капуста и мясо варятся два часа. Очередь в магазине.\n",
            ),
            (
                "worker.md",
                "# Воркер\n\nОчередь индексации берёт по одному проекту за раз.\n",
            ),
            ("api.md", "# Протокол\n\nОчередь вызовов в канале.\n"),
            ("misc.md", "# Разное\n\nОчередь задач и заметки.\n"),
        ],
    )
}

async fn register_and_index(harness: &Harness, cli: &mut Client) -> PathBuf {
    let root = docs(harness);
    cli.call(&RegisterParams {
        path: root.display().to_string(),
    })
    .await
    .unwrap();
    wait_status(cli, &root, "индексация во внешнем провайдере", indexed).await;
    root
}

#[tokio::test(flavor = "multi_thread")]
async fn remote_embed_and_rerank_serve_search_with_the_key_from_the_store() {
    let server = provider_server(200);
    let harness = harness("ext-remote").await;
    write_settings(
        &harness,
        &format!(
            "[embed]\nsource = \"remote\"\nmodel = \"emb\"\nbase_url = \"{url}\"\napi_key_name = \"bars/test\"\nbatch_size = 2\n\
             [rerank]\nenabled = true\nsource = \"remote\"\nmodel = \"rr\"\nbase_url = \"{url}\"\napi_key_name = \"bars/test\"\n",
            url = server.url()
        ),
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = register_and_index(&harness, &mut cli).await;

    let found = cli.call(&search(&root, "очередь")).await.unwrap();
    assert_eq!(found.rerank_error, None);
    // Rerank включён: по умолчанию 3 результата вместо 5 (Т-16), и первым идёт то, что оценил rerank.
    assert_eq!(found.applied.results, 3);
    assert!(found.results.len() <= 3);
    assert_eq!(found.results[0].path, "docs/cooking.md");

    let requests = server.requests();
    assert!(requests.iter().any(|r| r.path == "/rerank"));
    assert!(requests.iter().any(|r| r.path == "/v1/embeddings"));
    let bearer = format!("Bearer {KEY}");
    for request in &requests {
        assert_eq!(request.header("authorization"), Some(bearer.as_str()));
    }
    // Пачки по `batch_size` из настроек, а не по 4 встроенной модели.
    let max_batch = requests
        .iter()
        .filter(|r| r.path == "/v1/embeddings")
        .map(|r| r.json()["input"].as_array().unwrap().len())
        .max()
        .unwrap();
    assert!(max_batch <= 2, "{max_batch}");
}

#[tokio::test(flavor = "multi_thread")]
async fn local_openai_address_works_without_a_key() {
    let server = provider_server(200);
    let harness = harness("ext-local").await;
    write_settings(
        &harness,
        &format!(
            "[embed]\nsource = \"local-openai\"\nmodel = \"emb\"\nbase_url = \"{}/v1\"\n",
            server.url()
        ),
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = register_and_index(&harness, &mut cli).await;

    let found = cli.call(&search(&root, "очередь")).await.unwrap();
    assert_eq!(found.applied.results, 5, "rerank выключен");
    assert!(found.rerank_error.is_none());
    assert!(
        server
            .requests()
            .iter()
            .all(|r| r.header("authorization").is_none() && r.path == "/v1/embeddings")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_rerank_provider_leaves_the_search_without_rerank_and_says_why() {
    let server = provider_server(500);
    let harness = harness("ext-rerank-down").await;
    write_settings(
        &harness,
        &format!(
            "[embed]\nsource = \"local-openai\"\nmodel = \"emb\"\nbase_url = \"{url}\"\n\
             [rerank]\nenabled = true\nsource = \"local-openai\"\nmodel = \"rr\"\nbase_url = \"{url}\"\n",
            url = server.url()
        ),
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = register_and_index(&harness, &mut cli).await;

    let found = cli.call(&search(&root, "очередь")).await.unwrap();
    assert!(!found.results.is_empty());
    assert_eq!(found.applied.results, 5, "без реранка — число без реранка");
    let reason = found.rerank_error.expect("причина в ответе");
    assert!(reason.contains("500"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn switching_the_embed_model_asks_for_reindex_and_does_not_start_it() {
    let server = provider_server(200);
    let harness = harness("ext-switch").await;
    write_settings(
        &harness,
        &format!(
            "[embed]\nsource = \"local-openai\"\nmodel = \"emb\"\nbase_url = \"{}\"\n",
            server.url()
        ),
    );
    let mut cli = harness.client(ClientKind::Cli).await;
    let root = register_and_index(&harness, &mut cli).await;
    let before = server.requests().len();

    // Встроенная Qwen3 вместо внешней модели: отпечаток другой, модель при этом не загружается.
    write_settings(&harness, "[embed]\nmodel = \"Qwen/Qwen3-Embedding-0.6B\"\n");
    let _ = cli
        .call(&ReindexParams {
            target: target(&root),
            full: false,
            accept_model_change: false,
        })
        .await;
    let status = wait_status(
        &mut cli,
        &root,
        "нужна переиндексация",
        |status| status.state == ProjectState::ReindexRequired,
    )
    .await;
    assert_eq!(status.state, ProjectState::ReindexRequired);
    assert_eq!(server.requests().len(), before, "эмбеддинг не вызывался");
    let error = rpc(cli.call(&search(&root, "очередь")).await);
    assert!(error.message.contains("переиндексац"), "{error:?}");
}
