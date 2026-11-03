//! Индекс проекта: файл SQLite со схемой из `docs/architecture/schema.md`.
//!
//! Слой хранения не знает ни про обход файлов, ни про модель: он принимает готовые документы,
//! чанки и векторы и записывает каждый документ одной транзакцией.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};

use super::stem::{STEM_VERSION, TextStemmer};
use crate::indexing::chunking::{CHUNKING_VERSION, Chunk, ChunkingParams};
use crate::models::ModelFingerprint;

/// Версия схемы, которую создаёт и читает эта версия bars.
pub const SCHEMA_VERSION: u32 = 1;

/// `<проект>/.bars/index.sqlite` (`docs/architecture/files.md`).
pub fn index_path(project_root: &Path) -> PathBuf {
    project_root.join(".bars").join("index.sqlite")
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("ошибка SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("ошибка файла индекса: {0}")]
    Io(#[from] std::io::Error),
    #[error("схема индекса версии {found} новее известной {supported}: индекс не открыт")]
    SchemaTooNew { found: u32, supported: u32 },
    #[error("в `meta` индекса неверное значение ключа `{0}`")]
    BadMeta(String),
}

/// Режим журнала SQLite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Journal {
    /// Рабочий индекс: поиск читает, пока идёт запись.
    Wal,
    /// Индекс, который строится заново: один файл, готовый к подмене.
    Delete,
}

/// Подходит ли индекс текущим настройкам.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compatibility {
    /// Новый индекс: отпечатка модели в нём ещё нет.
    Fresh,
    Compatible,
    /// Отпечаток модели в индексе не совпадает с настройками (Т-25).
    ModelChanged,
    /// Схема старше текущей; миграции без нового эмбеддинга пока нет.
    SchemaOutdated,
}

/// Чем индекс нарезан: параметры нарезки и корень документации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub chunking: ChunkingParams,
    pub docs_root: String,
}

/// Документ, как он записан в индексе.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDocument {
    pub path: String,
    pub size: u64,
    pub modified_ns: Option<i64>,
    pub hash: String,
}

/// Документ для записи в индекс.
pub struct DocumentRecord<'a> {
    pub path: &'a str,
    pub module: &'a str,
    pub title: &'a str,
    pub size: u64,
    pub modified_ns: Option<i64>,
    /// Хеш blake3 содержимого файла, шестнадцатеричный.
    pub hash: &'a str,
    pub chunks: &'a [Chunk],
}

pub struct Index {
    conn: Connection,
    stemmer: TextStemmer,
}

impl Index {
    /// Чтение готового индекса без миграций, смены журнала и создания файлов.
    pub fn open_read_only(path: &Path) -> Result<Self, StorageError> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self {
            conn,
            stemmer: TextStemmer::new(),
        })
    }
    /// Открывает индекс; если файла нет, создаёт его вместе с папкой.
    pub fn open(path: &Path, journal: Journal) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let mode = match journal {
            Journal::Wal => "WAL",
            Journal::Delete => "DELETE",
        };
        conn.query_row(&format!("PRAGMA journal_mode = {mode}"), [], |row| {
            row.get::<_, String>(0)
        })?;
        let mut index = Self {
            conn,
            stemmer: TextStemmer::new(),
        };
        index.ensure_schema()?;
        Ok(index)
    }

    /// Разбор текста для FTS изменился с момента записи индекса: читатель его не чинит,
    /// FTS пересоберёт первый писатель при открытии (`docs/architecture/worker.md`).
    pub fn stem_outdated(&self) -> Result<bool, StorageError> {
        Ok(self.meta("stem_version")?.as_deref() != Some(STEM_VERSION))
    }

    /// Соединение для чтения: им пользуются поиск и тесты.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Соединение для транзакционной публикации дополнительных слоёв индекса.
    pub fn conn_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Закрывает соединение. Нужно перед подменой файла индекса.
    pub fn close(self) -> Result<(), StorageError> {
        self.conn.close().map_err(|(_, error)| error.into())
    }

    fn ensure_schema(&mut self) -> Result<(), StorageError> {
        let has_meta = self.conn.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'meta'",
            [],
            |row| row.get::<_, i64>(0),
        )? > 0;
        if !has_meta {
            self.create_schema()?;
            return Ok(());
        }
        let found = self.meta_u32("schema_version")?;
        if found > SCHEMA_VERSION {
            return Err(StorageError::SchemaTooNew {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        if found == SCHEMA_VERSION && self.meta("stem_version")?.as_deref() != Some(STEM_VERSION) {
            self.rebuild_fts()?;
        }
        Ok(())
    }

    fn create_schema(&mut self) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            ) WITHOUT ROWID;
            CREATE TABLE documents (
                id INTEGER PRIMARY KEY,
                path TEXT NOT NULL UNIQUE,
                module TEXT NOT NULL,
                title TEXT NOT NULL,
                size INTEGER NOT NULL,
                modified_ns INTEGER,
                hash TEXT NOT NULL
            );
            CREATE INDEX documents_module ON documents(module);
            CREATE TABLE chunks (
                id INTEGER PRIMARY KEY,
                document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
                ordinal INTEGER NOT NULL,
                first_line INTEGER NOT NULL,
                last_line INTEGER NOT NULL,
                heading_chain TEXT NOT NULL,
                text TEXT NOT NULL,
                embed_hash TEXT NOT NULL,
                UNIQUE (document_id, ordinal)
            );
            CREATE INDEX chunks_embed_hash ON chunks(embed_hash);
            CREATE VIRTUAL TABLE chunks_fts USING fts5(
                heading_chain, body,
                tokenize = 'unicode61 remove_diacritics 2'
            );
            CREATE TABLE vectors (
                embed_hash TEXT PRIMARY KEY,
                vector BLOB NOT NULL
            ) WITHOUT ROWID;",
        )?;
        set_meta(&tx, "schema_version", &SCHEMA_VERSION.to_string())?;
        set_meta(&tx, "stem_version", STEM_VERSION)?;
        set_meta(&tx, "bars_version", env!("CARGO_PKG_VERSION"))?;
        tx.commit()?;
        Ok(())
    }

    /// Пересобирает `chunks_fts` из `chunks.text`: разбор текста для FTS изменился.
    fn rebuild_fts(&mut self) -> Result<(), StorageError> {
        let rows: Vec<(i64, String, String)> = {
            let mut statement = self
                .conn
                .prepare("SELECT id, heading_chain, text FROM chunks")?;
            let rows =
                statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
            rows.collect::<Result<_, _>>()?
        };
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM chunks_fts", [])?;
        for (id, chain, text) in rows {
            insert_fts(&tx, &self.stemmer, id, &chain, &text)?;
        }
        set_meta(&tx, "stem_version", STEM_VERSION)?;
        tx.commit()?;
        Ok(())
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>, StorageError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StorageError> {
        Ok(set_meta(&self.conn, key, value)?)
    }

    fn meta_u32(&self, key: &str) -> Result<u32, StorageError> {
        self.meta(key)?
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| StorageError::BadMeta(key.to_string()))
    }

    fn meta_usize(&self, key: &str) -> Result<Option<usize>, StorageError> {
        self.meta(key)?
            .map(|value| {
                value
                    .parse()
                    .map_err(|_| StorageError::BadMeta(key.to_string()))
            })
            .transpose()
    }

    /// Отпечаток модели, которой построен индекс; `None` у нового индекса.
    pub fn stored_fingerprint(&self) -> Result<Option<ModelFingerprint>, StorageError> {
        let (Some(source), Some(model), Some(dim), Some(doc_prefix)) = (
            self.meta("model_source")?,
            self.meta("model_id")?,
            self.meta_usize("model_dim")?,
            self.meta("model_doc_prefix")?,
        ) else {
            return Ok(None);
        };
        Ok(Some(ModelFingerprint {
            source,
            model,
            dim,
            doc_prefix,
        }))
    }

    pub fn write_fingerprint(
        &mut self,
        fingerprint: &ModelFingerprint,
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        set_meta(&tx, "model_source", &fingerprint.source)?;
        set_meta(&tx, "model_id", &fingerprint.model)?;
        set_meta(&tx, "model_dim", &fingerprint.dim.to_string())?;
        set_meta(&tx, "model_doc_prefix", &fingerprint.doc_prefix)?;
        tx.commit()?;
        Ok(())
    }

    pub fn compatibility(
        &self,
        fingerprint: &ModelFingerprint,
    ) -> Result<Compatibility, StorageError> {
        if self.meta_u32("schema_version")? < SCHEMA_VERSION {
            return Ok(Compatibility::SchemaOutdated);
        }
        Ok(match self.stored_fingerprint()? {
            None => Compatibility::Fresh,
            Some(stored) if stored.matches(fingerprint) => Compatibility::Compatible,
            Some(_) => Compatibility::ModelChanged,
        })
    }

    /// Параметры нарезки и корень документации, которыми построен индекс; `None`, если запись
    /// не дошла до конца или индекс новый.
    pub fn stored_layout(&self) -> Result<Option<Layout>, StorageError> {
        if self.meta("chunking_version")?.as_deref() != Some(CHUNKING_VERSION) {
            return Ok(None);
        }
        let (Some(min_chars), Some(max_chars), Some(docs_root)) = (
            self.meta_usize("chunk_min_chars")?,
            self.meta_usize("chunk_max_chars")?,
            self.meta("docs_root")?,
        ) else {
            return Ok(None);
        };
        Ok(Some(Layout {
            chunking: ChunkingParams {
                min_chars,
                max_chars,
            },
            docs_root,
        }))
    }

    pub fn write_layout(&mut self, layout: &Layout) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        set_meta(
            &tx,
            "chunk_min_chars",
            &layout.chunking.min_chars.to_string(),
        )?;
        set_meta(
            &tx,
            "chunk_max_chars",
            &layout.chunking.max_chars.to_string(),
        )?;
        set_meta(&tx, "docs_root", &layout.docs_root)?;
        set_meta(&tx, "chunking_version", CHUNKING_VERSION)?;
        tx.commit()?;
        Ok(())
    }

    /// Записывает время последнего обновления.
    pub fn touch_updated_at(&self) -> Result<(), StorageError> {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        self.set_meta("updated_at", &seconds.to_string())
    }

    /// Все документы индекса.
    pub fn documents(&self) -> Result<Vec<StoredDocument>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT path, size, modified_ns, hash FROM documents ORDER BY path")?;
        let rows = statement.query_map([], |row| {
            Ok(StoredDocument {
                path: row.get(0)?,
                size: row.get::<_, i64>(1)? as u64,
                modified_ns: row.get(2)?,
                hash: row.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn has_vector(&self, embed_hash: &str) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .prepare_cached("SELECT 1 FROM vectors WHERE embed_hash = ?1")?
            .exists([embed_hash])?)
    }

    pub fn vector(&self, embed_hash: &str) -> Result<Option<Vec<f32>>, StorageError> {
        let blob: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT vector FROM vectors WHERE embed_hash = ?1",
                [embed_hash],
                |row| row.get(0),
            )
            .optional()?;
        Ok(blob.map(|bytes| decode_vector(&bytes)))
    }

    /// Заменяет документ по пути (или добавляет новый) вместе с чанками и новыми векторами одной
    /// транзакцией. Векторы нормализуются до длины 1; вектор, который уже есть, не перезаписывается.
    pub fn replace_document(
        &mut self,
        record: &DocumentRecord<'_>,
        new_vectors: &[(&str, &[f32])],
    ) -> Result<(), StorageError> {
        let tx = self.conn.transaction()?;
        delete_document(&tx, record.path)?;
        {
            let mut insert = tx.prepare_cached(
                "INSERT OR IGNORE INTO vectors(embed_hash, vector) VALUES (?1, ?2)",
            )?;
            for (hash, vector) in new_vectors {
                insert.execute(params![hash, encode_vector(vector)])?;
            }
        }
        tx.execute(
            "INSERT INTO documents(path, module, title, size, modified_ns, hash)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                record.path,
                record.module,
                record.title,
                record.size as i64,
                record.modified_ns,
                record.hash
            ],
        )?;
        let document_id = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare_cached(
                "INSERT INTO chunks(document_id, ordinal, first_line, last_line, heading_chain, text, embed_hash)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for chunk in record.chunks {
                insert.execute(params![
                    document_id,
                    chunk.ordinal as i64,
                    chunk.first_line as i64,
                    chunk.last_line as i64,
                    chunk.heading_chain,
                    chunk.text,
                    chunk.embed_hash
                ])?;
                let chunk_id = tx.last_insert_rowid();
                insert_fts(
                    &tx,
                    &self.stemmer,
                    chunk_id,
                    &chunk.heading_chain,
                    &chunk.text,
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Обновляет размер и время изменения документа, содержимое которого не изменилось.
    pub fn touch_document(
        &self,
        path: &str,
        size: u64,
        modified_ns: Option<i64>,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE documents SET size = ?2, modified_ns = ?3 WHERE path = ?1",
            params![path, size as i64, modified_ns],
        )?;
        Ok(())
    }

    /// Удаляет документ и его чанки. `false`, если такого документа нет.
    pub fn delete_document(&mut self, path: &str) -> Result<bool, StorageError> {
        let tx = self.conn.transaction()?;
        let deleted = delete_document(&tx, path)?;
        tx.commit()?;
        Ok(deleted)
    }

    /// Удаляет векторы, на которые не ссылается ни один чанк; возвращает их число.
    pub fn remove_orphan_vectors(&self) -> Result<usize, StorageError> {
        Ok(self.conn.execute(
            "DELETE FROM vectors WHERE embed_hash NOT IN (SELECT embed_hash FROM chunks)",
            [],
        )?)
    }

    pub fn document_count(&self) -> Result<usize, StorageError> {
        self.count("documents")
    }

    pub fn chunk_count(&self) -> Result<usize, StorageError> {
        self.count("chunks")
    }

    pub fn vector_count(&self) -> Result<usize, StorageError> {
        self.count("vectors")
    }

    fn count(&self, table: &str) -> Result<usize, StorageError> {
        Ok(self
            .conn
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                row.get::<_, i64>(0)
            })? as usize)
    }
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value
         WHERE value IS NOT excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn insert_fts(
    conn: &Connection,
    stemmer: &TextStemmer,
    chunk_id: i64,
    heading_chain: &str,
    text: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO chunks_fts(rowid, heading_chain, body) VALUES (?1, ?2, ?3)",
        params![
            chunk_id,
            stemmer.stem_text(heading_chain),
            stemmer.stem_text(text)
        ],
    )?;
    Ok(())
}

/// Удаляет документ с чанками (каскадом) и их строки в `chunks_fts`: у виртуальной таблицы
/// каскада нет.
fn delete_document(conn: &Connection, path: &str) -> rusqlite::Result<bool> {
    let id: Option<i64> = conn
        .query_row("SELECT id FROM documents WHERE path = ?1", [path], |row| {
            row.get(0)
        })
        .optional()?;
    let Some(id) = id else {
        return Ok(false);
    };
    conn.execute(
        "DELETE FROM chunks_fts WHERE rowid IN (SELECT id FROM chunks WHERE document_id = ?1)",
        [id],
    )?;
    conn.execute("DELETE FROM documents WHERE id = ?1", [id])?;
    Ok(true)
}

/// Нормализует вектор до длины 1 и кодирует его как `float32` little-endian.
fn encode_vector(vector: &[f32]) -> Vec<u8> {
    let norm = vector.iter().map(|x| x * x).sum::<f32>().sqrt();
    let scale = if norm > 0.0 { 1.0 / norm } else { 1.0 };
    let mut bytes = Vec::with_capacity(vector.len() * 4);
    for x in vector {
        bytes.extend_from_slice(&(x * scale).to_le_bytes());
    }
    bytes
}

pub(crate) fn decode_vector(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::chunking::chunk_document;

    fn fingerprint(model: &str) -> ModelFingerprint {
        ModelFingerprint {
            source: "builtin".into(),
            model: model.into(),
            dim: 3,
            doc_prefix: String::new(),
        }
    }

    fn open(dir: &tempfile::TempDir) -> Index {
        Index::open(&dir.path().join("sub").join("index.sqlite"), Journal::Wal).unwrap()
    }

    fn record<'a>(path: &'a str, chunks: &'a [Chunk]) -> DocumentRecord<'a> {
        DocumentRecord {
            path,
            module: "m",
            title: "T",
            size: 10,
            modified_ns: Some(5),
            hash: "h",
            chunks,
        }
    }

    fn fts_count(index: &Index, word: &str) -> i64 {
        index
            .conn()
            .query_row(
                "SELECT count(*) FROM chunks_fts WHERE chunks_fts MATCH ?1",
                [word],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn open_creates_file_with_schema_and_wal() {
        let dir = tempfile::tempdir().unwrap();
        let index = open(&dir);
        assert_eq!(index.meta("schema_version").unwrap().as_deref(), Some("1"));
        assert_eq!(
            index.meta("stem_version").unwrap().as_deref(),
            Some(STEM_VERSION)
        );
        let mode: String = index
            .conn()
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
        assert_eq!(index.document_count().unwrap(), 0);
    }

    #[test]
    fn replace_document_writes_chunks_fts_and_normalized_vectors() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let doc = chunk_document(
            "docs/a.md",
            "# Заголовок\n\nНастройки поиска.\n",
            &ChunkingParams::default(),
        );
        let hash = doc.chunks[0].embed_hash.clone();
        index
            .replace_document(
                &record("docs/a.md", &doc.chunks),
                &[(&hash, &[3.0, 4.0, 0.0])],
            )
            .unwrap();

        assert_eq!(index.document_count().unwrap(), 1);
        assert_eq!(index.chunk_count().unwrap(), doc.chunks.len());
        let stored = index.vector(&hash).unwrap().unwrap();
        for (got, want) in stored.iter().zip([0.6_f32, 0.8, 0.0]) {
            assert!((got - want).abs() < 1e-6, "{stored:?}");
        }
        // Стемминг: в индексе «настройки», ищем «настройка».
        assert_eq!(fts_count(&index, "настройк*"), 1);
        assert!(index.has_vector(&hash).unwrap());
    }

    #[test]
    fn replace_document_replaces_old_version_and_its_fts_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let params = ChunkingParams::default();
        let old = chunk_document("docs/a.md", "# A\n\nстарый текст\n", &params);
        index
            .replace_document(&record("docs/a.md", &old.chunks), &[])
            .unwrap();
        let new = chunk_document("docs/a.md", "# A\n\nновый текст\n", &params);
        index
            .replace_document(&record("docs/a.md", &new.chunks), &[])
            .unwrap();

        assert_eq!(index.document_count().unwrap(), 1);
        assert_eq!(fts_count(&index, "стар*"), 0);
        assert_eq!(fts_count(&index, "нов*"), 1);
    }

    #[test]
    fn delete_document_removes_chunks_and_fts_rows() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let doc = chunk_document("docs/a.md", "# A\n\nтекст\n", &ChunkingParams::default());
        index
            .replace_document(&record("docs/a.md", &doc.chunks), &[])
            .unwrap();

        assert!(index.delete_document("docs/a.md").unwrap());
        assert!(!index.delete_document("docs/a.md").unwrap());
        assert_eq!(index.chunk_count().unwrap(), 0);
        let fts_rows: i64 = index
            .conn()
            .query_row("SELECT count(*) FROM chunks_fts_docsize", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(fts_rows, 0);
    }

    #[test]
    fn orphan_vectors_are_removed_and_shared_ones_stay() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let doc = chunk_document("docs/a.md", "# A\n\nтекст\n", &ChunkingParams::default());
        let used = doc.chunks[0].embed_hash.clone();
        index
            .replace_document(
                &record("docs/a.md", &doc.chunks),
                &[(&used, &[1.0, 0.0, 0.0]), ("orphan", &[0.0, 1.0, 0.0])],
            )
            .unwrap();

        assert_eq!(index.remove_orphan_vectors().unwrap(), 1);
        assert!(index.has_vector(&used).unwrap());
        assert!(!index.has_vector("orphan").unwrap());
    }

    #[test]
    fn replace_document_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let params = ChunkingParams::default();
        let old = chunk_document("docs/a.md", "# A\n\nстарый\n", &params);
        index
            .replace_document(&record("docs/a.md", &old.chunks), &[])
            .unwrap();

        // Два чанка с одним номером нарушают UNIQUE (document_id, ordinal) посреди записи.
        let mut broken = chunk_document("docs/a.md", "# A\n\nновый\n", &params).chunks;
        broken.push(broken[0].clone());
        assert!(
            index
                .replace_document(&record("docs/a.md", &broken), &[])
                .is_err()
        );

        assert_eq!(index.document_count().unwrap(), 1);
        assert_eq!(fts_count(&index, "стар*"), 1);
        assert_eq!(fts_count(&index, "нов*"), 0);
    }

    #[test]
    fn compatibility_follows_the_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        let a = fingerprint("a");
        assert_eq!(index.compatibility(&a).unwrap(), Compatibility::Fresh);
        index.write_fingerprint(&a).unwrap();
        assert_eq!(index.stored_fingerprint().unwrap(), Some(a.clone()));
        assert_eq!(index.compatibility(&a).unwrap(), Compatibility::Compatible);
        assert_eq!(
            index.compatibility(&fingerprint("b")).unwrap(),
            Compatibility::ModelChanged
        );
        let mut other_dim = a.clone();
        other_dim.dim = 4;
        assert_eq!(
            index.compatibility(&other_dim).unwrap(),
            Compatibility::ModelChanged
        );
        index.set_meta("schema_version", "0").unwrap();
        assert_eq!(
            index.compatibility(&a).unwrap(),
            Compatibility::SchemaOutdated
        );
    }

    #[test]
    fn layout_roundtrips_and_is_absent_until_written() {
        let dir = tempfile::tempdir().unwrap();
        let mut index = open(&dir);
        assert_eq!(index.stored_layout().unwrap(), None);
        let layout = Layout {
            chunking: ChunkingParams {
                min_chars: 100,
                max_chars: 900,
            },
            docs_root: "docs".into(),
        };
        index.write_layout(&layout).unwrap();
        assert_eq!(index.stored_layout().unwrap(), Some(layout.clone()));
        index
            .conn()
            .execute("DELETE FROM meta WHERE key='chunking_version'", [])
            .unwrap();
        assert_eq!(index.stored_layout().unwrap(), None);
        index.write_layout(&layout).unwrap();
        index.set_meta("chunking_version", "older").unwrap();
        assert_eq!(index.stored_layout().unwrap(), None);
        index.write_layout(&layout).unwrap();
        assert_eq!(index.stored_layout().unwrap(), Some(layout));
    }

    #[test]
    fn newer_schema_is_not_opened() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite");
        let index = Index::open(&path, Journal::Wal).unwrap();
        index.set_meta("schema_version", "99").unwrap();
        index.close().unwrap();
        let error = Index::open(&path, Journal::Wal).err().unwrap();
        assert!(matches!(
            error,
            StorageError::SchemaTooNew { found: 99, .. }
        ));
    }

    #[test]
    fn changed_stem_version_rebuilds_fts_from_chunks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index.sqlite");
        let mut index = Index::open(&path, Journal::Wal).unwrap();
        let doc = chunk_document(
            "docs/a.md",
            "# A\n\nнастройки\n",
            &ChunkingParams::default(),
        );
        index
            .replace_document(&record("docs/a.md", &doc.chunks), &[])
            .unwrap();
        // Портим FTS и откатываем версию разбора, как будто индекс построен старым правилом.
        index.conn().execute("DELETE FROM chunks_fts", []).unwrap();
        index.set_meta("stem_version", "0").unwrap();
        index.close().unwrap();

        let index = Index::open(&path, Journal::Wal).unwrap();
        assert_eq!(
            index.meta("stem_version").unwrap().as_deref(),
            Some(STEM_VERSION)
        );
        assert_eq!(fts_count(&index, "настройк*"), 1);
    }
}
