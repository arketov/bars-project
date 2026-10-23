use super::*;

fn finding(key: &str, group: &str) -> NewFinding {
    NewFinding {
        fingerprint: key.to_string(),
        tag: "broken".into(),
        group: group.into(),
        reason: "причина".into(),
        score: None,
        subjects: vec![Subject {
            path: format!("docs/{key}.md"),
            first_line: 3,
            last_line: 3,
            heading: String::new(),
            role: "document".into(),
        }],
        detail: String::new(),
    }
}

fn run(store: &mut AuditStore, status: RunStatus, findings: Vec<NewFinding>) -> RunRow {
    store
        .finish_run(
            AuditKind::BrokenLinks,
            10,
            20,
            status,
            None,
            Some("stamp"),
            findings,
        )
        .unwrap()
}

#[test]
fn roles_follow_the_path() {
    assert_eq!(
        document_role("docs/chats/2026-10-22-x/handoff.md"),
        "session_log"
    );
    assert_eq!(document_role("docs/manual/search.md"), "manual");
    assert_eq!(document_role("docs/features/a/Index.md"), "index");
    assert_eq!(document_role("docs/features/a/design.md"), "feature");
    assert_eq!(document_role("docs/architecture/worker.md"), "architecture");
    assert_eq!(document_role("docs/adr/0001-x.md"), "decision");
    assert_eq!(document_role("docs/knowledge/x.md"), "research");
    assert_eq!(document_role("docs/backlog/ideas.md"), "backlog");
    assert_eq!(document_role("docs\\manual\\x.md"), "manual");
    assert_eq!(document_role("docs/MAIN_IDEA.md"), "document");
}

#[test]
fn first_complete_run_has_no_new_findings_and_second_marks_new_known_gone() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AuditStore::open(dir.path()).unwrap();
    let first = run(
        &mut store,
        RunStatus::Complete,
        vec![finding("a", "main"), finding("b", "main")],
    );
    assert_eq!((first.total, first.new_count, first.gone_count), (2, 0, 0));

    let second = run(
        &mut store,
        RunStatus::Complete,
        vec![finding("b", "main"), finding("c", "main")],
    );
    assert_eq!(
        (second.total, second.new_count, second.gone_count),
        (2, 1, 1)
    );
    let page = store.findings(second.id, None, true, 0, 10).unwrap();
    let standings: Vec<_> = page
        .items
        .iter()
        .map(|f| (f.fingerprint.as_str(), f.standing))
        .collect();
    assert_eq!(
        standings,
        vec![
            ("b", Standing::Known),
            ("c", Standing::New),
            ("a", Standing::Gone)
        ]
    );
    let without_gone = store.findings(second.id, None, false, 0, 10).unwrap();
    assert_eq!(without_gone.total, 2);
}

#[test]
fn overlap_group_is_counted_apart_and_never_new_in_the_headline() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AuditStore::open(dir.path()).unwrap();
    run(&mut store, RunStatus::Complete, vec![finding("a", "main")]);
    let second = run(
        &mut store,
        RunStatus::Complete,
        vec![finding("a", "main"), finding("z", "overlap")],
    );
    assert_eq!(
        (second.total, second.new_count, second.overlap_total),
        (1, 0, 1)
    );
    let only_main = store
        .findings(second.id, Some("main"), true, 0, 10)
        .unwrap();
    assert_eq!(only_main.total, 1);
}

#[test]
fn incomplete_runs_keep_the_previous_complete_result() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AuditStore::open(dir.path()).unwrap();
    let complete = run(&mut store, RunStatus::Complete, vec![finding("a", "main")]);
    for status in [RunStatus::Partial, RunStatus::Failed, RunStatus::Cancelled] {
        let row = run(&mut store, status, vec![finding("x", "main")]);
        assert_eq!(row.total, 0);
        assert_eq!(
            store
                .latest_run(AuditKind::BrokenLinks)
                .unwrap()
                .unwrap()
                .id,
            row.id
        );
    }
    let kept = store
        .latest_complete(AuditKind::BrokenLinks)
        .unwrap()
        .unwrap();
    assert_eq!(kept.id, complete.id);
    assert_eq!(store.findings(kept.id, None, true, 0, 10).unwrap().total, 1);
}

#[test]
fn only_the_last_twenty_runs_and_the_last_complete_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AuditStore::open(dir.path()).unwrap();
    let complete = run(&mut store, RunStatus::Complete, vec![finding("a", "main")]);
    for _ in 0..25 {
        run(&mut store, RunStatus::Failed, Vec::new());
    }
    assert_eq!(
        store
            .latest_complete(AuditKind::BrokenLinks)
            .unwrap()
            .unwrap()
            .id,
        complete.id
    );
}

#[test]
fn paging_returns_a_cursor_until_the_end() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AuditStore::open(dir.path()).unwrap();
    let items: Vec<_> = (0..5).map(|n| finding(&format!("f{n}"), "main")).collect();
    let row = run(&mut store, RunStatus::Complete, items);
    let first = store.findings(row.id, None, true, 0, 2).unwrap();
    assert_eq!(
        (first.items.len(), first.next, first.total),
        (2, Some(2), 5)
    );
    let last = store.findings(row.id, None, true, 4, 2).unwrap();
    assert_eq!((last.items.len(), last.next), (1, None));
}

#[test]
fn schedule_defaults_to_off_and_on_change_is_only_for_broken_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = AuditStore::open(dir.path()).unwrap();
    assert_eq!(
        store.schedule(AuditKind::Duplicates).unwrap(),
        ScheduleMode::Off
    );
    store
        .set_schedule(AuditKind::BrokenLinks, ScheduleMode::OnChange)
        .unwrap();
    assert_eq!(
        store.schedule(AuditKind::BrokenLinks).unwrap(),
        ScheduleMode::OnChange
    );
    store
        .set_schedule(AuditKind::Duplicates, ScheduleMode::OnChange)
        .unwrap();
    assert_eq!(
        store.schedule(AuditKind::Duplicates).unwrap(),
        ScheduleMode::Off
    );
}

#[test]
fn corrupt_store_is_replaced() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".bars")).unwrap();
    std::fs::write(store_path(dir.path()), b"not a database at all, just text").unwrap();
    let store = AuditStore::open(dir.path()).unwrap();
    assert!(store.latest_run(AuditKind::Duplicates).unwrap().is_none());
}
