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
fn normalization_keeps_negations_numbers_and_identifiers() {
    assert_ne!(normalize_question("не работает"), normalize_question("работает"));
    assert_ne!(normalize_question("версия 1.2"), normalize_question("версия 1.3"));
    assert_ne!(normalize_question("ключ API_KEY"), normalize_question("ключ api_key"));
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
    s.record(&with_vector(obs("1", "как настроить сбор двух тем"), bge(), [1.0, 0.0, 0.0]))
        .unwrap();
    // Косинус около 0,98 и нет лексической разницы: сливается.
    let merged = s
        .record(&with_vector(
            obs("2", "как мне настроить сбор двух тем"),
            bge(),
            [1.0, 0.2, 0.0],
        ))
        .unwrap();
    assert!(matches!(merged, Recorded::Merged { by: Match::Semantic, .. }));
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
        .record(&with_vector(obs("4", "что-то совсем иное"), bge(), [0.0, 1.0, 0.0]))
        .unwrap();
    assert!(matches!(far, Recorded::Created { .. }));
}

#[test]
fn unmeasured_space_never_merges_semantically() {
    let (_dir, mut s) = store();
    s.record(&with_vector(obs("1", "один вопрос"), unmeasured(), [1.0, 0.0, 0.0]))
        .unwrap();
    let second = s
        .record(&with_vector(obs("2", "другой вопрос"), unmeasured(), [1.0, 0.0, 0.0]))
        .unwrap();
    assert!(matches!(second, Recorded::Created { .. }));
}

#[test]
fn vectors_of_different_spaces_are_not_compared() {
    let (_dir, mut s) = store();
    s.record(&with_vector(obs("1", "один вопрос"), bge(), [1.0, 0.0, 0.0]))
        .unwrap();
    let other_space = EmbeddingSpace {
        key: "builtin|BAAI/bge-m3|3|query: |l2".into(),
    };
    let second = s
        .record(&with_vector(obs("2", "другой вопрос"), other_space, [1.0, 0.0, 0.0]))
        .unwrap();
    assert!(matches!(second, Recorded::Created { .. }));
}

#[test]
fn chain_a_b_c_is_not_glued_through_the_middle_question() {
    let (_dir, mut s) = store();
    // A-B и B-C близки (cos > 0,79), A-C — нет: C не сливается с темой A через B.
    s.record(&with_vector(obs("1", "вопрос про ключи"), bge(), [1.0, 0.0, 0.0]))
        .unwrap();
    s.record(&with_vector(obs("2", "вопрос про ключи и токены"), bge(), [1.0, 0.8, 0.0]))
        .unwrap();
    let c = s
        .record(&with_vector(obs("3", "вопрос про токены"), bge(), [0.0, 1.0, 0.0]))
        .unwrap();
    assert!(matches!(c, Recorded::Created { .. }), "{c:?}");
}

#[test]
fn text_without_vector_is_stored_and_merges_exactly() {
    let (_dir, mut s) = store();
    s.record(&obs("1", "пустой корпус")).unwrap();
    let again = s.record(&obs("2", "Пустой корпус")).unwrap();
    assert!(matches!(again, Recorded::Merged { by: Match::Exact, .. }));
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
    assert_eq!(s.record(&obs("2", "вопрос")).unwrap(), Recorded::StaleGeneration);
    let mut fresh = obs("3", "вопрос");
    fresh.generation = next;
    assert!(matches!(s.record(&fresh).unwrap(), Recorded::Created { .. }));
    let after_disable = s.set_enabled(false).unwrap();
    assert!(after_disable > next);
    let mut late = obs("4", "иной вопрос");
    late.generation = next;
    assert_eq!(s.record(&late).unwrap(), Recorded::Disabled);
    assert_eq!(s.topic_count().unwrap(), 1, "выключение сохраняет существующие");
    assert_eq!(s.set_enabled(false).unwrap(), after_disable, "без смены поколение прежнее");
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
        .record(&with_vector(obs("0", "базовый вопрос"), bge(), [1.0, 0.0, 0.0]))
        .unwrap();
    let Recorded::Created { topic_id } = first else {
        panic!()
    };
    // Варианты одной темы ограничены: лишние наблюдения считаются на теме.
    for i in 0..(MAX_VARIANTS_PER_TOPIC + 5) {
        let mut o = with_vector(
            obs(&format!("v{i}"), &format!("базовый вопрос вариант {}", letters(i))),
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
    let error = TopicStore::open(dir.path()).err().expect("новая схема не открывается");
    assert!(matches!(error, StoreError::NewerSchema(99)));
    let db = rusqlite::Connection::open(&path).unwrap();
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
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
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32) - 0.5
    };
    let mut times = Vec::new();
    for i in 0..MAX_TOPICS {
        let mut v: Vec<f32> = (0..DIM).map(|_| next()).collect();
        let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        v.iter_mut().for_each(|x| *x /= n);
        let mut o = obs(&format!("e{i}"), &format!("вопрос номер {} про тему {}", letters(i), letters(i * 7 + 3)));
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
        q(0.5), q(0.95), sorted.last().unwrap(), times.last().unwrap(), list_ms, size as f64 / 1048576.0
    );
}
