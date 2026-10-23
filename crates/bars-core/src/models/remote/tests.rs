use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use super::*;
use crate::models::testing::{FakeServer, Response};
use crate::secrets::{KeyringSecrets, SecretStore};

const KEY: &str = "sk-test-not-a-real-key-123456";

#[test]
fn tei_uses_native_batches_and_qwen_query_instruction() {
    let server = FakeServer::start(|request| {
        assert_eq!(request.path, "/embed");
        let body = request.json();
        assert_eq!(body["truncate"], false);
        assert_eq!(body["normalize"], true);
        assert!(body.get("model").is_none());
        let inputs = body["inputs"].as_array().unwrap();
        Response::ok(json!(
            inputs.iter().map(|_| vec![1.0, 2.0]).collect::<Vec<_>>()
        ))
    });
    let mut config = target(&server, "tei", true);
    config.model = "Qwen/Qwen3-Embedding-0.6B".into();
    let mut embedder = HttpEmbedder::new(config, 2);
    assert_eq!(embedder.embed(&["а", "б", "в"]).unwrap().len(), 3);
    embedder.embed_query("поиск").unwrap();
    assert_eq!(embedder.fingerprint().source, "tei");
    assert_eq!(embedder.fingerprint().dim, 2);
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].json()["inputs"], json!(["а", "б"]));
    assert_eq!(
        requests[2].json()["inputs"],
        json!([format!(
            "{}поиск",
            super::super::catalog::QWEN3_QUERY_PREFIX
        )])
    );
    assert_eq!(
        requests[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
}

#[test]
fn tei_rejects_bad_embedding_responses() {
    for response in [
        json!([[]]),
        json!([[0.0, 0.0]]),
        json!([[1e100]]),
        json!([["a"]]),
        json!({"data": []}),
        json!([]),
        json!([[1.0], [1.0, 2.0]]),
    ] {
        let inputs = if response.as_array().is_some_and(|items| items.len() == 2) {
            vec!["а", "б"]
        } else {
            vec!["а"]
        };
        let server = FakeServer::start(move |_| Response::ok(response.clone()));
        let mut embedder = HttpEmbedder::new(target(&server, "tei", false), 2);
        assert!(embedder.embed(&inputs).is_err());
    }
}

#[test]
fn tei_native_rerank_restores_order_and_requests_all_scores() {
    let server = FakeServer::start(|request| {
        assert_eq!(request.path, "/rerank");
        assert_eq!(
            request.json(),
            json!({"query":"запрос", "texts":["а", "б"],
            "truncate":false, "raw_scores":false, "return_text":false})
        );
        Response::ok(json!([{"index":1,"score":0.9},{"index":0,"score":0.2}]))
    });
    let mut reranker = HttpReranker::new(target(&server, "tei", false));
    assert_eq!(reranker.rerank("запрос", &["а", "б"]).unwrap(), [0.2, 0.9]);
}

#[test]
fn tei_malformed_embedding_does_not_echo_a_key() {
    let server = FakeServer::start(|_| Response::ok(json!([[KEY]])));
    let mut embedder = HttpEmbedder::new(target(&server, "tei", true), 2);
    let message = embedder.embed(&["текст"]).unwrap_err().to_string();
    assert!(!message.contains(KEY));
}

#[test]
fn local_qwen_query_has_instruction_but_documents_do_not() {
    let server = embedding_server();
    let mut config = target(&server, "local-openai", false);
    config.model = "Qwen3-Embedding-0.6B".into();
    let mut embedder = HttpEmbedder::new(config, 2);
    embedder.embed(&["документ"]).unwrap();
    embedder.embed_query("запрос").unwrap();
    assert_eq!(server.requests()[0].json()["input"], json!(["документ"]));
    assert_eq!(
        server.requests()[1].json()["input"],
        json!([format!(
            "{}запрос",
            super::super::catalog::QWEN3_QUERY_PREFIX
        )])
    );
}

#[test]
fn tei_rerank_rejects_invalid_scores_indices_and_shapes() {
    for response in [
        json!([{"index":0,"score":0.1},{"index":0,"score":0.2}]),
        json!([{"index":2,"score":0.1}]),
        json!([{"index":0,"score":0.1}]),
        json!([{"index":0,"score":1e100},{"index":1,"score":0.2}]),
        json!([{"index":0,"score":-0.1},{"index":1,"score":0.2}]),
        json!({"results":[{"index":0,"score":0.1}]}),
    ] {
        let server = FakeServer::start(move |_| Response::ok(response.clone()));
        let mut reranker = HttpReranker::new(target(&server, "tei", false));
        assert!(reranker.rerank("запрос", &["а", "б"]).is_err());
    }
}

#[test]
fn tei_context_error_and_missing_route_do_not_probe_other_protocols() {
    for status in [400, 413, 404, 422] {
        let server =
            FakeServer::start(move |_| Response::status(status, "input too long or route missing"));
        let mut embedder = HttpEmbedder::new(target(&server, "tei", false), 2);
        assert!(matches!(
            embedder.embed_query("текст"),
            Err(EmbedError::Rejected(_))
        ));
        let mut reranker = HttpReranker::new(target(&server, "tei", false));
        assert!(reranker.rerank("запрос", &["текст"]).is_err());
        assert_eq!(server.requests().len(), 2);
    }
}

#[test]
fn lmstudio_batches_sort_response_and_instruct_only_qwen_queries() {
    for model in [
        "text-embedding-qwen3-embedding-0.6b",
        "Qwen/Qwen3-Embedding-4B-GGUF",
        "nomic-embed-text-v1.5",
    ] {
        let server = FakeServer::start(|request| {
            assert_eq!(request.path, "/v1/embeddings");
            let input = request.json()["input"].as_array().unwrap().clone();
            let data: Vec<_> = input.iter().enumerate().rev().map(|(index, text)|
                json!({"index":index,"embedding":[text.as_str().unwrap().len() as f64, 1.0]})).collect();
            Response::ok(json!({"data":data}))
        });
        let mut config = target(&server, "lmstudio", false);
        config.base_url.push_str("/v1/");
        config.model = model.into();
        let mut embedder = HttpEmbedder::new(config, 2);
        assert_eq!(
            embedder.embed(&["а", "abc", "abcd"]).unwrap(),
            vec![vec![2.0, 1.0], vec![3.0, 1.0], vec![4.0, 1.0]]
        );
        embedder.embed_query("запрос").unwrap();
        let requests = server.requests();
        let expected = if model.contains("qwen3") || model.contains("Qwen3") {
            format!("{}запрос", super::super::catalog::QWEN3_QUERY_PREFIX)
        } else {
            "запрос".into()
        };
        assert_eq!(requests[2].json()["input"], json!([expected]));
        assert_eq!(embedder.fingerprint().source, "lmstudio");
    }
}

#[test]
fn lmstudio_rejects_corrupt_vectors_and_response_indices() {
    for data in [
        json!([{"embedding":[]}]),
        json!([{"embedding":[1e100]}]),
        json!([{"embedding":[1],"index":-1}]),
        json!([{"embedding":[1],"index":2}]),
        json!([{"embedding":[1],"index":0},{"embedding":[2],"index":0}]),
    ] {
        let server = FakeServer::start(move |_| Response::ok(json!({"data":data})));
        let mut embedder = HttpEmbedder::new(target(&server, "lmstudio", false), 2);
        assert!(embedder.embed_query("текст").is_err());
        assert_eq!(server.requests().len(), 1);
    }
}

#[test]
fn ollama_batches_use_native_api_and_never_truncate() {
    let server = FakeServer::start(|request| {
        assert_eq!(request.path, "/api/embed");
        assert_eq!(request.json()["truncate"], false);
        assert_eq!(request.header("authorization"), None);
        let vectors: Vec<_> = request.json()["input"]
            .as_array()
            .unwrap()
            .iter()
            .map(|text| json!([text.as_str().unwrap().len() as f64, 1.0]))
            .collect();
        Response::ok(json!({"embeddings": vectors}))
    });
    let mut config = target(&server, "ollama", false);
    config.model = "qwen3-embedding:0.6b".into();
    let mut embedder = HttpEmbedder::new(config, 2);
    assert_eq!(
        embedder.embed(&["а", "abc", "abcd"]).unwrap(),
        vec![vec![2.0, 1.0], vec![3.0, 1.0], vec![4.0, 1.0]]
    );
    embedder.embed_query("запрос").unwrap();
    assert_eq!(embedder.fingerprint().source, "ollama");
    assert_eq!(embedder.fingerprint().dim, 2);
    let requests = server.requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0].json()["input"], json!(["а", "abc"]));
    assert_eq!(
        requests[2].json()["input"],
        json!([format!(
            "{}запрос",
            super::super::catalog::QWEN3_QUERY_PREFIX
        )])
    );
}

#[test]
fn ollama_rejects_bad_vectors_and_counts() {
    for response in [
        json!({"embeddings": [[]]}),
        json!({"embeddings": [[1e100]]}),
        json!({"embeddings": [["a"]]}),
        json!({"data": []}),
        json!({"embeddings": []}),
    ] {
        let server = FakeServer::start(move |_| Response::ok(response.clone()));
        let mut embedder = HttpEmbedder::new(target(&server, "ollama", false), 2);
        assert!(embedder.embed(&["текст"]).is_err());
    }
}

#[test]
fn ollama_context_error_is_not_retried() {
    let server = FakeServer::start(|_| Response::status(400, "input length exceeds context"));
    let mut embedder =
        HttpEmbedder::new(target(&server, "ollama", false), 2).with_retry_pause(Duration::ZERO);
    assert!(matches!(
        embedder.embed_query("длинный вход"),
        Err(EmbedError::Rejected(_))
    ));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn ollama_dimension_changes_are_rejected() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&calls);
    let server = FakeServer::start(move |_| {
        let dim = 2 + count.fetch_add(1, Ordering::SeqCst);
        Response::ok(json!({"embeddings": [vec![1.0; dim]]}))
    });
    let mut embedder = HttpEmbedder::new(target(&server, "ollama", false), 2);
    embedder.embed_query("первый").unwrap();
    assert!(matches!(
        embedder.embed_query("второй"),
        Err(EmbedError::Dimension {
            expected: 2,
            got: 3
        })
    ));
}

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
