use super::*;

fn docs(paths: &[&str]) -> DocSet {
    DocSet::new(paths.iter().map(|p| p.to_string()))
}

fn kinds(text: &str) -> Vec<(LinkKind, String)> {
    parse_links(text)
        .into_iter()
        .map(|l| (l.kind, l.raw_target))
        .collect()
}

#[test]
fn parses_inline_reference_and_wiki_links_with_lines() {
    let text = "# T\n\n[a](b.md) и [c][r]\n\n[[Имя|псевдоним]]\n\n[r]: d.md\n";
    let links = parse_links(text);
    assert_eq!(links.len(), 3);
    assert_eq!(links[0].kind, LinkKind::Inline);
    assert_eq!(
        (
            links[0].raw_target.as_str(),
            links[0].text.as_str(),
            links[0].line
        ),
        ("b.md", "a", 3)
    );
    assert_eq!(links[1].kind, LinkKind::Reference);
    assert_eq!(
        (links[1].raw_target.as_str(), links[1].text.as_str()),
        ("d.md", "c")
    );
    assert_eq!(links[2].kind, LinkKind::Wiki);
    assert_eq!((links[2].text.as_str(), links[2].line), ("псевдоним", 5));
    assert_eq!(&text[links[2].start..links[2].end], "[[Имя|псевдоним]]");
}

#[test]
fn code_url_and_frontmatter_are_not_links() {
    let text = "---\ntitle: [x](front.md)\n---\n\n`[a](code.md)` и `[[код]]`\n\n```\n[b](block.md)\n[[блок]]\n```\n\n[u](https://e.org/x.md) [m](mailto:a@b.c)\n";
    let links = parse_links(text);
    let targets: Vec<_> = links.iter().filter_map(ParsedLink::target).collect();
    assert!(targets.is_empty(), "{targets:?}");
    assert_eq!(
        kinds(text).len(),
        2,
        "URL остаются ссылками для разбора, но не адресатами"
    );
}

#[test]
fn fragments_and_own_section_links() {
    let links = parse_links("[a](b.md#раздел) [[Имя#часть]] [[#тут]] [s](#здесь)\n");
    assert_eq!(links[0].fragment.as_deref(), Some("раздел"));
    assert_eq!(links[0].target(), Some(Target::Path("b.md".into())));
    assert_eq!(links[1].kind, LinkKind::Inline);
    assert_eq!(links[1].target(), None, "якорь своего документа");
    assert_eq!(links[2].fragment.as_deref(), Some("часть"));
    assert_eq!(links[2].target(), Some(Target::Name("Имя".into())));
    assert_eq!(links[3].target(), None, "[[#раздел]] — свой документ");
}

#[test]
fn resolves_paths_names_and_broken() {
    let set = docs(&["a/one.md", "a/two.md", "b/One.md", "root.md"]);
    let link = |s: &str| parse_links(s).into_iter().next().unwrap();
    assert_eq!(
        set.resolve("a/two.md", &link("[x](one.md)")),
        Resolved::Found("a/one.md")
    );
    assert_eq!(
        set.resolve("a/two.md", &link("[x](../b/One.md)")),
        Resolved::Found("b/One.md")
    );
    assert_eq!(
        set.resolve("a/two.md", &link("[x](gone.md)")),
        Resolved::Broken
    );
    assert_eq!(
        set.resolve("a/two.md", &link("[x](pic.png)")),
        Resolved::NotALink
    );
    assert_eq!(
        set.resolve("a/two.md", &link("[x](../../out.md)")),
        Resolved::NotALink
    );
    // Одинаковые имена: сначала папка источника.
    assert_eq!(
        set.resolve("a/two.md", &link("[[one]]")),
        Resolved::Found("a/one.md")
    );
    assert_eq!(
        set.resolve("b/x.md", &link("[[one]]")),
        Resolved::Found("b/One.md")
    );
    assert_eq!(
        set.resolve("root.md", &link("[[b/one]]")),
        Resolved::Found("b/One.md")
    );
    assert_eq!(
        set.resolve("root.md", &link("[[нет такого]]")),
        Resolved::Broken
    );
}

#[test]
fn percent_encoded_paths_resolve() {
    let set = docs(&["папка/файл с пробелом.md"]);
    let link = &parse_links(
        "[x](%D0%BF%D0%B0%D0%BF%D0%BA%D0%B0/%D1%84%D0%B0%D0%B9%D0%BB%20%D1%81%20%D0%BF%D1%80%D0%BE%D0%B1%D0%B5%D0%BB%D0%BE%D0%BC.md)",
    )[0];
    assert_eq!(
        set.resolve("root.md", link),
        Resolved::Found("папка/файл с пробелом.md")
    );
}
