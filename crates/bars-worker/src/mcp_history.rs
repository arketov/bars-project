//! История допущенных MCP-вызовов. Контракт: window/mcp-statistics.md, этап 29.
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

const WEEK: i64 = 7 * 86400;
const MAX_ROWS: i64 = 10_000;
const MAX_BYTES: i64 = 32 * 1024 * 1024;
pub use crate::mcp_statistics::now;

#[derive(Clone)]
pub struct History(Arc<Inner>);
struct Inner {
    home: PathBuf,
    epoch: String,
    lost: AtomicU64,
    last_loss: AtomicU8,
    writes: Arc<tokio::sync::Semaphore>,
    storage: Mutex<()>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub version: u32,
    pub id: i64,
    pub generation: i64,
    pub epoch: String,
    pub session: String,
    pub tool: String,
    pub project: String,
    pub project_name: Option<String>,
    pub started: i64,
    pub finished: Option<i64>,
    pub duration_ms: Option<u64>,
    pub outcome: String,
    pub requested: Value,
    pub applied: Value,
    pub result: Value,
    pub error: Value,
    pub incomplete: Vec<String>,
    /// Доставка ответа клиенту не наблюдается этой границей.
    pub delivery: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    pub from: Option<i64>,
    pub to: Option<i64>,
    pub project: Option<String>,
    pub session: Option<String>,
    pub tool: Option<String>,
    pub outcome: Option<String>,
    pub cursor: Option<Cursor>,
    pub limit: Option<u32>,
    pub id: Option<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursor {
    pub started: i64,
    pub id: i64,
    pub ceiling: i64,
    pub generation: i64,
    pub from: i64,
    pub to: i64,
    pub filter_key: String,
}
#[derive(Debug, Serialize)]
pub struct Page {
    pub records: Vec<Attempt>,
    pub next: Option<Cursor>,
    pub summary: Value,
    pub enabled: bool,
    pub lost_events: u64,
    pub last_storage_error: Option<&'static str>,
    pub coverage: &'static str,
}

fn open(home: &Path) -> rusqlite::Result<Connection> {
    let mut db = Connection::open(home.join("mcp-statistics.sqlite3"))?;
    db.busy_timeout(Duration::from_millis(25))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 2 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA journal_size_limit=1048576;")?;
    if version == 2 {
        return Ok(db);
    }
    let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS calls (
        day INTEGER NOT NULL, session TEXT NOT NULL, project TEXT NOT NULL, tool TEXT NOT NULL,
        calls INTEGER NOT NULL, errors INTEGER NOT NULL, total_ms INTEGER NOT NULL,
        first INTEGER NOT NULL, last INTEGER NOT NULL, PRIMARY KEY(day,session,project,tool));
        CREATE TABLE IF NOT EXISTS history_settings (id INTEGER PRIMARY KEY CHECK(id=1), enabled INTEGER NOT NULL, generation INTEGER NOT NULL);
        INSERT OR IGNORE INTO history_settings VALUES(1,1,0);
        CREATE TABLE IF NOT EXISTS attempts (
        id INTEGER PRIMARY KEY AUTOINCREMENT, generation INTEGER NOT NULL, epoch TEXT NOT NULL,
        started INTEGER NOT NULL, project TEXT NOT NULL, session TEXT NOT NULL,
        tool TEXT NOT NULL, outcome TEXT NOT NULL, bytes INTEGER NOT NULL, data TEXT NOT NULL);
        CREATE INDEX IF NOT EXISTS attempts_time ON attempts(started,id);
        CREATE INDEX IF NOT EXISTS attempts_scope ON attempts(project,session,tool,outcome,id);
        PRAGMA user_version=2;")?;
    prune(&tx, now())?;
    tx.commit()?;
    Ok(db)
}
fn prune(db: &Connection, time: i64) -> rusqlite::Result<()> {
    db.execute("DELETE FROM attempts WHERE started < ?1", [time - WEEK])?;
    // У legacy неизвестно время отдельных вызовов в пограничном дне.
    db.execute("DELETE FROM calls WHERE day * 86400 < ?1", [time - WEEK])?;
    db.execute(
        "DELETE FROM attempts WHERE id IN (SELECT id FROM (SELECT id,
        ROW_NUMBER() OVER(ORDER BY id DESC) n, SUM(bytes) OVER(ORDER BY id DESC) b FROM attempts)
        WHERE n > ?1 OR b > ?2)",
        params![MAX_ROWS, MAX_BYTES],
    )?;
    Ok(())
}
fn decode(s: String) -> rusqlite::Result<Attempt> {
    serde_json::from_str(&s).map_err(|_| rusqlite::Error::InvalidQuery)
}
fn save(db: &Connection, a: &Attempt) -> rusqlite::Result<()> {
    let mut safe = a.clone();
    let mut data = serde_json::to_string(&safe).map_err(|_| rusqlite::Error::InvalidQuery)?;
    // JSON escaping тоже расходует бюджет: UTF-8-длина полей недостаточна.
    for budget in [6000, 3000, 1000, 0] {
        if data.len() <= 32768 {
            break;
        }
        let mut requested_budget = budget;
        let mut applied_budget = budget;
        safe.requested = bounded(
            safe.requested,
            "requested",
            &mut requested_budget,
            &mut safe.incomplete,
        );
        safe.applied = bounded(
            safe.applied,
            "applied",
            &mut applied_budget,
            &mut safe.incomplete,
        );
        data = serde_json::to_string(&safe).map_err(|_| rusqlite::Error::InvalidQuery)?;
    }
    if data.len() > 32768 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    db.execute(
        "UPDATE attempts SET outcome=?1,bytes=?2,data=?3,project=?6 WHERE id=?4 AND generation=?5",
        params![
            a.outcome,
            data.len() as i64,
            data,
            a.id,
            a.generation,
            a.project
        ],
    )?;
    Ok(())
}

impl History {
    fn connection(&self) -> rusqlite::Result<(MutexGuard<'_, ()>, Connection)> {
        // Свои writer'ы ждут здесь, не расходуя короткий timeout внешней блокировки.
        let storage = self
            .0
            .storage
            .lock()
            .map_err(|_| rusqlite::Error::InvalidQuery)?;
        Ok((storage, open(&self.0.home)?))
    }
    pub fn write_permit(&self) -> Option<tokio::sync::OwnedSemaphorePermit> {
        match self.0.writes.clone().try_acquire_owned() {
            Ok(permit) => Some(permit),
            Err(_) => {
                self.0.last_loss.store(2, Ordering::Relaxed);
                self.0.lost.fetch_add(1, Ordering::Relaxed);
                log::warn!("История MCP неполна: лимит одновременно записываемых событий");
                None
            }
        }
    }
    pub fn start_maintenance(&self) {
        let weak = Arc::downgrade(&self.0);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                let history = Self(inner);
                let _ = tokio::task::spawn_blocking(move || history.maintain()).await;
            }
        });
    }
    pub fn new(home: PathBuf) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let epoch = format!(
            "{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let this = Self(Arc::new(Inner {
            home,
            epoch,
            lost: AtomicU64::new(0),
            last_loss: AtomicU8::new(0),
            writes: Arc::new(tokio::sync::Semaphore::new(8)),
            storage: Mutex::new(()),
        }));
        this.observe(this.recover());
        this
    }
    pub fn lost(&self) -> u64 {
        self.0.lost.load(Ordering::Relaxed)
    }
    pub fn last_error(&self) -> Option<&'static str> {
        match self.0.last_loss.load(Ordering::Relaxed) {
            1 => Some("storage_unavailable"),
            2 => Some("writer_capacity_exceeded"),
            _ => None,
        }
    }
    pub fn observe<T>(&self, result: rusqlite::Result<T>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(_) => {
                self.0.last_loss.store(1, Ordering::Relaxed);
                self.0.lost.fetch_add(1, Ordering::Relaxed);
                log::warn!("История MCP неполна: операция хранения не выполнена");
                None
            }
        }
    }
    fn recover(&self) -> rusqlite::Result<()> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        prune(&tx, now())?;
        self.mark_unknown(&tx, "worker_restarted")?;
        tx.commit()
    }
    fn mark_unknown(&self, db: &Connection, reason: &str) -> rusqlite::Result<()> {
        let mut stmt = db.prepare("SELECT data FROM attempts WHERE outcome='running'")?;
        let rows = stmt
            .query_map([], |r| decode(r.get(0)?))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for mut a in rows {
            a.outcome = "unknown".into();
            a.incomplete.push(reason.into());
            save(db, &a)?;
        }
        Ok(())
    }
    pub fn begin(
        &self,
        session: &str,
        tool: &str,
        project: &str,
        name: Option<String>,
        requested: Value,
        complete: bool,
    ) -> rusqlite::Result<Option<Attempt>> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let (enabled, generation): (bool, i64) = tx.query_row(
            "SELECT enabled,generation FROM history_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if !enabled {
            return Ok(None);
        }
        let mut incomplete = Vec::new();
        if !complete {
            incomplete.push("original_mcp_input_unavailable".into());
        }
        let mut budget = 12000;
        let requested = bounded(requested, "requested", &mut budget, &mut incomplete);
        let mut a = Attempt {
            version: 2,
            id: 0,
            generation,
            epoch: self.0.epoch.clone(),
            session: session.into(),
            tool: tool.into(),
            project: project.into(),
            project_name: name,
            started: now(),
            finished: None,
            duration_ms: None,
            outcome: "running".into(),
            requested,
            applied: Value::Null,
            result: Value::Null,
            error: Value::Null,
            incomplete,
            delivery: "unknown".into(),
        };
        tx.execute("INSERT INTO attempts(generation,epoch,started,project,session,tool,outcome,bytes,data) VALUES(?1,?2,?3,?4,?5,?6,'running',0,'{}')",params![generation,a.epoch,a.started,a.project,a.session,a.tool])?;
        a.id = tx.last_insert_rowid();
        save(&tx, &a)?;
        prune(&tx, now())?;
        tx.commit()?;
        Ok(Some(a))
    }
    pub fn finish(&self, a: &Attempt) -> rusqlite::Result<()> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let live:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM attempts a JOIN history_settings s ON a.generation=s.generation WHERE a.id=?1 AND s.enabled=1 AND a.outcome='running')",[a.id],|r|r.get(0))?;
        if live {
            save(&tx, a)?;
        }
        prune(&tx, now())?;
        tx.commit()
    }
    pub fn set_enabled(&self, enabled: bool) -> rusqlite::Result<()> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        prune(&tx, now())?;
        if !enabled {
            self.mark_unknown(&tx, "collection_stopped")?;
        }
        tx.execute("UPDATE history_settings SET enabled=?1,generation=generation+1 WHERE id=1 AND enabled<>?1",[enabled])?;
        tx.commit()
    }
    pub fn clear(&self, confirmed: bool) -> rusqlite::Result<()> {
        if !confirmed {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute_batch("DELETE FROM attempts; DELETE FROM calls; UPDATE history_settings SET generation=generation+1;")?;
        tx.commit()?;
        db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        Ok(())
    }
    pub fn page(&self, f: &Filter) -> rusqlite::Result<Page> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        prune(&tx, now())?;
        let time = now();
        let (enabled, generation): (bool, i64) = tx.query_row(
            "SELECT enabled,generation FROM history_settings WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let key = serde_json::to_string(&(
            f.from, f.to, &f.project, &f.session, &f.tool, &f.outcome, f.id,
        ))
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
        if f.cursor
            .as_ref()
            .is_some_and(|c| c.generation != generation || c.filter_key != key)
        {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let from = f
            .cursor
            .as_ref()
            .map(|c| c.from)
            .or(f.from)
            .unwrap_or(time - 86400)
            .max(time - WEEK);
        let to = f.cursor.as_ref().map(|c| c.to).or(f.to).unwrap_or(time + 1);
        let ceiling = match &f.cursor {
            Some(c) => c.ceiling,
            None => tx.query_row("SELECT COALESCE(MAX(id),0) FROM attempts", [], |r| r.get(0))?,
        };
        if from > to {
            return Err(rusqlite::Error::InvalidQuery);
        }
        let clause = "started>=?1 AND started<?2 AND (?3 IS NULL OR project=?3) AND (?4 IS NULL OR session=?4) AND (?5 IS NULL OR tool=?5) AND (?6 IS NULL OR outcome=?6) AND id<=?7";
        let summary=tx.query_row(&format!("SELECT COUNT(*),SUM(outcome='success'),SUM(outcome='error'),SUM(outcome='unknown'),SUM(outcome='running'),SUM(COALESCE(json_extract(data,'$.duration_ms'),0)),COUNT(json_extract(data,'$.duration_ms')),SUM(outcome='success' AND json_extract(data,'$.result.count')=0) FROM attempts WHERE {clause}"),params![from,to,f.project,f.session,f.tool,f.outcome,ceiling],|r|Ok(json!({"calls":r.get::<_,i64>(0)?,"success":r.get::<_,Option<i64>>(1)?.unwrap_or(0),"errors":r.get::<_,Option<i64>>(2)?.unwrap_or(0),"unknown":r.get::<_,Option<i64>>(3)?.unwrap_or(0),"running":r.get::<_,Option<i64>>(4)?.unwrap_or(0),"total_ms":r.get::<_,Option<i64>>(5)?.unwrap_or(0),"timed_calls":r.get::<_,i64>(6)?,"empty_success":r.get::<_,Option<i64>>(7)?.unwrap_or(0)})))?;
        let limit = f.limit.unwrap_or(50).clamp(1, 100) as usize;
        let mut records = {
            let mut stmt=tx.prepare(&format!("SELECT data FROM attempts WHERE {clause} AND (?8 IS NULL OR started<?8 OR (started=?8 AND id<?9)) AND (?10 IS NULL OR id=?10) ORDER BY started DESC,id DESC LIMIT ?11"))?;
            stmt.query_map(
                params![
                    from,
                    to,
                    f.project,
                    f.session,
                    f.tool,
                    f.outcome,
                    ceiling,
                    f.cursor.as_ref().map(|c| c.started),
                    f.cursor.as_ref().map(|c| c.id),
                    f.id,
                    (limit + 1) as i64
                ],
                |r| decode(r.get(0)?),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let next = if records.len() > limit {
            records.truncate(limit);
            records.last().map(|a| Cursor {
                started: a.started,
                id: a.id,
                ceiling,
                generation,
                from,
                to,
                filter_key: key,
            })
        } else {
            None
        };
        tx.commit()?;
        Ok(Page {
            records,
            next,
            summary,
            enabled,
            lost_events: self.lost(),
            last_storage_error: self.last_error(),
            coverage: "Допущенные MCP-вызовы worker; подробности шести основных инструментов. GraphRAG — только метаданные. Ошибки до допуска и доставка клиенту не наблюдаются",
        })
    }
    /// Подсказки по всей истории периода, без загрузки payload попыток.
    pub fn options(
        &self,
        f: &Filter,
        project_query: &str,
        session_query: &str,
    ) -> rusqlite::Result<Value> {
        let (_storage, mut db) = self.connection()?;
        let tx = db.transaction()?;
        let from = f.from.unwrap_or(now() - 86400).max(now() - WEEK);
        let to = f.to.unwrap_or(now() + 1);
        let projects = {
            let mut stmt = tx.prepare("SELECT DISTINCT project FROM attempts WHERE started>=?1 AND started<?2 AND project<>'' AND instr(project,?3)>0 ORDER BY project LIMIT 201")?;
            stmt.query_map(params![from, to, project_query], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        let sessions = {
            let mut stmt = tx.prepare("SELECT DISTINCT session FROM attempts WHERE started>=?1 AND started<?2 AND (?3 IS NULL OR project=?3) AND instr(session,?4)>0 ORDER BY session LIMIT 201")?;
            stmt.query_map(params![from, to, f.project, session_query], |r| {
                r.get::<_, String>(0)
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(
            json!({"projects":projects.iter().take(200).collect::<Vec<_>>(),"sessions":sessions.iter().take(200).collect::<Vec<_>>(),"projects_more":projects.len()>200,"sessions_more":sessions.len()>200}),
        )
    }
    pub fn legacy(&self) -> rusqlite::Result<Vec<crate::mcp_statistics::Row>> {
        // open выполняет миграцию/отсечение до legacy-чтения.
        let (_storage, db) = self.connection()?;
        prune(&db, now())?;
        drop(db);
        crate::mcp_statistics::legacy_snapshot(&self.0.home)
    }
    pub fn maintain(&self) {
        self.observe(self.connection().and_then(|(_storage, db)| {
            prune(&db, now())?;
            db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
        }));
    }
}

/// Ограничение по UTF-8; исходные размеры и усечение видны в полноте записи.
pub fn bounded(v: Value, field: &str, budget: &mut usize, notes: &mut Vec<String>) -> Value {
    match v {
        Value::String(mut s) => {
            let len = s.len();
            let mut n = len.min(8192).min(*budget);
            while !s.is_char_boundary(n) {
                n -= 1;
            }
            s.truncate(n);
            *budget -= n;
            if n < len {
                let prefix = format!("{field}:bytes=");
                if let Some(old) = notes.iter_mut().find(|s| s.starts_with(&prefix)) {
                    let original = old[prefix.len()..]
                        .split(',')
                        .next()
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(len);
                    *old = format!("{field}:bytes={original},saved={n}");
                } else {
                    notes.push(format!("{field}:bytes={len},saved={n}"));
                }
            }
            Value::String(s)
        }
        Value::Array(a) => {
            let len = a.len();
            if len > 100 {
                notes.push(format!("{field}:items={len},saved=100"));
            }
            Value::Array(
                a.into_iter()
                    .take(100)
                    .enumerate()
                    .map(|(i, v)| bounded(v, &format!("{field}[{i}]"), budget, notes))
                    .collect(),
            )
        }
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| {
                    let v = bounded(v, &format!("{field}.{k}"), budget, notes);
                    (k, v)
                })
                .collect(),
        ),
        other => other,
    }
}

/// Только плоский allowlist с проверкой типов; nested payload не проходит.
pub fn requested(tool: &str, source: &Value) -> Value {
    let fields: &[&str] = match tool {
        "bars_rag_search" => &[
            "format",
            "mode",
            "project",
            "query",
            "modules",
            "files",
            "limit",
            "candidates",
            "min_score",
        ],
        "bars_get_duplicates" => &["format", "project", "modules", "limit"],
        "bars_get_similar" => &["format", "project", "chunk_id", "modules", "limit"],
        "bars_read" => &["format", "project", "path"],
        "bars_text_search" => &[
            "format",
            "project",
            "query",
            "case_sensitive",
            "regex",
            "path_filter",
            "modules",
            "files",
            "limit_docs",
            "limit_hits",
            "path",
            "offset",
            "revision",
        ],
        "bars_links" => &[
            "format",
            "project",
            "path",
            "direction",
            "limit",
            "cursor_present",
        ],
        "bars_outline" => &["format", "project", "path", "levels", "under", "under_line"],
        "bars_status" => &["format", "project"],
        "bars_reindex" => &["format", "project", "full"],
        _ => &["format"],
    };
    let mut out = serde_json::Map::new();
    for key in fields {
        if let Some(v) = source.get(key) {
            let valid = v.is_null()
                || match *key {
                    "limit" | "candidates" | "under_line" | "limit_docs" | "limit_hits"
                    | "offset" => v.as_u64().is_some(),
                    "min_score" => v.is_number(),
                    "full" | "regex" | "case_sensitive" | "cursor_present" => v.is_boolean(),
                    "modules" | "files" => {
                        v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
                    }
                    "levels" => v
                        .as_array()
                        .is_some_and(|a| a.iter().all(|v| v.as_u64().is_some())),
                    _ => v.is_string(),
                };
            if valid {
                out.insert((*key).into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn begin(h: &History, session: &str, project: &str) -> Attempt {
        h.begin(
            session,
            "bars_read",
            project,
            None,
            json!({"path":"guide.md"}),
            true,
        )
        .unwrap()
        .unwrap()
    }
    #[test]
    fn options_find_unloaded_sessions_and_bound_results() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        for i in 0..205 {
            begin(&h, &format!("session-{i:03}"), "p");
        }
        begin(&h, "outside-first-page", "q");
        let options = h.options(&Filter::default(), "", "").unwrap();
        assert_eq!(options["sessions"].as_array().unwrap().len(), 200);
        assert_eq!(options["sessions_more"], true);
        let f = Filter {
            project: Some("q".into()),
            ..Default::default()
        };
        let options = h.options(&f, "q", "outside").unwrap();
        assert_eq!(options["sessions"], json!(["outside-first-page"]));
        assert_eq!(options["projects"], json!(["q"]));
        assert_eq!(options["sessions_more"], false);
        let f = Filter {
            from: Some(now() + 5),
            ..Default::default()
        };
        assert_eq!(h.options(&f, "", "").unwrap()["projects"], json!([]));
    }

    #[test]
    fn restart_disable_and_clear_never_resurrect_inflight_calls() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let mut a = begin(&h, "s", "p");
        assert!(h.clear(false).is_err());
        h.clear(true).unwrap();
        a.outcome = "success".into();
        h.finish(&a).unwrap();
        assert!(h.page(&Filter::default()).unwrap().records.is_empty());
        let mut b = begin(&h, "s", "p");
        h.set_enabled(false).unwrap();
        h.set_enabled(true).unwrap();
        b.outcome = "success".into();
        h.finish(&b).unwrap();
        let rows = h.page(&Filter::default()).unwrap().records;
        assert_eq!(rows[0].outcome, "unknown");
        assert!(rows[0].finished.is_none());
        let c = begin(&h, "s", "p");
        let restarted = History::new(home.path().into());
        assert_eq!(
            restarted.page(&Filter::default()).unwrap().records[0].outcome,
            "unknown"
        );
        assert_ne!(c.id, a.id);
        restarted.set_enabled(false).unwrap();
        assert!(
            restarted
                .begin("s", "bars_read", "p", None, json!({}), true)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn paging_filters_summary_and_retention_use_the_same_scope() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        for i in 0..5 {
            let mut a = begin(&h, "s", if i == 0 { "q" } else { "p" });
            a.outcome = if i == 1 { "error" } else { "success" }.into();
            h.finish(&a).unwrap();
        }
        let filter = Filter {
            project: Some("p".into()),
            session: Some("s".into()),
            tool: Some("bars_read".into()),
            outcome: Some("success".into()),
            limit: Some(2),
            ..Default::default()
        };
        let page = h.page(&filter).unwrap();
        assert_eq!(page.summary["calls"], 3);
        assert_eq!(page.records.len(), 2);
        begin(&h, "s", "p");
        let next = h
            .page(&Filter {
                cursor: page.next.clone(),
                ..filter.clone()
            })
            .unwrap();
        assert_eq!(next.records.len(), 1);
        assert_eq!(next.summary["calls"], 3);
        assert!(
            h.page(&Filter {
                cursor: page.next.clone(),
                tool: Some("bars_status".into()),
                ..filter.clone()
            })
            .is_err()
        );
        let db = open(home.path()).unwrap();
        db.execute("UPDATE attempts SET started=?1", [now() - WEEK - 1])
            .unwrap();
        assert!(
            h.page(&Filter {
                from: Some(0),
                ..Default::default()
            })
            .unwrap()
            .records
            .is_empty()
        );
        assert_eq!(
            db.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    #[test]
    fn migration_preserves_recent_legacy_without_inventing_attempts() {
        let home = tempfile::tempdir().unwrap();
        let db = crate::mcp_statistics::Row {
            session: "s".into(),
            project: "p".into(),
            tool: "bars_read".into(),
            calls: 1,
            errors: 0,
            total_ms: 1,
            first: now(),
            last: now(),
        };
        crate::mcp_statistics::record(home.path(), &db).unwrap();
        let h = History::new(home.path().into());
        assert_eq!(h.legacy().unwrap().len(), 1);
        assert!(h.page(&Filter::default()).unwrap().records.is_empty());
        h.clear(true).unwrap();
        assert!(h.legacy().unwrap().is_empty());
    }
    #[test]
    fn corrupt_or_future_database_is_not_replaced() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("mcp-statistics.sqlite3");
        std::fs::write(&path, b"broken").unwrap();
        let h = History::new(home.path().into());
        assert_eq!(h.lost(), 1);
        assert!(h.page(&Filter::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"broken");
        let other = tempfile::tempdir().unwrap();
        let db = Connection::open(other.path().join("mcp-statistics.sqlite3")).unwrap();
        db.execute_batch("PRAGMA user_version=99").unwrap();
        let h = History::new(other.path().into());
        assert!(h.page(&Filter::default()).is_err());
        assert_eq!(
            db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            99
        );
    }
    #[test]
    fn allowlist_and_utf8_budget_do_not_capture_unrelated_payloads() {
        let source = json!({"query":"я".repeat(10000),"min_score":null,"files":["a"],"api_key":"SECRET","response":{"text":"BODY"},"project":{"token":"SECRET"}});
        let v = requested("bars_rag_search", &source);
        assert!(v.get("api_key").is_none());
        assert!(v.get("project").is_none());
        assert!(v.get("limit").is_none());
        assert!(v["min_score"].is_null());
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let a = h
            .begin("s", "bars_rag_search", "p", None, v, true)
            .unwrap()
            .unwrap();
        assert_eq!(a.requested["query"].as_str().unwrap().len(), 8192);
        assert!(!a.incomplete.is_empty());
        assert!(serde_json::to_string(&a).unwrap().len() <= 32768);
        assert!(!serde_json::to_string(&a).unwrap().contains("SECRET"));
    }
    #[test]
    fn own_writer_waits_without_spending_sqlite_busy_timeout() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        // Задержавшийся writer этого же экземпляра, а не внешний захват БД.
        let storage = h.0.storage.lock().unwrap();
        let db = open(home.path()).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        let (started, ready) = std::sync::mpsc::channel();
        let (done, result) = std::sync::mpsc::channel();
        let other = h.clone();
        let writer = std::thread::spawn(move || {
            started.send(()).unwrap();
            done.send(other.begin("s", "bars_read", "p", None, json!({}), true))
                .unwrap();
        });
        ready.recv().unwrap();
        let premature = result.recv_timeout(Duration::from_millis(100));
        db.execute_batch("ROLLBACK").unwrap();
        drop(db);
        drop(storage);
        writer.join().unwrap();
        assert!(
            matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "свой writer не должен отказывать по busy timeout: {premature:?}"
        );
        assert!(result.recv().unwrap().unwrap().is_some());
        assert_eq!(h.page(&Filter::default()).unwrap().records.len(), 1);
        assert_eq!(h.lost(), 0);
    }
    #[test]
    fn refused_writer_is_visible_and_does_not_insert_a_false_success() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let db = open(home.path()).unwrap();
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        assert!(
            h.observe(h.begin("s", "bars_read", "p", None, json!({}), true))
                .is_none()
        );
        assert_eq!(h.lost(), 1);
        db.execute_batch("ROLLBACK").unwrap();
        assert!(h.page(&Filter::default()).unwrap().records.is_empty());
    }
    #[test]
    fn row_and_byte_limits_evict_oldest() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let a = begin(&h, "s", "p");
        let db = open(home.path()).unwrap();
        db.execute("UPDATE attempts SET bytes=?1", [MAX_BYTES + 1])
            .unwrap();
        prune(&db, now()).unwrap();
        assert!(h.page(&Filter::default()).unwrap().records.is_empty());
        let mut db = open(home.path()).unwrap();
        let tx = db.transaction().unwrap();
        for _ in 0..MAX_ROWS + 1 {
            tx.execute("INSERT INTO attempts(generation,epoch,started,project,session,tool,outcome,bytes,data) VALUES(0,'e',?1,'p','s','bars_read','success',1,?2)",params![now(),serde_json::to_string(&a).unwrap()]).unwrap();
        }
        prune(&tx, now()).unwrap();
        assert_eq!(
            tx.query_row("SELECT COUNT(*) FROM attempts", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            MAX_ROWS
        );
        tx.commit().unwrap();
    }
    #[test]
    fn measured_storage_overhead_and_file_size() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let mut times = Vec::new();
        for _ in 0..200 {
            let start = std::time::Instant::now();
            let mut a = h
                .begin(
                    "s",
                    "bars_rag_search",
                    "p",
                    None,
                    json!({"query":"Как работает локальный поиск?","limit":10,"format":"json"}),
                    true,
                )
                .unwrap()
                .unwrap();
            a.outcome = "success".into();
            a.finished = Some(now());
            a.duration_ms = Some(5);
            a.result = json!({"count":3,"unit":"fragments"});
            h.finish(&a).unwrap();
            times.push(start.elapsed().as_micros() as u64);
        }
        times.sort_unstable();
        h.maintain();
        let db = open(home.path()).unwrap();
        let payload: i64 = db
            .query_row("SELECT SUM(bytes) FROM attempts", [], |r| r.get(0))
            .unwrap();
        drop(db);
        h.maintain();
        let size = std::fs::metadata(home.path().join("mcp-statistics.sqlite3"))
            .unwrap()
            .len();
        eprintln!(
            "MCP_HISTORY_MEASURE {}",
            json!({"attempts":200,"begin_finish_p50_us":times[100],"begin_finish_p95_us":times[190],"payload_bytes":payload,"sqlite_bytes":size,"lost":h.lost()})
        );
        assert_eq!(h.lost(), 0);
        assert!(payload < MAX_BYTES);
        assert_eq!(h.page(&Filter::default()).unwrap().summary["calls"], 200);
    }
    #[test]
    fn serialized_budget_and_writer_capacity_are_bounded() {
        let home = tempfile::tempdir().unwrap();
        let h = History::new(home.path().into());
        let large =
            json!({"query":"\u{0001}".repeat(20000),"files":vec!["\u{0002}".repeat(10000);120]});
        let mut a = h
            .begin(
                "s",
                "bars_rag_search",
                "p",
                None,
                requested("bars_rag_search", &large),
                true,
            )
            .unwrap()
            .unwrap();
        a.applied = json!({"cwd":"\u{0003}".repeat(8000)});
        a.outcome = "success".into();
        h.finish(&a).unwrap();
        let page = h.page(&Default::default()).unwrap();
        let stored = serde_json::to_string(&page.records[0]).unwrap();
        assert!(stored.len() <= 32768);
        assert!(!page.records[0].incomplete.is_empty());
        let permits: Vec<_> = (0..8).map(|_| h.write_permit().unwrap()).collect();
        assert!(h.write_permit().is_none());
        assert_eq!(h.lost(), 1);
        drop(permits);
        assert!(h.write_permit().is_some());
    }
}
