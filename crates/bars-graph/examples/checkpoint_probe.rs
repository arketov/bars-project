//! Изолированный парный стенд ISS-101; рабочие индексы не открывает.
use bars_graph::{
    source::{InputDocument, build},
    store,
};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{path::Path, time::Instant};

#[cfg(windows)]
mod metrics {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Io {
        reads: u64,
        writes: u64,
        other: u64,
        read_bytes: u64,
        write_bytes: u64,
        other_bytes: u64,
    }
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        size: u32,
        faults: u32,
        peak: usize,
        working: usize,
        peak_paged: usize,
        paged: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessIoCounters(process: *mut c_void, io: *mut Io) -> i32;
        fn GetProcessTimes(
            process: *mut c_void,
            created: *mut u64,
            ended: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(process: *mut c_void, memory: *mut Memory, size: u32) -> i32;
    }
    pub fn sample() -> (u64, u64, u64, usize, usize) {
        let mut io = Io::default();
        let mut memory = Memory {
            size: std::mem::size_of::<Memory>() as u32,
            ..Default::default()
        };
        let (mut created, mut ended, mut kernel, mut user) = (0, 0, 0, 0);
        // Pseudo-handle текущего процесса и буферы точных Win32 ABI-размеров.
        unsafe {
            let p = GetCurrentProcess();
            assert_ne!(GetProcessIoCounters(p, &mut io), 0);
            assert_ne!(
                GetProcessTimes(p, &mut created, &mut ended, &mut kernel, &mut user),
                0
            );
            assert_ne!(GetProcessMemoryInfo(p, &mut memory, memory.size), 0);
        }
        (
            io.read_bytes,
            io.write_bytes,
            kernel + user,
            memory.peak,
            memory.peak_pagefile,
        )
    }
}

#[cfg(not(windows))]
mod metrics {
    pub fn sample() -> (u64, u64, u64, usize, usize) {
        panic!("Win32-счётчики стенда доступны только на Windows")
    }
}

fn measure(db: &Connection, action: impl FnOnce()) -> Value {
    let before = metrics::sample();
    let started = Instant::now();
    action();
    let ms = started.elapsed().as_secs_f64() * 1000.;
    let after = metrics::sample();
    let pages: i64 = db.query_row("PRAGMA page_count", [], |r| r.get(0)).unwrap();
    let page_size: i64 = db.query_row("PRAGMA page_size", [], |r| r.get(0)).unwrap();
    json!({"ms":ms,"cpu_s":(after.2-before.2) as f64/10_000_000.,"read_bytes":after.0-before.0,"write_bytes":after.1-before.1,"process_peak_working_bytes":after.3,"process_peak_commit_bytes":after.4,"db_bytes":pages*page_size})
}

fn downgrade(db: &Connection, metadata: &Value, synthetic: bool, small: bool) {
    let triggers: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='trigger'")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    for name in triggers {
        db.execute_batch(&format!("DROP TRIGGER {name}")).unwrap();
    }
    db.execute_batch("CREATE TABLE old_meta(key TEXT PRIMARY KEY,value TEXT NOT NULL) WITHOUT ROWID; INSERT INTO old_meta SELECT * FROM graph_meta; INSERT INTO old_meta SELECT * FROM graph_payload;
        INSERT INTO old_meta VALUES('read_epoch','legacy'),('read_revision','0'),('knn_input_epoch','legacy'),('knn_input_revision','0');
        DROP TABLE graph_meta; ALTER TABLE old_meta RENAME TO graph_meta; DROP TABLE graph_payload; DROP TABLE graph_revisions; UPDATE graph_meta SET value='1' WHERE key='schema_version';").unwrap();
    if synthetic {
        for item in metadata.as_array().unwrap() {
            let key = item["key"].as_str().unwrap();
            if ["schema_version", "read_revision"].contains(&key) {
                continue;
            }
            let n = item["bytes"].as_u64().unwrap() as usize;
            db.execute(
                "INSERT OR REPLACE INTO graph_meta VALUES(?1,?2)",
                params![key, "x".repeat(if small { n.min(100) } else { n })],
            )
            .unwrap();
        }
    }
    for table in [
        "graph_meta",
        "graph_sources",
        "graph_nodes",
        "graph_edges",
        "graph_evidence",
        "graph_aux",
    ] {
        for event in ["INSERT", "UPDATE", "DELETE"] {
            let row = if event == "DELETE" { "OLD" } else { "NEW" };
            let filter = if table == "graph_meta" {
                format!(
                    "WHEN {row}.key NOT IN ('read_revision','read_epoch','knn_prepared','knn_checkpoint','knn_complete','knn_input_revision','knn_input_epoch')"
                )
            } else {
                String::new()
            };
            db.execute_batch(&format!("CREATE TRIGGER graph_read_{table}_{event} AFTER {event} ON {table} {filter} BEGIN UPDATE graph_meta SET value=CAST(value AS INTEGER)+1 WHERE key='read_revision'; END")).unwrap();
        }
    }
}

fn setup(path: &Path) -> Connection {
    let mut db = Connection::open(path).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA cache_size=-2000; CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT); CREATE TABLE documents(path TEXT PRIMARY KEY,hash TEXT NOT NULL);").unwrap();
    store::ensure_schema(&mut db).unwrap();
    db
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let schema: Value = serde_json::from_slice(
        &std::fs::read(root.join("docs/knowledge/idle-load-2026-11-01/sql-schema.json")).unwrap(),
    )
    .unwrap();
    let metadata = &schema[1]["metadata"];
    let v2 = args.get(2).is_some_and(|s| s == "v2");
    let rows: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(100);
    let small = args.get(4).is_some_and(|s| s == "small");
    let dir = tempfile::Builder::new()
        .prefix("iss101-")
        .tempdir_in(root.join(".bars"))
        .unwrap();
    let path = dir.path().join("index.sqlite");
    let mut db = setup(&path);
    let mode = args.get(1).map(String::as_str).unwrap_or("delete");
    let mut output = json!({"mode":mode,"schema":if v2 {2}else{1},"rows":rows,"small":small,"sqlite":rusqlite::version()});
    if mode == "delete" || mode == "migration" {
        db.execute(
            "INSERT INTO graph_nodes VALUES('node','chunk','revision','{}')",
            [],
        )
        .unwrap();
        for i in 0..rows {
            db.execute(
                "INSERT INTO graph_edges VALUES(?1,'node','node','links_to','{}')",
                [i.to_string()],
            )
            .unwrap();
        }
        downgrade(&db, metadata, true, small);
        let start = Instant::now();
        let before = metrics::sample();
        if v2 || mode == "migration" {
            store::ensure_schema(&mut db).unwrap();
        }
        let after = metrics::sample();
        output["migration"] = json!({"ms":start.elapsed().as_secs_f64()*1000.,"cpu_s":(after.2-before.2) as f64/10_000_000.,"read_bytes":after.0-before.0,"write_bytes":after.1-before.1});
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA shrink_memory;")
            .unwrap();
        output["measurement"] = measure(&db, || {
            db.execute("DELETE FROM graph_edges", []).unwrap();
        });
        assert_eq!(
            db.query_row("SELECT count(*) FROM graph_edges", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        if v2 {
            output["stamp"] = json!(store::revision_stamp(&db).unwrap().unwrap().read);
        }
    } else {
        let data = build(
            "probe",
            "docs",
            (0..rows)
                .map(|i| InputDocument {
                    path: format!("docs/{i}.md"),
                    docs_path: format!("{i}.md"),
                    module: ".".into(),
                    title: format!("Документ {i}"),
                    text: format!("# Документ {i}\n\nТ-1 зависит от Т-2.\n"),
                    chunks: vec![],
                })
                .collect(),
        )
        .unwrap();
        for s in &data.sources {
            db.execute(
                "INSERT INTO documents VALUES(?1,?2)",
                params![s.meta.path, s.meta.hash],
            )
            .unwrap();
        }
        let expected = store::document_manifest(&db).unwrap();
        store::publish(&mut db, &data, &expected).unwrap();
        for (key, n) in [
            (store::PayloadKey::Prepared, 53_998_830),
            (store::PayloadKey::Checkpoint, 3_413_607),
            (store::PayloadKey::Complete, 3_412_977),
        ] {
            store::write_payload(&db, key, &"x".repeat(if small { 100 } else { n })).unwrap();
        }
        if !v2 {
            downgrade(&db, metadata, false, small);
        }
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); PRAGMA shrink_memory;")
            .unwrap();
        // Единственный mutable borrow — публикация; метрики снимаются отдельно.
        let before = metrics::sample();
        let started = Instant::now();
        store::publish(&mut db, &data, &expected).unwrap();
        let after = metrics::sample();
        output["measurement"] = json!({"ms":started.elapsed().as_secs_f64()*1000.,"cpu_s":(after.2-before.2) as f64/10_000_000.,"read_bytes":after.0-before.0,"write_bytes":after.1-before.1,"process_peak_working_bytes":after.3,"process_peak_commit_bytes":after.4});
        let start = Instant::now();
        let loaded = store::load_previous(&db).unwrap();
        output["query_ms"] = json!(start.elapsed().as_secs_f64() * 1000.);
        assert_eq!(loaded.snapshot, data.snapshot);
        assert_eq!(loaded.evidence, data.evidence);
        assert_eq!(loaded.edges.len(), data.edges.len());
        output["graph_hash"] = json!(
            blake3::hash(
                &serde_json::to_vec(&(
                    &loaded.sources,
                    &loaded.nodes,
                    &loaded.edges,
                    &loaded.evidence
                ))
                .unwrap()
            )
            .to_hex()
            .to_string()
        );
    }
    output["wal_bytes"] =
        json!(std::fs::metadata(path.with_extension("sqlite-wal")).map_or(0, |m| m.len()));
    println!("{output}");
}
