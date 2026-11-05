use std::collections::HashSet;
use std::fs;
use std::path::Path;

use bars_graph::links::LinkKind;
use tempfile::TempDir;

use super::*;

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

fn project() -> TempDir {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "docs/Index.md",
        "---\ntitle: Главная\n---\n# Главная\n",
    );
    write(
        dir.path(),
        "docs/a/one.md",
        "---\ntitle: Один\n---\n[два](two.md) и [[three]]\n\n[[нет такого]]\n",
    );
    write(
        dir.path(),
        "docs/a/two.md",
        "# Два\n\n[обратно](one.md#часть)\n",
    );
    write(
        dir.path(),
        "docs/b/three.md",
        "# Три\n\n```\n[скрыто](one.md)\n```\n[[a/one|Один]]\n",
    );
    dir
}

fn index(dir: &TempDir) -> LinkIndex {
    let mut index = LinkIndex::open(dir.path()).unwrap();
    index.refresh(dir.path(), Path::new("docs")).unwrap();
    index
}

fn broken(index: &LinkIndex, path: &str) -> usize {
    index
        .links_of(path)
        .unwrap()
        .outgoing
        .iter()
        .filter(|l| l.broken)
        .count()
}

#[test]
fn bounded_pages_preserve_occurrences_and_snapshot() {
    let dir = project();
    write(dir.path(), "docs/many.md", &"[x](a/one.md) ".repeat(3000));
    let mut index = index(&dir);
    let snapshot = index.snapshot().unwrap();
    let first = index.page("docs/a/one.md", true, 0, 2).unwrap();
    assert_eq!(first.items.len(), 2);
    assert!(first.has_more);
    let mut count = 0;
    let mut offset = 0;
    loop {
        let page = index.page("docs/many.md", false, offset, 199).unwrap();
        count += page.items.len();
        if !page.has_more {
            break;
        }
        assert!(page.next_offset > offset);
        offset = page.next_offset;
    }
    assert_eq!(count, 3000);
    index
        .refresh_checked(dir.path(), Path::new("docs"))
        .unwrap();
    assert_eq!(snapshot, index.snapshot().unwrap());
    write(dir.path(), "docs/new.md", "# Новый\n");
    index
        .refresh_checked(dir.path(), Path::new("docs"))
        .unwrap();
    assert_ne!(snapshot, index.snapshot().unwrap());
    write(
        dir.path(),
        "docs/long.md",
        &format!("[{}](a/one.md)", "😀".repeat(2000)),
    );
    index
        .refresh_checked(dir.path(), Path::new("docs"))
        .unwrap();
    let page = index.page("docs/long.md", false, 0, 50).unwrap();
    assert!(page.fields_truncated);
    assert_eq!(
        page.items[0]["text"].as_str().unwrap().chars().count(),
        1024
    );
    assert!(!index.page("docs/unknown.md", false, 0, 50).unwrap().known);
}

#[test]
fn link_page_byte_budget_keeps_a_resumable_valid_page() {
    let dir = project();
    let label = "😀".repeat(1024);
    write(
        dir.path(),
        "docs/title.md",
        &format!("---\ntitle: {label}\n---\n# Заголовок\n"),
    );
    write(
        dir.path(),
        "docs/large.md",
        &format!("[{label}](title.md)\n").repeat(200),
    );
    let index = index(&dir);
    let page = index.page("docs/large.md", false, 0, 200).unwrap();
    assert!(page.bytes_limited && page.has_more);
    assert!(!page.items.is_empty() && page.items.len() < 200);
    assert!(serde_json::to_vec(&page.items).unwrap().len() < 512 * 1024 - 2048);
    let mut offset = page.next_offset;
    loop {
        let next = index.page("docs/large.md", false, offset, 200).unwrap();
        assert!(next.next_offset > offset);
        offset = next.next_offset;
        if !next.has_more {
            break;
        }
    }
    assert_eq!(offset, 200);
}

#[test]
fn outgoing_and_incoming_links_match_by_path_and_name() {
    let dir = project();
    let index = index(&dir);
    let one = index.links_of("docs/a/one.md").unwrap();
    assert!(one.known);
    let targets: Vec<_> = one
        .outgoing
        .iter()
        .map(|l| (l.kind, l.target.clone(), l.broken))
        .collect();
    assert_eq!(
        targets,
        vec![
            (LinkKind::Inline, Some("docs/a/two.md".into()), false),
            (LinkKind::Wiki, Some("docs/b/three.md".into()), false),
            (LinkKind::Wiki, None, true),
        ]
    );
    assert_eq!(one.outgoing[0].line, 4);
    let from: Vec<_> = one
        .incoming
        .iter()
        .map(|l| (l.path.as_str(), l.lines.clone()))
        .collect();
    // `[скрыто](one.md)` в коде не ссылка; `[[a/one|Один]]` и `one.md#часть` — по одной ссылке.
    assert_eq!(
        from,
        vec![("docs/a/two.md", vec![3]), ("docs/b/three.md", vec![6])]
    );
}

#[test]
fn links_into_set_skip_sources_inside_the_set() {
    let dir = project();
    let index = index(&dir);
    // Удаляется один документ: ссылаются two.md (строка 3) и three.md (строка 6).
    let one = index.links_into(&["docs/a/one.md".to_string()]).unwrap();
    let view: Vec<_> = one
        .iter()
        .map(|s| {
            (
                s.path.as_str(),
                s.links.iter().map(|l| l.line).collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        view,
        vec![("docs/a/two.md", vec![3]), ("docs/b/three.md", vec![6])]
    );
    assert_eq!(one[0].links[0].text, "обратно");
    assert_eq!(one[1].title, None);
    // Удаляется вся папка a: ссылки one↔two внутри набора не считаются, остаётся three → one.
    let folder = index
        .links_into(&["docs/a/one.md".to_string(), "docs/a/two.md".to_string()])
        .unwrap();
    assert_eq!(folder.len(), 1);
    assert_eq!(folder[0].path, "docs/b/three.md");
    // На документ без входящих ссылок и на пустой набор — ничего.
    assert!(
        index
            .links_into(&["docs/Index.md".to_string()])
            .unwrap()
            .is_empty()
    );
    assert!(index.links_into(&[]).unwrap().is_empty());
}

#[test]
fn creating_and_removing_files_repairs_and_breaks_links() {
    let dir = project();
    let mut index = index(&dir);
    assert_eq!(broken(&index, "docs/a/one.md"), 1);
    write(dir.path(), "docs/нет такого.md", "# Появился\n");
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    assert_eq!((stats.parsed, stats.removed), (1, 0));
    assert_eq!(broken(&index, "docs/a/one.md"), 0);
    fs::remove_file(dir.path().join("docs/a/two.md")).unwrap();
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    assert_eq!(stats.removed, 1);
    assert_eq!(broken(&index, "docs/a/one.md"), 1);
    assert!(!index.links_of("docs/a/two.md").unwrap().known);
}

#[test]
fn unchanged_files_are_not_parsed_again() {
    let dir = project();
    let mut index = index(&dir);
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    assert_eq!((stats.documents, stats.parsed, stats.removed), (4, 0, 0));
    write(
        dir.path(),
        "docs/a/two.md",
        "# Два\n\nтеперь [Главная](../Index.md)\n",
    );
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    assert_eq!(stats.parsed, 1);
    let home = index.links_of("docs/Index.md").unwrap();
    assert_eq!(home.incoming.len(), 1);
    assert_eq!(home.incoming[0].path, "docs/a/two.md");
}

#[test]
fn title_comes_from_frontmatter() {
    let dir = project();
    let index = index(&dir);
    let two = index.links_of("docs/a/two.md").unwrap();
    assert_eq!(two.incoming[0].title.as_deref(), Some("Один"));
    assert_eq!(two.outgoing[0].target_title.as_deref(), Some("Один"));
}

#[test]
fn works_without_model_and_rebuilds_after_file_loss() {
    let dir = project();
    drop(index(&dir));
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{}", store_path(dir.path()).display(), suffix));
    }
    let index = index(&dir);
    assert_eq!(index.links_of("docs/a/one.md").unwrap().outgoing.len(), 3);
}

#[test]
fn corrupt_file_is_replaced() {
    let dir = project();
    drop(index(&dir));
    fs::write(
        store_path(dir.path()),
        b"not a database at all, just plain text, long enough to look like a header....",
    )
    .unwrap();
    let index = index(&dir);
    assert!(index.links_of("docs/a/one.md").unwrap().known);
}

#[test]
fn changed_parser_version_rebuilds_everything() {
    let dir = project();
    let mut index = index(&dir);
    index
        .db_for_tests()
        .execute("UPDATE meta SET value='old' WHERE key='parser'", [])
        .unwrap();
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    assert_eq!(stats.parsed, 4);
}

#[test]
fn links_to_non_markdown_outside_root_external_and_own_section_are_ignored() {
    let dir = TempDir::new().unwrap();
    write(
        dir.path(),
        "docs/x.md",
        "[p](pic.png) [o](../README.md) [u](https://e.org/a.md) [s](#тут) [[#тут]]\n",
    );
    let index = index(&dir);
    assert!(index.links_of("docs/x.md").unwrap().outgoing.is_empty());
}

/// Замер L-09: `cargo test -p bars-core link_index::tests::limits -- --ignored --nocapture`.
#[test]
#[ignore = "замер, не проверка"]
fn limits_ten_thousand_documents() {
    use std::time::Instant;
    let dir = TempDir::new().unwrap();
    for i in 0..10_000 {
        let folder = i % 50;
        let text = format!(
            "---\ntitle: Док {i}\n---\n# Док {i}\n\n[a](../f{}/d{}.md) [[d{}]] [[f{}/d{}]] [x](d{}.md)\n",
            (i + 1) % 50,
            i + 1,
            (i + 7) % 10_000,
            (i + 3) % 50,
            (i + 3) % 10_000,
            i,
        );
        write(dir.path(), &format!("docs/f{folder}/d{i}.md"), &text);
    }
    let started = Instant::now();
    let mut index = LinkIndex::open(dir.path()).unwrap();
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    println!("первая сборка: {:?}, {stats:?}", started.elapsed());
    let started = Instant::now();
    let stats = index.refresh(dir.path(), Path::new("docs")).unwrap();
    println!(
        "повторная сверка без правок: {:?}, {stats:?}",
        started.elapsed()
    );
    let started = Instant::now();
    let of = index.links_of("docs/f5/d5.md").unwrap();
    println!(
        "links_of: {:?}, исходящих {}, входящих {}",
        started.elapsed(),
        of.outgoing.len(),
        of.incoming.len()
    );
    println!(
        "размер файла: {} байт",
        fs::metadata(store_path(dir.path())).unwrap().len()
    );
}

fn set(paths: &[&str]) -> HashSet<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

fn strings(paths: &[&str]) -> Vec<String> {
    paths.iter().map(|path| path.to_string()).collect()
}

#[test]
fn linked_documents_follow_links_in_both_directions() {
    let dir = project();
    let index = index(&dir);
    let all = strings(&[
        "docs/Index.md",
        "docs/a/one.md",
        "docs/a/two.md",
        "docs/b/three.md",
    ]);

    // `two` ссылается на `one`; на `two` ссылается `one`.
    let from_two = index
        .linked_documents(&strings(&["docs/a/two.md"]), &all)
        .unwrap();
    assert_eq!(from_two, set(&["docs/a/one.md"]));

    // `one` ссылается на `two` и `three`, а `two` и `three` ссылаются на `one`; сам с собой — нет.
    let from_one = index
        .linked_documents(&strings(&["docs/a/one.md"]), &all)
        .unwrap();
    assert_eq!(from_one, set(&["docs/a/two.md", "docs/b/three.md"]));
}

#[test]
fn linked_documents_of_an_anchor_set_include_anchors_linked_to_each_other() {
    let dir = project();
    let index = index(&dir);

    let found = index
        .linked_documents(
            &strings(&["docs/a/one.md", "docs/a/two.md"]),
            &strings(&["docs/a/one.md", "docs/a/two.md", "docs/Index.md"]),
        )
        .unwrap();

    assert_eq!(found, set(&["docs/a/one.md", "docs/a/two.md"]));
}

#[test]
fn linked_documents_ignore_unlinked_documents_and_empty_sets() {
    let dir = project();
    let index = index(&dir);
    let all = strings(&["docs/a/one.md", "docs/b/three.md"]);

    assert!(
        index
            .linked_documents(&strings(&["docs/Index.md"]), &all)
            .unwrap()
            .is_empty()
    );
    assert!(index.linked_documents(&[], &all).unwrap().is_empty());
    assert!(index.linked_documents(&all, &[]).unwrap().is_empty());
}
