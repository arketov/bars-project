use bars_core::{models::{Embedder, ModelFingerprint, BuiltinEmbedder, Accelerator},
    indexing::{indexer::{update_index, IndexSettings, UpdateOutcome}, chunking::ChunkingParams},
    storage::{Index, Journal}, search::{SearchSettings, SearchRequest, search}};
use llama_cpp_2::{llama_backend::LlamaBackend, model::{LlamaModel,params::LlamaModelParams}, context::params::{LlamaContextParams,LlamaPoolingType}};
use std::{fs::{self,File},io::Write,path::Path,time::Instant,num::NonZeroU32};
#[path = "../../../crates/bars-core/examples/quality/mod.rs"]
#[allow(dead_code)]
mod quality;

pub fn run(a: &[String]) -> Result<(),Box<dyn std::error::Error>> {
    if a.len()!=5 { return Err("bench <gguf|bge-models-dir> <cpu|gpu|bge> <frozen-project> <cases.json> <new-output>".into()); }
    let out=Path::new(&a[4]);
    if out.exists() {return Err("нужен новый output".into());}
    let cases=quality::load_cases(Path::new(&a[3]))?;
    quality::validate_cases(Path::new(&a[2]), &cases)?;
    fs::create_dir_all(out)?;
    let start=Instant::now();
    if a[1]=="bge" {
        let mut model=BuiltinEmbedder::bge_m3(Path::new(&a[0]),Accelerator::Auto)?;
        return measure(a,&cases,&mut model,start.elapsed().as_secs_f64());
    }
    if !["cpu","gpu"].contains(&a[1].as_str()) || (a[1]=="gpu" && !cfg!(feature="vulkan")) { return Err("неподдерживаемое устройство".into()); }
    let backend=LlamaBackend::init()?;
    let model=LlamaModel::load_from_file(&backend,&a[0], &LlamaModelParams::default().with_n_gpu_layers(if a[1]=="gpu" {1000} else {0}))?;
    let ctx=model.new_context(&backend,LlamaContextParams::default().with_n_ctx(NonZeroU32::new(8192)).with_n_batch(8192).with_n_ubatch(512).with_n_threads(8).with_n_threads_batch(8).with_embeddings(true).with_pooling_type(LlamaPoolingType::Last))?;
    let mut embed=super::embed::Gguf {model:&model,ctx,fp:ModelFingerprint{source:"spike-gguf".into(),model:quality::file_digest(Path::new(&a[0]))?,dim:model.n_embd() as usize,doc_prefix:"".into()}};
    measure(a,&cases,&mut embed,start.elapsed().as_secs_f64())
}
fn measure(a:&[String],cases:&[quality::Case],model:&mut dyn Embedder,load:f64)->Result<(),Box<dyn std::error::Error>> {
    let out=Path::new(&a[4]);
    let mut index=Index::open(&out.join("index.sqlite"),Journal::Delete)?;
    let start=Instant::now();
    let result=update_index(Path::new(&a[2]),&mut index,&IndexSettings {docs_root:"docs".into(),chunking:ChunkingParams::default()},model)?;
    let UpdateOutcome::Updated(report)=result else { return Err(format!("{result:?}").into()); };
    if !report.problems.is_empty() {return Err(format!("{:?}",report.problems).into());}
    let index_seconds=start.elapsed().as_secs_f64();
    fs::write(out.join("config.json"),serde_json::to_vec_pretty(&serde_json::json!({"args":a,"load_seconds":load,"index_seconds":index_seconds,"cases_blake3":quality::file_digest(Path::new(&a[3]))?,"binary_blake3":quality::file_digest(&std::env::current_exe()?)?,"report":format!("{report:?}")}))?)?;
    let settings=SearchSettings {results:30,max_results:30,min_score_vector:0.0,..Default::default()};
    model.embed_query("порядок запуска ботов")?;
    let mut output=File::create(out.join("rows.jsonl"))?;
    for repeat in 0..3 {
        for case in cases {
            let start=Instant::now();
            let response=search(&index,model,None,&settings,&SearchRequest {query:case.query.clone(),..Default::default()})?;
            let total=start.elapsed().as_secs_f64();
            writeln!(output,"{}",serde_json::to_string(&quality::Row {id:case.id.clone(),candidates:30,k:0,repeat,mode:"search".into(),total,embed:0.0,rerank:0.0,hits:quality::pack_hits(&response.hits,case)})?)?;
            output.flush()?;
        }
    }
    Ok(())
}
