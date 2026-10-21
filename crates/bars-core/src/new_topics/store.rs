use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::guard::{differs, normalize_question};
use crate::models::ModelFingerprint;

/// Предел числа тем в проекте. Достигнув его, сбор останавливает новые записи с видимой причиной;
/// существующие темы не вытесняются.
pub const MAX_TOPICS: usize = 2000;
/// Предел вариантов формулировки одной темы; дальше наблюдения считаются на теме без нового варианта.
pub const MAX_VARIANTS_PER_TOPIC: usize = 50;

const SCHEMA_VERSION: i64 = 2;

/// Сколько последних проверок темы хранится: последняя и предыдущая (`design.md`, «Данные и хранение»).
const KEPT_CHECKS: i64 = 2;

/// `<проект>/.bars/new-topics.sqlite3`, отдельно от заменяемого `index.sqlite` (NT-09).
pub fn store_path(project_root: &Path) -> PathBuf {
    project_root.join(".bars").join("new-topics.sqlite3")
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("БД новых тем: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("БД новых тем создана более новой версией bars (схема {0}); она не изменена")]
    NewerSchema(i64),
    #[error("не удалось создать папку БД новых тем: {0}")]
    Io(#[from] std::io::Error),
    #[error("достигнут предел хранения новых тем ({0}); новые записи остановлены")]
    LimitReached(usize),
}

/// Откуда пришло обращение.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Window,
    Mcp,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Mcp => "mcp",
        }
    }
}

/// Основание появления наблюдения.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Успешный поиск вернул ноль результатов.
    EmptyResult,
    /// Пользователь вручную отметил «Не хватило знаний».
    ManualMissing,
}

impl Reason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyResult => "empty_result",
            Self::ManualMissing => "manual_missing",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicStatus {
    Open,
    MaterialsFound,
    Resolved,
}

impl TopicStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::MaterialsFound => "materials_found",
            Self::Resolved => "resolved",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "materials_found" => Self::MaterialsFound,
            "resolved" => Self::Resolved,
            _ => Self::Open,
        }
    }
}

/// Пространство векторов: векторы сравниваются только внутри одного (`design.md`, «Дедупликация темы»).
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddingSpace {
    pub key: String,
}

impl EmbeddingSpace {
    pub fn from_fingerprint(fingerprint: &ModelFingerprint) -> Self {
        Self {
            key: format!(
                "{}|{}|{}|{}|l2",
                fingerprint.source, fingerprint.model, fingerprint.dim, fingerprint.doc_prefix
            ),
        }
    }

    /// Порог косинуса вопрос–вопрос: для bge-m3 автор выбрал 0,68 (20-10-2026, NT-04).
    /// Для неизмеренного пространства — `None`:
    /// семантического автослияния нет, остаётся точное совпадение (NT-04).
    pub fn merge_threshold(&self) -> Option<f32> {
        let model = self.key.split('|').nth(1)?.to_lowercase();
        model.contains("bge-m3").then_some(0.68)
    }
}

/// Одно наблюдение: факт поиска, не доказательство отсутствия знания.
#[derive(Debug, Clone)]
pub struct Observation {
    /// Уникальный ID обращения: повторная доставка не меняет счётчик.
    pub event_id: String,
    /// Поколение списка на момент допуска; запись с прежним поколением отклоняется.
    pub generation: i64,
    pub text: String,
    /// Каноническая область: `project` или `m=<модули>;f=<файлы>`.
    pub scope: String,
    pub source: Source,
    pub reason: Reason,
    /// Применённые параметры поиска, JSON.
    pub params: String,
    /// Метка состояния индекса на момент поиска.
    pub index_stamp: Option<String>,
    /// Нормализованный вектор запроса, если он был вычислен.
    pub vector: Option<(EmbeddingSpace, Vec<f32>)>,
    /// Время, секунды Unix.
    pub at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Exact,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recorded {
    Created {
        topic_id: i64,
    },
    Merged {
        topic_id: i64,
        by: Match,
    },
    /// Тот же `event_id` уже учтён.
    Duplicate,
    /// Сбор выключен.
    Disabled,
    /// Список очищен или сбор переключали после допуска наблюдения.
    StaleGeneration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VariantRow {
    pub id: i64,
    pub text: String,
    pub scope: String,
    /// Параметры первого сохранённого обращения этой формулировки, JSON.
    pub original_params: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopicRow {
    pub id: i64,
    pub revision: i64,
    pub title: String,
    pub status: TopicStatus,
    pub first_seen: i64,
    pub last_seen: i64,
    pub resolved_at: Option<i64>,
    /// Число уникальных самостоятельных наблюдений, не вариантов.
    pub observations: i64,
    /// Источники наблюдений: `window`, `mcp`.
    pub sources: Vec<String>,
    /// Основания наблюдений: `empty_result`, `manual_missing`.
    pub reasons: Vec<String>,
    pub variants: Vec<VariantRow>,
    /// Последняя и предыдущая проверки, новые первыми; заполняется только `get`.
    pub checks: Vec<CheckRow>,
}

/// Итог проверки одного варианта темы.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantOutcome {
    /// Поиск нашёл материалы.
    Found,
    /// Поиск полностью и успешно вернул ноль результатов.
    Empty,
    /// Проверка не дала вывода: индексация, ошибка, область исчезла.
    Incomplete,
}

impl VariantOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Found => "found",
            Self::Empty => "empty",
            Self::Incomplete => "incomplete",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "found" => Self::Found,
            "empty" => Self::Empty,
            _ => Self::Incomplete,
        }
    }
}

/// Результат проверки одного варианта до записи.
#[derive(Debug, Clone)]
pub struct NewCheckResult {
    pub variant_id: i64,
    /// Область, по которой реально искали: исходная или `project` при явном расширении.
    pub scope: String,
    pub outcome: VariantOutcome,
    /// Причина неполноты: `indexing`, `model_loading`, `reindex_required`, `scope_missing`, `error`.
    pub reason: Option<String>,
    pub index_stamp: Option<String>,
    /// Найденные фрагменты, JSON: путь, заголовок, строки, оценка; без текста документа.
    pub hits: String,
}

/// Проверка темы до записи.
#[derive(Debug, Clone)]
pub struct NewCheck {
    /// Ревизия темы в момент начала проверки.
    pub topic_revision: i64,
    pub at: i64,
    /// Явное расширение на весь проект.
    pub expanded: bool,
    pub model: String,
    /// Применённые параметры поиска, JSON.
    pub params: String,
    pub results: Vec<NewCheckResult>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckResultRow {
    pub variant_id: i64,
    pub scope: String,
    pub outcome: VariantOutcome,
    pub reason: Option<String>,
    pub index_stamp: Option<String>,
    pub hits: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CheckRow {
    pub id: i64,
    pub topic_revision: i64,
    pub at: i64,
    pub expanded: bool,
    pub model: String,
    pub params: String,
    pub results: Vec<CheckResultRow>,
}

/// Исход операции над темой с ожидаемой ревизией.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Updated {
        revision: i64,
    },
    /// Темы нет: её удалили или очистили список. ID не переиспользуются.
    NotFound,
    /// Тема изменилась после того, как пользователь её увидел; ничего не записано.
    Conflict {
        current: i64,
    },
}

pub struct TopicStore {
    db: Connection,
}

fn encode(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn decode(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

impl TopicStore {
    /// Открывает БД проекта, создавая её при первой записи. Более новая или повреждённая схема —
    /// ошибка, а не пустой список: БД не заменяется (NT-11).
    pub fn open(project_root: &Path) -> Result<Self, StoreError> {
        let path = store_path(project_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::open_file(&path)
    }

    fn open_file(path: &Path) -> Result<Self, StoreError> {
        let mut db = Connection::open(path)?;
        db.busy_timeout(Duration::from_millis(250))?;
        let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema(version));
        }
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        if version < SCHEMA_VERSION {
            let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS meta (
                    id INTEGER PRIMARY KEY CHECK(id=1), generation INTEGER NOT NULL, enabled INTEGER NOT NULL);
                 INSERT OR IGNORE INTO meta VALUES(1,0,1);
                 CREATE TABLE IF NOT EXISTS topics (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('open','materials_found','resolved')),
                    first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, resolved_at INTEGER);
                 CREATE TABLE IF NOT EXISTS variants (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                    text TEXT NOT NULL, norm_key TEXT NOT NULL, scope TEXT NOT NULL,
                    space TEXT, vector BLOB, created INTEGER NOT NULL);
                 CREATE INDEX IF NOT EXISTS variants_key ON variants(scope, norm_key);
                 CREATE INDEX IF NOT EXISTS variants_topic ON variants(topic_id, id);
                 CREATE TABLE IF NOT EXISTS observations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE,
                    topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                    variant_id INTEGER NOT NULL REFERENCES variants(id) ON DELETE CASCADE,
                    at INTEGER NOT NULL, source TEXT NOT NULL, reason TEXT NOT NULL,
                    params TEXT NOT NULL, index_stamp TEXT);
                 CREATE INDEX IF NOT EXISTS observations_topic ON observations(topic_id);
                 CREATE TABLE IF NOT EXISTS checks (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                    topic_revision INTEGER NOT NULL, at INTEGER NOT NULL,
                    expanded INTEGER NOT NULL, model TEXT NOT NULL, params TEXT NOT NULL);
                 CREATE INDEX IF NOT EXISTS checks_topic ON checks(topic_id, id);
                 CREATE TABLE IF NOT EXISTS check_results (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    check_id INTEGER NOT NULL REFERENCES checks(id) ON DELETE CASCADE,
                    variant_id INTEGER NOT NULL REFERENCES variants(id) ON DELETE CASCADE,
                    scope TEXT NOT NULL,
                    outcome TEXT NOT NULL CHECK(outcome IN ('found','empty','incomplete')),
                    reason TEXT, index_stamp TEXT, hits TEXT NOT NULL);
                 CREATE INDEX IF NOT EXISTS check_results_check ON check_results(check_id);
                 PRAGMA user_version=2;",
            )?;
            tx.commit()?;
        }
        Ok(Self { db })
    }

    /// Поколение списка и включён ли сбор.
    pub fn settings(&self) -> Result<(i64, bool), StoreError> {
        Ok(self
            .db
            .query_row("SELECT generation, enabled FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get::<_, i64>(1)? != 0))
            })?)
    }

    /// Включает или выключает сбор. Смена меняет поколение: не завершённые наблюдения не добавляются.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<i64, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (generation, current): (i64, i64) =
            tx.query_row("SELECT generation, enabled FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        let next = if (current != 0) == enabled {
            generation
        } else {
            tx.execute(
                "UPDATE meta SET generation=?1, enabled=?2 WHERE id=1",
                params![generation + 1, i64::from(enabled)],
            )?;
            generation + 1
        };
        tx.commit()?;
        Ok(next)
    }

    /// Удаляет все темы; сбор не отключается. Поколение растёт: поздние наблюдения отклоняются.
    pub fn clear(&mut self) -> Result<i64, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(
            "DELETE FROM observations; DELETE FROM check_results; DELETE FROM checks;
             DELETE FROM variants; DELETE FROM topics;",
        )?;
        tx.execute("UPDATE meta SET generation=generation+1 WHERE id=1", [])?;
        let generation =
            tx.query_row("SELECT generation FROM meta WHERE id=1", [], |r| r.get(0))?;
        tx.commit()?;
        Ok(generation)
    }

    /// Удаляет тему с вариантами и наблюдениями. `true`, если тема была.
    pub fn delete_topic(&mut self, id: i64) -> Result<bool, StoreError> {
        Ok(self.db.execute("DELETE FROM topics WHERE id=?1", [id])? > 0)
    }

    /// Решение пользователя «Знаний достаточно»: тема закрыта, дата решения сохранена.
    pub fn resolve(&mut self, id: i64, at: i64) -> Result<bool, StoreError> {
        Ok(self.db.execute(
            "UPDATE topics SET status='resolved', resolved_at=?2, revision=revision+1 WHERE id=?1",
            params![id, at],
        )? > 0)
    }

    pub fn topic_count(&self) -> Result<usize, StoreError> {
        Ok(self
            .db
            .query_row("SELECT count(*) FROM topics", [], |r| r.get::<_, i64>(0))?
            as usize)
    }

    /// Записывает наблюдение: точное совпадение, затем семантическое слияние, иначе новая тема.
    pub fn record(&mut self, observation: &Observation) -> Result<Recorded, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (generation, enabled): (i64, i64) =
            tx.query_row("SELECT generation, enabled FROM meta WHERE id=1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if enabled == 0 {
            return Ok(Recorded::Disabled);
        }
        if generation != observation.generation {
            return Ok(Recorded::StaleGeneration);
        }
        let seen: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM observations WHERE event_id=?1",
                [&observation.event_id],
                |r| r.get(0),
            )
            .optional()?;
        if seen.is_some() {
            return Ok(Recorded::Duplicate);
        }
        let key = normalize_question(&observation.text);
        let space = observation.vector.as_ref().map(|(s, _)| s.key.clone());
        let blob = observation.vector.as_ref().map(|(_, v)| encode(v));

        let exact: Option<(i64, i64)> = tx
            .query_row(
                "SELECT id, topic_id FROM variants WHERE scope=?1 AND norm_key=?2 ORDER BY id LIMIT 1",
                params![observation.scope, key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let mut matched = exact.map(|(variant, topic)| (topic, Some(variant), Match::Exact));
        if matched.is_none() {
            matched = Self::semantic(&tx, observation)?.map(|t| (t, None, Match::Semantic));
        }

        let (topic_id, variant_id, outcome) = match matched {
            Some((topic, variant, by)) => {
                let variant = match variant {
                    Some(id) => id,
                    None => {
                        let count: i64 = tx.query_row(
                            "SELECT count(*) FROM variants WHERE topic_id=?1",
                            [topic],
                            |r| r.get(0),
                        )?;
                        if count as usize >= MAX_VARIANTS_PER_TOPIC {
                            tx.query_row(
                                "SELECT max(id) FROM variants WHERE topic_id=?1",
                                [topic],
                                |r| r.get(0),
                            )?
                        } else {
                            Self::insert_variant(&tx, topic, observation, &key, &space, &blob)?
                        }
                    }
                };
                (
                    topic,
                    variant,
                    Recorded::Merged {
                        topic_id: topic,
                        by,
                    },
                )
            }
            None => {
                let topics: i64 = tx.query_row("SELECT count(*) FROM topics", [], |r| r.get(0))?;
                if topics as usize >= MAX_TOPICS {
                    return Err(StoreError::LimitReached(MAX_TOPICS));
                }
                tx.execute(
                    "INSERT INTO topics(revision, title, status, first_seen, last_seen) VALUES(1,?1,'open',?2,?2)",
                    params![observation.text.trim(), observation.at],
                )?;
                let topic = tx.last_insert_rowid();
                let variant = Self::insert_variant(&tx, topic, observation, &key, &space, &blob)?;
                (topic, variant, Recorded::Created { topic_id: topic })
            }
        };
        if matches!(outcome, Recorded::Merged { .. }) {
            // Тот же спрос к закрытой теме возвращает её в работу; решение и его дата сохраняются.
            tx.execute(
                "UPDATE topics SET revision=revision+1, last_seen=max(last_seen,?2),
                 status=CASE WHEN status='resolved' THEN 'open' ELSE status END WHERE id=?1",
                params![topic_id, observation.at],
            )?;
        }
        tx.execute(
            "INSERT INTO observations(event_id, topic_id, variant_id, at, source, reason, params, index_stamp)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                observation.event_id,
                topic_id,
                variant_id,
                observation.at,
                observation.source.as_str(),
                observation.reason.as_str(),
                observation.params,
                observation.index_stamp
            ],
        )?;
        tx.commit()?;
        Ok(outcome)
    }

    fn insert_variant(
        tx: &rusqlite::Transaction<'_>,
        topic: i64,
        observation: &Observation,
        key: &str,
        space: &Option<String>,
        blob: &Option<Vec<u8>>,
    ) -> Result<i64, StoreError> {
        tx.execute(
            "INSERT INTO variants(topic_id, text, norm_key, scope, space, vector, created)
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                topic,
                observation.text,
                key,
                observation.scope,
                space,
                blob,
                observation.at
            ],
        )?;
        Ok(tx.last_insert_rowid())
    }

    /// Ближайшая тема в той же области и том же пространстве векторов: сравнение с устойчивым
    /// представителем (первым вариантом с вектором этого пространства), а не с последним участником, чтобы не склеить цепочку A≈B≈C.
    fn semantic(
        tx: &rusqlite::Transaction<'_>,
        observation: &Observation,
    ) -> Result<Option<i64>, StoreError> {
        let Some((space, vector)) = &observation.vector else {
            return Ok(None);
        };
        let Some(threshold) = space.merge_threshold() else {
            return Ok(None);
        };
        let mut statement = tx.prepare(
            "SELECT v.topic_id, v.text, v.vector FROM variants v
             WHERE v.scope=?1 AND v.space=?2 AND v.vector IS NOT NULL
               AND v.id=(SELECT min(id) FROM variants WHERE topic_id=v.topic_id AND space=v.space AND vector IS NOT NULL)",
        )?;
        let rows = statement.query_map(params![observation.scope, space.key], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?;
        let mut best: Option<(i64, f32)> = None;
        for row in rows {
            let (topic, text, blob) = row?;
            let other = decode(&blob);
            if other.len() != vector.len() {
                continue;
            }
            let cosine = dot(vector, &other);
            if cosine >= threshold
                && best.is_none_or(|(_, b)| cosine > b)
                && !differs(&observation.text, &text)
            {
                best = Some((topic, cosine));
            }
        }
        Ok(best.map(|(topic, _)| topic))
    }

    /// Темы от свежих к старым: по последнему обращению. Проверки не читаются (см. `get`).
    pub fn list(&self) -> Result<Vec<TopicRow>, StoreError> {
        self.load(None)
    }

    /// Тема, к которой привязано наблюдение с этим `event_id`.
    pub fn topic_of_event(&self, event_id: &str) -> Result<Option<i64>, StoreError> {
        Ok(self
            .db
            .query_row(
                "SELECT topic_id FROM observations WHERE event_id=?1",
                [event_id],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Одна тема с последней и предыдущей проверками; `None`, если её нет.
    pub fn get(&self, id: i64) -> Result<Option<TopicRow>, StoreError> {
        let Some(mut topic) = self.load(Some(id))?.pop() else {
            return Ok(None);
        };
        topic.checks = self.checks(id)?;
        Ok(Some(topic))
    }

    fn load(&self, only: Option<i64>) -> Result<Vec<TopicRow>, StoreError> {
        let mut statement = self.db.prepare(
            "SELECT t.id, t.revision, t.title, t.status, t.first_seen, t.last_seen, t.resolved_at,
                    (SELECT count(*) FROM observations o WHERE o.topic_id=t.id),
                    (SELECT group_concat(DISTINCT o.source) FROM observations o WHERE o.topic_id=t.id),
                    (SELECT group_concat(DISTINCT o.reason) FROM observations o WHERE o.topic_id=t.id)
             FROM topics t WHERE (?1 IS NULL OR t.id=?1) ORDER BY t.last_seen DESC, t.id DESC",
        )?;
        let split = |value: Option<String>| -> Vec<String> {
            value
                .map(|v| v.split(',').map(str::to_owned).collect())
                .unwrap_or_default()
        };
        let mut topics = statement
            .query_map([only], |r| {
                Ok(TopicRow {
                    id: r.get(0)?,
                    revision: r.get(1)?,
                    title: r.get(2)?,
                    status: TopicStatus::parse(&r.get::<_, String>(3)?),
                    first_seen: r.get(4)?,
                    last_seen: r.get(5)?,
                    resolved_at: r.get(6)?,
                    observations: r.get(7)?,
                    sources: split(r.get(8)?),
                    reasons: split(r.get(9)?),
                    variants: Vec::new(),
                    checks: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut variants = self.db.prepare(
            "WITH first_observations AS (
                SELECT o.variant_id, min(o.id) AS id FROM observations o
                JOIN variants v ON v.id=o.variant_id
                WHERE (?1 IS NULL OR v.topic_id=?1) GROUP BY o.variant_id
             )
             SELECT v.id, v.topic_id, v.text, v.scope, COALESCE(o.params, '{}')
             FROM variants v
             LEFT JOIN first_observations f ON f.variant_id=v.id
             LEFT JOIN observations o ON o.id=f.id
             WHERE (?1 IS NULL OR v.topic_id=?1) ORDER BY v.id",
        )?;
        let rows = variants.query_map([only], |r| {
            Ok((
                r.get::<_, i64>(1)?,
                VariantRow {
                    id: r.get(0)?,
                    text: r.get(2)?,
                    scope: r.get(3)?,
                    original_params: r.get(4)?,
                },
            ))
        })?;
        for row in rows {
            let (topic, variant) = row?;
            if let Some(t) = topics.iter_mut().find(|t| t.id == topic) {
                t.variants.push(variant);
            }
        }
        Ok(topics)
    }

    /// Последняя и предыдущая проверки темы, новые первыми.
    fn checks(&self, topic: i64) -> Result<Vec<CheckRow>, StoreError> {
        let mut statement = self.db.prepare(
            "SELECT id, topic_revision, at, expanded, model, params FROM checks
             WHERE topic_id=?1 ORDER BY id DESC LIMIT ?2",
        )?;
        let mut checks = statement
            .query_map(params![topic, KEPT_CHECKS], |r| {
                Ok(CheckRow {
                    id: r.get(0)?,
                    topic_revision: r.get(1)?,
                    at: r.get(2)?,
                    expanded: r.get::<_, i64>(3)? != 0,
                    model: r.get(4)?,
                    params: r.get(5)?,
                    results: Vec::new(),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut results = self.db.prepare(
            "SELECT variant_id, scope, outcome, reason, index_stamp, hits FROM check_results
             WHERE check_id=?1 ORDER BY id",
        )?;
        for check in &mut checks {
            check.results = results
                .query_map([check.id], |r| {
                    Ok(CheckResultRow {
                        variant_id: r.get(0)?,
                        scope: r.get(1)?,
                        outcome: VariantOutcome::parse(&r.get::<_, String>(2)?),
                        reason: r.get(3)?,
                        index_stamp: r.get(4)?,
                        hits: r.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?;
        }
        Ok(checks)
    }

    /// Текущая ревизия темы и статус под транзакцией; `Err(Change)` — тема исчезла или изменилась.
    fn expect_revision(
        tx: &rusqlite::Transaction<'_>,
        id: i64,
        revision: i64,
    ) -> Result<Result<String, Change>, StoreError> {
        let current: Option<(i64, String)> = tx
            .query_row(
                "SELECT revision, status FROM topics WHERE id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(match current {
            None => Err(Change::NotFound),
            Some((current, _)) if current != revision => Err(Change::Conflict { current }),
            Some((_, status)) => Ok(status),
        })
    }

    /// Решение пользователя: «Знаний достаточно» (`Resolved`) или «Всё ещё не хватает» (`Open`).
    /// Материалы последней проверки не стираются. Дата решения сохраняется при закрытии.
    pub fn set_status(
        &mut self,
        id: i64,
        revision: i64,
        status: TopicStatus,
        at: i64,
    ) -> Result<Change, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Err(change) = Self::expect_revision(&tx, id, revision)? {
            return Ok(change);
        }
        tx.execute(
            "UPDATE topics SET status=?2, revision=revision+1,
             resolved_at=CASE WHEN ?2='resolved' THEN ?3 ELSE resolved_at END WHERE id=?1",
            params![id, status.as_str(), at],
        )?;
        tx.commit()?;
        Ok(Change::Updated {
            revision: revision + 1,
        })
    }

    /// Переименование заголовка; исходные формулировки вариантов не меняются.
    pub fn rename(&mut self, id: i64, revision: i64, title: &str) -> Result<Change, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Err(change) = Self::expect_revision(&tx, id, revision)? {
            return Ok(change);
        }
        tx.execute(
            "UPDATE topics SET title=?2, revision=revision+1 WHERE id=?1",
            params![id, title],
        )?;
        tx.commit()?;
        Ok(Change::Updated {
            revision: revision + 1,
        })
    }

    /// Удаляет тему, если она не менялась с момента, когда пользователь её видел.
    pub fn delete_checked(&mut self, id: i64, revision: i64) -> Result<Change, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Err(change) = Self::expect_revision(&tx, id, revision)? {
            return Ok(change);
        }
        tx.execute("DELETE FROM topics WHERE id=?1", [id])?;
        tx.commit()?;
        Ok(Change::Updated { revision })
    }

    /// Записывает проверку темы; прежних сохраняется последняя и новая. Тема, которой уже нет, не
    /// восстанавливается: `None`. Если хоть один вариант нашёл материалы, открытая тема получает статус
    /// «Найдены совпадения»; закрытую решением пользователя не трогает. Полностью пустая проверка
    /// по одному индексу снимает прежнюю автоматическую находку; неполная статус не меняет.
    pub fn record_check(
        &mut self,
        topic: i64,
        check: &NewCheck,
    ) -> Result<Option<CheckRow>, StoreError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<(String, i64)> = tx
            .query_row(
                "SELECT status, revision FROM topics WHERE id=?1",
                [topic],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((status, revision)) = current else {
            return Ok(None);
        };
        tx.execute(
            "INSERT INTO checks(topic_id, topic_revision, at, expanded, model, params)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![
                topic,
                check.topic_revision,
                check.at,
                i64::from(check.expanded),
                check.model,
                check.params
            ],
        )?;
        let check_id = tx.last_insert_rowid();
        for result in &check.results {
            // Вариант могли убрать разъединением или лимитом; результат без варианта не пишем.
            let exists: Option<i64> = tx
                .query_row(
                    "SELECT 1 FROM variants WHERE id=?1 AND topic_id=?2",
                    params![result.variant_id, topic],
                    |r| r.get(0),
                )
                .optional()?;
            if exists.is_none() {
                continue;
            }
            tx.execute(
                "INSERT INTO check_results(check_id, variant_id, scope, outcome, reason, index_stamp, hits)
                 VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    check_id,
                    result.variant_id,
                    result.scope,
                    result.outcome.as_str(),
                    result.reason,
                    result.index_stamp,
                    result.hits
                ],
            )?;
        }
        tx.execute(
            "DELETE FROM checks WHERE topic_id=?1 AND id NOT IN
             (SELECT id FROM checks WHERE topic_id=?1 ORDER BY id DESC LIMIT ?2)",
            params![topic, KEPT_CHECKS],
        )?;
        let found = check
            .results
            .iter()
            .any(|r| r.outcome == VariantOutcome::Found);
        let all_empty = check.results.first().is_some_and(|first| {
            check.results.iter().all(|result| {
                result.outcome == VariantOutcome::Empty && result.index_stamp == first.index_stamp
            })
        });
        let next_status = if found && status == "open" {
            Some("materials_found")
        } else if all_empty && status == "materials_found" {
            Some("open")
        } else {
            None
        };
        if let Some(next_status) = next_status {
            tx.execute(
                "UPDATE topics SET status=?2, revision=revision+1 WHERE id=?1",
                params![topic, next_status],
            )?;
            // Переход сделала сама проверка: пока тему не меняли другие, проверка относится к новой
            // ревизии и не считается устаревшей (устаревает только от чужого изменения).
            if revision == check.topic_revision {
                tx.execute(
                    "UPDATE checks SET topic_revision=?1 WHERE id=?2",
                    params![revision + 1, check_id],
                )?;
            }
        }
        tx.commit()?;
        Ok(self.get(topic)?.and_then(|t| t.checks.into_iter().next()))
    }
}
