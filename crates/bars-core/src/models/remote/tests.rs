use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::*;
use crate::models::testing::{FakeServer, Response};
use crate::secrets::{KeyringSecrets, SecretStore};

const KEY: &str = "sk-test-not-a-real-key-123456";

fn store_with_key() -> Arc<dyn SecretStore> {
    let store = KeyringSecrets::with_store(keyring_core::mock::Store::new().unwrap());
    store.set("bars/test", KEY).unwrap();
    Arc::new(store)
}

fn target(server: &FakeServer, source: &'static str, with_key: bool) -> RemoteTarget {
    RemoteTarget {
        source,
        base_url: server.url(),
        model: "emb-1".into(),
        key: KeyRef {
            name: with_key.then(|| "bars/test".to_string()),
            env: None,
            store: store_with_key(),
        },
    }
}

/// Сервер, который отвечает вектором `[длина, 1, 2]` на каждый текст.
fn embedding_server() -> FakeServer {
    FakeServer::start(|request| {
        let input = request.json()["input"].clone();
        let data: Vec<_> = input
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, text)| {
                json!({"index": index, "embedding": [text.as_str().unwrap().len() as f64, 1.0, 2.0]})
            })
            .collect();
        Response::ok(json!({ "data": data }))
    })
}

#[test]
fn embeddings_go_to_v1_embeddings_with_the_key_and_the_model() {
    let server = embedding_server();
    let mut embedder = HttpEmbedder::new(target(&server, "remote", true), 8);
    assert_eq!(
        embedder.fingerprint().dim,
        0,
        "до первого ответа размерность неизвестна"
    );

    let vectors = embedder.embed(&["аб", "abc"]).unwrap();
    assert_eq!(vectors, vec![vec![4.0, 1.0, 2.0], vec![3.0, 1.0, 2.0]]);
    assert_eq!(
        embedder.fingerprint().dim,
        3,
        "размерность узнана из ответа"
    );
    assert_eq!(embedder.fingerprint().source, "remote");
    assert_eq!(embedder.fingerprint().model, "emb-1");

    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].method, "POST");
    assert_eq!(requests[0].path, "/v1/embeddings");
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(requests[0].json()["model"], "emb-1");
    assert_eq!(requests[0].json()["input"], json!(["аб", "abc"]));
}

#[test]
fn local_address_needs_no_key_and_sends_no_authorization() {
    let server = embedding_server();
    let mut embedder = HttpEmbedder::new(target(&server, "local-openai", false), 8);
    embedder.embed(&["a"]).unwrap();
    assert_eq!(embedder.fingerprint().source, "local-openai");
    assert_eq!(server.requests()[0].header("authorization"), None);
}

#[test]
fn base_url_ending_in_v1_is_not_doubled() {
    let server = embedding_server();
    let mut remote = target(&server, "remote", false);
    remote.base_url = format!("{}/v1/", server.url());
    HttpEmbedder::new(remote, 8).embed(&["a"]).unwrap();
    assert_eq!(server.requests()[0].path, "/v1/embeddings");
}

#[test]
fn texts_are_sent_in_batches_and_keep_their_order() {
    let server = embedding_server();
    let mut embedder = HttpEmbedder::new(target(&server, "remote", false), 2);
    let vectors = embedder
        .embed(&["a", "bb", "ccc", "dddd", "eeeee"])
        .unwrap();
    let lengths: Vec<f32> = vectors.iter().map(|v| v[0]).collect();
    assert_eq!(lengths, [1.0, 2.0, 3.0, 4.0, 5.0]);
    let batches: Vec<usize> = server
        .requests()
        .iter()
        .map(|r| r.json()["input"].as_array().unwrap().len())
        .collect();
    assert_eq!(batches, [2, 2, 1]);
}

#[test]
fn answers_out_of_order_are_put_back_by_index() {
    let server = FakeServer::start(|_| {
        Response::ok(json!({"data": [
            {"index": 1, "embedding": [2.0]},
            {"index": 0, "embedding": [1.0]},
        ]}))
    });
    let mut embedder = HttpEmbedder::new(target(&server, "remote", false), 8);
    assert_eq!(
        embedder.embed(&["x", "y"]).unwrap(),
        vec![vec![1.0], vec![2.0]]
    );
}

#[test]
fn server_errors_are_retried_three_times_then_reported() {
    let server = FakeServer::start(|_| Response::status(503, "перегрузка"));
    let mut embedder =
        HttpEmbedder::new(target(&server, "remote", false), 8).with_retry_pause(Duration::ZERO);
    let error = embedder.embed_query("a").unwrap_err().to_string();
    assert!(
        error.contains("503") && error.contains("перегрузка"),
        "{error}"
    );
    assert_eq!(server.requests().len(), 4, "первая попытка и три повтора");
}

#[test]
fn document_embedding_does_not_multiply_the_indexer_retry_budget() {
    use crate::indexing::indexer::{
        IndexObserver, IndexSettings, UpdateOutcome, update_index_observed,
    };
    struct NoWait;
    impl IndexObserver for NoWait {
        fn wait_retry(&self, _: Duration) -> bool {
            false
        }
    }
    for status in [429, 503, 401] {
        let server = FakeServer::start(move |_| Response::status(status, "injected"));
        let mut embedder = HttpEmbedder::new(target(&server, "local-openai", false), 8);
        // Known dimension: the test counts only document requests, not the preparation probe.
        embedder.fingerprint.dim = 2;
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("docs/a.md"), "# A\n\nText").unwrap();
        let mut index = crate::storage::Index::open(
            &crate::storage::index_path(dir.path()),
            crate::storage::Journal::Wal,
        )
        .unwrap();
        let UpdateOutcome::Updated(report) = update_index_observed(
            dir.path(),
            &mut index,
            &IndexSettings {
                chunking: Default::default(),
                docs_root: "docs".into(),
            },
            &mut embedder,
            &NoWait,
        )
        .unwrap() else {
            panic!()
        };
        let attempts = if status == 401 { 1 } else { 4 };
        assert_eq!(server.requests().len(), attempts);
        assert_eq!(report.problems[0].attempts, attempts);
        assert_eq!(index.document_count().unwrap(), 0);
    }
}

#[test]
fn a_transient_failure_that_clears_up_is_not_an_error() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let server = FakeServer::start(move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) < 2 {
            Response::status(502, "bad gateway")
        } else {
            Response::ok(json!({"data": [{"index": 0, "embedding": [1.0, 2.0]}]}))
        }
    });
    let mut embedder =
        HttpEmbedder::new(target(&server, "remote", false), 8).with_retry_pause(Duration::ZERO);
    assert_eq!(embedder.embed_query("a").unwrap(), vec![1.0, 2.0]);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[test]
fn client_errors_are_not_retried_and_name_the_cause() {
    let server = FakeServer::start(|_| {
        Response::status(400, r#"{"error":"input is too long for this model"}"#)
    });
    let mut embedder =
        HttpEmbedder::new(target(&server, "remote", false), 8).with_retry_pause(Duration::ZERO);
    let error = embedder.embed(&["a"]).unwrap_err().to_string();
    assert!(
        error.contains("400") && error.contains("too long"),
        "{error}"
    );
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn the_key_never_reaches_an_error_even_when_the_provider_echoes_it() {
    let server = FakeServer::start(|request| {
        let auth = request
            .header("authorization")
            .unwrap_or_default()
            .to_string();
        Response::status(401, &format!("неверный ключ: {auth}"))
    });
    let mut embedder = HttpEmbedder::new(target(&server, "remote", true), 8);
    let error = embedder.embed(&["a"]).unwrap_err().to_string();
    assert!(error.contains("отклонил ключ"), "{error}");
    assert!(!error.contains(KEY), "{error}");
}

#[test]
fn unreachable_address_is_an_error_not_a_panic() {
    let server = embedding_server();
    let mut remote = target(&server, "remote", false);
    drop(server);
    remote.base_url = "http://127.0.0.1:1".into();
    let mut embedder = HttpEmbedder::new(remote, 8).with_retry_pause(Duration::ZERO);
    assert!(matches!(embedder.embed(&["a"]), Err(EmbedError::Embed(_))));
}

#[test]
fn a_vector_of_another_size_than_the_first_is_rejected() {
    let server = FakeServer::start(|_| {
        Response::ok(json!({"data": [
            {"index": 0, "embedding": [1.0, 2.0]},
            {"index": 1, "embedding": [1.0]},
        ]}))
    });
    let mut embedder = HttpEmbedder::new(target(&server, "remote", false), 8);
    assert!(matches!(
        embedder.embed(&["a", "b"]),
        Err(EmbedError::Dimension {
            expected: 2,
            got: 1
        })
    ));
}

#[test]
fn a_short_answer_is_a_count_error() {
    let server = FakeServer::start(|_| Response::ok(json!({"data": [{"embedding": [1.0]}]})));
    let mut embedder = HttpEmbedder::new(target(&server, "remote", false), 8);
    assert!(matches!(
        embedder.embed(&["a", "b"]),
        Err(EmbedError::Count {
            expected: 2,
            got: 1
        })
    ));
}

fn rerank_server(path: &'static str) -> FakeServer {
    FakeServer::start(move |request| {
        if request.path != path {
            return Response::status(404, "not found");
        }
        let docs = request.json()["documents"].as_array().unwrap().len();
        // Чем позже документ, тем выше оценка; порядок ответа — по убыванию, как у Cohere.
        let results: Vec<_> = (0..docs)
            .rev()
            .map(|index| json!({"index": index, "relevance_score": (index as f64 + 1.0) / 10.0}))
            .collect();
        Response::ok(json!({ "results": results }))
    })
}

#[test]
fn rerank_posts_cohere_shaped_request_and_returns_scores_in_document_order() {
    let server = rerank_server("/rerank");
    let mut reranker = HttpReranker::new(target(&server, "remote", true));
    let scores = reranker.rerank("запрос", &["a", "b", "c"]).unwrap();
    assert_eq!(scores, [0.1, 0.2, 0.3]);

    let request = &server.requests()[0];
    assert_eq!(request.path, "/rerank");
    assert_eq!(request.json()["query"], "запрос");
    assert_eq!(request.json()["documents"], json!(["a", "b", "c"]));
    assert_eq!(request.json()["top_n"], 3);
    assert_eq!(request.json()["model"], "emb-1");
    assert_eq!(
        request.header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

#[test]
fn rerank_falls_back_to_v1_rerank_and_remembers_it() {
    let server = rerank_server("/v1/rerank");
    let mut reranker = HttpReranker::new(target(&server, "local-openai", false));
    reranker.rerank("q", &["a", "b"]).unwrap();
    reranker.rerank("q", &["a", "b"]).unwrap();
    let paths: Vec<_> = server.requests().into_iter().map(|r| r.path).collect();
    assert_eq!(paths, ["/rerank", "/v1/rerank", "/v1/rerank"]);
}

#[test]
fn rerank_logits_outside_the_unit_range_go_through_the_sigmoid() {
    let server = FakeServer::start(|_| {
        Response::ok(json!({"results": [
            {"index": 0, "relevance_score": 4.0},
            {"index": 1, "relevance_score": -4.0},
        ]}))
    });
    let mut reranker = HttpReranker::new(target(&server, "local-openai", false));
    let scores = reranker.rerank("q", &["a", "b"]).unwrap();
    assert!(scores[0] > 0.98 && scores[1] < 0.02, "{scores:?}");
}

#[test]
fn rerank_errors_name_the_status() {
    let server = FakeServer::start(|_| Response::status(500, "сломалось"));
    let mut reranker = HttpReranker::new(target(&server, "remote", false));
    let error = reranker.rerank("q", &["a"]).unwrap_err().to_string();
    assert!(
        error.contains("500") && error.contains("сломалось"),
        "{error}"
    );
}

#[test]
fn rerank_with_a_missing_score_is_an_error() {
    let server = FakeServer::start(|_| {
        Response::ok(json!({"results": [{"index": 0, "relevance_score": 0.5}]}))
    });
    let mut reranker = HttpReranker::new(target(&server, "remote", false));
    assert!(reranker.rerank("q", &["a", "b"]).is_err());
}
