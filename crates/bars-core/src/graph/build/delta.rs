//! Частичное обновление графа после правки документов (НФТ-7): разбираются только изменённые
//! и новые документы, близость чанков считается инкрементально, а транзакция заменяет только
//! их строки и связи. Проект — docs/architecture/graph.md, «Проект частичного обновления графа».
use super::*;
use crate::graph::similarity::VectorNode;
use crate::graph::similarity_job::{Calculation, Checkpoint, METHOD};
use bars_graph::source::{self, InputChunk, InputDocument};
use bars_graph::store::{self, DeltaInput, PayloadKey, SimilarityDelta};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

/// Больше этого числа затронутых документов и больше четверти проекта — выигрыша нет,
/// граф перестраивается целиком. Малые проекты всегда обновляются частично: полная сборка
/// там дешёвая, но тот же путь проверяется на них тестами.
const FULL_REBUILD_MIN_DOCUMENTS: usize = 16;

/// Предел одного расчёта близости в дельте: дольше — полная перестройка с продолжением.
const SIMILARITY_QUANTUM: Duration = Duration::from_secs(2);

/// `Ok(Some)` — граф обновлён частично; `Ok(None)` — дельта неприменима, нужна полная сборка.
pub(super) fn try_delta(
    project: &Path,
    index: &mut Index,
    cancelled: &mut impl FnMut() -> bool,
) -> Result<Option<bars_graph::store::GraphStatus>, GraphError> {
    let Some(base) = store::delta_base(index.conn())? else {
        return Ok(None);
    };
    let unfinished: bool = index.conn().query_row(
        "SELECT EXISTS(SELECT 1 FROM graph_payload WHERE key IN ('knn_prepared','knn_checkpoint'))",
        [],
        |r| r.get(0),
    )?;
    if unfinished {
        return Ok(None);
    }
    let Some(layout) = index
        .stored_layout()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
    else {
        return Ok(None);
    };
    let Some(previous) = store::read_payload(index.conn(), PayloadKey::Complete)?
        .and_then(|s| serde_json::from_str::<Checkpoint>(&s).ok())
    else {
        return Ok(None);
    };
    let namespace = project
        .canonicalize()
        .map_err(|e| GraphError::InvalidData(e.to_string()))?
        .to_string_lossy()
        .replace('\\', "/");
    let config = job::config(index)?;

    // Подготовка идёт из индекса одной read-транзакцией: хеши, тексты, чанки и векторы согласованы.
    let (input, calculation) = {
        let read = index.conn().unchecked_transaction()?;
        let stamp = store::revision_stamp(&read)?
            .ok_or_else(|| GraphError::InvalidData("требуется схема графа v3".into()))?
            .input;
        let now = store::document_manifest(&read)?;
        let changed: Vec<&String> = now
            .iter()
            .filter(|(path, hash)| base.get(*path) != Some(*hash))
            .map(|(path, _)| path)
            .collect();
        let removed: Vec<&String> = base.keys().filter(|p| !now.contains_key(*p)).collect();
        let affected = changed.len() + removed.len();
        if affected == 0
            || (affected > FULL_REBUILD_MIN_DOCUMENTS && affected * 4 > now.len().max(base.len()))
        {
            return Ok(None);
        }
        let mut docs = BTreeMap::new();
        let mut paths = BTreeMap::new();
        for path in now.keys() {
            let Some(docs_path) = relative_path(&layout.docs_root, path) else {
                return Ok(None);
            };
            docs.insert(
                docs_path.clone(),
                bars_graph::object_ref(&namespace, Kind::Document, &docs_path, "").id,
            );
            paths.insert(docs_path, path.clone());
        }
        let mut removed_docs = Vec::new();
        for path in removed {
            let Some(docs_path) = relative_path(&layout.docs_root, path) else {
                return Ok(None);
            };
            removed_docs.push((path.clone(), docs_path));
        }
        let mut parses = Vec::new();
        for path in changed {
            if cancelled() {
                return Err(GraphError::Cancelled);
            }
            let hash = &now[path];
            let docs_path = relative_path(&layout.docs_root, path)
                .ok_or_else(|| GraphError::InvalidData("путь вне корня документации".into()))?;
            // Тест в индексе записан вместе с хешем; запасной путь — диск со сверкой хеша.
            let text = match index.document_source(path) {
                Ok(Some((stored, text))) if &stored == hash => text,
                _ => match std::fs::read_to_string(project.join(path)) {
                    Ok(text) if blake3::hash(text.as_bytes()).to_hex().as_str() == hash => text,
                    _ => return Ok(None),
                },
            };
            if text.len() > 16 * 1024 * 1024 {
                return Ok(None);
            }
            let (module, title): (String, String) = read.query_row(
                "SELECT module,title FROM documents WHERE path=?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let mapped = chunk_document_with_spans(path, &text, &layout.chunking);
            let mut statement = read.prepare(
                "SELECT c.ordinal,c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.path=?1 ORDER BY c.ordinal",
            )?;
            let stored: Vec<(usize, String)> = statement
                .query_map([path], |r| Ok((r.get::<_, u32>(0)? as usize, r.get(1)?)))?
                .collect::<Result<_, _>>()?;
            if stored.len() != mapped.document.chunks.len() {
                return Ok(None);
            }
            let mut chunks = Vec::new();
            for (ordinal, chunk_hash) in stored {
                let Some(chunk) = mapped.document.chunks.get(ordinal) else {
                    return Ok(None);
                };
                if chunk.embed_hash != chunk_hash {
                    return Ok(None);
                }
                chunks.push(InputChunk {
                    key: format!("{ordinal}:{chunk_hash}"),
                    label: chunk.heading_chain.clone(),
                    spans: mapped.source_spans[ordinal].clone(),
                });
            }
            let mut no_cancel = || cancelled();
            parses.push(source::parse_document(
                &namespace,
                &layout.docs_root,
                InputDocument {
                    path: path.clone(),
                    docs_path,
                    module,
                    title,
                    text,
                    chunks,
                },
                &mut no_cancel,
            )?);
        }
        // Векторы всех чанков проекта: близость точная, поэтому считается по полному набору,
        // а прежнее основание (`knn_complete`) отделяет неизменные строки.
        let mut vectors = Vec::new();
        let mut total = 0usize;
        {
            let mut statement = read.prepare(
                "SELECT d.path,c.ordinal,c.embed_hash FROM chunks c JOIN documents d ON d.id=c.document_id",
            )?;
            let rows = statement.query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            for row in rows {
                let (path, ordinal, hash) = row?;
                total += 1;
                let Some(docs_path) = relative_path(&layout.docs_root, &path) else {
                    return Ok(None);
                };
                let Some(vector) = index
                    .vector(&hash)
                    .map_err(|e| GraphError::InvalidData(e.to_string()))?
                else {
                    continue;
                };
                vectors.push(VectorNode {
                    id: bars_graph::object_ref(
                        &namespace,
                        Kind::Chunk,
                        &format!("{docs_path}:{ordinal}:{hash}"),
                        &hash,
                    )
                    .id,
                    vector,
                });
            }
        }
        // Непригодные векторы — ошибка полной сборки, а не дельты: пусть её сообщит тот же путь.
        let Ok(mut calculation) =
            Calculation::from_vectors(vectors, &config, None, Some(previous.clone()))
        else {
            return Ok(None);
        };
        if !calculation.advance(SIMILARITY_QUANTUM, &mut *cancelled) {
            if cancelled() {
                return Err(GraphError::Cancelled);
            }
            // Изменено слишком много чанков для одного кванта: полный путь умеет продолжаться.
            return Ok(None);
        }
        let c = &calculation.checkpoint;
        let n = c.ids.len();
        let old_rows: BTreeMap<&str, &Vec<crate::graph::similarity::SimilarNeighbor>> = previous
            .ids
            .iter()
            .map(String::as_str)
            .zip(&previous.rows)
            .collect();
        let mut rows = BTreeMap::new();
        let mut changed_rows = BTreeSet::new();
        for (id, row) in c.ids.iter().zip(&c.rows) {
            if old_rows.get(id.as_str()) != Some(&row) {
                changed_rows.insert(id.clone());
            }
            rows.insert(
                id.clone(),
                row.iter().map(|n| (n.id.clone(), n.cosine)).collect(),
            );
        }
        let missing = total.saturating_sub(n);
        let state = if missing == 0 {
            LayerState::Ready
        } else {
            LayerState::Partial
        };
        let layer = bars_graph::GraphLayerStatus {
            freshness: bars_graph::Freshness::Current,
            completeness: if state == LayerState::Ready {
                bars_graph::Completeness::Complete
            } else {
                bars_graph::Completeness::Partial
            },
            availability: bars_graph::Availability::Enabled,
            stale_reason: None,
            coverage: bars_graph::LayerCoverage {
                unit: "chunks".into(),
                processed: n,
                total,
                missing,
                comparisons: (n as u64) * (n as u64).saturating_sub(1) / 2,
                reason: (missing > 0)
                    .then(|| "У части чанков нет пригодного вектора или файл исключён".into()),
            },
            source_revisions: now.clone(),
            state,
            generation: None,
            fingerprint: METHOD.into(),
            processed: now.len(),
            total: now.len(),
            failed: 0,
        };
        let digest = blake3::hash(&serde_json::to_vec(&(&c.ids, &c.hashes))?)
            .to_hex()
            .to_string();
        let snapshot = blake3::hash(&serde_json::to_vec(&(
            &namespace,
            bars_graph::METHOD_VERSION,
            "delta-v1",
            &now,
            &digest,
        ))?)
        .to_hex()
        .to_string();
        read.commit()?;
        (
            DeltaInput {
                expected: now,
                input_stamp: stamp,
                removed: removed_docs,
                parses,
                docs,
                paths,
                similarity: SimilarityDelta {
                    rows,
                    changed: changed_rows,
                    state,
                    layer,
                    digest,
                },
                snapshot,
            },
            calculation,
        )
    };

    store::set_progress(
        index.conn(),
        &bars_graph::GraphProgress {
            phase: "publish".into(),
            processed: input.parses.len() + input.removed.len(),
            total: Some(input.parses.len() + input.removed.len()),
            elapsed_ms: 0,
        },
    )?;
    let checkpoint = serde_json::to_string(&calculation.checkpoint)?;
    let tx = index
        .conn_mut()
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !store::apply_delta(&tx, &input)? {
        // База графа не подходит: откат (tx закрывается без commit) и полная сборка.
        return Ok(None);
    }
    store::write_payload(&tx, PayloadKey::Complete, &checkpoint)?;
    if cancelled() {
        return Err(GraphError::Cancelled);
    }
    tx.commit()?;
    Ok(Some(store::status(index.conn())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexing::chunking::{ChunkingParams, chunk_document};
    use crate::storage::{DocumentRecord, Journal, Layout};
    use bars_graph::BuildLayer;

    fn project() -> (tempfile::TempDir, Index) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Wal).unwrap();
        index
            .write_layout(&Layout {
                chunking: ChunkingParams::default(),
                docs_root: "docs".into(),
            })
            .unwrap();
        (dir, index)
    }

    /// Записывает документ в индекс вместе с текстом; векторы выводятся из хеша чанка.
    fn put(index: &mut Index, path: &str, text: &str) {
        let mapped = chunk_document(path, text, &ChunkingParams::default());
        let vectors: Vec<(String, Vec<f32>)> = mapped
            .chunks
            .iter()
            .map(|c| {
                let b = blake3::hash(c.embed_hash.as_bytes());
                let b = b.as_bytes();
                (
                    c.embed_hash.clone(),
                    vec![1.0, f32::from(b[0]) / 255.0, f32::from(b[1]) / 255.0],
                )
            })
            .collect();
        let refs: Vec<(&str, &[f32])> = vectors
            .iter()
            .map(|(h, v)| (h.as_str(), v.as_slice()))
            .collect();
        index
            .replace_document(
                &DocumentRecord {
                    path,
                    module: ".",
                    title: &mapped.title,
                    size: text.len() as u64,
                    modified_ns: None,
                    hash: &blake3::hash(text.as_bytes()).to_hex(),
                    chunks: &mapped.chunks,
                    text: Some(text),
                },
                &refs,
            )
            .unwrap();
    }

    fn corpus() -> Vec<(&'static str, String)> {
        let mut docs = vec![
            (
                "docs/a.md",
                "# A\n\nТ-1 зависит от Т-2.\n\nСм. [раздел B](b.md#раздел-b), [[c]] и [нет](нет.md).\n\n## Раздел A\n\nТекст A про граф.".to_string(),
            ),
            (
                "docs/b.md",
                "# B\n\n## Раздел B\n\nТ-2 зависит от Т-3. Текст B про граф.".to_string(),
            ),
            (
                "docs/c.md",
                "# C\n\nТ-1 использует `core.Cache`. Ссылка на [A](a.md#раздел-a).".to_string(),
            ),
            ("docs/d.md", "# D\n\nОтдельный документ про индекс.".to_string()),
        ];
        for (i, path) in ["docs/e.md", "docs/f.md", "docs/g.md", "docs/h.md"]
            .into_iter()
            .enumerate()
        {
            docs.push((
                path,
                format!("# Документ {i}\n\nТекст про поиск и граф номер {i}."),
            ));
        }
        docs
    }

    fn full(project: &Path, index: &mut Index) {
        crate::graph::build_index_graph(project, index, || false).unwrap();
    }

    fn partial(project: &Path, index: &mut Index) {
        let mut state = GraphBuildState::automatic();
        loop {
            if build_index_graph_step_with_state(
                project,
                index,
                BuildLayer::All,
                &mut state,
                || false,
            )
            .unwrap()
            .is_some()
            {
                return;
            }
        }
    }

    fn dump(index: &Index) -> BTreeMap<&'static str, Vec<String>> {
        let mut result = BTreeMap::new();
        for table in [
            "graph_sources",
            "graph_nodes",
            "graph_edges",
            "graph_evidence",
            "graph_node_sources",
            "graph_edge_evidence",
            "graph_parts",
            "graph_aux",
        ] {
            let mut stmt = index
                .conn()
                .prepare(&format!("SELECT * FROM {table} ORDER BY 1,2"))
                .unwrap();
            let columns = stmt.column_count();
            let rows = stmt
                .query_map([], |row| {
                    Ok((0..columns)
                        .map(|i| format!("{:?}", row.get::<_, rusqlite::types::Value>(i).unwrap()))
                        .collect::<Vec<_>>()
                        .join("|"))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            result.insert(table, rows);
        }
        result
    }

    /// Строки всех таблиц, которые полная публикация удаляет и вставляет заново.
    fn rows_of_graph(index: &Index) -> i64 {
        [
            "graph_sources",
            "graph_nodes",
            "graph_node_sources",
            "graph_edges",
            "graph_evidence",
            "graph_edge_evidence",
            "graph_parts",
        ]
        .iter()
        .map(|t| {
            index
                .conn()
                .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap()
        })
        .sum()
    }

    fn complete(index: &Index) -> Checkpoint {
        let json: String = index
            .conn()
            .query_row(
                "SELECT value FROM graph_payload WHERE key='knn_complete'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        serde_json::from_str(&json).unwrap()
    }

    /// Оракул: частичное обновление равно полной сборке тех же входов (таблицы построчно).
    fn assert_equals_full(project: &Path, index: &mut Index) {
        let delta = dump(index);
        let delta_checkpoint = complete(index);
        index
            .conn()
            .execute("UPDATE graph_meta SET value='stale' WHERE key='state'", [])
            .unwrap();
        full(project, index);
        let reference = dump(index);
        for (table, rows) in &reference {
            assert_eq!(
                &delta[table], rows,
                "таблица {table} расходится с полной сборкой"
            );
        }
        let full_checkpoint = complete(index);
        assert_eq!(delta_checkpoint.ids, full_checkpoint.ids);
        assert_eq!(delta_checkpoint.hashes, full_checkpoint.hashes);
        assert_eq!(delta_checkpoint.rows, full_checkpoint.rows);
    }

    fn ready() -> (tempfile::TempDir, Index) {
        let (dir, mut index) = project();
        for (path, text) in corpus() {
            put(&mut index, path, &text);
        }
        full(dir.path(), &mut index);
        assert!(store::delta_base(index.conn()).unwrap().is_some());
        (dir, index)
    }

    #[test]
    fn edit_one_document_rewrites_only_its_rows_and_equals_full_build() {
        let (dir, mut index) = ready();
        let total = rows_of_graph(&index);
        put(
            &mut index,
            "docs/b.md",
            "# B\n\n## Новый раздел B\n\nТ-2 зависит от Т-4. Текст B стал другим.",
        );
        // Считается только запись графа: строки индекса и FTS от самой правки не входят.
        let before = index.conn().total_changes();
        partial(dir.path(), &mut index);
        let changed = i64::try_from(index.conn().total_changes() - before).unwrap();
        // Полная публикация удаляет и вставляет каждую строку графа: 2 × total записей. Дельта
        // пишет только затронутые строки; на восьми документах близость задевает почти всех,
        // поэтому здесь проверяется «меньше полной перезаписи», а долю на большом корпусе
        // меряет приёмка НФТ-7.
        assert!(
            changed < 2 * total,
            "изменено {changed} строк, полная перезапись — {}",
            2 * total
        );
        assert_equals_full(dir.path(), &mut index);
    }

    #[test]
    fn broken_and_wiki_links_follow_added_removed_and_renamed_documents() {
        let (dir, mut index) = ready();
        // Якорь `#раздел-b` исчез, ссылка из a.md стала битой.
        put(&mut index, "docs/b.md", "# B\n\nБез раздела.");
        partial(dir.path(), &mut index);
        assert_equals_full(dir.path(), &mut index);
        // Новый документ даёт цель прежде битой ссылке `нет.md`.
        put(
            &mut index,
            "docs/нет.md",
            "# Нет\n\nТеперь есть. Т-3 зависит от Т-1.",
        );
        partial(dir.path(), &mut index);
        assert_equals_full(dir.path(), &mut index);
        // Удаление цели вики-ссылки и обычной ссылки.
        index.delete_document("docs/c.md").unwrap();
        partial(dir.path(), &mut index);
        assert_equals_full(dir.path(), &mut index);
        // Переименование: удалён путь и добавлен новый с тем же текстом.
        index.delete_document("docs/d.md").unwrap();
        put(
            &mut index,
            "docs/d2.md",
            "# D\n\nОтдельный документ про индекс.",
        );
        partial(dir.path(), &mut index);
        assert_equals_full(dir.path(), &mut index);
    }

    #[test]
    fn last_mention_of_shared_entity_removes_its_node() {
        let (dir, mut index) = ready();
        let count = |index: &Index| -> i64 {
            index
                .conn()
                .query_row(
                    "SELECT count(*) FROM graph_nodes WHERE kind='\"entity\"'",
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        let before = count(&index);
        // Т-3 упомянут только в b.md; после правки сущность исчезает вместе с рёбрами.
        put(
            &mut index,
            "docs/b.md",
            "# B\n\n## Раздел B\n\nНичего о требованиях.",
        );
        partial(dir.path(), &mut index);
        assert!(count(&index) < before);
        assert_equals_full(dir.path(), &mut index);
    }

    #[test]
    fn explicit_build_and_unusable_base_stay_full() {
        let (dir, mut index) = ready();
        put(&mut index, "docs/d.md", "# D\n\nДругой текст.");
        // Без признака delta (явное построение) публикуется полный граф и снова ставит базу.
        full(dir.path(), &mut index);
        assert!(store::delta_base(index.conn()).unwrap().is_some());
        // База без признака частичного обновления: автоматическое обновление уходит на полный путь.
        index
            .conn()
            .execute("UPDATE graph_meta SET value='' WHERE key='delta_base'", [])
            .unwrap();
        put(&mut index, "docs/d.md", "# D\n\nЕщё текст.");
        partial(dir.path(), &mut index);
        assert!(store::delta_base(index.conn()).unwrap().is_some());
        assert_equals_full(dir.path(), &mut index);
    }

    #[test]
    fn preparation_uses_indexed_text_not_the_disk() {
        let (dir, mut index) = project();
        for (path, text) in corpus() {
            put(&mut index, path, &text);
        }
        // На диске другой текст: раньше это давало «ссылка на объект устарела».
        std::fs::write(dir.path().join("docs/a.md"), "# Совсем другой").unwrap();
        full(dir.path(), &mut index);
        let text = corpus()[0].1.clone();
        assert_eq!(
            store::source_text(
                index.conn(),
                "docs/a.md",
                &blake3::hash(text.as_bytes()).to_hex()
            )
            .unwrap()
            .as_deref(),
            Some(text.as_str())
        );
    }
}
