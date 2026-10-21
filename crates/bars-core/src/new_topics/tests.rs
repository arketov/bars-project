use super::*;

fn bge() -> EmbeddingSpace {
    EmbeddingSpace {
        key: "builtin|BAAI/bge-m3|3||l2".into(),
    }
}

fn unmeasured() -> EmbeddingSpace {
    EmbeddingSpace {
        key: "builtin|nomic|3||l2".into(),
    }
}

fn unit(v: [f32; 3]) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.iter().map(|x| x / n).collect()
}

fn obs(id: &str, text: &str) -> Observation {
    Observation {
        event_id: id.into(),
        generation: 0,
        text: text.into(),
        scope: "project".into(),
        source: Source::Mcp,
        reason: Reason::EmptyResult,
        params: "{}".into(),
        index_stamp: None,
        vector: None,
        at: 100,
    }
}

fn with_vector(mut o: Observation, space: EmbeddingSpace, v: [f32; 3]) -> Observation {
    o.vector = Some((space, unit(v)));
    o
}

/// Слово из кириллических букв без цифр: цифры лексическая проверка считает отличием.
fn letters(mut i: usize) -> String {
    let mut word = String::new();
    loop {
        word.push(char::from_u32('а' as u32 + (i % 26) as u32).unwrap());
        i /= 26;
        if i == 0 {
            return word;
        }
    }
}

fn store() -> (tempfile::TempDir, TopicStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = TopicStore::open(dir.path()).unwrap();
    (dir, store)
}

#[test]
fn exact_text_merges_after_careful_normalization() {
    let (_dir, mut s) = store();
    let a = s.record(&obs("1", "Как  включить\nсбор?")).unwrap();
    let b = s.record(&obs("2", "как включить сбор?")).unwrap();
    let Recorded::Created { topic_id } = a else {
        panic!("{a:?}")
    };
    assert_eq!(
        b,
        Recorded::Merged {
            topic_id,
            by: Match::Exact
        }
    );
    let topics = s.list().unwrap();
    assert_eq!(topics.len(), 1);
    assert_eq!(topics[0].observations, 2);
    assert_eq!(topics[0].variants.len(), 1, "тот же ключ — один вариант");
}

#[test]
fn variant_keeps_first_search_parameters_after_repeat_and_reopen() {
    let (dir, mut s) = store();
    let mut first = obs("first", "проверка знаний");
    first.params = r#"{"score_kind":"rerank","min_score":0.18}"#.into();
    let Recorded::Created { topic_id } = s.record(&first).unwrap() else {
        panic!("тема не создана")
    };
    let mut repeat = obs("second", "проверка знаний");
    repeat.params = r#"{"score_kind":"cosine","min_score":0.24}"#.into();
    s.record(&repeat).unwrap();
    drop(s);
    let s = TopicStore::open(dir.path()).unwrap();
    let topic = s.get(topic_id).unwrap().unwrap();
    assert_eq!(topic.variants.len(), 1);
    assert_eq!(topic.variants[0].original_params, first.params);
}

#[test]
fn normalization_keeps_negations_numbers_and_identifiers() {
    assert_ne!(
        normalize_question("не работает"),
        normalize_question("работает")
    );
    assert_ne!(
        normalize_question("версия 1.2"),
        normalize_question("версия 1.3")
    );
    assert_ne!(
        normalize_question("ключ API_KEY"),
        normalize_question("ключ api_key")
    );
    assert_eq!(normalize_question("  Ошибка   Сборки "), "ошибка сборки");
}

#[test]
fn same_event_id_is_counted_once() {
    let (_dir, mut s) = store();
    s.record(&obs("1", "вопрос")).unwrap();
    assert_eq!(s.record(&obs("1", "вопрос")).unwrap(), Recorded::Duplicate);
    assert_eq!(s.list().unwrap()[0].observations, 1);
}

#[test]
fn same_text_in_another_scope_is_a_separate_topic() {
    let (_dir, mut s) = store();
    s.record(&obs("1", "вопрос")).unwrap();
    let mut other = obs("2", "вопрос");
    other.scope = "m=features;f=".into();
    assert!(matches!(
        s.record(&other).unwrap(),
        Recorded::Created { .. }
    ));
    assert_eq!(s.topic_count().unwrap(), 2);
}

#[test]
fn close_vectors_merge_only_in_a_measured_space_and_without_lexical_difference() {
    let (_dir, mut s) = store();
    s.record(&with_vector(
        obs("1", "как настроить сбор двух тем"),
        bge(),
        [1.0, 0.0, 0.0],
    ))
    .unwrap();
    // Косинус около 0,98 и нет лексической разницы: сливается.
    let merged = s
        .record(&with_vector(
            obs("2", "как мне настроить сбор двух тем"),
            bge(),
            [1.0, 0.2, 0.0],
        ))
        .unwrap();
    assert!(matches!(
        merged,
        Recorded::Merged {
            by: Match::Semantic,
            ..
        }
    ));
    // Близкий вектор, но другое числительное: слияния нет.
    let guarded = s
        .record(&with_vector(
            obs("3", "как настроить сбор трёх тем"),
            bge(),
            [1.0, 0.1, 0.0],
        ))
        .unwrap();
    assert!(matches!(guarded, Recorded::Created { .. }), "{guarded:?}");
    // Далёкий вектор: отдельная тема.
    let far = s
        .record(&with_vector(
            obs("4", "что-то совсем иное"),
            bge(),
            [0.0, 1.0, 0.0],
        ))
        .unwrap();
    assert!(matches!(far, Recorded::Created { .. }));
}

#[test]
fn bge_threshold_merges_owner_case_and_includes_boundary_for_both_profiles() {
    for model in ["BAAI/bge-m3", "BAAI/bge-m3@fp16:revision"] {
        let space = EmbeddingSpace {
            key: format!("builtin|{model}|3||l2"),
        };
        for (cosine, should_merge) in [
            (0.6799_f32, false),
            (0.68, true),
            (0.6801, true),
            (0.7104555, true),
        ] {
            let (_dir, mut s) = store();
            let original = with_vector(
                obs(
                    "1",
                    "Аудит журналов возврата в боксы и предыдущей попытки входа результаты сессий",
                ),
                space.clone(),
                [1.0, 0.0, 0.0],
            );
            s.record(&original).unwrap();
            let short = with_vector(
                obs("2", "журналы возврата боксы"),
                space.clone(),
                [cosine, (1.0 - cosine * cosine).sqrt(), 0.0],
            );
            let outcome = s.record(&short).unwrap();
            assert_eq!(
                matches!(
                    outcome,
                    Recorded::Merged {
                        by: Match::Semantic,
                        ..
                    }
                ),
                should_merge,
                "{model}, cosine={cosine}: {outcome:?}"
            );
            let topics = s.list().unwrap();
            assert_eq!(topics.len(), if should_merge { 1 } else { 2 });
            if should_merge {
                assert_eq!(topics[0].observations, 2);
                assert_eq!(topics[0].variants.len(), 2);
            }
        }
    }
}

#[test]
fn unmeasured_space_never_merges_semantically() {
    let (_dir, mut s) = store();
    s.record(&with_vector(
        obs("1", "один вопрос"),
        unmeasured(),
        [1.0, 0.0, 0.0],
    ))
    .unwrap();
    let second = s
        .record(&with_vector(
            obs("2", "другой вопрос"),
            unmeasured(),
            [1.0, 0.0, 0.0],
        ))
        .unwrap();
    assert!(matches!(second, Recorded::Created { .. }));
}

#[test]
fn vectors_of_different_spaces_are_not_compared() {
    let (_dir, mut s) = store();
    s.record(&with_vector(
        obs("1", "один вопрос"),
        bge(),
        [1.0, 0.0, 0.0],
    ))
    .unwrap();
    let other_space = EmbeddingSpace {
        key: "builtin|BAAI/bge-m3|3|query: |l2".into(),
    };
    let second = s
        .record(&with_vector(
            obs("2", "другой вопрос"),
            other_space,
            [1.0, 0.0, 0.0],
        ))
        .unwrap();
    assert!(matches!(second, Recorded::Created { .. }));
}

#[test]
fn chain_a_b_c_is_not_glued_through_the_middle_question() {
    let (_dir, mut s) = store();
    // A-B и B-C близки (cos > 0,79), A-C — нет: C не сливается с темой A через B.
    s.record(&with_vector(
        obs("1", "вопрос про ключи"),
        bge(),
        [1.0, 0.0, 0.0],
    ))
    .unwrap();
    s.record(&with_vector(
        obs("2", "вопрос про ключи и токены"),
        bge(),
        [1.0, 0.8, 0.0],
    ))
    .unwrap();
    let c = s
        .record(&with_vector(
            obs("3", "вопрос про токены"),
            bge(),
            [0.0, 1.0, 0.0],
        ))
        .unwrap();
    assert!(matches!(c, Recorded::Created { .. }), "{c:?}");
}

#[test]
fn text_without_vector_is_stored_and_merges_exactly() {
    let (_dir, mut s) = store();
    s.record(&obs("1", "пустой корпус")).unwrap();
    let again = s.record(&obs("2", "Пустой корпус")).unwrap();
    assert!(matches!(
        again,
        Recorded::Merged {
            by: Match::Exact,
            ..
        }
    ));
}

#[test]
fn clear_and_disable_reject_late_observations() {
    let (_dir, mut s) = store();
    let (generation, enabled) = s.settings().unwrap();
    assert!(enabled);
    s.record(&obs("1", "вопрос")).unwrap();
    let next = s.clear().unwrap();
    assert!(next > generation);
    assert_eq!(s.topic_count().unwrap(), 0);
    // Наблюдение, допущенное до очистки, не воскрешает тему.
    assert_eq!(
        s.record(&obs("2", "вопрос")).unwrap(),
        Recorded::StaleGeneration
    );
    let mut fresh = obs("3", "вопрос");
    fresh.generation = next;
    assert!(matches!(
        s.record(&fresh).unwrap(),
        Recorded::Created { .. }
    ));
    let after_disable = s.set_enabled(false).unwrap();
    assert!(after_disable > next);
    let mut late = obs("4", "иной вопрос");
    late.generation = next;
    assert_eq!(s.record(&late).unwrap(), Recorded::Disabled);
    assert_eq!(
        s.topic_count().unwrap(),
        1,
        "выключение сохраняет существующие"
    );
    assert_eq!(
        s.set_enabled(false).unwrap(),
        after_disable,
        "без смены поколение прежнее"
    );
}

#[test]
fn deleting_a_topic_allows_a_new_independent_appearance() {
    let (_dir, mut s) = store();
    let Recorded::Created { topic_id } = s.record(&obs("1", "вопрос")).unwrap() else {
        panic!()
    };
    assert!(s.delete_topic(topic_id).unwrap());
    assert!(!s.delete_topic(topic_id).unwrap());
    assert!(s.list().unwrap().is_empty());
    let again = s.record(&obs("2", "вопрос")).unwrap();
    let Recorded::Created { topic_id: new_id } = again else {
        panic!("{again:?}")
    };
    assert_ne!(new_id, topic_id, "ID не переиспользуются");
}

#[test]
fn demand_reopens_a_resolved_topic_and_keeps_the_decision_date() {
    let (_dir, mut s) = store();
    let Recorded::Created { topic_id } = s.record(&obs("1", "вопрос")).unwrap() else {
        panic!()
    };
    s.resolve(topic_id, 150).unwrap();
    assert_eq!(s.list().unwrap()[0].status, TopicStatus::Resolved);
    let mut later = obs("2", "вопрос");
    later.at = 200;
    s.record(&later).unwrap();
    let topic = &s.list().unwrap()[0];
    assert_eq!(topic.status, TopicStatus::Open);
    assert_eq!(topic.resolved_at, Some(150));
    assert_eq!(topic.last_seen, 200);
    assert_eq!(topic.first_seen, 100);
}

#[test]
fn topic_and_variant_limits_stop_new_records_without_evicting() {
    let (_dir, mut s) = store();
    let first = s
        .record(&with_vector(
            obs("0", "базовый вопрос"),
            bge(),
            [1.0, 0.0, 0.0],
        ))
        .unwrap();
    let Recorded::Created { topic_id } = first else {
        panic!()
    };
    // Варианты одной темы ограничены: лишние наблюдения считаются на теме.
    for i in 0..(MAX_VARIANTS_PER_TOPIC + 5) {
        let mut o = with_vector(
            obs(
                &format!("v{i}"),
                &format!("базовый вопрос вариант {}", letters(i)),
            ),
            bge(),
            [1.0, 0.0, 0.0],
        );
        o.at = 101;
        s.record(&o).unwrap();
    }
    let topics = s.list().unwrap();
    assert_eq!(topics.len(), 1, "{topics:?}");
    assert!(topics[0].variants.len() <= MAX_VARIANTS_PER_TOPIC);
    assert!(topics[0].observations as usize > MAX_VARIANTS_PER_TOPIC);
    assert_eq!(topics[0].id, topic_id);
}

#[test]
fn newer_schema_is_refused_and_left_untouched() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bars")).unwrap();
    let path = store_path(dir.path());
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch("CREATE TABLE marker(x); PRAGMA user_version=99;")
            .unwrap();
    }
    let error = TopicStore::open(dir.path())
        .err()
        .expect("новая схема не открывается");
    assert!(matches!(error, StoreError::NewerSchema(99)));
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = db
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 99);
    db.query_row("SELECT count(*) FROM marker", [], |r| r.get::<_, i64>(0))
        .unwrap();
}

#[test]
fn corrupt_file_is_an_error_not_an_empty_list() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bars")).unwrap();
    std::fs::write(store_path(dir.path()), b"not a database at all, definitely").unwrap();
    assert!(TopicStore::open(dir.path()).is_err());
}

#[test]
fn data_survives_reopening() {
    let dir = tempfile::tempdir().unwrap();
    {
        let mut s = TopicStore::open(dir.path()).unwrap();
        s.record(&obs("1", "вопрос")).unwrap();
    }
    let s = TopicStore::open(dir.path()).unwrap();
    assert_eq!(s.list().unwrap().len(), 1);
}

#[test]
fn guard_detects_identifier_number_and_antonym_differences() {
    assert!(differs("как включить логи", "как отключить логи"));
    assert!(differs("ошибка в версии 1.2.3", "ошибка в версии 1.2.4"));
    assert!(differs("что делает SearchHit", "что делает ScoreKind"));
    assert!(differs("первый этап", "второй этап"));
    assert!(!differs("как настроить сбор", "как мне настроить сбор"));
}

/// Замер для бюджетов: `cargo test -p bars-core --lib new_topics::tests::measure_limits -- --ignored --nocapture`.
#[test]
#[ignore = "замер, не проверка"]
fn measure_limits() {
    const DIM: usize = 1024;
    let (dir, mut s) = store();
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    };
    let mut times = Vec::new();
    for i in 0..MAX_TOPICS {
        let mut v: Vec<f32> = (0..DIM).map(|_| next()).collect();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter_mut().for_each(|x| *x /= n);
        let mut o = obs(
            &format!("e{i}"),
            &format!(
                "вопрос номер {} про тему {}",
                letters(i),
                letters(i * 7 + 3)
            ),
        );
        o.vector = Some((bge(), v));
        o.at = 100 + i as i64;
        let start = std::time::Instant::now();
        s.record(&o).unwrap();
        times.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let q = |p: f64| sorted[((sorted.len() as f64 * p) as usize).min(sorted.len() - 1)];
    let list_start = std::time::Instant::now();
    let n = s.list().unwrap().len();
    let list_ms = list_start.elapsed().as_secs_f64() * 1000.0;
    drop(s);
    let size: u64 = std::fs::read_dir(dir.path().join(".bars"))
        .unwrap()
        .map(|e| e.unwrap().metadata().unwrap().len())
        .sum();
    eprintln!(
        "тем {n}; запись: медиана {:.2} мс, p95 {:.2} мс, макс {:.2} мс; последняя {:.2} мс; list {:.1} мс; файлы {:.1} МБ",
        q(0.5),
        q(0.95),
        sorted.last().unwrap(),
        times.last().unwrap(),
        list_ms,
        size as f64 / 1048576.0
    );
}

fn created(s: &mut TopicStore, id: &str, text: &str) -> i64 {
    let Recorded::Created { topic_id } = s.record(&obs(id, text)).unwrap() else {
        panic!("тема не создана")
    };
    topic_id
}

fn found(variant_id: i64, stamp: &str) -> NewCheckResult {
    NewCheckResult {
        variant_id,
        scope: "project".into(),
        outcome: VariantOutcome::Found,
        reason: None,
        index_stamp: Some(stamp.into()),
        hits: r#"[{"path":"a.md"}]"#.into(),
    }
}

fn empty_result(variant_id: i64) -> NewCheckResult {
    NewCheckResult {
        outcome: VariantOutcome::Empty,
        hits: "[]".into(),
        ..found(variant_id, "t1")
    }
}

fn check_of(topic: &TopicRow, at: i64, results: Vec<NewCheckResult>) -> NewCheck {
    NewCheck {
        topic_revision: topic.revision,
        at,
        expanded: false,
        model: "m".into(),
        params: "{}".into(),
        results,
    }
}

#[test]
fn schema_one_database_is_migrated_with_its_topics_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = store_path(dir.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    {
        let db = rusqlite::Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE meta (id INTEGER PRIMARY KEY CHECK(id=1), generation INTEGER NOT NULL, enabled INTEGER NOT NULL);
             INSERT INTO meta VALUES(1,3,1);
             CREATE TABLE topics (id INTEGER PRIMARY KEY AUTOINCREMENT, revision INTEGER NOT NULL, title TEXT NOT NULL,
                status TEXT NOT NULL CHECK(status IN ('open','materials_found','resolved')),
                first_seen INTEGER NOT NULL, last_seen INTEGER NOT NULL, resolved_at INTEGER);
             CREATE TABLE variants (id INTEGER PRIMARY KEY AUTOINCREMENT, topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                text TEXT NOT NULL, norm_key TEXT NOT NULL, scope TEXT NOT NULL, space TEXT, vector BLOB, created INTEGER NOT NULL);
             CREATE TABLE observations (id INTEGER PRIMARY KEY AUTOINCREMENT, event_id TEXT NOT NULL UNIQUE,
                topic_id INTEGER NOT NULL REFERENCES topics(id) ON DELETE CASCADE,
                variant_id INTEGER NOT NULL REFERENCES variants(id) ON DELETE CASCADE,
                at INTEGER NOT NULL, source TEXT NOT NULL, reason TEXT NOT NULL, params TEXT NOT NULL, index_stamp TEXT);
             INSERT INTO topics VALUES(1,1,'старый вопрос','open',10,10,NULL);
             INSERT INTO variants VALUES(1,1,'старый вопрос','старый вопрос','project',NULL,NULL,10);
             INSERT INTO observations VALUES(1,'e1',1,1,10,'mcp','empty_result','{}',NULL);
             PRAGMA user_version=1;",
        )
        .unwrap();
    }
    let mut s = TopicStore::open(dir.path()).unwrap();
    assert_eq!(s.settings().unwrap(), (3, true), "поколение сохранено");
    let topic = s.get(1).unwrap().unwrap();
    assert_eq!(topic.title, "старый вопрос");
    assert_eq!(topic.sources, vec!["mcp"]);
    assert!(topic.checks.is_empty());
    s.record_check(1, &check_of(&topic, 20, vec![found(1, "t1")]))
        .unwrap()
        .unwrap();
}

#[test]
fn user_decisions_check_the_expected_revision() {
    let (_dir, mut s) = store();
    let id = created(&mut s, "1", "вопрос про сборку");
    let topic = s.get(id).unwrap().unwrap();

    let Change::Updated { revision } = s.rename(id, topic.revision, "Сборка проекта").unwrap()
    else {
        panic!("переименование отклонено")
    };
    assert_eq!(revision, topic.revision + 1);
    let renamed = s.get(id).unwrap().unwrap();
    assert_eq!(renamed.title, "Сборка проекта");
    assert_eq!(
        renamed.variants[0].text, "вопрос про сборку",
        "исходная формулировка цела"
    );

    // Старая ревизия: конфликт, ничего не записано.
    assert_eq!(
        s.set_status(id, topic.revision, TopicStatus::Resolved, 50)
            .unwrap(),
        Change::Conflict { current: revision }
    );
    assert_eq!(s.get(id).unwrap().unwrap().status, TopicStatus::Open);

    assert!(matches!(
        s.set_status(id, revision, TopicStatus::Resolved, 50)
            .unwrap(),
        Change::Updated { .. }
    ));
    let resolved = s.get(id).unwrap().unwrap();
    assert_eq!(
        (resolved.status, resolved.resolved_at),
        (TopicStatus::Resolved, Some(50))
    );

    // «Всё ещё не хватает»: открыта, дата прежнего решения остаётся.
    s.set_status(id, resolved.revision, TopicStatus::Open, 60)
        .unwrap();
    let reopened = s.get(id).unwrap().unwrap();
    assert_eq!(
        (reopened.status, reopened.resolved_at),
        (TopicStatus::Open, Some(50))
    );

    assert_eq!(
        s.delete_checked(id, topic.revision).unwrap(),
        Change::Conflict {
            current: reopened.revision
        }
    );
    assert!(matches!(
        s.delete_checked(id, reopened.revision).unwrap(),
        Change::Updated { .. }
    ));
    assert_eq!(
        s.delete_checked(id, reopened.revision).unwrap(),
        Change::NotFound
    );
}

#[test]
fn a_check_with_materials_moves_an_open_topic_and_keeps_two_checks() {
    let (_dir, mut s) = store();
    let id = created(&mut s, "1", "вопрос про сборку");
    let variant = s.get(id).unwrap().unwrap().variants[0].id;

    // Пустой и неполный результат статус не меняют.
    let topic = s.get(id).unwrap().unwrap();
    s.record_check(id, &check_of(&topic, 10, vec![empty_result(variant)]))
        .unwrap();
    let incomplete = NewCheckResult {
        outcome: VariantOutcome::Incomplete,
        reason: Some("indexing".into()),
        ..empty_result(variant)
    };
    s.record_check(id, &check_of(&topic, 11, vec![incomplete]))
        .unwrap();
    assert_eq!(s.get(id).unwrap().unwrap().status, TopicStatus::Open);

    let checked = s
        .record_check(id, &check_of(&topic, 12, vec![found(variant, "t2")]))
        .unwrap()
        .unwrap();
    assert_eq!(checked.results[0].outcome, VariantOutcome::Found);
    assert_eq!(checked.results[0].hits, r#"[{"path":"a.md"}]"#);
    let after = s.get(id).unwrap().unwrap();
    assert_eq!(after.status, TopicStatus::MaterialsFound);
    assert_eq!(after.revision, topic.revision + 1);
    // Хранятся последняя и предыдущая проверки; самая старая вытеснена.
    assert_eq!(
        after.checks.iter().map(|c| c.at).collect::<Vec<_>>(),
        vec![12, 11]
    );
    // Переход сделала сама проверка: она относится к новой ревизии; прежняя осталась со своей.
    assert_eq!(after.checks[0].topic_revision, after.revision);
    assert_eq!(after.checks[1].topic_revision, topic.revision);
}

#[test]
fn a_check_never_reopens_a_resolved_topic_or_restores_a_deleted_one() {
    let (_dir, mut s) = store();
    let id = created(&mut s, "1", "закрытая тема");
    let topic = s.get(id).unwrap().unwrap();
    let variant = topic.variants[0].id;
    s.set_status(id, topic.revision, TopicStatus::Resolved, 5)
        .unwrap();
    s.record_check(id, &check_of(&topic, 10, vec![found(variant, "t")]))
        .unwrap();
    assert_eq!(s.get(id).unwrap().unwrap().status, TopicStatus::Resolved);

    let current = s.get(id).unwrap().unwrap();
    s.delete_checked(id, current.revision).unwrap();
    assert!(
        s.record_check(id, &check_of(&current, 20, vec![found(variant, "t")]))
            .unwrap()
            .is_none(),
        "поздняя проверка удалённой темы"
    );
    assert!(s.list().unwrap().is_empty());
}

#[test]
fn clearing_removes_checks_and_ids_are_not_reused() {
    let (_dir, mut s) = store();
    let id = created(&mut s, "1", "первая тема");
    let topic = s.get(id).unwrap().unwrap();
    s.record_check(
        id,
        &check_of(&topic, 10, vec![found(topic.variants[0].id, "t")]),
    )
    .unwrap();
    s.clear().unwrap();
    assert!(s.get(id).unwrap().is_none());
    let mut late = obs("2", "вторая тема");
    late.generation = s.settings().unwrap().0;
    let Recorded::Created { topic_id } = s.record(&late).unwrap() else {
        panic!("тема не создана")
    };
    assert_ne!(topic_id, id, "ID темы не переиспользуется");
    assert!(s.get(topic_id).unwrap().unwrap().checks.is_empty());
    // Поздняя проверка прежней темы не попадает в новую.
    assert!(
        s.record_check(id, &check_of(&topic, 30, vec![found(1, "t")]))
            .unwrap()
            .is_none()
    );
}

#[test]
fn complete_empty_check_clears_previous_automatic_match_but_preserves_user_resolution() {
    let (_dir, mut s) = store();
    let id = created(&mut s, "1", "проверка знаний");
    let topic = s.get(id).unwrap().unwrap();
    let variant = topic.variants[0].id;
    s.record_check(id, &check_of(&topic, 10, vec![found(variant, "t")]))
        .unwrap();
    let matched = s.get(id).unwrap().unwrap();
    assert_eq!(matched.status, TopicStatus::MaterialsFound);
    let mut partial = empty_result(variant);
    partial.outcome = VariantOutcome::Incomplete;
    s.record_check(id, &check_of(&matched, 11, vec![partial]))
        .unwrap();
    assert_eq!(
        s.get(id).unwrap().unwrap().status,
        TopicStatus::MaterialsFound
    );
    s.record_check(id, &check_of(&matched, 12, vec![empty_result(variant)]))
        .unwrap();
    let empty = s.get(id).unwrap().unwrap();
    assert_eq!(empty.status, TopicStatus::Open);
    assert_eq!(empty.revision, matched.revision + 1);
    assert_eq!(empty.checks[0].topic_revision, empty.revision);
    s.set_status(id, empty.revision, TopicStatus::Resolved, 13)
        .unwrap();
    let resolved = s.get(id).unwrap().unwrap();
    s.record_check(id, &check_of(&resolved, 14, vec![empty_result(variant)]))
        .unwrap();
    assert_eq!(s.get(id).unwrap().unwrap().status, TopicStatus::Resolved);
}
