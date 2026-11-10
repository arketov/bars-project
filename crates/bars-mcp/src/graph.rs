use super::*;
use bars_graph::{
    Analysis, Budgets, Direction, Endpoint as GraphEndpoint, GraphRequest, Grouping, ObjectRef,
    Operation, PathKind, Polarity, Predicate, Role, Scope, TaskProfile, View,
};
use std::collections::BTreeMap;

/// Время графовой операции в MCP по умолчанию, мс; воркер прижимает любое значение к тому же пределу (OQ-219).
const GRAPH_TIMEOUT_MS: u32 = 5000;

/// Схема без служебного шума: `default`, `uint32`, `minimum: 0` и `["x","null"]` не помогают
/// агенту, а шесть схем списка инструментов держат в 14 КБ (контракт ответа графа, 4.3.2).
fn slim(schema: &mut schemars::Schema) {
    fn walk(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("default");
                if map.get("format").is_some_and(|v| v == "uint32") {
                    map.remove("format");
                }
                if map.get("minimum").is_some_and(|v| v == 0) {
                    map.remove("minimum");
                }
                if let Some(serde_json::Value::Array(kinds)) = map.get("type") {
                    let rest: Vec<_> = kinds.iter().filter(|k| *k != "null").cloned().collect();
                    if rest.len() == 1 {
                        map.insert("type".into(), rest[0].clone());
                    }
                }
                map.values_mut().for_each(walk);
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    if let Some(map) = schema.as_object_mut() {
        let mut value = serde_json::Value::Object(std::mem::take(map));
        walk(&mut value);
        if let serde_json::Value::Object(slim) = value {
            *map = slim;
        }
    }
}

/// Вид ответа графовой операции.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Txt,
    Json,
}

/// `format` без отдельного определения Format: оно ещё раз повторяло бы описания в каждой схеме.
fn format_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "description": "По умолчанию `txt`.",
        "type": "string",
        "enum": ["txt", "json"]
    })
}

/// Объект графа как в другом ответе: `kind`, `id`, `revision` без изменений.
fn object_ref_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "description": "kind, id, revision из ответа.",
        "type": "object",
        "properties": {
            "kind": {"type": "string"},
            "id": {"type": "string"},
            "revision": {"type": "string"}
        },
        "required": ["kind", "id", "revision"]
    })
}

/// Массив объектов графа для `roots`.
fn object_refs_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "description": "Корни из ответа.",
        "type": "array",
        "items": object_ref_schema(generator)
    })
}

/// Список предикатов связи по именам, без определения с описаниями вариантов.
fn predicates_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "description": "Типы связей.",
        "type": "array",
        "items": {
            "type": "string",
            "enum": [
                "contains", "covers", "next_chunk", "links_to", "mentions", "depends_on", "uses",
                "implements", "verifies", "supersedes", "requires", "similar_to", "co_occurs",
                "member_of"
            ]
        }
    })
}

/// Структура аргументов одной операции: общие поля и поля операции. Своя структура на операцию даёт
/// схему без чужих полей (ISS-23); `deny_unknown_fields` отклоняет поле другой операции, а не глотает.
macro_rules! graph_args {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident: $ty:ty,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
        #[serde(default, deny_unknown_fields)]
        #[schemars(transform = slim)]
        pub struct $name {
            /// Проект; иначе проект сессии.
            pub project: Option<String>,
            #[schemars(schema_with = "format_schema")]
            pub format: Option<Format>,
            /// Папки первого уровня docs-root.
            pub modules: Vec<String>,
            /// Документы от docs-root.
            pub files: Vec<String>,
            /// Мс; умолчание и максимум 5000.
            pub timeout_ms: Option<u32>,
            $($(#[$fmeta])* pub $field: $ty,)*
        }
    };
}

graph_args!(RelationsArgs {
    /// Имя или вопрос; вместо `entity_ref`.
    query: Option<String>,
    #[schemars(schema_with = "object_ref_schema")]
    entity_ref: Option<ObjectRef>,
    #[schemars(schema_with = "predicates_schema")]
    predicates: Vec<Predicate>,
    /// Направление связей; по умолчанию `both`.
    direction: Direction,
    /// `positive`, `negative` или `any`: утверждение или «связи нет» в тексте.
    polarity: Option<Polarity>,
    /// Только с этими условиями: `condition`, `platform`, `version`, `kind`.
    qualifiers: BTreeMap<String, String>,
    /// Сколько фактов вернуть.
    limit: Option<u32>,
    /// `next_cursor` прошлого ответа.
    cursor: Option<String>,
});

graph_args!(PathsArgs {
    /// Начало: ObjectRef, {document:путь} или {label,kind?,namespace?}.
    from: Option<GraphEndpoint>,
    /// Конец пути в той же форме.
    to: Option<GraphEndpoint>,
    /// `semantic` (по умолчанию), `structural` или `mixed`.
    path_kind: PathKind,
    /// Не длиннее стольких шагов.
    max_hops: Option<u32>,
    /// Не больше стольких путей (максимум 20).
    max_paths: Option<u32>,
    #[schemars(schema_with = "predicates_schema")]
    predicates: Vec<Predicate>,
});

graph_args!(ImpactArgs {
    /// Документы от docs-root или проекта; до 20, вместе с roots.
    documents: Vec<String>,
    #[schemars(schema_with = "object_refs_schema")]
    roots: Vec<ObjectRef>,
    #[schemars(schema_with = "predicates_schema")]
    predicates: Vec<Predicate>,
    /// Глубина обхода зависимостей (по умолчанию 2, максимум 6).
    max_depth: Option<u32>,
    /// Сколько документов вернуть.
    limit: Option<u32>,
    /// `next_cursor` прошлого ответа.
    cursor: Option<String>,
});

graph_args!(ContextArgs {
    /// Документы (до 20); query ранжирует окрестность.
    documents: Vec<String>,
    #[schemars(schema_with = "object_refs_schema")]
    roots: Vec<ObjectRef>,
    /// Задача или вопрос на языке пользователя; обязателен.
    query: Option<String>,
    /// Профиль: `implementation`, `investigation` или `decision`.
    task_profile: Option<TaskProfile>,
    /// Только эти роли вместо набора профиля.
    roles: Vec<Role>,
    /// Размер ответа в токенах (оценка: токен — 3 байта).
    token_budget: Option<u32>,
});

graph_args!(ExploreArgs {
    /// Тема обзора; вместо `whole_scope`.
    topic: Option<String>,
    /// `true` — весь проект или выбранная область; вместо `topic`.
    whole_scope: bool,
    /// `topics`, `connectivity`, `coverage` или `conflict_candidates`.
    analysis: Analysis,
    /// Группировка тем: `communities` или `modules`.
    grouping: Grouping,
    /// Сколько элементов вернуть.
    limit: Option<u32>,
    /// `next_cursor` прошлого ответа.
    cursor: Option<String>,
});

graph_args!(GraphViewArgs {
    /// Документы локального графа; до 20, вместе с roots.
    documents: Vec<String>,
    /// Что показывать; по умолчанию `documents`.
    view: View,
    /// `true` — окрестность `roots`; иначе общий граф.
    local: bool,
    #[schemars(schema_with = "object_refs_schema")]
    roots: Vec<ObjectRef>,
    #[schemars(schema_with = "predicates_schema")]
    predicates: Vec<Predicate>,
    /// Направление связей от корней; по умолчанию `both`.
    direction: Direction,
    /// Глубина окрестности для `local` (по умолчанию 2, максимум 6).
    max_depth: Option<u32>,
    /// Добавить пути между найденными узлами.
    include_paths: bool,
    /// Сколько рёбер и узлов вернуть.
    limit: Option<u32>,
    /// `next_cursor` прошлого ответа.
    cursor: Option<String>,
});

/// Готовый вызов графовой операции: проект, формат и запрос воркеру.
pub(super) struct Call {
    project: Option<String>,
    format: Option<Format>,
    request: GraphRequest,
}

fn call(
    operation: Operation,
    project: Option<String>,
    format: Option<Format>,
    scope: Scope,
    timeout_ms: Option<u32>,
) -> Call {
    Call {
        project,
        format,
        request: GraphRequest {
            operation,
            scope,
            budgets: Budgets {
                timeout_ms: Some(timeout_ms.unwrap_or(GRAPH_TIMEOUT_MS)),
                ..Budgets::default()
            },
            ..GraphRequest::default()
        },
    }
}

impl RelationsArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Relations,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.query = self.query;
        r.entity_ref = self.entity_ref;
        r.predicates = self.predicates;
        r.direction = self.direction;
        r.polarity = self.polarity;
        r.qualifiers = self.qualifiers;
        r.limit = self.limit;
        r.cursor = self.cursor;
        c
    }
}

impl PathsArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Paths,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.from = self.from;
        r.to = self.to;
        r.path_kind = self.path_kind;
        r.max_hops = self.max_hops;
        r.max_paths = self.max_paths;
        r.predicates = self.predicates;
        c
    }
}

impl ImpactArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Impact,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.roots = self.roots;
        r.documents = self.documents;
        r.predicates = self.predicates;
        r.budgets.max_depth = self.max_depth;
        r.limit = self.limit;
        r.cursor = self.cursor;
        c
    }
}

impl ContextArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Context,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.query = self.query;
        r.documents = self.documents;
        r.roots = self.roots;
        r.task_profile = self.task_profile;
        r.roles = self.roles;
        r.token_budget = self.token_budget;
        c
    }
}

impl ExploreArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Explore,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.topic = self.topic;
        r.whole_scope = self.whole_scope;
        r.analysis = self.analysis;
        r.grouping = self.grouping;
        r.limit = self.limit;
        r.cursor = self.cursor;
        c
    }
}

impl GraphViewArgs {
    pub(super) fn into_call(self) -> Call {
        let scope = Scope {
            modules: self.modules,
            files: self.files,
        };
        let mut c = call(
            Operation::Graph,
            self.project,
            self.format,
            scope,
            self.timeout_ms,
        );
        let r = &mut c.request;
        r.view = self.view;
        r.local = self.local;
        r.roots = self.roots;
        r.documents = self.documents;
        r.predicates = self.predicates;
        r.direction = self.direction;
        r.budgets.max_depth = self.max_depth;
        r.include_paths = self.include_paths;
        r.limit = self.limit;
        r.cursor = self.cursor;
        c
    }
}

impl BarsMcp {
    pub(super) async fn graph_call(
        &self,
        original: observation::Original,
        call: Call,
    ) -> CallToolResult {
        let Call {
            project,
            format,
            request,
        } = call;
        let target = self.target(project);
        let result = self
            .call_observed(
                &bars_protocol::GraphQueryParams {
                    target,
                    request,
                    text: !matches!(format, Some(Format::Json)),
                },
                Some(&original.0),
            )
            .await;
        match format.unwrap_or_default() {
            Format::Txt => present(result, bars_protocol::graph_text::render_graph_text),
            Format::Json => present(result, json),
        }
    }
}
