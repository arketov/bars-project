use super::*;
use crate::models::testing::{FakeServer, Response};

fn target(server: &FakeServer, source: &'static str) -> RemoteTarget {
    RemoteTarget {
        source,
        base_url: server.url(),
        model: "Qwen3-Reranker-0.6B".into(),
        key: crate::secrets::KeyRef {
            name: None,
            env: None,
            store: std::sync::Arc::new(crate::secrets::MemorySecrets::default()),
        },
    }
}

fn token(yes: f64, no: f64) -> Value {
    json!({"token":"yes", "logprob":yes, "top_logprobs":[{"token":"yes","logprob":yes},{"token":"no","logprob":no}]})
}

fn ollama(token: Value) -> Value {
    json!({"done":true,"eval_count":1,"prompt_eval_count":100,"logprobs":[token]})
}

#[test]
fn softmax_is_stable_and_missing_class_uses_lowest_logprob() {
    assert!((score(&token(-1000.0, -1001.0), true).unwrap() - 0.7310586).abs() < 1e-6);
    let one =
        json!({"top_logprobs":[{"token":"yes","logprob":-1.0},{"token":"other","logprob":-20.0}]});
    assert!(score(&one, true).unwrap() > 0.99999);
    assert!(
        score(
            &json!({"top_logprobs":[{"token":"other","logprob":-1.0}]}),
            true
        )
        .is_err()
    );
    assert!(score(&token(0.1, -1.0), true).is_err());
    assert!(score(&json!({"top_logprobs":[{"token":"yes","logprob":-1.0},{"token":"yes","logprob":-2.0}]}),true).is_err());
}

#[test]
fn variants_are_summed_only_for_non_qwen_models() {
    let value = json!({"top_logprobs":[{"token":"Yes","logprob":-2.0},{"token":" yes","logprob":-2.0},{"token":"no","logprob":-2.0}]});
    assert!((score(&value, false).unwrap() - 2.0 / 3.0).abs() < 1e-6);
    assert!(score(&value, true).unwrap() < 0.001);
}

#[test]
fn ollama_raw_prompt_options_and_order_are_preserved() {
    let server = FakeServer::start(|request| {
        let body = request.json();
        assert_eq!(request.path, "/api/generate");
        assert_eq!(body["raw"], true);
        assert_eq!(body["options"]["repeat_penalty"], 1);
        assert_eq!(body["options"]["num_predict"], 1);
        assert_eq!(body["options"]["num_ctx"], 32768);
        let prompt = body["prompt"].as_str().unwrap();
        assert!(prompt.ends_with(SUFFIX));
        if prompt.contains("медленно") {
            std::thread::sleep(Duration::from_millis(60));
        }
        Response::ok(ollama(if prompt.contains("медленно") {
            token(-0.1, -2.0)
        } else {
            token(-2.0, -0.1)
        }))
    });
    let mut model = LogprobReranker::new(target(&server, "ollama"), 2, 32768);
    let scores = model.rerank("запрос", &["медленно", "быстро"]).unwrap();
    assert!(scores[0] > scores[1]);
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn lmstudio_responses_endpoint_and_reasoning_rejection() {
    let server = FakeServer::start(|request| {
        if request.method == "GET" {
            assert_eq!(request.path, "/api/v0/models");
            return Response::ok(
                json!({"data":[{"id":"Qwen3-Reranker-0.6B","loaded_context_length":32768}]}),
            );
        }
        assert_eq!(request.path, "/v1/responses");
        let body = request.json();
        assert_eq!(body["include"], json!(["message.output_text.logprobs"]));
        assert_eq!(body["reasoning"]["effort"], "none");
        assert_eq!(body["store"], false);
        assert_eq!(body["truncation"], "disabled");
        if body["input"][1]["content"]
            .as_str()
            .unwrap()
            .contains("reasoning")
        {
            Response::ok(
                json!({"usage":{"output_tokens":1,"input_tokens":100},"output":[{"type":"reasoning"},{"type":"message","content":[{"type":"output_text","logprobs":[token(-0.1,-2.0)]}]}]}),
            )
        } else {
            Response::ok(
                json!({"usage":{"output_tokens":1,"input_tokens":100},"output":[{"type":"message","content":[{"type":"output_text","logprobs":[token(-0.1,-2.0)]}]}]}),
            )
        }
    });
    let mut config = target(&server, "lmstudio");
    config.base_url.push_str("/v1/");
    let mut model = LogprobReranker::new(config, 4, 32768);
    assert!(model.rerank("q", &["ok"]).unwrap()[0] > 0.8);
    assert!(model.rerank("q", &["reasoning"]).is_err());
}

#[test]
fn a_failed_pair_discards_all_scores_and_never_retries() {
    let server = FakeServer::start(|request| {
        if request.body.contains("bad") {
            Response::status(500, "error")
        } else {
            Response::ok(ollama(token(-0.1, -2.0)))
        }
    });
    let mut model = LogprobReranker::new(target(&server, "ollama"), 1, 32768);
    assert!(model.rerank("q", &["ok", "bad", "never"]).is_err());
    assert_eq!(server.requests().len(), 2);
}

#[test]
fn common_deadline_includes_queued_pairs() {
    let server = FakeServer::start(|_| {
        std::thread::sleep(Duration::from_millis(90));
        Response::ok(ollama(token(-0.1, -2.0)))
    });
    let mut model = LogprobReranker::new(target(&server, "ollama"), 1, 32768);
    model.timeout = Duration::from_millis(140);
    let start = Instant::now();
    assert!(model.rerank("q", &["one", "two", "three"]).is_err());
    assert!(start.elapsed() < Duration::from_millis(500));
    assert!(server.requests().len() <= 2);
}

#[test]
fn oversized_input_fails_before_http_and_empty_input_sends_nothing() {
    let server = FakeServer::start(|_| panic!("must not send"));
    let mut model = LogprobReranker::new(target(&server, "ollama"), 4, 512);
    assert!(model.rerank("q", &[&"я".repeat(1000)]).is_err());
    assert_eq!(model.rerank("q", &[]).unwrap(), Vec::<f32>::new());
    assert!(server.requests().is_empty());
}

#[test]
fn lmstudio_checks_loaded_context_instead_of_trusting_truncation_flag() {
    let server = FakeServer::start(|request| {
        assert_eq!(request.method, "GET");
        Response::ok(json!({"data":[{"id":"Qwen3-Reranker-0.6B","loaded_context_length":512}]}))
    });
    let mut model = LogprobReranker::new(target(&server, "lmstudio"), 4, 32768);
    assert!(model.rerank("q", &[&"я".repeat(1000)]).is_err());
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn pool_never_exceeds_parallel_limit() {
    let active = std::sync::Arc::new(AtomicUsize::new(0));
    let maximum = std::sync::Arc::new(AtomicUsize::new(0));
    let server = FakeServer::start({
        let active = active.clone();
        let maximum = maximum.clone();
        move |_| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(current, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(20));
            active.fetch_sub(1, Ordering::SeqCst);
            Response::ok(ollama(token(-0.1, -2.0)))
        }
    });
    let mut model = LogprobReranker::new(target(&server, "ollama"), 2, 32768);
    assert_eq!(
        model.rerank("q", &["a", "b", "c", "d", "e"]).unwrap().len(),
        5
    );
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
}
