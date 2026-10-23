//! 3.8.3: release-замер на согласованной копии индекса, без загрузки модели.
use bars_core::{
    audits::contradictions::{self, Chunk},
    search::{DuplicatesRequest, SearchSettings, duplicates_from_snapshot, duplicates_snapshot},
    storage::{Index, Journal},
};
use std::{path::PathBuf, sync::atomic::AtomicBool, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("audit_measure <copied-index.sqlite> <new-output.json>".into());
    }
    let index = Index::open(&PathBuf::from(&args[0]), Journal::Wal)?;
    let mut stmt = index.conn().prepare("SELECT d.path,d.module,c.heading_chain,c.first_line,c.text,c.last_line FROM chunks c JOIN documents d ON d.id=c.document_id ORDER BY d.path,c.ordinal")?;
    let chunks = stmt
        .query_map([], |r| {
            Ok(Chunk {
                path: r.get(0)?,
                module: r.get(1)?,
                heading: r.get(2)?,
                first_line: r.get(3)?,
                text: r.get(4)?,
                last_line: r.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut samples = Vec::new();
    let mut settings = SearchSettings::default();
    settings.max_results = 5000;
    for _ in 0..3 {
        let clock = Instant::now();
        let request = DuplicatesRequest {
            modules: vec![],
            results: Some(5000),
        };
        let snapshot = duplicates_snapshot(&index, &request)?;
        let snapshot_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let dup = duplicates_from_snapshot(snapshot, &settings, request.results)?;
        let dup_ms = clock.elapsed().as_secs_f64() * 1000.0;
        let clock = Instant::now();
        let conflicts = contradictions::find(&chunks, &AtomicBool::new(false));
        samples.push(serde_json::json!({"snapshot_ms":snapshot_ms,"duplicates_ms":dup_ms,"duplicates_total":dup.total,"duplicates_returned":dup.items.len(),"contradictions_ms":clock.elapsed().as_secs_f64()*1000.0,"contradictions_total":conflicts.as_ref().ok().map(Vec::len),"contradictions_error":conflicts.err()}));
    }
    let result = serde_json::json!({"profile":"release","chunks":chunks.len(),"samples":samples});
    let mut output = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&args[1])?;
    use std::io::Write;
    output.write_all(serde_json::to_string_pretty(&result)?.as_bytes())?;
    println!("{result}");
    Ok(())
}
