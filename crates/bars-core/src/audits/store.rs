use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::AuditKind;

const SCHEMA_VERSION: i64 = 1;

/// Сколько последних запусков вида хранится (`audits.md`, «Находка и жизнь результата»).
const KEEP_RUNS: i64 = 20;

#[derive(Debug, thiserror::Error)]
pub enum AuditError {
    #[error("хранилище аудитов: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("хранилище аудитов создано более новой версией bars (схема {0}); оно не изменено")]
    NewerSchema(i64),
    #[error("хранилище аудитов: {0}")]
    Io(#[from] std::io::Error),
    #[error("хранилище аудитов: {0}")]
    Json(#[from] serde_json::Error),
}

/// `<проект>/.bars/audits.sqlite3`.
pub fn store_path(project_root: &Path) -> PathBuf {
    project_root.join(".bars").join("audits.sqlite3")
}

/// Итог запуска. Идущий запуск в БД не хранится: он живёт в памяти воркера.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Complete,
    Partial,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Complete => "complete",
            RunStatus::Partial => "partial",
            RunStatus::Failed => "failed",
            RunStatus::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "complete" => RunStatus::Complete,
            "partial" => RunStatus::Partial,
            "cancelled" => RunStatus::Cancelled,
            _ => RunStatus::Failed,
        }
    }
}

/// Как часто запускать вид сам. `OnChange` — только у `broken_links`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleMode {
    Off,
    Daily,
    Weekly,
    OnChange,
}

impl ScheduleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ScheduleMode::Off => "off",
            ScheduleMode::Daily => "daily",
            ScheduleMode::Weekly => "weekly",
            ScheduleMode::OnChange => "on_change",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        [
            ScheduleMode::Off,
            ScheduleMode::Daily,
            ScheduleMode::Weekly,
            ScheduleMode::OnChange,
        ]
        .into_iter()
        .find(|mode| mode.as_str() == value)
    }

    /// Период между запусками в секундах; у `Off` и `OnChange` его нет.
    pub fn interval_secs(self) -> Option<i64> {
        match self {
            ScheduleMode::Daily => Some(86_400),
            ScheduleMode::Weekly => Some(604_800),
            ScheduleMode::Off | ScheduleMode::OnChange => None,
        }
    }
}

/// Состояние находки относительно прежнего полного запуска.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    New,
    Known,
    Gone,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Standing::New => "new",
            Standing::Known => "known",
            Standing::Gone => "gone",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "new" => Standing::New,
            "gone" => Standing::Gone,
            _ => Standing::Known,
        }
    }
}

/// Документ и место в нём, к которым относится находка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subject {
    pub path: String,
    pub first_line: u32,
    pub last_line: u32,
    pub heading: String,
    /// Роль документа ([`super::document_role`]).
    pub role: String,
}

/// Находка, как её отдаёт вид. Отпечаток считает вид: он не должен зависеть от сдвига строк.
#[derive(Debug, Clone, PartialEq)]
pub struct NewFinding {
    pub fingerprint: String,
    /// Подвид: `broken`, `exact`, `near`, `contained`, `partial`, `semantic`.
    pub tag: String,
    /// `main` — в умолчаниях экрана и в счётчиках; `overlap` — по переключателю «Перекрытие содержания».
    pub group: String,
    pub reason: String,
    pub score: Option<f32>,
    pub subjects: Vec<Subject>,
    /// Общий отрывок или текст ссылки; может быть пустым.
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FindingRow {
    pub id: i64,
    pub standing: Standing,
    pub fingerprint: String,
    pub tag: String,
    pub group: String,
    pub reason: String,
    pub score: Option<f32>,
    pub subjects: Vec<Subject>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    pub id: i64,
    pub kind: AuditKind,
    pub started: i64,
    pub finished: i64,
    pub status: RunStatus,
    pub error: Option<String>,
    /// Метка индекса, по которому шёл запуск (для режима «при изменении»).
    pub stamp: Option<String>,
    /// Находок группы `main` без исчезнувших.
    pub total: i64,
    /// Из них новых.
    pub new_count: i64,
    /// Исчезнувших относительно прежнего полного запуска.
    pub gone_count: i64,
    /// Находок группы `overlap` без исчезнувших.
    pub overlap_total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FindingsPage {
    pub items: Vec<FindingRow>,
    /// Всего строк под выбранный фильтр.
    pub total: i64,
    /// Смещение следующей страницы; `None` на последней.
    pub next: Option<i64>,
}

/// Строка прежней находки, переносимая в новый запуск как исчезнувшая: отпечаток, подвид, группа,
/// причина, мера, субъекты (JSON), отрывок.
type GoneRow = (String, String, String, String, Option<f64>, String, String);

pub struct AuditStore {
    db: Connection,
}

fn is_corrupt(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(f.code, rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

fn run_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    let kind: String = r.get(1)?;
    let status: String = r.get(4)?;
    Ok(RunRow {
        id: r.get(0)?,
        kind: AuditKind::parse(&kind).unwrap_or(AuditKind::BrokenLinks),
        started: r.get(2)?,
        finished: r.get(3)?,
        status: RunStatus::parse(&status),
        error: r.get(5)?,
        stamp: r.get(6)?,
        total: r.get(7)?,
        new_count: r.get(8)?,
        gone_count: r.get(9)?,
        overlap_total: r.get(10)?,
    })
}

const RUN_COLUMNS: &str = "id, kind, started, finished, status, error, stamp, total, new_count, gone_count, overlap_total";

impl AuditStore {
    /// Открывает БД проекта. Повреждённый файл заменяется: аудит можно просто запустить заново.
    pub fn open(project_root: &Path) -> Result<Self, AuditError> {
        let path = store_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match Self::open_file(&path) {
            Err(AuditError::Sqlite(error)) if is_corrupt(&error) => {
                for suffix in ["", "-wal", "-shm"] {
                    let mut name = path.as_os_str().to_owned();
                    name.push(suffix);
                    let _ = std::fs::remove_file(name);
                }
                Self::open_file(&path)
            }
            other => other,
        }
    }

    fn open_file(path: &Path) -> Result<Self, AuditError> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(2000))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(AuditError::NewerSchema(version));
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        if version < SCHEMA_VERSION {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS runs (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, kind TEXT NOT NULL,
                    started INTEGER NOT NULL, finished INTEGER NOT NULL, status TEXT NOT NULL,
                    error TEXT, stamp TEXT, total INTEGER NOT NULL DEFAULT 0,
                    new_count INTEGER NOT NULL DEFAULT 0, gone_count INTEGER NOT NULL DEFAULT 0,
                    overlap_total INTEGER NOT NULL DEFAULT 0);
                 CREATE INDEX IF NOT EXISTS runs_kind ON runs(kind, id);
                 CREATE TABLE IF NOT EXISTS findings (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    run_id INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
                    ord INTEGER NOT NULL, standing TEXT NOT NULL, fingerprint TEXT NOT NULL,
                    tag TEXT NOT NULL, grp TEXT NOT NULL, reason TEXT NOT NULL, score REAL,
                    subjects TEXT NOT NULL, detail TEXT NOT NULL);
                 CREATE INDEX IF NOT EXISTS findings_run ON findings(run_id, ord);
                 CREATE TABLE IF NOT EXISTS schedule (kind TEXT PRIMARY KEY, mode TEXT NOT NULL);
                 INSERT OR REPLACE INTO meta(key,value) VALUES('schema','1');
                 PRAGMA user_version=1;",
            )?;
            tx.commit()?;
        }
        Ok(Self { db })
    }

    /// Расписание вида; без записи — умолчание вида.
    pub fn schedule(&self, kind: AuditKind) -> Result<ScheduleMode, AuditError> {
        let mode: Option<String> = self
            .db
            .query_row(
                "SELECT mode FROM schedule WHERE kind=?1",
                [kind.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(mode
            .as_deref()
            .and_then(ScheduleMode::parse)
            .filter(|mode| kind.allows(*mode))
            .unwrap_or_else(|| kind.default_schedule()))
    }

    pub fn set_schedule(&self, kind: AuditKind, mode: ScheduleMode) -> Result<(), AuditError> {
        self.db.execute(
            "INSERT OR REPLACE INTO schedule(kind, mode) VALUES(?1, ?2)",
            params![kind.as_str(), mode.as_str()],
        )?;
        Ok(())
    }

    /// Последний запуск вида, каким бы ни был итог.
    pub fn latest_run(&self, kind: AuditKind) -> Result<Option<RunRow>, AuditError> {
        Ok(self
            .db
            .query_row(
                &format!("SELECT {RUN_COLUMNS} FROM runs WHERE kind=?1 ORDER BY id DESC LIMIT 1"),
                [kind.as_str()],
                run_from_row,
            )
            .optional()?)
    }

    /// Последний полный запуск: только он хранит находки и служит основой сравнения.
    pub fn latest_complete(&self, kind: AuditKind) -> Result<Option<RunRow>, AuditError> {
        Ok(self
            .db
            .query_row(
                &format!(
                    "SELECT {RUN_COLUMNS} FROM runs WHERE kind=?1 AND status='complete' \
                     ORDER BY id DESC LIMIT 1"
                ),
                [kind.as_str()],
                run_from_row,
            )
            .optional()?)
    }

    /// Записывает итог запуска. Полный запуск сохраняет находки и сравнивает их с прежним полным;
    /// неполный, упавший и отменённый — только строку запуска, прежний полный результат цел.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_run(
        &mut self,
        kind: AuditKind,
        started: i64,
        finished: i64,
        status: RunStatus,
        error: Option<&str>,
        stamp: Option<&str>,
        findings: Vec<NewFinding>,
    ) -> Result<RunRow, AuditError> {
        let previous = if status == RunStatus::Complete {
            self.latest_complete(kind)?
        } else {
            None
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO runs(kind, started, finished, status, error, stamp) VALUES(?1,?2,?3,?4,?5,?6)",
            params![kind.as_str(), started, finished, status.as_str(), error, stamp],
        )?;
        let run_id = tx.last_insert_rowid();
        let (mut total, mut new_count, mut gone_count, mut overlap_total) =
            (0i64, 0i64, 0i64, 0i64);
        if status == RunStatus::Complete {
            let mut before: HashSet<String> = HashSet::new();
            if let Some(previous) = &previous {
                let mut statement = tx.prepare(
                    "SELECT fingerprint FROM findings WHERE run_id=?1 AND standing<>'gone'",
                )?;
                for row in statement.query_map([previous.id], |r| r.get::<_, String>(0))? {
                    before.insert(row?);
                }
            }
            let mut seen: HashSet<String> = HashSet::new();
            let mut ord = 0i64;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO findings(run_id, ord, standing, fingerprint, tag, grp, reason, score, subjects, detail)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                )?;
                for finding in &findings {
                    if !seen.insert(finding.fingerprint.clone()) {
                        continue;
                    }
                    let standing = if previous.is_none() || before.contains(&finding.fingerprint) {
                        // Первый запуск не помечает всё «новым»: сравнивать не с чем.
                        Standing::Known
                    } else {
                        Standing::New
                    };
                    let main = finding.group == "main";
                    if main {
                        total += 1;
                        if standing == Standing::New {
                            new_count += 1;
                        }
                    } else {
                        overlap_total += 1;
                    }
                    insert.execute(params![
                        run_id,
                        ord,
                        standing.as_str(),
                        finding.fingerprint,
                        finding.tag,
                        finding.group,
                        finding.reason,
                        finding.score,
                        serde_json::to_string(&finding.subjects)?,
                        finding.detail,
                    ])?;
                    ord += 1;
                }
            }
            if let Some(previous) = &previous {
                // Исчезнувшие: были в прежнем полном запуске, теперь их нет.
                let mut statement = tx.prepare(
                    "SELECT fingerprint, tag, grp, reason, score, subjects, detail FROM findings
                     WHERE run_id=?1 AND standing<>'gone' ORDER BY ord",
                )?;
                let gone: Vec<GoneRow> = statement
                    .query_map([previous.id], |r| {
                        Ok((
                            r.get(0)?,
                            r.get(1)?,
                            r.get(2)?,
                            r.get(3)?,
                            r.get(4)?,
                            r.get(5)?,
                            r.get(6)?,
                        ))
                    })?
                    .collect::<Result<_, _>>()?;
                drop(statement);
                for (fingerprint, tag, group, reason, score, subjects, detail) in gone {
                    if seen.contains(&fingerprint) {
                        continue;
                    }
                    if group == "main" {
                        gone_count += 1;
                    }
                    tx.execute(
                        "INSERT INTO findings(run_id, ord, standing, fingerprint, tag, grp, reason, score, subjects, detail)
                         VALUES(?1,?2,'gone',?3,?4,?5,?6,?7,?8,?9)",
                        params![
                            run_id, ord, fingerprint, tag, group, reason, score, subjects, detail
                        ],
                    )?;
                    ord += 1;
                }
            }
            tx.execute(
                "UPDATE runs SET total=?2, new_count=?3, gone_count=?4, overlap_total=?5 WHERE id=?1",
                params![run_id, total, new_count, gone_count, overlap_total],
            )?;
        }
        // Хранятся последние запуски вида и последний полный (на него опирается сравнение).
        tx.execute(
            "DELETE FROM runs WHERE kind=?1 AND id NOT IN (
                SELECT id FROM runs WHERE kind=?1 ORDER BY id DESC LIMIT ?2)
             AND id NOT IN (
                SELECT id FROM runs WHERE kind=?1 AND status='complete' ORDER BY id DESC LIMIT 1)",
            params![kind.as_str(), KEEP_RUNS],
        )?;
        let row = tx.query_row(
            &format!("SELECT {RUN_COLUMNS} FROM runs WHERE id=?1"),
            [run_id],
            run_from_row,
        )?;
        tx.commit()?;
        Ok(row)
    }

    /// Страница находок запуска. `group` — `main` или `overlap`; пусто — все. Исчезнувшие идут
    /// последними и показываются только при `include_gone`.
    pub fn findings(
        &self,
        run_id: i64,
        group: Option<&str>,
        include_gone: bool,
        offset: i64,
        limit: i64,
    ) -> Result<FindingsPage, AuditError> {
        let filter = "run_id=?1 AND (?2 IS NULL OR grp=?2) AND (?3 OR standing<>'gone')";
        let total: i64 = self.db.query_row(
            &format!("SELECT COUNT(*) FROM findings WHERE {filter}"),
            params![run_id, group, include_gone],
            |r| r.get(0),
        )?;
        let mut statement = self.db.prepare(&format!(
            "SELECT id, standing, fingerprint, tag, grp, reason, score, subjects, detail FROM findings
             WHERE {filter} ORDER BY (standing='gone'), ord LIMIT ?4 OFFSET ?5"
        ))?;
        let rows =
            statement.query_map(params![run_id, group, include_gone, limit, offset], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, Option<f64>>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                ))
            })?;
        let mut items = Vec::new();
        for row in rows {
            let (id, standing, fingerprint, tag, group, reason, score, subjects, detail) = row?;
            items.push(FindingRow {
                id,
                standing: Standing::parse(&standing),
                fingerprint,
                tag,
                group,
                reason,
                score: score.map(|value| value as f32),
                subjects: serde_json::from_str(&subjects)?,
                detail,
            });
        }
        let end = offset + items.len() as i64;
        let next = (end < total).then_some(end);
        Ok(FindingsPage { items, total, next })
    }
}
