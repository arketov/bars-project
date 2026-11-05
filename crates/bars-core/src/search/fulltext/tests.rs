use super::*;
use crate::indexing::chunking::{ChunkingParams, chunk_document};
use crate::storage::{DocumentRecord, Index, Journal};

/// Индекс из документов `(путь, название, исходный текст)` плюс пять документов-заполнителей без
/// искомых слов: у слова, которое есть в каждом документе, `bm25` не отличает чанки.
fn index_with(docs: &[(&str, &str, &str)]) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let mut index = Index::open(&dir.path().join("index.sqlite"), Journal::Delete).unwrap();
    let fillers: Vec<(String, String, String)> = (0..5)
        .map(|n| {
            (
                format!("docs/filler{n}.md"),
                format!("Заполнитель {n}"),
                format!("# Заполнитель {n}\n\nпустой текст номер {n} без значимых слов\n"),
            )
        })
        .collect();
    let all = docs
        .iter()
        .map(|(path, title, source)| (path.to_string(), title.to_string(), source.to_string()))
        .chain(fillers);
    for (path, title, source) in all {
        let doc = chunk_document(&path, &source, &ChunkingParams::default());
        index
            .replace_document(
                &DocumentRecord {
                    path: &path,
                    module: "m",
                    title: &title,
                    size: 1,
                    modified_ns: None,
                    hash: "h",
                    chunks: &doc.chunks,
                    text: None,
                },
                &[],
            )
            .unwrap();
    }
    (dir, index)
}

/// Пути документов по убыванию `bm25` для выражения FTS5.
fn paths(index: &Index, expression: &str) -> Vec<String> {
    let mut statement = index
        .conn()
        .prepare(&format!(
            "SELECT d.path FROM chunks_fts \
             JOIN chunks c ON c.id = chunks_fts.rowid \
             JOIN documents d ON d.id = c.document_id \
             WHERE chunks_fts MATCH ?1 ORDER BY {BM25} LIMIT 10"
        ))
        .unwrap();
    statement
        .query_map([expression], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn search_paths(index: &Index, query: &str) -> Vec<String> {
    let expression = fts_query_for(index.conn(), query).unwrap().unwrap().all;
    paths(index, &expression)
}

#[test]
fn query_without_words_has_no_expression() {
    assert_eq!(fts_query("?! --"), None);
    assert_eq!(fts_query(""), None);
}

#[test]
fn expression_searches_stems_in_three_columns_and_exact_forms_in_the_fourth() {
    let query = fts_query("Ёлка настройки").unwrap();
    assert_eq!(
        query.all,
        "{name heading_chain body} : (\"елк\" OR \"настройк\") \
         OR exact : (\"елка\" OR \"настройки\")"
    );
    assert_eq!(query.identifiers, None);
}

#[test]
fn identifiers_are_limited_to_words_with_inner_separators() {
    let query = fts_query("где agent_min_score.").unwrap();
    let identifiers = query.identifiers.unwrap();
    assert!(identifiers.contains("exact : (\"agent_min_score\")"));
    assert!(!identifiers.contains("где"));
    // Дефисное слово в идентификаторы не идёт, но его целая форма ищется в точной колонке.
    let hyphenated = fts_query("по-русски").unwrap();
    assert_eq!(hyphenated.identifiers, None);
    assert!(hyphenated.all.contains("\"по_русски\""));
}

#[test]
fn cjk_word_goes_as_bigrams_and_a_phrase() {
    let query = fts_query("向量检索").unwrap();
    assert!(query.all.contains("\"向量 量检 检索\""));
    assert!(query.all.contains("\"向量\" OR \"量检\" OR \"检索\""));
}

#[test]
fn edit_distance_counts_transposition_as_one_edit() {
    let d = |a: &str, b: &str| {
        edit_distance(
            &a.chars().collect::<Vec<_>>(),
            &b.chars().collect::<Vec<_>>(),
        )
    };
    assert_eq!(d("zebra", "zebra"), 0);
    assert_eq!(d("zebra", "zebar"), 1);
    assert_eq!(d("zebra", "zebr"), 1);
    assert_eq!(d("zebra", "zxbra"), 1);
    assert_eq!(d("abc", "xyz"), 3);
}

#[test]
fn title_and_file_name_outrank_a_single_mention_in_the_text() {
    let (_dir, index) = index_with(&[
        (
            "docs/a.md",
            "Токенизация",
            "# Токенизация\n\nпро прочее и разное\n",
        ),
        (
            "docs/b.md",
            "Прочее",
            "# Прочее\n\nздесь один раз сказано токенизация и всё\n",
        ),
        (
            "docs/hybrid-search.md",
            "Другое",
            "# Другое\n\nсовсем про иное\n",
        ),
    ]);
    let found = search_paths(&index, "токенизация");
    assert_eq!(found[0], "docs/a.md");
    assert!(found.contains(&"docs/b.md".to_string()));
    // Имя файла входит в колонку названия: слово из имени находит документ, где его нет в тексте.
    assert_eq!(search_paths(&index, "hybrid"), ["docs/hybrid-search.md"]);
}

#[test]
fn whole_identifier_form_finds_the_chunk_with_the_identifier() {
    let (_dir, index) = index_with(&[
        (
            "docs/a.md",
            "Настройки",
            "# Настройки\n\nпараметр zq_widget_limit задаёт предел\n",
        ),
        (
            "docs/b.md",
            "Другое",
            "# Другое\n\nwidget без предела и limit без виджета, zq отдельно\n",
        ),
    ]);
    let query = fts_query_for(index.conn(), "zq_widget_limit")
        .unwrap()
        .unwrap();
    assert_eq!(paths(&index, &query.identifiers.unwrap()), ["docs/a.md"]);
    assert_eq!(search_paths(&index, "zq_widget_limit")[0], "docs/a.md");
}

#[test]
fn a_word_missing_from_the_index_is_replaced_by_the_nearest_words() {
    let (_dir, index) = index_with(&[(
        "docs/a.md",
        "Животные",
        "# Животные\n\nzebra пасётся рядом с антилопой\n",
    )]);
    let query = fts_query_for(index.conn(), "zebar").unwrap().unwrap();
    assert!(query.all.contains("\"zebra\""), "{}", query.all);
    assert_eq!(search_paths(&index, "zebar")[0], "docs/a.md");
    // Слово, которое в индексе есть, не дополняется.
    let known = fts_query_for(index.conn(), "zebra").unwrap().unwrap();
    assert_eq!(
        known.all,
        fts_query("zebra").unwrap().all,
        "известное слово не исправляется"
    );
}

#[test]
fn short_words_and_words_with_digits_are_not_corrected() {
    let (_dir, index) = index_with(&[("docs/a.md", "Код", "# Код\n\nv2 abc\n")]);
    for word in ["abd", "v3", "ab12"] {
        assert_eq!(
            fts_query_for(index.conn(), word).unwrap().unwrap().all,
            fts_query(word).unwrap().all,
            "{word}"
        );
    }
}

#[test]
fn query_works_when_the_vocabulary_table_is_absent() {
    let (_dir, index) = index_with(&[("docs/a.md", "Звери", "# Звери\n\nzebra\n")]);
    index
        .conn()
        .execute("DROP TABLE chunks_fts_vocab", [])
        .unwrap();
    let query = fts_query_for(index.conn(), "zebar").unwrap().unwrap();
    assert_eq!(query.all, fts_query("zebar").unwrap().all);
}

#[test]
fn chinese_and_japanese_text_is_found_by_bigrams() {
    let (_dir, index) = index_with(&[
        ("docs/zh.md", "中文", "# 中文\n\n向量检索设置说明\n"),
        ("docs/ja.md", "日本語", "# 日本語\n\n検索の深さを設定する\n"),
        // Знак, стоящий отдельно, остаётся одним токеном и ищется одним знаком.
        ("docs/one.md", "Знак", "# Знак\n\nслово 猫 рядом\n"),
    ]);
    assert_eq!(search_paths(&index, "检索")[0], "docs/zh.md");
    assert_eq!(search_paths(&index, "向量检索")[0], "docs/zh.md");
    assert_eq!(search_paths(&index, "設定")[0], "docs/ja.md");
    assert_eq!(search_paths(&index, "猫"), ["docs/one.md"]);
}

#[test]
fn f_signal_divides_by_the_best_listed_score_and_scores_outsiders_by_rowid() {
    let (_dir, index) = index_with(&[
        (
            "docs/a.md",
            "Зебры",
            "# Зебры\n\nzebra zebra zebra рядом\n",
        ),
        ("docs/b.md", "Прочее", "# Прочее\n\nодна zebra здесь\n"),
        ("docs/c.md", "Пусто", "# Пусто\n\nничего подходящего\n"),
    ]);
    let expression = fts_query("zebra").unwrap().all;
    let rows: Vec<(i64, f32)> = {
        let mut statement = index
            .conn()
            .prepare(&format!(
                "SELECT rowid, {BM25} FROM chunks_fts WHERE chunks_fts MATCH ?1 ORDER BY {BM25}"
            ))
            .unwrap();
        statement
            .query_map([&expression], |row| {
                Ok((row.get::<_, i64>(0)?, raw_score(row.get(1)?)))
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    assert_eq!(rows.len(), 2);
    let (best, second) = (rows[0].0, rows[1].0);
    let nothing: i64 = index
        .conn()
        .query_row(
            "SELECT c.id FROM chunks c JOIN documents d ON d.id = c.document_id \
             WHERE d.path = 'docs/c.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    // В список попал только лучший; остальные — по `rowid`.
    let scores = f_signal(
        index.conn(),
        &expression,
        &rows[..1],
        &[best, second, nothing],
    )
    .unwrap();
    assert_eq!(scores[&best], 1.0);
    assert!(scores[&second] > 0.0 && scores[&second] < 1.0);
    assert!((scores[&second] - rows[1].1 / rows[0].1).abs() < 1e-6);
    assert_eq!(scores[&nothing], 0.0);
    // Пустой список FTS не даёт баллов никому.
    let none = f_signal(index.conn(), &expression, &[], &[best]).unwrap();
    assert_eq!(none[&best], 0.0);
}
