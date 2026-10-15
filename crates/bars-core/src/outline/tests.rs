use super::*;

const DOC: &str = "\
# bars

Вступление.

## Стадия

Текст.

### Подробности

## Установка

### Windows

### macOS

#### Нюанс

## Лицензия
";

fn lines(headings: &[Heading]) -> Vec<String> {
    headings.iter().map(ToString::to_string).collect()
}

fn query(levels: &[usize], under: Option<Under>) -> OutlineQuery {
    OutlineQuery {
        levels: levels.to_vec(),
        under,
    }
}

#[test]
fn returns_all_headings_with_line_numbers() {
    let result = outline(DOC, &OutlineQuery::default()).unwrap();
    assert_eq!(
        lines(&result),
        [
            "1 # bars",
            "5 ## Стадия",
            "9 ### Подробности",
            "11 ## Установка",
            "13 ### Windows",
            "15 ### macOS",
            "17 #### Нюанс",
            "19 ## Лицензия",
        ]
    );
}

#[test]
fn filters_by_level() {
    let result = outline(DOC, &query(&[2], None)).unwrap();
    assert_eq!(
        lines(&result),
        ["5 ## Стадия", "11 ## Установка", "19 ## Лицензия"]
    );

    let result = outline(DOC, &query(&[3, 4], None)).unwrap();
    assert_eq!(
        lines(&result),
        [
            "9 ### Подробности",
            "13 ### Windows",
            "15 ### macOS",
            "17 #### Нюанс",
        ]
    );
}

#[test]
fn rejects_level_outside_one_to_six() {
    for level in [0, 7] {
        assert_eq!(
            outline(DOC, &query(&[2, level], None)),
            Err(OutlineError::InvalidLevel(level))
        );
    }
}

#[test]
fn under_chain_returns_nested_headings_without_the_heading_itself() {
    let under = Under::Chain("bars/Установка".into());
    let result = outline(DOC, &query(&[], Some(under))).unwrap();
    assert_eq!(
        lines(&result),
        ["13 ### Windows", "15 ### macOS", "17 #### Нюанс"]
    );
}

#[test]
fn under_goes_to_any_depth_and_levels_filter_it() {
    let under = Under::Chain("bars/Установка".into());
    let result = outline(DOC, &query(&[4], Some(under))).unwrap();
    assert_eq!(lines(&result), ["17 #### Нюанс"]);

    let result = outline(DOC, &query(&[], Some(Under::Line(1)))).unwrap();
    assert_eq!(result.len(), 7);
}

#[test]
fn under_line_matches_chain() {
    let by_line = outline(DOC, &query(&[], Some(Under::Line(11)))).unwrap();
    let by_chain = outline(
        DOC,
        &query(&[], Some(Under::Chain("bars/Установка".into()))),
    )
    .unwrap();
    assert_eq!(by_line, by_chain);
}

#[test]
fn under_a_heading_without_children_is_empty() {
    let result = outline(DOC, &query(&[], Some(Under::Line(19)))).unwrap();
    assert!(result.is_empty());
}

#[test]
fn under_stops_at_a_heading_of_the_same_or_higher_level() {
    let result = outline(DOC, &query(&[], Some(Under::Line(5)))).unwrap();
    assert_eq!(lines(&result), ["9 ### Подробности"]);
}

#[test]
fn unknown_chain_and_non_heading_line_are_errors() {
    let missing = Under::Chain("bars/Нет такого".into());
    assert_eq!(
        outline(DOC, &query(&[], Some(missing))),
        Err(OutlineError::HeadingNotFound("bars/Нет такого".into()))
    );
    // Цепочка полная: заголовок без родителя не находится.
    let partial = Under::Chain("Установка".into());
    assert!(matches!(
        outline(DOC, &query(&[], Some(partial))),
        Err(OutlineError::HeadingNotFound(_))
    ));
    assert_eq!(
        outline(DOC, &query(&[], Some(Under::Line(3)))),
        Err(OutlineError::NotAHeading(3))
    );
}

#[test]
fn duplicate_chains_are_merged_in_document_order() {
    let source = "# A\n## Раздел\n### x\n## Раздел\n### y\n#### z\n## Другой\n### w\n";
    let under = Under::Chain("A/Раздел".into());
    let result = outline(source, &query(&[], Some(under))).unwrap();
    assert_eq!(lines(&result), ["3 ### x", "5 ### y", "6 #### z"]);
}

#[test]
fn headings_inside_code_blocks_are_ignored() {
    let source = "# A\n\n```\n# не заголовок\n## и это\n```\n\n~~~text\n# и тут\n~~~\n\n## B\n";
    let result = outline(source, &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["1 # A", "12 ## B"]);
}

#[test]
fn line_numbers_count_frontmatter() {
    let source = "---\ntitle: Док\n---\n\n# A\n\n## B\n";
    let result = outline(source, &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["5 # A", "7 ## B"]);
}

#[test]
fn heading_like_line_inside_frontmatter_is_not_a_heading() {
    let source = "---\ntitle: Док\n# комментарий\n---\n# A\n";
    let result = outline(source, &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["5 # A"]);
}

#[test]
fn crlf_and_bom_do_not_shift_lines() {
    let source = "\u{feff}# A\r\n\r\n## B\r\n\r\ntext\r\n## C\r\n";
    let result = outline(source, &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["1 # A", "3 ## B", "6 ## C"]);
}

#[test]
fn setext_and_headings_without_text_are_ignored() {
    let source = "Заголовок\n=========\n\n#\n##  \n#нет пробела\n    # отступ в четыре пробела\n## Настоящий\n";
    let result = outline(source, &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["8 ## Настоящий"]);
}

#[test]
fn closing_hashes_are_dropped_from_the_text() {
    let result = outline("## Раздел ##\n", &OutlineQuery::default()).unwrap();
    assert_eq!(lines(&result), ["1 ## Раздел"]);
}

#[test]
fn skipped_levels_still_nest_by_chain() {
    let source = "# A\n### C\n## B\n";
    let under = Under::Chain("A".into());
    let result = outline(source, &query(&[], Some(under))).unwrap();
    assert_eq!(lines(&result), ["2 ### C", "3 ## B"]);
    let under = Under::Chain("A/B".into());
    assert!(
        outline(source, &query(&[], Some(under)))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn document_without_headings_is_empty() {
    assert!(
        outline("просто текст\n", &OutlineQuery::default())
            .unwrap()
            .is_empty()
    );
    assert!(outline("", &OutlineQuery::default()).unwrap().is_empty());
}
