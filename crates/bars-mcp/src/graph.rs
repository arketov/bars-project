use super::*;
use bars_graph::{
    Analysis, Budgets, Direction, Endpoint as GraphEndpoint, GraphRequest, Grouping, ObjectRef,
    Operation, PathKind, Polarity, Predicate, Role, Scope, TaskProfile, View,
};
use std::collections::BTreeMap;

/// Время графовой операции в MCP по умолчанию, мс; воркер прижимает любое значение к тому же пределу (OQ-219).
const GRAPH_TIMEOUT_MS: u32 = 5000;

/// Структура аргументов одной операции: общие поля и поля операции. Своя структура на операцию даёт
/// схему без чужих полей (ISS-23); `deny_unknown_fields` отклоняет поле другой операции, а не глотает.
macro_rules! graph_args {
    ($(#[$meta:meta])* $name:ident { $($(#[$fmeta:meta])* $field:ident: $ty:ty,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Default, Deserialize, Serialize, schemars::JsonSchema)]
        #[serde(default, deny_unknown_fields)]
        pub struct $name {
            /// Проект: путь к корню или имя. Без него — проект сессии.
            pub project: Option<String>,
            /// Вид ответа: `txt` (по умолчанию) или `json`.
            pub format: Option<Format>,
            /// Только эти папки первого уровня docs-root (имена из `bars_projects`).
            pub modules: Vec<String>,
            /// Только эти документы, пути от docs-root.
            pub files: Vec<String>,
            /// Предел времени, мс (по умолчанию и максимум 5000); при срыве приходит найденное.
            pub timeout_ms: Option<u32>,
            $($(#[$fmeta])* pub $field: $ty,)*
        }
    };
}

graph_args!(RelationsArgs {
    /// Имя или вопрос; вместо `entity_ref`.
    query: Option<String>,
    /// Объект из другого ответа графа; вместо `query`.
    entity_ref: Option<ObjectRef>,
    /// Только эти предикаты связи.
    predicates: Vec<Predicate>,
    /// Какие связи объекта: `incoming`, `outgoing` или `both` (по умолчанию).
    direction: Direction,
    /// `positive`, `negative` или `any`: утверждение или «связи нет» в тексте.
    polarity: Option<Polarity>,
    /// Только с этими условиями: `condition`, `platform`, `version`, `kind`.
    qualifiers: BTreeMap<String, String>,
    /// Сколько фактов вернуть.
    limit: Option<u32>,
    /// Продолжение: `next_cursor` предыдущего ответа.
    cursor: Option<String>,
});

graph_args!(PathsArgs {
    /// Начало пути: объект из ответа графа или `{"label":"…"}` с необязательным `kind`.
    from: Option<GraphEndpoint>,
    /// Конец пути в той же форме.
    to: Option<GraphEndpoint>,
    /// `semantic` (по умолчанию), `structural` или `mixed`.
    path_kind: PathKind,
    /// Не длиннее стольких шагов.
    max_hops: Option<u32>,
    /// Не больше стольких путей (максимум 20).
    max_paths: Option<u32>,
    /// Только эти предикаты связи.
    predicates: Vec<Predicate>,
});

graph_args!(ImpactArgs {
    /// Объекты из ответа графа, чьё изменение проверяется; обязательны.
    roots: Vec<ObjectRef>,
    /// Только эти предикаты связи.
    predicates: Vec<Predicate>,
    /// Глубина обхода зависимостей (по умолчанию 2, максимум 6).
    max_depth: Option<u32>,
    /// Сколько документов вернуть.
    limit: Option<u32>,
    /// Продолжение: `next_cursor` предыдущего ответа.
    cursor: Option<String>,
});

graph_args!(ContextArgs {
    /// Задача или вопрос на языке пользователя; обязателен.
    query: Option<String>,
    /// `implementation`, `investigation` или `decision`: какие роли нужны задаче.
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
    /// Продолжение: `next_cursor` предыдущего ответа.
    cursor: Option<String>,
});

graph_args!(GraphViewArgs {
    /// Что показывать: `documents`, `sections`, `chunks`, `entities` или `mixed`.
    view: View,
    /// `true` — окрестность `roots`; без него — общий граф в пределах бюджета.
    local: bool,
    /// Объекты из ответа графа для `local`.
    roots: Vec<ObjectRef>,
    /// Только эти предикаты связи.
    predicates: Vec<Predicate>,
    /// Какие связи от корней: `incoming`, `outgoing` или `both`.
    direction: Direction,
    /// Глубина окрестности для `local` (по умолчанию 2, максимум 6).
    max_depth: Option<u32>,
    /// Добавить пути между найденными узлами.
    include_paths: bool,
    /// Сколько рёбер и узлов вернуть.
    limit: Option<u32>,
    /// Продолжение: `next_cursor` предыдущего ответа.
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
        finish(
            self.call_observed(
                &bars_protocol::GraphQueryParams { target, request },
                Some(&original.0),
            )
            .await,
            format,
            |out| {
                format!(
                    "=== GRAPH: {:?} ===\n{}\n=== END GRAPH ===",
                    out.operation,
                    json(out)
                )
            },
        )
    }
}
