//! Read-only реальный корпус: готовые векторы, никаких моделей или записей профиля.
use bars_core::{
    graph::{
        similarity::{KnnOptions, VectorNode, build_knn},
        *,
    },
    indexing::chunking::{ChunkingParams, chunk_document_with_spans},
};
use rusqlite::{Connection, OpenFlags};
use std::{collections::BTreeMap, path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    if output.exists() {
        return Err("требуется новая папка результата".into());
    }
    let db = Connection::open_with_flags(
        root.join(".bars/index.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )?;
    db.execute_batch("BEGIN")?;
    let meta: BTreeMap<String, String> = db
        .prepare("SELECT key,value FROM meta")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let docs_root = meta.get("docs_root").ok_or("нет docs_root")?;
    let chunking = ChunkingParams {
        min_chars: meta["chunk_min_chars"].parse()?,
        max_chars: meta["chunk_max_chars"].parse()?,
    };
    let namespace = "bars-project:corpus:2026-10-18".to_string();
    let mut inputs = Vec::new();
    let mut vectors = Vec::new();
    let mut exclusions = BTreeMap::new();
    let documents: Vec<(String, String, String, String)> = db
        .prepare("SELECT path,module,title,hash FROM documents ORDER BY path")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<Result<_, _>>()?;
    for (path, module, title, hash) in documents {
        let relative = path
            .strip_prefix(&format!("{docs_root}/"))
            .ok_or("путь вне docs-root")?
            .to_string();
        if relative.starts_with("chats/")
            || relative.starts_with("features/graphrag/")
            || relative.starts_with("knowledge/graph")
            || relative.starts_with("knowledge/agent-tool")
            || relative.starts_with("knowledge/local-documentation-tools")
        {
            exclusions.insert(
                path,
                "исследование/журнал, исключён из независимой оценки".to_string(),
            );
            continue;
        }
        let text = match std::fs::read_to_string(root.join(&path)) {
            Ok(text) if blake3::hash(text.as_bytes()).to_hex().as_str() == hash => text,
            _ => {
                exclusions.insert(path, "диск не совпадает с индексированной ревизией".into());
                continue;
            }
        };
        let mapped = chunk_document_with_spans(&path, &text, &chunking);
        let mut chunks = Vec::new();
        let rows:Vec<(u32,String,Vec<u8>)>=db.prepare("SELECT c.ordinal,c.embed_hash,v.vector FROM chunks c JOIN documents d ON d.id=c.document_id JOIN vectors v ON v.embed_hash=c.embed_hash WHERE d.path=?1 ORDER BY c.ordinal")?.query_map([&path],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<Result<_,_>>()?;
        if rows.len() != mapped.document.chunks.len()
            || rows.iter().any(|(n, hash, _)| {
                mapped
                    .document
                    .chunks
                    .get(*n as usize)
                    .is_none_or(|c| &c.embed_hash != hash)
            })
        {
            exclusions.insert(path, "нарезка/prefix не совпадает".into());
            continue;
        }
        for (ordinal, hash, blob) in rows {
            let key = format!("{ordinal}:{hash}");
            let reference =
                object_ref(&namespace, Kind::Chunk, &format!("{relative}:{key}"), &hash);
            vectors.push(VectorNode {
                id: reference.id,
                vector: blob
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect(),
            });
            chunks.push(source::InputChunk {
                key,
                label: mapped.document.chunks[ordinal as usize]
                    .heading_chain
                    .clone(),
                spans: mapped.source_spans[ordinal as usize].clone(),
            });
        }
        inputs.push(source::InputDocument {
            path,
            docs_path: relative,
            module,
            title,
            text,
            chunks,
        });
    }
    db.execute_batch("COMMIT")?;
    let started = Instant::now();
    let mut data = source::build(&namespace, docs_root, inputs)?;
    let parse_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let knn = build_knn(
        &vectors,
        KnnOptions {
            k: 5,
            min_cosine: 0.5,
            max_comparisons: 1_000_000,
        },
        || false,
    )?;
    let knn_ms = started.elapsed().as_secs_f64() * 1000.0;
    for (from, neighbors) in &knn.neighbors {
        for neighbor in neighbors {
            let a = data.nodes.iter().find(|n| &n.reference.id == from).unwrap();
            let b = data
                .nodes
                .iter()
                .find(|n| n.reference.id == neighbor.id)
                .unwrap();
            let mut proof = a.evidence.clone();
            proof.extend(b.evidence.clone());
            proof.sort();
            proof.dedup();
            data.edges.push(Edge {
                id: blake3::hash(format!("similar:{from}:{}", neighbor.id).as_bytes())
                    .to_hex()
                    .to_string(),
                from: from.clone(),
                to: neighbor.id.clone(),
                predicate: Predicate::SimilarTo,
                polarity: Polarity::Positive,
                qualifiers: BTreeMap::new(),
                origin: Origin::Similarity,
                evidence: proof,
                score: Some(neighbor.cosine),
            });
        }
    }
    data.fingerprint();
    store::validate(&data)?;
    let mut rows = Vec::new();
    for operation in [
        Operation::Graph,
        Operation::Relations,
        Operation::Context,
        Operation::Explore,
    ] {
        for run in 0..20 {
            let request = GraphRequest {
                operation,
                query: Some("Смена модели согласие переиндексация".into()),
                whole_scope: true,
                ..Default::default()
            };
            let start = Instant::now();
            let result = query::execute(&data, &request, || false)?;
            rows.push(serde_json::json!({"operation":operation,"run":run,"wall_ms":start.elapsed().as_secs_f64()*1000.0,"items":result.items.len(),"bytes":result.used_bytes,"truncated":result.truncated,"reasons":result.limit_reasons}));
        }
    }
    std::fs::create_dir_all(output)?;
    std::fs::write(output.join("dataset.json"), serde_json::to_vec(&data)?)?;
    std::fs::write(
        output.join("measurement.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"meta":meta,"documents":data.sources.len(),"chunks":vectors.len(),"nodes":data.nodes.len(),"edges":data.edges.len(),"explicit_assertions":data.edges.iter().filter(|e|e.predicate.semantic()).count(),"parse_ms":parse_ms,"knn_ms":knn_ms,"knn_stop":format!("{:?}",knn.stop_reason),"exclusions":exclusions,"rows":rows}),
        )?,
    )?;
    println!(
        "{} документов, {} чанков; parse {:.1} ms, kNN {:.1} ms",
        data.sources.len(),
        vectors.len(),
        parse_ms,
        knn_ms
    );
    Ok(())
}
