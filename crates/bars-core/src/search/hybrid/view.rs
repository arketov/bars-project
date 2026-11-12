//! Слой доступа поиска к индексам: один источник (обычный поиск проекта) или два — индекс
//! владельца с исключёнными путями и дельта worktree со смещением номеров (`docs/features/
//! projects/worktrees.md`, «Идентификаторы чанков», «Слияние кандидатов до rerank»).
//!
//! Номер чанка или документа дельты в пуле, в ответе и в коде ранжирования — `локальный +
//! DELTA_ID_OFFSET`; слой вычитает смещение, когда читает дельту. У вида из одного источника
//! смещения нет, а поведение, баллы и порядок совпадают с поиском по `Index`.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::params_from_iter;

use super::{
    Scope, SearchError, SearchHit, escape_like, load_hits_numbered, placeholders,
};
use crate::search::fulltext::{ExactProbe, f_signal, p_signal};
use crate::search::operators::OperatorFilter;
use crate::storage::Index;

/// Смещение номеров чанков и документов дельты: номер ≥ смещения — «читать из дельты».
pub const DELTA_ID_OFFSET: i64 = 1 << 40;

/// Условие, которое скрывает у владельца исключённые пути (временная таблица читающего
/// соединения `temp.wt_excluded`).
const NOT_EXCLUDED: &str = "path NOT IN (SELECT path FROM temp.wt_excluded)";

/// Источники поиска: владелец и, для worktree, дельта.
#[derive(Clone, Copy)]
pub struct IndexView<'a> {
    owner: &'a Index,
    delta: Option<&'a Index>,
    excluded: bool,
}

impl<'a> IndexView<'a> {
    /// Обычный поиск проекта: один источник без исключений.
    pub fn single(owner: &'a Index) -> Self {
        Self {
            owner,
            delta: None,
            excluded: false,
        }
    }

    /// Вид worktree: индекс владельца без `excluded` путей и дельта. Список кладётся во временную
    /// таблицу читающего соединения владельца; сам индекс не меняется.
    pub fn worktree(
        owner: &'a Index,
        delta: &'a Index,
        excluded: &[String],
    ) -> Result<Self, SearchError> {
        let conn = owner.conn();
        conn.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS wt_excluded(path TEXT PRIMARY KEY) WITHOUT ROWID;
             DELETE FROM temp.wt_excluded;",
        )?;
        let transaction = conn
            .is_autocommit()
            .then(|| conn.unchecked_transaction())
            .transpose()?;
        {
            let mut insert =
                conn.prepare_cached("INSERT OR IGNORE INTO temp.wt_excluded(path) VALUES (?1)")?;
            for path in excluded {
                insert.execute([path])?;
            }
        }
        if let Some(transaction) = transaction {
            transaction.commit()?;
        }
        Ok(Self {
            owner,
            delta: Some(delta),
            excluded: true,
        })
    }

    /// Индекс владельца: отпечаток модели, словарь исправления опечаток и язык проекта берутся
    /// из него.
    pub fn owner(&self) -> &'a Index {
        self.owner
    }

    pub fn delta(&self) -> Option<&'a Index> {
        self.delta
    }

    /// Есть ли второй источник: слияние кандидатов, `l` и расширение связями не работают.
    pub fn is_merged(&self) -> bool {
        self.delta.is_some()
    }

    /// База и локальный номер для номера чанка или документа вида.
    pub fn source(&self, id: i64) -> (&'a Index, i64) {
        match self.delta {
            Some(delta) if id >= DELTA_ID_OFFSET => (delta, id - DELTA_ID_OFFSET),
            _ => (self.owner, id),
        }
    }

    /// Номера владельца и локальные номера дельты.
    fn split(&self, ids: &[i64]) -> (Vec<i64>, Vec<i64>) {
        let mut owner = Vec::new();
        let mut delta = Vec::new();
        for &id in ids {
            if self.delta.is_some() && id >= DELTA_ID_OFFSET {
                delta.push(id - DELTA_ID_OFFSET);
            } else {
                owner.push(id);
            }
        }
        (owner, delta)
    }

    /// Область для запросов к владельцу: с условием исключённых путей, если оно есть.
    fn owner_scope(&self, scope: &Scope) -> Option<Scope> {
        self.excluded.then(|| scope.excluding_wt())
    }

    /// Чанков в области по обоим источникам.
    pub(super) fn chunk_count(&self, scope: &Scope) -> Result<i64, SearchError> {
        let owner_scope = self.owner_scope(scope);
        let mut total = owner_scope
            .as_ref()
            .unwrap_or(scope)
            .chunk_count(self.owner)?;
        if let Some(delta) = self.delta {
            total += scope.chunk_count(delta)?;
        }
        Ok(total)
    }

    /// Первые `limit` чанков каждой базы по составному `bm25` с сырым баллом; номера дельты со
    /// смещением. Сырые баллы баз сравнивать нельзя: `f` считается внутри базы (`f_signal`).
    pub(super) fn text_scored(
        &self,
        scope: &Scope,
        match_query: &str,
        limit: usize,
    ) -> Result<Vec<(i64, f32)>, SearchError> {
        let owner_scope = self.owner_scope(scope);
        let mut rows = owner_scope
            .as_ref()
            .unwrap_or(scope)
            .text_scored(self.owner, match_query, limit)?;
        if let Some(delta) = self.delta {
            rows.extend(
                scope
                    .text_scored(delta, match_query, limit)?
                    .into_iter()
                    .map(|(id, score)| (id + DELTA_ID_OFFSET, score)),
            );
        }
        Ok(rows)
    }

    /// То же без баллов.
    pub(super) fn text_candidates(
        &self,
        scope: &Scope,
        match_query: &str,
        limit: usize,
    ) -> Result<Vec<i64>, SearchError> {
        Ok(self
            .text_scored(scope, match_query, limit)?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// Косинусы области: по каждой базе — первые `candidates` и заданные `text`/`exact`; карта
    /// общая, номера дельты со смещением. Косинус абсолютный, слияние точное.
    pub(super) fn cosines(
        &self,
        scope: &Scope,
        query_vector: &[f32],
        candidates: usize,
        text: &[i64],
        exact: &[i64],
    ) -> Result<HashMap<i64, f32>, SearchError> {
        let owner_scope = self.owner_scope(scope);
        let owner_scope = owner_scope.as_ref().unwrap_or(scope);
        let Some(delta) = self.delta else {
            return owner_scope.cosines(self.owner, query_vector, candidates, text, exact);
        };
        let (text_owner, text_delta) = self.split(text);
        let (exact_owner, exact_delta) = self.split(exact);
        let mut scores = owner_scope.cosines(
            self.owner,
            query_vector,
            candidates,
            &text_owner,
            &exact_owner,
        )?;
        scores.extend(
            scope
                .cosines(delta, query_vector, candidates, &text_delta, &exact_delta)?
                .into_iter()
                .map(|(id, score)| (id + DELTA_ID_OFFSET, score)),
        );
        Ok(scores)
    }

    /// Балл `f`: сырой балл чанка делится на лучший балл списка **своей** базы.
    pub(super) fn f_signal(
        &self,
        expression: &str,
        listed: &[(i64, f32)],
        ids: &[i64],
    ) -> rusqlite::Result<HashMap<i64, f32>> {
        let Some(delta) = self.delta else {
            return f_signal(self.owner.conn(), expression, listed, ids);
        };
        let mut listed_owner = Vec::new();
        let mut listed_delta = Vec::new();
        for &(id, score) in listed {
            if id >= DELTA_ID_OFFSET {
                listed_delta.push((id - DELTA_ID_OFFSET, score));
            } else {
                listed_owner.push((id, score));
            }
        }
        let (ids_owner, ids_delta) = self.split(ids);
        let mut scores = f_signal(self.owner.conn(), expression, &listed_owner, &ids_owner)?;
        scores.extend(
            f_signal(delta.conn(), expression, &listed_delta, &ids_delta)?
                .into_iter()
                .map(|(id, score)| (id + DELTA_ID_OFFSET, score)),
        );
        Ok(scores)
    }

    /// Балл `p`: не зависит от корпуса, каждая база считает по своим чанкам.
    pub(super) fn p_signal(
        &self,
        probe: &ExactProbe,
        ids: &[i64],
    ) -> rusqlite::Result<HashMap<i64, f32>> {
        let Some(delta) = self.delta else {
            return p_signal(self.owner.conn(), probe, ids);
        };
        let (ids_owner, ids_delta) = self.split(ids);
        let mut scores = p_signal(self.owner.conn(), probe, &ids_owner)?;
        scores.extend(
            p_signal(delta.conn(), probe, &ids_delta)?
                .into_iter()
                .map(|(id, score)| (id + DELTA_ID_OFFSET, score)),
        );
        Ok(scores)
    }

    /// Условия операторов запроса для кандидатов, пришедших не из полнотекстового списка.
    pub(super) fn retain(
        &self,
        filter: &OperatorFilter,
        ids: &mut Vec<i64>,
    ) -> rusqlite::Result<()> {
        let Some(delta) = self.delta else {
            return filter.retain(self.owner.conn(), ids);
        };
        if filter.is_empty() || ids.is_empty() {
            return Ok(());
        }
        let (mut owner, mut local) = self.split(ids);
        filter.retain(self.owner.conn(), &mut owner)?;
        filter.retain(delta.conn(), &mut local)?;
        let kept: HashSet<i64> = owner
            .into_iter()
            .chain(local.into_iter().map(|id| id + DELTA_ID_OFFSET))
            .collect();
        ids.retain(|id| kept.contains(id));
        Ok(())
    }

    /// Чанки с документами в порядке `ids`; `chunk_id` результата — номер вида.
    pub(super) fn load_hits(&self, ids: &[i64]) -> Result<Vec<SearchHit>, SearchError> {
        if self.delta.is_none() {
            return load_hits_numbered(self.owner, ids.iter().map(|&id| (id, id)));
        }
        let mut hits = Vec::with_capacity(ids.len());
        for &id in ids {
            let (index, local) = self.source(id);
            hits.extend(load_hits_numbered(index, [(local, id)])?);
        }
        Ok(hits)
    }

    /// Документ, цепочка заголовков и номер куска по номеру чанка вида; номер документа — тоже
    /// номер вида (дельта со смещением).
    pub(super) fn chunk_document(
        &self,
        id: i64,
    ) -> Result<(i64, String, String, i64), SearchError> {
        let (index, local) = self.source(id);
        let (document_id, path, heading_chain, ordinal) = index
            .conn()
            .prepare_cached(
                "SELECT c.document_id, d.path, c.heading_chain, c.ordinal
                 FROM chunks c JOIN documents d ON d.id = c.document_id WHERE c.id = ?1",
            )?
            .query_row([local], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?;
        let shifted = if std::ptr::eq(index, self.owner) {
            document_id
        } else {
            document_id + DELTA_ID_OFFSET
        };
        Ok((shifted, path, heading_chain, ordinal))
    }

    /// Куски раздела: сколько их, первая и последняя строка. `document_id` — номер вида.
    pub(super) fn section_span(
        &self,
        document_id: i64,
        heading_chain: &str,
    ) -> Result<(i64, i64, i64), SearchError> {
        let (index, local) = self.source(document_id);
        Ok(index.conn().query_row(
            "SELECT count(*), min(first_line), max(last_line) FROM chunks
             WHERE document_id = ?1 AND heading_chain = ?2",
            rusqlite::params![local, heading_chain],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?)
    }

    /// Сколько чанков пула в каждом модуле (по обоим источникам).
    pub(super) fn count_candidate_modules(
        &self,
        ids: &[i64],
    ) -> Result<BTreeMap<String, usize>, SearchError> {
        let mut modules = BTreeMap::new();
        let (owner, delta) = self.split(ids);
        let sources = [(Some(self.owner), owner), (self.delta, delta)];
        for (index, ids) in sources {
            let Some(index) = index.filter(|_| !ids.is_empty()) else {
                continue;
            };
            let mut statement = index.conn().prepare(&format!(
                "SELECT d.module, count(*) FROM chunks c JOIN documents d ON d.id = c.document_id \
                 WHERE c.id IN ({}) GROUP BY d.module",
                placeholders(ids.len())
            ))?;
            let rows = statement.query_map(params_from_iter(&ids), |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
            })?;
            for row in rows {
                let (module, count) = row?;
                *modules.entry(module).or_default() += count;
            }
        }
        Ok(modules)
    }
}

/// Документы, по которым проверяется область запроса: модули, файлы и префиксы `in` приводятся к
/// путям индекса по этому каталогу. У `Index` — его `documents`; у вида — допустимые документы
/// владельца и документы дельты.
pub(in crate::search) trait Catalog {
    /// `docs_root` владельца: путь документа принимается от корня документации и от корня проекта.
    fn docs_root(&self) -> Result<String, SearchError>;
    fn has_module(&self, module: &str) -> Result<bool, SearchError>;
    fn has_path(&self, path: &str) -> Result<bool, SearchError>;
    /// Есть ли документ с этим путём или под папкой с этим путём.
    fn has_under(&self, path: &str) -> Result<bool, SearchError>;
    /// Все пары `(модуль, путь)`: для подсказки ближайших.
    fn documents(&self) -> Result<Vec<(String, String)>, SearchError>;
}

fn exists(index: &Index, sql: &str, params: &[&str]) -> Result<bool, SearchError> {
    Ok(index
        .conn()
        .prepare_cached(sql)?
        .query_row(params_from_iter(params), |row| row.get::<_, bool>(0))?)
}

fn pairs(index: &Index, condition: &str) -> Result<Vec<(String, String)>, SearchError> {
    let mut statement = index
        .conn()
        .prepare(&format!("SELECT module, path FROM documents WHERE {condition}"))?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

impl Catalog for Index {
    fn docs_root(&self) -> Result<String, SearchError> {
        Ok(self
            .stored_layout()?
            .map(|layout| layout.docs_root)
            .unwrap_or_default())
    }

    fn has_module(&self, module: &str) -> Result<bool, SearchError> {
        exists(
            self,
            "SELECT EXISTS(SELECT 1 FROM documents WHERE module = ?1)",
            &[module],
        )
    }

    fn has_path(&self, path: &str) -> Result<bool, SearchError> {
        exists(
            self,
            "SELECT EXISTS(SELECT 1 FROM documents WHERE path = ?1)",
            &[path],
        )
    }

    fn has_under(&self, path: &str) -> Result<bool, SearchError> {
        let like = format!("{}/%", escape_like(path));
        exists(
            self,
            "SELECT EXISTS(SELECT 1 FROM documents WHERE path = ?1 OR path LIKE ?2 ESCAPE '\\')",
            &[path, like.as_str()],
        )
    }

    fn documents(&self) -> Result<Vec<(String, String)>, SearchError> {
        pairs(self, "1")
    }
}

impl Catalog for IndexView<'_> {
    fn docs_root(&self) -> Result<String, SearchError> {
        self.owner.docs_root()
    }

    fn has_module(&self, module: &str) -> Result<bool, SearchError> {
        let filter = if self.excluded {
            format!(" AND {NOT_EXCLUDED}")
        } else {
            String::new()
        };
        let sql = format!("SELECT EXISTS(SELECT 1 FROM documents WHERE module = ?1{filter})");
        if exists(self.owner, &sql, &[module])? {
            return Ok(true);
        }
        match self.delta {
            Some(delta) => delta.has_module(module),
            None => Ok(false),
        }
    }

    fn has_path(&self, path: &str) -> Result<bool, SearchError> {
        let filter = if self.excluded {
            format!(" AND {NOT_EXCLUDED}")
        } else {
            String::new()
        };
        let sql = format!("SELECT EXISTS(SELECT 1 FROM documents WHERE path = ?1{filter})");
        if exists(self.owner, &sql, &[path])? {
            return Ok(true);
        }
        match self.delta {
            Some(delta) => delta.has_path(path),
            None => Ok(false),
        }
    }

    fn has_under(&self, path: &str) -> Result<bool, SearchError> {
        let filter = if self.excluded {
            format!(" AND {NOT_EXCLUDED}")
        } else {
            String::new()
        };
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM documents
             WHERE (path = ?1 OR path LIKE ?2 ESCAPE '\\'){filter})"
        );
        let like = format!("{}/%", escape_like(path));
        if exists(self.owner, &sql, &[path, like.as_str()])? {
            return Ok(true);
        }
        match self.delta {
            Some(delta) => delta.has_under(path),
            None => Ok(false),
        }
    }

    fn documents(&self) -> Result<Vec<(String, String)>, SearchError> {
        let filter = if self.excluded { NOT_EXCLUDED } else { "1" };
        let mut all = pairs(self.owner, filter)?;
        if let Some(delta) = self.delta {
            all.extend(pairs(delta, "1")?);
        }
        Ok(all)
    }
}

#[cfg(test)]
mod tests;
