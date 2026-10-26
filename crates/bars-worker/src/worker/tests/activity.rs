use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn busy_project_stays_merged_while_other_project_updates_and_scans_finish() {
    let env = Env::new();
    let one = env.project("blocked", &[("a.md", DOC)]);
    let two = env.project("ready", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&one).unwrap();
    worker.register(&two).unwrap();
    idle(&worker).await;
    let before = env.provider.counters.texts.load(Ordering::SeqCst);
    let job_gate = lock(&worker.inner.projects)[&one].job_gate.clone();
    let held = job_gate.lock_owned().await;
    worker.pause();
    fs::write(one.join("docs/a.md"), "# Изменение занятого проекта\n").unwrap();
    fs::write(two.join("docs/a.md"), "# Изменение готового проекта\n").unwrap();
    worker.scan_receipt(&one).unwrap();
    worker.scan_receipt(&two).unwrap();
    wait_until(
        "обе проверки закончились без обработчика обновления",
        || lock(&worker.inner.scans).is_empty(),
    )
    .await;
    let id = lock(&worker.inner.queue).get(&one).unwrap().operation_id;
    worker.resume();
    wait_until(
        "готовый проект обошёл занятый",
        || {
            worker
                .status(&two)
                .unwrap()
                .last_report
                .is_some_and(|report| report.changed == 1)
        },
    )
    .await;
    assert_eq!(
        env.provider.counters.texts.load(Ordering::SeqCst),
        before + 1
    );
    for _ in 0..3 {
        worker.scan_receipt(&one).unwrap();
        wait_until(
            "повторная проверка завершена",
            || lock(&worker.inner.scans).is_empty(),
        )
        .await;
        let queued = worker.queued();
        assert_eq!(queued.len(), 1);
        assert_eq!(
            lock(&worker.inner.queue).get(&one).unwrap().operation_id,
            id
        );
        let active = worker.operation_events(&Default::default()).active;
        let updates: Vec<_> = active
            .iter()
            .filter(|op| op.project.as_deref() == one.to_str() && op.kind != "scan")
            .collect();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].state, "queued");
    }
    drop(held);
    idle(&worker).await;
    assert_eq!(
        env.provider.counters.texts.load(Ordering::SeqCst),
        before + 2
    );
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn inactive_checks_do_not_queue_unchanged_projects_or_embed() {
    let env = Env::new();
    let project = env.project("quiet", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    assert!(!worker.status(&project).unwrap().watching);
    let embedded = env.provider.counters.texts.load(Ordering::SeqCst);
    worker.pause();
    fs::write(
        project.join("docs/BARS_INSTRUCTION.md"),
        "устаревшая инструкция",
    )
    .unwrap();
    for _ in 0..3 {
        worker.check_all();
        wait_until("проверка завершена", || {
            lock(&worker.inner.scans).is_empty()
        })
        .await;
        assert!(worker.queued().is_empty());
        assert!(!lock(&worker.inner.docs).has_pending());
        assert_eq!(
            fs::read_to_string(project.join("docs/BARS_INSTRUCTION.md")).unwrap(),
            bars_core::instruction::INSTRUCTION_TEXT
        );
    }
    assert_eq!(env.provider.counters.texts.load(Ordering::SeqCst), embedded);
    assert_eq!(PERIODIC_INTERVAL, Duration::from_secs(3600));
    fs::write(project.join("docs/new.md"), "# Новое\n").unwrap();
    worker.check_all();
    wait_until(
        "часовая проверка нашла правку",
        || !worker.queued().is_empty(),
    )
    .await;
    assert_eq!(worker.queued()[0].priority, Priority::Periodic);
    assert!(!worker.status(&project).unwrap().watching);
    assert_eq!(env.provider.counters.texts.load(Ordering::SeqCst), embedded);
    worker.resume();
    idle(&worker).await;
    assert_eq!(documents(&project), 2);
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mcp_renews_one_hour_without_scanning_each_request() {
    let env = Env::new();
    let project = env.project("mcp", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&project).unwrap();
    idle(&worker).await;
    worker.observe_mcp(&project).unwrap();
    idle(&worker).await;
    assert!(worker.status(&project).unwrap().watching);
    let first = lock(&worker.inner.projects)[&project].mcp_until.unwrap();
    worker.observe_mcp(&project).unwrap();
    assert!(lock(&worker.inner.scans).is_empty());
    let renewed = lock(&worker.inner.projects)[&project].mcp_until.unwrap();
    assert!(renewed >= first);
    worker
        .inner
        .expire_observers(renewed + Duration::from_millis(1));
    assert!(!worker.status(&project).unwrap().watching);
    worker.inner.on_watch_event(&project, WatchEvent::Rescan);
    assert!(worker.queued().is_empty());
    assert!(lock(&worker.inner.scans).is_empty());
    worker.shutdown().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_switch_close_expire_and_preserve_other_observers() {
    let env = Env::new();
    let one = env.project("one", &[("a.md", DOC)]);
    let two = env.project("two", &[("a.md", DOC)]);
    let worker = env.start();
    worker.register(&one).unwrap();
    worker.register(&two).unwrap();
    idle(&worker).await;
    worker.observe_window("main", Some(&one)).unwrap();
    worker.observe_window("document", Some(&one)).unwrap();
    worker.observe_window("main", Some(&two)).unwrap();
    assert!(worker.status(&one).unwrap().watching);
    assert!(worker.status(&two).unwrap().watching);
    worker.observe_window("document", None).unwrap();
    assert!(!worker.status(&one).unwrap().watching);
    worker.observe_mcp(&two).unwrap();
    worker.observe_window("main", None).unwrap();
    assert!(worker.status(&two).unwrap().watching);
    let until = lock(&worker.inner.projects)[&two].mcp_until.unwrap();
    worker
        .inner
        .expire_observers(until + Duration::from_secs(1));
    assert!(!worker.status(&two).unwrap().watching);
    worker.observe_window("crashed", Some(&one)).unwrap();
    worker
        .inner
        .expire_observers(Instant::now() + Duration::from_secs(31));
    assert!(!worker.status(&one).unwrap().watching);
    worker.shutdown().await;
}
