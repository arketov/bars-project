//! Research driver: isolated frozen index only; existing query model, no reindex/download.
use bars_core::{
    graph::{self, *},
    models::{EmbedError, Embedder, ModelFingerprint, bge_m3_fingerprint},
    search::{SearchRequest, SearchSettings, search, search_expanded},
    storage::{Index, Journal},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, time::Instant};

struct Replay {
    fingerprint: ModelFingerprint,
    vector: Vec<f32>,
}
struct LocalModel(fastembed::TextEmbedding, ModelFingerprint);
impl LocalModel {
    #[cfg(windows)]
    fn load(models: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        use fastembed::{
            EmbeddingModel, Pooling, TextEmbedding, TextInitOptions, TokenizerFiles,
            UserDefinedEmbeddingModel,
        };
        let repo = hf_hub::Cache::new(models.into()).model("BAAI/bge-m3".into());
        let bytes = |name: &str| -> Result<Vec<u8>, Box<dyn std::error::Error>> {
            Ok(std::fs::read(repo.get(name).ok_or_else(|| {
                format!("local artifact missing: {name}")
            })?)?)
        };
        let tokenizer = TokenizerFiles {
            config_file: bytes("config.json")?,
            tokenizer_file: bytes("tokenizer.json")?,
            special_tokens_map_file: bytes("special_tokens_map.json")?,
            tokenizer_config_file: bytes("tokenizer_config.json")?,
        };
        let model = UserDefinedEmbeddingModel::new(bytes("onnx/model.onnx")?, tokenizer)
            .with_pooling(Pooling::Cls)
            .with_external_initializer("model.onnx_data".into(), bytes("onnx/model.onnx_data")?)
            .with_external_initializer(
                "Constant_7_attr__value".into(),
                bytes("onnx/Constant_7_attr__value")?,
            );
        let options = TextInitOptions::new(EmbeddingModel::BGEM3)
            .with_max_length(8192)
            .with_execution_providers(vec![
                ort::ep::DirectML::default()
                    .with_device_id(0)
                    .build()
                    .error_on_failure(),
            ]);
        Ok(Self(
            TextEmbedding::try_new_from_user_defined(model, options.into())?,
            bge_m3_fingerprint(),
        ))
    }
    #[cfg(not(windows))]
    fn load(_: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        Err("this measurement requires Windows DirectML".into())
    }
}
impl Embedder for LocalModel {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.1
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        self.0
            .embed(texts, Some(1))
            .map_err(|e| EmbedError::Embed(e.to_string()))
    }
}
impl Embedder for Replay {
    fn fingerprint(&self) -> &ModelFingerprint {
        &self.fingerprint
    }
    fn embed(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        if texts.len() != 1 {
            return Err(EmbedError::Embed("query only".into()));
        }
        Ok(vec![self.vector.clone()])
    }
}
fn read(p: &Path) -> Result<Value, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(p)?)?)
}
fn write(p: &Path, v: &impl serde::Serialize) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)?;
    f.write_all(&serde_json::to_vec(v)?)?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if !((args.len() == 3 && args[1] == "freeze") || (args.len() == 6 && args[1] == "run")) {
        return Err("freeze <snapshot> | run <snapshot> <gold> <local-models> <NEW-result>".into());
    }
    let command = &args[1];
    let root = Path::new(&args[2]).canonicalize()?;
    if root.join(".bars").exists() || !root.join("freeze.json").is_file() {
        return Err("only an isolated graph-quality-freeze snapshot is allowed".into());
    }
    if command == "freeze" {
        let manifest = read(&root.join("freeze.json"))?;
        let mut index = Index::open(&root.join("index.sqlite"), Journal::Delete)?;
        let selected: BTreeMap<_, _> = manifest["selected"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| {
                (
                    d["path"].as_str().unwrap().to_string(),
                    d["hash"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let mut exclusions = Vec::new();
        for doc in index.documents()? {
            let valid = selected.get(&doc.path).is_some_and(|hash| {
                std::fs::read(root.join(&doc.path))
                    .is_ok_and(|bytes| blake3::hash(&bytes).to_hex().as_str() == hash)
            });
            if !valid {
                index.delete_document(&doc.path)?;
                exclusions.push(doc.path);
            }
        }
        index.remove_orphan_vectors()?;
        index.conn().execute_batch("VACUUM")?;
        store::ensure_schema(index.conn_mut())?;
        store::set_state(index.conn(), LayerState::Stale, None, None)?;
        let start = Instant::now();
        graph::build_index_graph(&root, &mut index, || false)?;
        let data = store::load(index.conn())?;
        write(&root.join("dataset.json"), &data)?;
        write(
            &root.join("corpus.json"),
            &json!({"sources":data.sources.iter().map(|s| &s.meta).collect::<Vec<_>>(),"excluded":exclusions,"build_ms":start.elapsed().as_secs_f64()*1000.0,"snapshot":data.snapshot}),
        )?;
        println!(
            "frozen: {} documents, {} chunks",
            data.sources.len(),
            index.chunk_count()?
        );
        return Ok(());
    }
    let index = Index::open_read_only(&root.join("index.sqlite"))?;
    if index.stored_fingerprint()?.as_ref() != Some(&bge_m3_fingerprint()) {
        return Err("this round requires the frozen BAAI/bge-m3 FP32 fingerprint".into());
    }
    let data: Dataset = serde_json::from_slice(&std::fs::read(root.join("dataset.json"))?)?;
    let mut structural = data.clone();
    structural.edges.retain(|e| {
        matches!(
            e.predicate,
            Predicate::Contains | Predicate::Covers | Predicate::NextChunk | Predicate::LinksTo
        )
    });
    let gold = read(Path::new(&args[3]))?;
    let cache = root.join("query-vectors.json");
    let vectors: BTreeMap<String, Vec<f32>> = if cache.exists() {
        serde_json::from_slice(&std::fs::read(&cache)?)?
    } else {
        let models = Path::new(&args[4]);
        // Constructor reads local artifacts; is_downloaded prevents an implicit fetch.
        if !bars_core::models::is_downloaded(models, "BAAI/bge-m3") {
            return Err("local bge-m3 required".into());
        }
        let mut model = LocalModel::load(models)?;
        let mut vectors = BTreeMap::new();
        for case in gold["cases"].as_array().unwrap() {
            let q = case["query"].as_str().unwrap();
            let start = Instant::now();
            vectors.insert(q.to_string(), model.embed_query(q)?);
            eprintln!(
                "embed {} {:.0} ms",
                case["id"],
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        write(&cache, &vectors)?;
        write(
            &root.join("query-model.json"),
            &json!({"device":"DirectML device 0; EP error_on_failure; no provider fallback","loading":"local hf_hub::Cache only; fastembed user-defined, FP32, CLS, max_length=8192; existing BAAI artifacts","query_count":vectors.len(),"model":"BAAI/bge-m3","rerank":false,"documents_embedded":0}),
        )?;
        vectors
    };
    let mut rows = Vec::new();
    for case in gold["cases"].as_array().unwrap() {
        for ev in case["evidence"].as_array().unwrap() {
            if !data
                .sources
                .iter()
                .any(|s| s.meta.docs_path == ev["path"].as_str().unwrap())
            {
                return Err(format!("gold source excluded: {}", ev["path"]).into());
            }
        }
        let q = case["query"].as_str().unwrap();
        let mut embedder = Replay {
            fingerprint: index.stored_fingerprint()?.ok_or("missing fingerprint")?,
            vector: vectors[q].clone(),
        };
        let mut graph_request: GraphRequest = serde_json::from_value(case["request"].clone())?;
        if let Some(paths) = case["root_paths"].as_array() {
            graph_request.roots = paths
                .iter()
                .map(|p| {
                    data.sources
                        .iter()
                        .find(|s| s.meta.docs_path == p.as_str().unwrap())
                        .unwrap()
                        .meta
                        .reference
                        .clone()
                })
                .collect();
        }
        let start = Instant::now();
        let scores = if graph_request.operation == Operation::Context {
            graph::context::evidence_scores(
                &index,
                &data,
                &mut embedder,
                &SearchSettings::default(),
                &graph_request,
            )?
            .0
        } else {
            BTreeMap::new()
        };
        let reply =
            query::execute_ranked(&data, &graph_request, &scores, start.elapsed(), || false);
        let graph_ms = start.elapsed().as_secs_f64() * 1000.0;
        let modules_reply = if graph_request.operation == Operation::Explore
            && graph_request.analysis == Analysis::Topics
        {
            let mut request = graph_request.clone();
            request.grouping = Grouping::Modules;
            Some(query::execute(&data, &request, || false))
        } else {
            None
        };
        let mut variants = Vec::new();
        for (name, candidates, results, expand) in [
            ("hybrid", 30, 5, false),
            ("hybrid_wide", 100, 20, false),
            ("structural", 30, 5, true),
            ("mix", 30, 5, true),
        ] {
            let request = SearchRequest {
                query: q.into(),
                files: data
                    .sources
                    .iter()
                    .map(|s| s.meta.docs_path.clone())
                    .collect(),
                candidates: Some(candidates),
                results: Some(results),
                ..Default::default()
            };
            let mut related = None;
            let start = Instant::now();
            let mut expansion = |idx: &Index, seeds: &[i64], limit| {
                let graph_data = if name == "structural" {
                    &structural
                } else {
                    &data
                };
                let (ids, reply) = graph::retrieval::expand(
                    idx,
                    graph_data,
                    &request,
                    seeds,
                    limit,
                    Budgets::default(),
                )?;
                related = Some(reply);
                Ok(ids)
            };
            let result = if expand {
                search_expanded(
                    &index,
                    &mut embedder,
                    None,
                    &SearchSettings::default(),
                    &request,
                    Some(&mut expansion),
                )?
            } else {
                search(
                    &index,
                    &mut embedder,
                    None,
                    &SearchSettings::default(),
                    &request,
                )?
            };
            variants.push(json!({"name":name,"elapsed_ms":start.elapsed().as_secs_f64()*1000.0,"candidates":result.applied.candidates,"results_limit":result.applied.results,"min_score":result.applied.min_score,"score_kind":format!("{:?}",result.applied.score_kind),"hits":result.hits.iter().map(|h|json!({"path":h.path,"first_line":h.first_line,"last_line":h.last_line,"text":h.text,"score":h.score,"id":h.public_id})).collect::<Vec<_>>(),"related":related}));
        }
        rows.push(json!({"id":case["id"],"graph":match reply { Ok(r)=>json!(r), Err(e)=>json!({"error":e.to_string()}) },"graph_modules":modules_reply.map(|r|match r{Ok(r)=>json!(r),Err(e)=>json!({"error":e.to_string()})}),"graph_ms":graph_ms,"variants":variants}));
    }
    let mut probes = Vec::new();
    for (name, request) in [
        (
            "context_roots",
            json!({"operation":"context","query":"модель","roots":[data.sources[0].meta.reference]}),
        ),
        (
            "context_tokenizer",
            json!({"operation":"context","query":"модель","tokenizer_id":"exact"}),
        ),
        (
            "impact_change_description",
            json!({"operation":"impact","roots":[data.sources[0].meta.reference],"change_description":"изменение"}),
        ),
        (
            "context_include_related",
            json!({"operation":"context","query":"модель","include_related":true}),
        ),
        (
            "graph_edge_types",
            json!({"operation":"graph","edge_types":["links_to"]}),
        ),
        (
            "graph_include_unresolved",
            json!({"operation":"graph","include_unresolved":true}),
        ),
    ] {
        let outcome = match serde_json::from_value::<GraphRequest>(request) {
            Ok(r) => query::execute(&data, &r, || false)
                .map(|_| "accepted".into())
                .unwrap_or_else(|e| e.to_string()),
            Err(e) => e.to_string(),
        };
        probes.push(json!({"name":name,"outcome":outcome}));
    }
    write(
        Path::new(&args[5]),
        &json!({"snapshot":data.snapshot,"rows":rows,"contract_probes":probes}),
    )?;
    Ok(())
}
