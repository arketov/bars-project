use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bars_graph::links::{DocSet, LinkKind, ParsedLink, Resolved, parse_links};
use rusqlite::{Connection, TransactionBehavior, params};

use crate::indexing::chunking::split_frontmatter;
use crate::indexing::walk::{WalkError, read_utf8_path, walk_docs};

/// Версия разбора и разрешения ссылок: смена — полная пересборка индекса без модели.
pub const LINKS_PARSER_VERSION: &str = "links-v1";

const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, thiserror::Error)]
pub enum LinkIndexError {
    #[error("индекс ссылок: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("индекс ссылок создан более новой версией bars (схема {0}); он не изменён")]
    NewerSchema(i64),
    #[error("индекс ссылок: {0}")]
    Io(#[from] std::io::Error),
    #[error("индекс ссылок: {0}")]
    Walk(#[from] WalkError),
}

/// `.bars/links.sqlite3` проекта.
pub fn store_path(project_root: &Path) -> PathBuf {
    project_root.join(".bars").join("links.sqlite3")
}

/// Что сделало обновление: для логов и замеров.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct RefreshStats {
    pub documents: usize,
    pub parsed: usize,
    pub removed: usize,
}

/// Исходящая ссылка документа.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingLink {
    pub line: u32,
    pub text: String,
    /// Адресат как написан.
    pub raw: String,
    pub kind: LinkKind,
    /// Путь адресата от корня проекта; `None` у битой ссылки.
    pub target: Option<String>,
    pub target_title: Option<String>,
    pub broken: bool,
}

/// Документ, который ссылается на данный.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingLink {
    pub path: String,
    pub title: Option<String>,
    pub lines: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinksOf {
    /// Документ учтён индексом ссылок (есть на диске и найден обходом).
    pub known: bool,
    pub outgoing: Vec<OutgoingLink>,
    pub incoming: Vec<IncomingLink>,
}

pub struct LinkIndex {
    db: Connection,
}

/// Корень документации как префикс путей проекта (`docs`, `a/b` или пусто для `.`).
fn docs_prefix(docs_root: &Path) -> Result<String, WalkError> {
    let mut parts = Vec::new();
    for component in docs_root.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            _ => return Err(WalkError::InvalidDocsRoot(docs_root.display().to_string())),
        }
    }
    Ok(parts.join("/"))
}

fn relative_to_docs<'a>(path: &'a str, prefix: &str) -> &'a str {
    if prefix.is_empty() {
        path
    } else {
        path.strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix('/'))
            .unwrap_or(path)
    }
}

fn title_of(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .trim_start_matches('\u{feff}')
        .lines()
        .take(200)
        .collect();
    split_frontmatter(&lines).0
}

fn modified_ns(modified: Option<SystemTime>) -> i64 {
    modified
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(-1, |d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
}

struct Parsed {
    path: String,
    size: i64,
    modified_ns: i64,
    hash: String,
    title: Option<String>,
    links: Vec<ParsedLink>,
}

fn is_corrupt(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(f, _)
            if matches!(f.code, rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt)
    )
}

impl LinkIndex {
    /// Открывает БД проекта. Повреждённый файл заменяется: он производная от текста на диске.
    pub fn open(project_root: &Path) -> Result<Self, LinkIndexError> {
        let path = store_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match Self::open_file(&path) {
            Err(LinkIndexError::Sqlite(error)) if is_corrupt(&error) => {
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

    fn open_file(path: &Path) -> Result<Self, LinkIndexError> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(2000))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(LinkIndexError::NewerSchema(version));
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        if version < SCHEMA_VERSION {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS docs (
                    path TEXT PRIMARY KEY, size INTEGER NOT NULL, modified_ns INTEGER NOT NULL,
                    hash TEXT NOT NULL, title TEXT);
                 CREATE TABLE IF NOT EXISTS links (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    src TEXT NOT NULL REFERENCES docs(path) ON DELETE CASCADE,
                    line INTEGER NOT NULL, start INTEGER NOT NULL, end INTEGER NOT NULL,
                    kind TEXT NOT NULL, raw_target TEXT NOT NULL, text TEXT NOT NULL, fragment TEXT);
                 CREATE INDEX IF NOT EXISTS links_src ON links(src);
                 PRAGMA user_version=1;",
            )?;
            tx.commit()?;
        }
        Ok(Self { db })
    }

    fn meta(&self, key: &str) -> Result<Option<String>, LinkIndexError> {
        let mut statement = self.db.prepare("SELECT value FROM meta WHERE key=?1")?;
        let mut rows = statement.query([key])?;
        Ok(rows.next()?.map(|row| row.get(0)).transpose()?)
    }

    /// Сверяет индекс с файлами на диске: изменившиеся разбирает, пропавшие удаляет. Без модели и без чанков.
    pub fn refresh(
        &mut self,
        project_root: &Path,
        docs_root: &Path,
    ) -> Result<RefreshStats, LinkIndexError> {
        let prefix = docs_prefix(docs_root)?;
        let walk = walk_docs(project_root, docs_root)?;
        let reset = self.meta("parser")?.as_deref() != Some(LINKS_PARSER_VERSION)
            || self.meta("docs_prefix")?.as_deref() != Some(prefix.as_str());

        let mut known: HashMap<String, (i64, i64, String)> = HashMap::new();
        if !reset {
            let mut statement = self
                .db
                .prepare("SELECT path, size, modified_ns, hash FROM docs")?;
            let rows = statement.query_map([], |r| {
                Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?)))
            })?;
            for row in rows {
                let (path, value) = row?;
                known.insert(path, value);
            }
        }

        // Чтение и разбор — вне транзакции: запись держит БД недолго.
        let mut changed: Vec<Parsed> = Vec::new();
        let mut touched: Vec<(String, i64, i64)> = Vec::new();
        for file in &walk.files {
            let size = i64::try_from(file.size).unwrap_or(i64::MAX);
            let modified = modified_ns(file.modified);
            let old = known.get(&file.path);
            if old.is_some_and(|(s, m, _)| *s == size && *m == modified && modified >= 0) {
                continue;
            }
            let text = read_utf8_path(&file.fs_path).ok();
            let hash = match &text {
                Some(text) => blake3::hash(text.as_bytes()).to_hex().to_string(),
                None => "unreadable".to_string(),
            };
            if old.is_some_and(|(_, _, h)| *h == hash) {
                touched.push((file.path.clone(), size, modified));
                continue;
            }
            changed.push(Parsed {
                path: file.path.clone(),
                size,
                modified_ns: modified,
                hash,
                title: text.as_deref().and_then(title_of),
                links: text.as_deref().map(parse_links).unwrap_or_default(),
            });
        }
        let present: HashSet<&str> = walk.files.iter().map(|f| f.path.as_str()).collect();
        let gone: Vec<String> = known
            .keys()
            .filter(|path| !present.contains(path.as_str()))
            .cloned()
            .collect();

        let stats = RefreshStats {
            documents: walk.files.len(),
            parsed: changed.len(),
            removed: gone.len(),
        };
        if !reset && changed.is_empty() && touched.is_empty() && gone.is_empty() {
            return Ok(stats);
        }
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if reset {
            tx.execute("DELETE FROM docs", [])?;
            tx.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES('parser',?1)",
                [LINKS_PARSER_VERSION],
            )?;
            tx.execute(
                "INSERT OR REPLACE INTO meta(key,value) VALUES('docs_prefix',?1)",
                [&prefix],
            )?;
        }
        for path in &gone {
            tx.execute("DELETE FROM docs WHERE path=?1", [path])?;
        }
        for (path, size, modified) in &touched {
            tx.execute(
                "UPDATE docs SET size=?2, modified_ns=?3 WHERE path=?1",
                params![path, size, modified],
            )?;
        }
        for doc in &changed {
            tx.execute("DELETE FROM docs WHERE path=?1", [&doc.path])?;
            tx.execute(
                "INSERT INTO docs(path,size,modified_ns,hash,title) VALUES(?1,?2,?3,?4,?5)",
                params![doc.path, doc.size, doc.modified_ns, doc.hash, doc.title],
            )?;
            let mut insert = tx.prepare_cached(
                "INSERT INTO links(src,line,start,end,kind,raw_target,text,fragment)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            )?;
            for link in &doc.links {
                insert.execute(params![
                    doc.path,
                    link.line,
                    link.start as i64,
                    link.end as i64,
                    link.kind.as_str(),
                    link.raw_target,
                    link.text,
                    link.fragment,
                ])?;
            }
        }
        tx.commit()?;
        Ok(stats)
    }

    /// Исходящие и входящие ссылки документа `path` (от корня проекта). Адресаты разрешаются по набору
    /// документов, который сейчас в индексе.
    pub fn links_of(&self, path: &str) -> Result<LinksOf, LinkIndexError> {
        let prefix = self.meta("docs_prefix")?.unwrap_or_default();
        let mut titles: HashMap<String, Option<String>> = HashMap::new();
        {
            let mut statement = self.db.prepare("SELECT path, title FROM docs")?;
            let rows = statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get(1)?)))?;
            for row in rows {
                let (doc, title) = row?;
                titles.insert(doc, title);
            }
        }
        let known = titles.contains_key(path);
        // Путь от корня документации → путь от корня проекта.
        let by_docs_path: HashMap<String, String> = titles
            .keys()
            .map(|doc| (relative_to_docs(doc, &prefix).to_string(), doc.clone()))
            .collect();
        let set = DocSet::new(by_docs_path.keys().cloned());

        let mut statement = self.db.prepare(
            "SELECT src, line, start, end, kind, raw_target, text, fragment FROM links ORDER BY src, start",
        )?;
        let rows = statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                ParsedLink {
                    line: r.get(1)?,
                    start: r.get::<_, i64>(2)? as usize,
                    end: r.get::<_, i64>(3)? as usize,
                    kind: LinkKind::parse(&r.get::<_, String>(4)?).unwrap_or(LinkKind::Inline),
                    raw_target: r.get(5)?,
                    text: r.get(6)?,
                    fragment: r.get(7)?,
                },
            ))
        })?;

        let own = relative_to_docs(path, &prefix).to_string();
        let mut outgoing = Vec::new();
        let mut incoming: HashMap<String, Vec<u32>> = HashMap::new();
        for row in rows {
            let (src, link) = row?;
            let resolved = set.resolve(relative_to_docs(&src, &prefix), &link);
            if src == path {
                let (target, broken) = match resolved {
                    Resolved::NotALink => continue,
                    Resolved::Found(found) => (by_docs_path.get(found).cloned(), false),
                    Resolved::Broken => (None, true),
                };
                outgoing.push(OutgoingLink {
                    line: link.line,
                    text: link.text,
                    raw: link.raw_target,
                    kind: link.kind,
                    target_title: target
                        .as_ref()
                        .and_then(|t| titles.get(t).cloned().flatten()),
                    target,
                    broken,
                });
            } else if resolved == Resolved::Found(own.as_str()) {
                incoming.entry(src).or_default().push(link.line);
            }
        }
        let mut incoming: Vec<IncomingLink> = incoming
            .into_iter()
            .map(|(src, mut lines)| {
                lines.sort_unstable();
                lines.dedup();
                IncomingLink {
                    title: titles.get(&src).cloned().flatten(),
                    path: src,
                    lines,
                }
            })
            .collect();
        incoming.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(LinksOf {
            known,
            outgoing,
            incoming,
        })
    }
}

#[cfg(test)]
impl LinkIndex {
    pub(super) fn db_for_tests(&self) -> &Connection {
        &self.db
    }
}
