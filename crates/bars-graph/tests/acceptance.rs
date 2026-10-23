use bars_graph::*;
use bars_graph::{
    query::execute,
    source::{InputDocument, build},
};

fn doc(path: &str, module: &str, text: &str) -> InputDocument {
    InputDocument {
        path: format!("docs/{path}"),
        docs_path: path.into(),
        module: module.into(),
        title: path.into(),
        text: text.into(),
        chunks: vec![],
    }
}
fn graph(text: &str) -> Dataset {
    build(
        "fixture",
        "docs",
        vec![doc("requirements.md", "features", text)],
    )
    .unwrap()
}

#[test]
fn uncertain_operands_abstain_without_losing_source_evidence() {
    for sentence in [
        "Она сама проверяет актуальность по проектам.",
        "Он не заменяет изолированную приёмку скорости.",
        "Точный перебор ничего не теряет и не требует обслуживать индекс.",
        "Панель логов не зависит от успешной загрузки настроек и не требует сохранения.",
        "У модели fastembed метод `embed требует исключительного доступа.",
        "`Worker` проверяет `Index` при обновлении.",
    ] {
        let data = graph(&format!("# Проверка\n\n{sentence}"));
        assert!(
            data.edges
                .iter()
                .all(|edge| edge.origin != Origin::ExplicitRule),
            "{sentence}"
        );
        assert!(
            data.evidence
                .iter()
                .any(|proof| proof.quote.contains(sentence)),
            "источник остаётся доступен: {sentence}"
        );
    }
    let data = graph(
        "# Проверка\n\n`Worker` проверяет `Index`.\n\n`Worker` не использует `Remote`, если отключена сеть.",
    );
    assert_eq!(
        data.edges
            .iter()
            .filter(|edge| edge.origin == Origin::ExplicitRule)
            .count(),
        2
    );
}

#[test]
fn context_profiles_select_roles_and_reject_silent_ignored_inputs() {
    let data = graph(
        "# Требование\n\nНужен индекс.\n\n# Архитектура\n\nИндекс хранит документы.\n\n# Ограничения\n\nИндекс локальный.",
    );
    let mut r = request(Operation::Context);
    r.query = Some("индекс".into());
    r.task_profile = Some(TaskProfile::Decision);
    let out = run(&data, &r);
    assert!(out.items.iter().all(|item| matches!(
        item,
        Item::RoleGroup {
            role: Role::Requirement | Role::Decision | Role::Constraint,
            ..
        }
    )));
    assert!(!out.items.is_empty());
    r.roles = vec![Role::Architecture];
    assert!(matches!(
        execute(&data, &r, || false),
        Err(GraphError::InvalidParams(_))
    ));
    r.roles.clear();
    r.roots = vec![data.nodes[0].reference.clone()];
    assert!(matches!(
        execute(&data, &r, || false),
        Err(GraphError::CapabilityUnavailable(_))
    ));
    r.roots.clear();
    r.operation = Operation::Graph;
    assert!(matches!(
        execute(&data, &r, || false),
        Err(GraphError::InvalidParams(_))
    ));
}

#[test]
fn context_budget_trims_extra_proofs_before_dropping_roles() {
    let mut text = String::new();
    for heading in ["Требование", "Архитектура", "Ограничения"] {
        text.push_str(&format!("# {heading}\n\n"));
        for n in 0..8 {
            text.push_str(&format!(
                "Индекс {n}: {}\n\n",
                "проверяемые подробности индекса ".repeat(12)
            ));
        }
    }
    let data = graph(&text);
    let mut r = request(Operation::Context);
    r.query = Some("индекс".into());
    r.roles = vec![Role::Requirement, Role::Architecture, Role::Constraint];
    r.budgets.max_nodes = Some(2000);
    let full = run(&data, &r);
    assert_eq!(full.items.len(), 3);
    r.budgets.max_bytes = Some((full.used_bytes - 1000) as u32);
    let bounded = run(&data, &r);
    assert_eq!(
        bounded.items.len(),
        full.items.len(),
        "сокращение extras сохраняет роли"
    );
    assert!(bounded.truncated);
    assert!(bounded.missing_roles.is_empty());
    assert!(bounded.next_cursor.is_none());
    assert!(serde_json::to_vec(&bounded).unwrap().len() <= r.budgets.max_bytes.unwrap() as usize);
    for proof in bounded.evidence {
        let source = data
            .sources
            .iter()
            .find(|source| source.meta.reference == proof.document)
            .unwrap();
        assert_eq!(&source.text[proof.span.start..proof.span.end], proof.quote);
    }
}
fn reference(data: &Dataset, label: &str) -> ObjectRef {
    data.nodes
        .iter()
        .find(|n| n.reference.kind == Kind::Entity && n.label == label)
        .unwrap()
        .reference
        .clone()
}
fn request(operation: Operation) -> GraphRequest {
    GraphRequest {
        operation,
        ..Default::default()
    }
}
fn run(data: &Dataset, r: &GraphRequest) -> GraphReply {
    execute(data, r, || false).unwrap()
}

#[test]
fn document_projection_keeps_all_evidence_and_first_page_has_edges() {
    let data = build(
        "fixture",
        "docs",
        vec![
            doc("a.md", ".", "# A\n\n[Один](b.md#b) и [другой](b.md#c)."),
            doc("b.md", ".", "# B\n\n## C\nТекст"),
        ],
    )
    .unwrap();
    let mut r = request(Operation::Graph);
    r.view = View::Documents;
    r.limit = Some(1);
    let out = run(&data, &r);
    let edge = out
        .assertions
        .iter()
        .find(|e| e.predicate == Predicate::LinksTo)
        .unwrap();
    assert_eq!(edge.evidence.len(), 2);
    assert_eq!(out.nodes.len(), 2);
    assert_eq!(out.layer_states["structure"], LayerState::Ready);
    assert_eq!(out.layer_states["semantic"], LayerState::Partial);
}

#[test]
fn unrelated_edges_do_not_exhaust_path_budget() {
    let mut text = String::from("# Связи\n\n`A` зависит от `B`.\n\n`B` зависит от `C`.\n\n");
    for i in 0..100 {
        text.push_str(&format!("`X{i}` зависит от `Y{i}`.\n\n"));
    }
    let data = graph(&text);
    let mut r = request(Operation::Paths);
    r.from = Some(Endpoint::Ref(reference(&data, "A")));
    r.to = Some(Endpoint::Ref(reference(&data, "C")));
    r.direction = Direction::Outgoing;
    r.budgets.max_edges = Some(20);
    let out = run(&data, &r);
    assert!(
        out.items
            .iter()
            .any(|i| matches!(i,Item::Path{path} if path.steps.len()==2))
    );
}

#[test]
fn expensive_read_operations_observe_cancellation_and_work_limits() {
    let data = graph("# Правило\n\n`A` использует `B`.\n\n`A` не использует `B`.");
    for operation in [Operation::Graph, Operation::Context, Operation::Explore] {
        let mut r = request(operation);
        r.query = Some("Правило".into());
        r.whole_scope = true;
        assert!(matches!(
            execute(&data, &r, || true),
            Err(GraphError::Cancelled)
        ));
    }
}

#[test]
fn communities_separate_topics_inside_one_module_and_recompute_under_scope() {
    let data = build(
        "fixture",
        "docs",
        vec![
            doc("a.md", "features", "# Индекс\n\n[Индексация](b.md)"),
            doc("b.md", "features", "# Нарезка\n\n[Индекс](a.md)"),
            doc("c.md", "features", "# Цвета\n\n[Палитра](d.md)"),
            doc("d.md", "features", "# Палитра\n\n[Цвета](c.md)"),
        ],
    )
    .unwrap();
    let mut r = request(Operation::Explore);
    r.whole_scope = true;
    let out = run(&data, &r);
    assert_eq!(out.items.len(), 2);
    r.budgets.max_edges = Some(1);
    let limited = run(&data, &r);
    assert!(!limited.items.is_empty());
    assert!(limited.truncated);
    r.budgets = Budgets::default();
    assert!(
        out.items
            .iter()
            .all(|i| matches!(i,Item::Topic{members,..} if members.len()==2))
    );
    let again = run(&data, &r);
    assert_eq!(
        serde_json::to_value(&out.items).unwrap(),
        serde_json::to_value(&again.items).unwrap()
    );
    r.grouping = Grouping::Modules;
    assert_eq!(run(&data, &r).items.len(), 1);
    r.grouping = Grouping::Communities;
    r.scope.files = vec!["a.md".into(), "c.md".into()];
    let scoped = run(&data, &r);
    assert_eq!(scoped.items.len(), 2);
    assert!(
        scoped
            .items
            .iter()
            .all(|i| matches!(i,Item::Topic{members,..} if members.len()==1))
    );
    assert!(
        scoped
            .sources
            .iter()
            .all(|s| s.docs_path == "a.md" || s.docs_path == "c.md")
    );
    let next = build(
        "fixture",
        "docs",
        vec![
            doc("a.md", "features", "# Индекс"),
            doc("c.md", "features", "# Цвета"),
        ],
    )
    .unwrap();
    r.scope = Scope::default();
    let changed = run(&next, &r);
    assert_eq!(changed.items.len(), 2);
    assert_ne!(out.snapshot, changed.snapshot);
}

#[test]
fn parsing_cancellation_stops_before_publishing_any_snapshot() {
    let mut calls = 0;
    let result = source::build_cancellable(
        "fixture",
        "docs",
        vec![doc("a.md", ".", &"# Раздел\n\nТекст.\n\n".repeat(100))],
        || {
            calls += 1;
            calls > 10
        },
    );
    assert!(matches!(result, Err(GraphError::Cancelled)));
    assert!(calls <= 11);
}

#[test]
fn real_pronoun_clause_is_not_a_reversed_verification_fact() {
    let data = graph(
        "# Чтение\n\n**Корень документации** при чтении не проверяется: его проверяет обход файлов ([updates.md](updates.md)).",
    );
    assert!(
        data.edges
            .iter()
            .all(|e| e.predicate != Predicate::Verifies)
    );
    let data = graph(
        "# Устройства\n\n`Модель` использует `Индекс`. Если выбран CPU, `Модель` не требует `GPU`.",
    );
    assert_eq!(
        data.edges.iter().filter(|e| e.predicate.semantic()).count(),
        2
    );
    assert!(
        data.edges
            .iter()
            .any(|e| e.predicate == Predicate::DependsOn
                && e.qualifiers.get("kind").map(String::as_str) == Some("precondition")
                && e.polarity == Polarity::Negative
                && e.qualifiers.contains_key("condition"))
    );
    for ev in &data.evidence {
        assert_eq!(
            data.sources[0].text.get(ev.span.start..ev.span.end),
            Some(ev.quote.as_str())
        );
    }
}

#[test]
fn cr_frontmatter_is_not_a_semantic_fact_and_terminal_newline_is_not_next_line() {
    let data = graph("\u{feff}---\rtitle: A использует B\r---\r# Правило\r\r`C` использует `D`.\r");
    let edges: Vec<_> = data
        .edges
        .iter()
        .filter(|e| e.predicate == Predicate::Uses)
        .collect();
    assert_eq!(edges.len(), 1);
    let ev = data
        .evidence
        .iter()
        .find(|ev| edges[0].evidence.contains(&ev.id))
        .unwrap();
    assert_eq!(ev.span.first_line, 6);
    assert_eq!(ev.span.last_line, 6);
}

#[test]
fn s1_positive_negative_conditional_impact() {
    for (case, text) in [
        "# Правило\n\n`A` зависит от `B`.",
        "# Правило\n\n`A` не зависит от `B`.",
        "# Правило\n\nЕсли выбран CPU, `A` зависит от `B`.",
    ]
    .iter()
    .enumerate()
    {
        let data = graph(text);
        let mut r = request(Operation::Impact);
        r.roots = vec![reference(&data, "B")];
        let out = run(&data, &r);
        assert_eq!(
            out.items.iter().any(|i| matches!(i, Item::Impact { .. })),
            case != 1
        );
        if case == 2 {
            assert!(
                out.assertions
                    .iter()
                    .any(|e| e.qualifiers.contains_key("condition"))
            );
        }
        assert!(
            out.assertions
                .iter()
                .all(|e| e.predicate != Predicate::LinksTo)
        );
    }
}
#[test]
fn s2_positive_negative_conditional_paths() {
    for (case, text) in [
        "# Связь\n\n`A` использует `B`.",
        "# Связь\n\n`A` не использует `B`.",
        "# Связь\n\nЕсли выбран CPU, `A` использует `B`.",
    ]
    .iter()
    .enumerate()
    {
        let data = graph(text);
        let mut r = request(Operation::Paths);
        r.from = Some(Endpoint::Ref(reference(&data, "A")));
        r.to = Some(Endpoint::Ref(reference(&data, "B")));
        r.polarity = Some(Polarity::Positive);
        if case == 2 {
            r.qualifiers
                .insert("condition".into(), "другая платформа".into());
        }
        let out = run(&data, &r);
        assert_eq!(out.items.len(), usize::from(case == 0));
        if case == 0 {
            assert!(!out.evidence.is_empty());
            assert!(matches!(&out.items[0],Item::Path{path} if path.complete&&path.steps.len()==1));
        }
    }
}
#[test]
fn s3_positive_negative_budgeted_context() {
    for case in 0..3 {
        let data = build(
            "fixture",
            "docs",
            vec![doc(
                if case == 1 {
                    "notes.md"
                } else {
                    "requirements.md"
                },
                "features",
                "# Запуск\n\nЗапуск использует локальный индекс.",
            )],
        )
        .unwrap();
        let mut r = request(Operation::Context);
        r.query = Some("Запуск индекс".into());
        r.roles = vec![Role::Requirement];
        if case == 2 {
            r.budgets.max_bytes = Some(2000);
        }
        match execute(&data, &r, || false) {
            Ok(out) => {
                assert_eq!(out.budget_mode, "estimated");
                assert!(out.used_bytes <= out.applied.max_bytes as usize);
                if case == 1 {
                    assert_eq!(out.missing_roles, vec![Role::Requirement]);
                }
                if case == 0 {
                    assert!(!out.items.is_empty());
                }
            }
            Err(GraphError::InvalidParams(_)) if case == 2 => {}
            other => panic!("unexpected: {other:?}"),
        }
    }
}
#[test]
fn s4_positive_negative_and_condition_are_not_interchangeable() {
    for case in 0..3 {
        let text = if case == 0 {
            "# Устройства\n\n`A` требует `GPU`."
        } else if case == 1 {
            "# Устройства\n\n`A` не требует `GPU`."
        } else {
            "# Устройства\n\nЕсли выбран CPU, `A` не требует `GPU`."
        };
        let data = graph(text);
        let mut r = request(Operation::Relations);
        r.entity_ref = Some(reference(&data, "A"));
        r.polarity = Some(if case == 0 {
            Polarity::Positive
        } else {
            Polarity::Negative
        });
        let out = run(&data, &r);
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.assertions[0].polarity, r.polarity.unwrap());
        assert_eq!(
            out.assertions[0].qualifiers.contains_key("condition"),
            case == 2
        );
    }
}
#[test]
fn s5_explicit_supersession_only_not_newer_date() {
    for case in 0..3 {
        let text = if case == 0 {
            "# Решение\n\n`Новое` заменяет `Старое`."
        } else if case == 1 {
            "# Решение\n\nНовое решение датировано позже старого."
        } else {
            "# Решение\n\nЕсли старое отключено, `Новое` заменяет `Старое`."
        };
        let data = graph(text);
        let mut r = request(Operation::Relations);
        r.query = Some("решение Новое Старое".into());
        r.predicates = vec![Predicate::Supersedes];
        let out = run(&data, &r);
        assert_eq!(out.items.len(), usize::from(case != 1));
        if case == 2 {
            assert!(!out.assertions[0].qualifiers.is_empty());
        }
    }
}
#[test]
fn s6_whole_scope_empty_topic_and_filtered_topics() {
    let data = build(
        "fixture",
        "docs",
        vec![
            doc("a.md", "models", "# Скорость\nМодель"),
            doc("b.md", "index", "# Индексация\nФайлы"),
            doc("c.md", "ui", "# Окно\nКнопки"),
        ],
    )
    .unwrap();
    for case in 0..3 {
        let mut r = request(Operation::Explore);
        r.whole_scope = case != 1;
        r.topic = if case == 1 {
            Some("несуществующая_тема".into())
        } else {
            None
        };
        if case == 2 {
            r.scope.modules = vec!["ui".into()];
        }
        let out = run(&data, &r);
        assert_eq!(out.items.len(), [3, 0, 1][case]);
        assert!(out.method.contains("communities"));
        if case == 2 {
            assert!(out.sources.iter().all(|s| s.module == "ui"));
        }
    }
}
#[test]
fn s7_two_hops_no_path_and_limited_chain() {
    let data = graph("# Зависимости\n\n`A` зависит от `B`.\n\n`B` зависит от `C`.");
    for case in 0..3 {
        let mut r = request(Operation::Paths);
        r.from = Some(Endpoint::Ref(reference(&data, "A")));
        r.to = Some(Endpoint::Ref(reference(&data, "C")));
        r.direction = Direction::Outgoing;
        if case == 1 {
            r.predicates = vec![Predicate::Verifies];
        }
        if case == 2 {
            r.max_hops = Some(1);
        }
        let out = run(&data, &r);
        if case == 0 {
            assert!(matches!(&out.items[0],Item::Path{path} if path.complete&&path.steps.len()==2));
            assert_eq!(out.assertions.len(), 2);
        } else {
            assert!(out.items.is_empty());
        }
        if case == 2 {
            assert!(out.truncated);
            assert!(out.limit_reasons.contains(&"max_depth".into()));
        }
    }
}
#[test]
fn s8_structural_defect_valid_link_and_partial_semantic_audit() {
    for case in 0..3 {
        let data = build(
            "fixture",
            "docs",
            vec![
                doc(
                    "a.md",
                    "features",
                    if case == 0 {
                        "# A\n\n[нет](missing.md)"
                    } else if case == 1 {
                        "# A\n\n[да](b.md)"
                    } else {
                        "# A\n\n`A` использует `B`."
                    },
                ),
                doc("b.md", "features", "# B"),
            ],
        )
        .unwrap();
        let mut r = request(Operation::Explore);
        r.whole_scope = true;
        r.analysis = if case == 2 {
            Analysis::Coverage
        } else {
            Analysis::Connectivity
        };
        let out = run(&data, &r);
        if case == 0 {
            assert!(
                out.items
                    .iter()
                    .any(|i| matches!(i,Item::Audit{kind,..} if kind=="target_not_indexed"))
            );
        }
        if case == 1 {
            assert!(out.items.is_empty());
        }
        if case == 2 {
            assert_eq!(out.layer_state, LayerState::Partial);
            assert!(out.coverage.semantic_completeness.is_none());
            assert!(
                out.items
                    .iter()
                    .all(|i| matches!(i,Item::Audit{detail,..} if detail.contains("не доказано")))
            );
        }
    }
}

#[test]
fn refs_scope_ambiguity_cursor_and_envelope_are_checked() {
    let data = build(
        "fixture",
        "docs",
        vec![
            doc("a.md", "a", "# A\n\n`X` использует `Y`."),
            doc("b.md", "b", "# B\n\n`X` использует `Z`."),
        ],
    )
    .unwrap();
    let mut r = request(Operation::Relations);
    r.query = Some("X".into());
    assert_eq!(run(&data, &r).resolution, "ambiguous");
    r.scope.files = vec!["a.md".into()];
    let out = run(&data, &r);
    assert!(out.sources.iter().all(|s| s.docs_path == "a.md"));
    assert!(!serde_json::to_string(&out).unwrap().contains("b.md"));
    let mut g = request(Operation::Graph);
    g.view = View::Mixed;
    g.limit = Some(1);
    let first = run(&data, &g);
    g.cursor = first.next_cursor.clone();
    assert!(g.cursor.is_some());
    g.limit = Some(2);
    assert!(matches!(
        execute(&data, &g, || false),
        Err(GraphError::CursorMismatch)
    ));
    g.cursor = None;
    g.snapshot = Some("old".into());
    assert!(matches!(
        execute(&data, &g, || false),
        Err(GraphError::SnapshotExpired)
    ));
    let mut stale = reference(&data, "Y");
    stale.revision = "old".into();
    r.query = None;
    r.entity_ref = Some(stale);
    assert!(matches!(
        execute(&data, &r, || false),
        Err(GraphError::StaleRef(_))
    ));
    let mut small = request(Operation::Graph);
    small.view = View::Mixed;
    small.budgets.max_bytes = Some(4200);
    let out = run(&data, &small);
    assert!(serde_json::to_vec(&out).unwrap().len() <= 4200);
    assert!(out.truncated);
    assert!(matches!(
        execute(&data, &request(Operation::Graph), || true),
        Err(GraphError::Cancelled)
    ));
}
