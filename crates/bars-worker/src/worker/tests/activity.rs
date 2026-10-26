use super::*;

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
