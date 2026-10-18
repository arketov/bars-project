//! Явно тестовые документы для браузерного макета окна; DTO создаёт настоящий query.
use bars_graph::{
    query::execute,
    source::{InputChunk, InputDocument, build},
    *,
};
fn main() {
    let inputs = [
        (
            "features/requirements.md",
            "# Требования\n\nТ-25 требует Т-26.\n\nТ-26 зависит от Т-27.\n\n[Архитектура](../architecture/worker.md)",
        ),
        (
            "architecture/worker.md",
            "# Архитектура\n\nТ-27 использует Локальный индекс.\n\n[Требования](../features/requirements.md)",
        ),
        (
            "features/models.md",
            "# Ограничения устройств\n\nЕсли выбран CPU, Т-26 не требует GPU.\n\n[Требования](requirements.md)",
        ),
        (
            "adr/0001-storage.md",
            "# Решение хранения\n\nТ-30 заменяет Т-31.\n\nПринято: локальное хранение.\n\n[Архитектура](../architecture/worker.md)",
        ),
        (
            "features/verification.md",
            "# Проверки\n\nПроверка согласия проверяет Т-25.\n\n[Требования](requirements.md)",
        ),
        (
            "features/behavior.md",
            "# Поведение\n\nТ-26 реализует Т-25.\n\n[Неизвестный документ](missing.md)",
        ),
    ];
    let data = build(
        "browser-fixture",
        "docs",
        inputs
            .iter()
            .map(|(path, text)| InputDocument {
                path: format!("docs/{path}"),
                docs_path: (*path).into(),
                module: path.split('/').next().unwrap().into(),
                title: path.rsplit('/').next().unwrap().into(),
                text: (*text).into(),
                chunks: vec![InputChunk {
                    key: "fixture-0".into(),
                    label: "Тестовый чанк".into(),
                    spans: std::iter::once(0..text.len()).collect(),
                }],
            })
            .collect(),
    )
    .unwrap();
    let entity = |label: &str| {
        data.nodes
            .iter()
            .find(|n| n.reference.kind == Kind::Entity && n.label == label)
            .unwrap()
            .reference
            .clone()
    };
    let mut replies = serde_json::Map::new();
    replies.insert(
        "__sources".into(),
        serde_json::to_value(
            data.sources
                .iter()
                .map(|s| (&s.meta.path, &s.text))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
        .unwrap(),
    );
    for view in [
        View::Documents,
        View::Sections,
        View::Chunks,
        View::Entities,
        View::Mixed,
    ] {
        let r = GraphRequest {
            operation: Operation::Graph,
            view,
            limit: Some(200),
            ..Default::default()
        };
        replies.insert(
            format!("graph:{view:?}").to_lowercase(),
            serde_json::to_value(execute(&data, &r, || false).unwrap()).unwrap(),
        );
    }
    for operation in [
        Operation::Relations,
        Operation::Paths,
        Operation::Impact,
        Operation::Context,
        Operation::Explore,
    ] {
        let r = GraphRequest {
            operation,
            view: View::Mixed,
            query: Some("Т-25 требует Т-26".into()),
            from: Some(Endpoint::Ref(entity("Т-25"))),
            to: Some(Endpoint::Ref(entity("Т-27"))),
            roots: vec![entity("Т-27")],
            whole_scope: true,
            ..Default::default()
        };
        replies.insert(
            format!("{operation:?}").to_lowercase(),
            serde_json::to_value(execute(&data, &r, || false).unwrap()).unwrap(),
        );
    }
    for analysis in [
        Analysis::Connectivity,
        Analysis::Coverage,
        Analysis::ConflictCandidates,
    ] {
        let r = GraphRequest {
            operation: Operation::Explore,
            whole_scope: true,
            analysis,
            ..Default::default()
        };
        replies.insert(
            format!("explore:{analysis:?}").to_lowercase(),
            serde_json::to_value(execute(&data, &r, || false).unwrap()).unwrap(),
        );
    }
    let output = serde_json::to_string(&replies).unwrap();
    if let Some(path) = std::env::args().nth(1) {
        std::fs::write(path, output).unwrap();
    } else {
        println!("{output}");
    }
}
