//! IDEA-05: ограниченные дневные агрегаты, без содержимого вызовов.
use rusqlite::Connection;
#[cfg(test)]
use rusqlite::params;
use serde::Serialize;
use std::path::Path;

const ROW_LIMIT: i64 = 100_000;
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub session: String,
    pub project: String,
    pub tool: String,
    pub calls: i64,
    pub errors: i64,
    pub total_ms: i64,
    pub first: i64,
    pub last: i64,
}

fn open(home: &Path) -> rusqlite::Result<Connection> {
    let db = Connection::open(home.join("mcp-statistics.sqlite3"))?;
    db.busy_timeout(std::time::Duration::from_millis(250))?;
    db.execute_batch(
        "PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS calls (
        day INTEGER NOT NULL, session TEXT NOT NULL, project TEXT NOT NULL, tool TEXT NOT NULL,
        calls INTEGER NOT NULL, errors INTEGER NOT NULL, total_ms INTEGER NOT NULL,
        first INTEGER NOT NULL, last INTEGER NOT NULL,
        PRIMARY KEY(day,session,project,tool));
        CREATE INDEX IF NOT EXISTS calls_last ON calls(last DESC);",
    )?;
    Ok(db)
}

fn prune(db: &Connection, time: i64, limit: i64) -> rusqlite::Result<()> {
    db.execute("DELETE FROM calls WHERE day < ?1", [time / 86400 - 29])?;
    db.execute("DELETE FROM calls WHERE rowid IN (SELECT rowid FROM calls ORDER BY last DESC, rowid DESC LIMIT -1 OFFSET ?1)", [limit])?;
    Ok(())
}

#[cfg(test)]
pub fn record(home: &Path, row: &Row) -> rusqlite::Result<()> {
    let mut db = open(home)?;
    let tx = db.transaction()?;
    tx.execute(
        "INSERT INTO calls VALUES (?1,?2,?3,?4,1,?5,?6,?7,?7)
        ON CONFLICT(day,session,project,tool) DO UPDATE SET calls=calls+1,
        errors=errors+excluded.errors,total_ms=total_ms+excluded.total_ms,
        first=MIN(first,excluded.first),last=MAX(last,excluded.last)",
        params![
            row.last / 86400,
            row.session,
            row.project,
            row.tool,
            row.errors,
            row.total_ms,
            row.last
        ],
    )?;
    prune(&tx, row.last, ROW_LIMIT)?;
    tx.commit()
}

pub fn snapshot(home: &Path) -> rusqlite::Result<Vec<Row>> {
    let db = open(home)?;
    let v2: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='attempts')",
        [],
        |r| r.get(0),
    )?;
    if !v2 {
        return legacy_snapshot(home);
    }
    let mut stmt=db.prepare("SELECT session,project,tool,COUNT(*),SUM(outcome='error'),SUM(COALESCE(json_extract(data,'$.duration_ms'),0)),MIN(started),MAX(started) FROM attempts WHERE started>=?1 AND outcome IN ('success','error') GROUP BY session,project,tool ORDER BY MAX(started) DESC,session,project,tool")?;
    stmt.query_map([now() - 7 * 86400], |r| {
        Ok(Row {
            session: r.get(0)?,
            project: r.get(1)?,
            tool: r.get(2)?,
            calls: r.get(3)?,
            errors: r.get(4)?,
            total_ms: r.get(5)?,
            first: r.get(6)?,
            last: r.get(7)?,
        })
    })?
    .collect()
}

pub fn legacy_snapshot(home: &Path) -> rusqlite::Result<Vec<Row>> {
    let mut db = open(home)?;
    let tx = db.transaction()?;
    prune(&tx, now(), ROW_LIMIT)?;
    let rows = {
        let mut stmt = tx.prepare(
            "SELECT session,project,tool,SUM(calls),SUM(errors),SUM(total_ms),MIN(first),MAX(last)
            FROM calls GROUP BY session,project,tool ORDER BY MAX(last) DESC,session,project,tool",
        )?;
        stmt.query_map([], |r| {
            Ok(Row {
                session: r.get(0)?,
                project: r.get(1)?,
                tool: r.get(2)?,
                calls: r.get(3)?,
                errors: r.get(4)?,
                total_ms: r.get(5)?,
                first: r.get(6)?,
                last: r.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
    };
    tx.commit()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn row(session: &str, project: &str, time: i64) -> Row {
        Row {
            session: session.into(),
            project: project.into(),
            tool: "bars_read".into(),
            calls: 1,
            errors: 0,
            total_ms: 12,
            first: time,
            last: time,
        }
    }
    #[test]
    fn persists_and_groups_by_session_project_tool_with_errors() {
        let home = tempfile::tempdir().unwrap();
        let time = now();
        record(home.path(), &row("a", "p", time)).unwrap();
        let mut error = row("a", "p", time + 1);
        error.errors = 1;
        record(home.path(), &error).unwrap();
        record(home.path(), &row("b", "p", time)).unwrap();
        record(home.path(), &row("a", "q", time)).unwrap();
        let out = snapshot(home.path()).unwrap();
        assert_eq!(out.len(), 3);
        let a = out
            .iter()
            .find(|r| r.session == "a" && r.project == "p")
            .unwrap();
        assert_eq!(
            (a.calls, a.errors, a.total_ms, a.first, a.last),
            (2, 1, 24, time, time + 1)
        );
    }
    #[test]
    fn utc_retention_and_row_cap() {
        let home = tempfile::tempdir().unwrap();
        let time = now();
        record(home.path(), &row("old", "p", time - 30 * 86400)).unwrap();
        record(home.path(), &row("keep", "p", time - 29 * 86400)).unwrap();
        record(home.path(), &row("new", "p", time)).unwrap();
        assert_eq!(snapshot(home.path()).unwrap().len(), 2);
        let db = open(home.path()).unwrap();
        prune(&db, time, 1).unwrap();
        assert_eq!(snapshot(home.path()).unwrap()[0].session, "new");
    }
    #[test]
    fn corrupt_database_is_reported_and_preserved() {
        let home = tempfile::tempdir().unwrap();
        let file = home.path().join("mcp-statistics.sqlite3");
        std::fs::write(&file, b"broken").unwrap();
        assert!(snapshot(home.path()).is_err());
        assert_eq!(std::fs::read(file).unwrap(), b"broken");
    }
}
