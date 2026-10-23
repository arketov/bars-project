//! Rebuild only explicit extraction from a frozen Dataset, without embeddings or live files.
use bars_graph::{
    source::{InputDocument, build},
    *,
};
use std::{collections::BTreeMap, io::Write};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: extraction_regression <dataset.json> <new-report.json>".into());
    }
    let original: Dataset = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let data = build(
        &original.namespace,
        &original.docs_root,
        original
            .sources
            .iter()
            .map(|s| InputDocument {
                path: s.meta.path.clone(),
                docs_path: s.meta.docs_path.clone(),
                module: s.meta.module.clone(),
                title: s.meta.title.clone(),
                text: s.text.clone(),
                chunks: vec![],
            })
            .collect(),
    )?;
    let nodes: BTreeMap<_, _> = data.nodes.iter().map(|n| (&n.reference.id, n)).collect();
    let evidence: BTreeMap<_, _> = data.evidence.iter().map(|e| (&e.id, e)).collect();
    let sources: BTreeMap<_, _> = data
        .sources
        .iter()
        .map(|s| (&s.meta.reference.id, s))
        .collect();
    for proof in &data.evidence {
        let source = sources[&proof.document.id];
        assert_eq!(
            source.text.get(proof.span.start..proof.span.end),
            Some(proof.quote.as_str())
        );
    }
    let facts: Vec<_> = data
        .edges
        .iter()
        .filter(|e| e.origin == Origin::ExplicitRule)
        .collect();
    let report = serde_json::json!({
        "method": METHOD_VERSION, "source_snapshot": original.snapshot, "snapshot": data.snapshot,
        "documents": data.sources.len(), "verified_spans": data.evidence.len(),
        "limitations": "development replay; no chunks, embeddings, retrieval, holdout or live worker acceptance",
        "assertions": facts.iter().map(|e| serde_json::json!({
            "subject": nodes[&e.from].label, "object": nodes[&e.to].label,
            "predicate": e.predicate, "polarity": e.polarity, "qualifiers": e.qualifiers,
            "proof": e.evidence.iter().map(|id| { let p = evidence[id]; serde_json::json!({
                "path": sources[&p.document.id].meta.docs_path, "quote": p.quote, "span": p.span,
            }) }).collect::<Vec<_>>()
        })).collect::<Vec<_>>(),
        "cross_file_entities": data.nodes.iter().filter(|n| n.reference.kind == Kind::Entity && n.source_ids.len() > 1).count(),
        "two_step_semantic_chains": facts.iter().map(|e| facts.iter().filter(|f| f.from == e.to && e.from != f.to).count()).sum::<usize>(),
    });
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    output.write_all(&serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{} documents, {} explicit assertions; report: {}",
        data.sources.len(),
        facts.len(),
        args[2]
    );
    Ok(())
}
