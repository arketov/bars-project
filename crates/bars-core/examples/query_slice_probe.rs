//! Только чтение: настоящие ответы поиска и готовый индекс, без новых моделей/индексации.
use bars_core::{
    graph::{self, LayerState, Scope, centroid, physical, query_slice},
    search::{SearchSettings, SimilarRequest, similar},
    storage::{Index, index_path},
};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = PathBuf::from(args.next().ok_or("project")?);
    let output = PathBuf::from(args.next().ok_or("output")?);
    let index = Index::open_read_only(&index_path(&root))?;
    let data = graph::store::load(index.conn())?;
    index.conn().execute_batch("BEGIN")?;
    let vectors = centroid::document_representations(&index, &data, || false)?;
    let mut report = vec![];
    for file in args {
        let text = std::fs::read_to_string(file)?;
        let start = text.find('{').ok_or("JSON response")?;
        let reply: Value = serde_json::from_str(&text[start..])?;
        let mut hits = vec![];
        for hit in reply["results"].as_array().ok_or("results")? {
            // Старый установленный воркер ещё не отдаёт source_hash. Проверяем его
            // непрозрачный chunk_id перед чтением хеша из текущего снимка SQLite.
            let source = similar(
                &index,
                &SearchSettings::default(),
                &SimilarRequest {
                    id: hit["chunk_id"].as_str().ok_or("chunk_id")?.into(),
                    results: Some(1),
                    ..Default::default()
                },
            )?
            .source;
            hits.push(query_slice::QueryHit {
                source_hash: source.source_hash,
                channel: Default::default(),
                path: source.path,
                title: source.title,
                heading: source.heading_chain,
                first: source.first_line,
                last: source.last_line,
                score: hit["score"].as_f64().map(|v| v as f32),
            });
        }
        let mode = reply["applied_mode"].as_str().unwrap_or("hybrid");
        let bundle = if mode == "graph" {
            &reply["graph_results"]
        } else {
            &reply["related"]
        };
        if let Some(sources) = bundle["sources"].as_array() {
            for source in sources {
                let path = source["path"].as_str().ok_or("source path")?;
                if hits.iter().any(|h| h.path == path) {
                    continue;
                }
                hits.push(query_slice::QueryHit {
                    source_hash: source["hash"].as_str().unwrap_or_default().into(),
                    channel: if mode == "graph" {
                        query_slice::QueryChannel::Graph
                    } else {
                        query_slice::QueryChannel::Related
                    },
                    path: path.into(),
                    title: source["title"].as_str().unwrap_or(path).into(),
                    heading: String::new(),
                    first: 0,
                    last: 0,
                    score: None,
                });
            }
        }
        let request = query_slice::QuerySliceRequest {
            id: format!("real-{}", report.len()),
            text: reply["_probe_query"].as_str().ok_or("query")?.into(),
            scope: Scope::default(),
            hits,
            score_kind: if mode == "graph" {
                query_slice::QueryScoreKind::None
            } else if reply["applied"]["score_kind"] == "rerank" {
                query_slice::QueryScoreKind::Rerank
            } else {
                query_slice::QueryScoreKind::Cosine
            },
            depth: 1,
            fanout: 4,
            min_strength: 0.85,
            max_nodes: 200,
            max_edges: 800,
            max_bytes: 262144,
            snapshot: None,
        };
        let layer = || physical::Layer {
            state: LayerState::Partial,
            note: "Настоящий готовый индекс; полнота не утверждается".into(),
        };
        let started = Instant::now();
        let slice = query_slice::project(
            &data,
            &request,
            physical::LayerInputs {
                markdown_link: layer(),
                document_similarity: layer(),
                chunk_similarity: layer(),
            },
            |id, targets| {
                centroid::exact_document_pairs(&vectors, id, targets, request.min_strength, || {
                    false
                })
            },
        )?;
        assert!(slice.used_bytes <= request.max_bytes);
        assert!(slice.query_edges.iter().all(|e| {
            slice
                .results
                .iter()
                .any(|r| r.node == Some(e.to) && r.channel != query_slice::QueryChannel::Related)
        }));
        report.push(json!({"request":request,"slice":slice,"elapsed_ms":started.elapsed().as_millis(),"bundle":bundle}));
    }
    std::fs::write(output, serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
