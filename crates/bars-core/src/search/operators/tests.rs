use super::*;

fn parse(query: &str) -> ParsedQuery {
    ParsedQuery::parse(query)
}

#[test]
fn query_without_operators_is_left_as_it_is() {
    let parsed = parse("как работает  поиск");

    assert_eq!(parsed.text, "как работает поиск");
    assert!(!parsed.has_operators());
    assert!(parsed.filter().is_empty());
}

#[test]
fn quoted_phrase_keeps_its_words_in_the_text_and_is_a_phrase_operator() {
    let parsed = parse("порог \"белый кот\" оценка");

    assert_eq!(parsed.text, "порог белый кот оценка");
    assert_eq!(parsed.phrases, ["белый кот"]);
    assert!(parsed.has_operators());
}

#[test]
fn minus_word_and_minus_phrase_leave_the_text() {
    let parsed = parse("кот -пёс -\"чёрный пёс\" дом");

    assert_eq!(parsed.text, "кот дом");
    assert_eq!(parsed.excluded_words, ["пёс"]);
    assert_eq!(parsed.excluded_phrases, ["чёрный пёс"]);
}

#[test]
fn path_prefixes_are_case_insensitive_and_may_be_quoted() {
    let parsed = parse("кот path:features/search ПАПКА:adr Path:\"my docs/a\"");

    assert_eq!(parsed.text, "кот");
    assert_eq!(parsed.paths, ["features/search", "adr", "my docs/a"]);
}

#[test]
fn trailing_slash_of_a_path_is_dropped_and_an_empty_path_is_a_plain_word() {
    assert_eq!(parse("кот path:adr/").paths, ["adr"]);
    let empty = parse("кот path:");
    assert!(empty.paths.is_empty());
    assert_eq!(empty.text, "кот path:");
}

#[test]
fn hyphens_digits_and_lone_quotes_are_plain_characters() {
    for query in [
        "-5",
        "по-русски",
        "274-ФЗ",
        "кот-пёс",
        "\"незакрытая кавычка",
        "слово\"",
        "кот' пёс",
        "- слово",
        "--слово",
        "-",
    ] {
        let parsed = parse(query);
        assert!(!parsed.has_operators(), "{query}: {parsed:?}");
        assert_eq!(parsed.text, query.split_whitespace().collect::<Vec<_>>().join(" "));
    }
}

#[test]
fn operators_are_recognised_only_at_the_start_of_a_word() {
    let parsed = parse("слово-пёс a\"b c\" xpath:dir");

    assert!(!parsed.has_operators(), "{parsed:?}");
}

#[test]
fn quoted_phrase_without_letters_is_ignored() {
    let parsed = parse("кот \"--\"");

    assert_eq!(parsed.text, "кот");
    assert!(parsed.phrases.is_empty());
}

#[test]
fn repeated_operators_are_listed_once() {
    let parsed = parse("-пёс -пёс \"а б\" \"а б\" path:x path:x");

    assert_eq!(parsed.excluded_words, ["пёс"]);
    assert_eq!(parsed.phrases, ["а б"]);
    assert_eq!(parsed.paths, ["x"]);
}

#[test]
fn only_exclusions_and_paths_leave_an_empty_text() {
    assert_eq!(parse("-пёс path:adr").text, "");
    assert_eq!(parse("-\"а б\"").text, "");
}

#[test]
fn phrase_becomes_an_exact_column_condition_and_exclusion_a_not_over_stem_and_exact() {
    let applied = parse("кот \"белый кот\" -пёс").filter().apply("X");

    assert!(
        applied.starts_with("(((X) AND (exact : \"белый кот\")) NOT ("),
        "{applied}"
    );
    assert!(applied.contains("{name heading_chain body} : ("), "{applied}");
    assert!(applied.contains(" OR exact : ("), "{applied}");
    assert!(applied.ends_with(")))"), "{applied}");
}

#[test]
fn filter_without_conditions_wraps_the_expression_and_nothing_else() {
    assert_eq!(parse("кот").filter().apply("X"), "(X)");
}
