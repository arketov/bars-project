//! Методы протокола: параметры и ответы (`docs/architecture/protocol.md`).

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{INVALID_PARAMS, METHOD_NOT_FOUND, RpcError};
use crate::jsonrpc::{Id, Request, empty_params};

/// Кто подключился к воркеру.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Mcp,
    Cli,
}

/// Метод протокола: имя, тип ответа и кому он доступен.
pub trait Method: Serialize + DeserializeOwned {
    const NAME: &'static str;
    /// Доступен ли метод клиенту `bars-mcp`. Остальные методы — только для `bars-cli`.
    const FROM_MCP: bool;
    type Output: Serialize + DeserializeOwned;

    /// Вызов с этими параметрами.
    fn request(&self, id: Id) -> Request {
        let params = serde_json::to_value(self).expect("параметры метода сериализуются в JSON");
        Request::new(id, Self::NAME, params)
    }
}

/// Как метод находит проект: рабочая папка клиента и необязательный явный проект.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub cwd: String,
    /// Абсолютный путь к корню проекта или однозначное имя.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

// --- hello ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloParams {
    pub protocol: u32,
    pub client: ClientKind,
    pub client_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloOutput {
    pub protocol: u32,
    pub worker_version: String,
}

// --- search ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchParams {
    #[serde(flatten)]
    pub target: Target,
    pub query: String,
    /// Модули (корневой — `.`), по которым искать; пусто — весь проект.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    /// Файлы относительно корня документации.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidates: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_score: Option<f32>,
}

/// Вид оценки результата: к нему применяется порог.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreKind {
    Rerank,
    Cosine,
}

/// Один результат поиска (Т-13).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchHit {
    /// Путь документа от корня проекта.
    pub path: String,
    pub module: String,
    pub title: String,
    /// Цепочка заголовков в виде `Заголовок 1/Заголовок 2`.
    pub heading_chain: String,
    pub first_line: u32,
    pub last_line: u32,
    pub text: String,
    pub score: f32,
    pub score_kind: ScoreKind,
}

/// Параметры, с которыми поиск выполнен на самом деле (Т-24).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppliedParams {
    pub results: u32,
    pub candidates: u32,
    pub rerank_candidates: u32,
    pub min_score: f32,
    pub score_kind: ScoreKind,
    /// Хотя бы одно значение из вызова вышло за пределы и прижато.
    pub clamped: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchOutput {
    pub results: Vec<SearchHit>,
    pub applied: AppliedParams,
    /// Проект индексируется: выдача может быть неполной (Т-19).
    pub indexing: bool,
    /// Сколько документов ждёт индексации.
    pub pending: u32,
    /// Причина, по которой rerank включён, но выдача вернулась без него.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rerank_error: Option<String>,
}

// --- outline ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineParams {
    #[serde(flatten)]
    pub target: Target,
    pub path: String,
    /// Уровни заголовков 1…6; пусто — все.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub levels: Vec<u8>,
    /// Цепочка заголовков `Заголовок 1/Заголовок 2`: вернуть только вложенные.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under: Option<String>,
    /// Номер строки заголовка вместо цепочки.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub under_line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub line: u32,
    pub level: u8,
    pub text: String,
}

impl Heading {
    /// Строка в том виде, в каком её печатают `bars-mcp` и `bars-cli`: `7 ## Стадия`.
    pub fn to_line(&self) -> String {
        format!(
            "{} {} {}",
            self.line,
            "#".repeat(usize::from(self.level)),
            self.text
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineOutput {
    pub headings: Vec<Heading>,
}

// --- read ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadParams {
    #[serde(flatten)]
    pub target: Target,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadOutput {
    pub title: String,
    /// Текст документа как есть; номера строк клиенты ставят одной функцией `number_lines`.
    pub text: String,
}

// --- projects, status ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectsParams {}

/// Состояние проекта в воркере.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectState {
    Ready,
    Indexing,
    ReindexRequired,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModuleInfo {
    /// Имя подпапки; корневой модуль — `.`.
    pub name: String,
    pub documents: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectInfo {
    pub path: String,
    pub name: String,
    pub docs_root: String,
    pub state: ProjectState,
    pub modules: Vec<ModuleInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectsOutput {
    pub projects: Vec<ProjectInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusParams {
    #[serde(flatten)]
    pub target: Target,
}

/// Отпечатки моделей: индекса и настроек. Разные — нужна переиндексация (Т-25).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelStatus {
    /// `None`, пока индекс пуст.
    pub index: Option<String>,
    pub settings: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusOutput {
    pub path: String,
    pub state: ProjectState,
    /// Время последней завершённой проверки, RFC 3339.
    pub updated_at: Option<String>,
    /// Сколько документов ждёт индексации.
    pub pending: u32,
    pub model: ModelStatus,
    /// Последняя ошибка провайдера или индексации, без ключей.
    pub last_error: Option<String>,
    /// Работает ли слежение за файлами.
    pub watching: bool,
}

// --- reindex ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexParams {
    #[serde(flatten)]
    pub target: Target,
    /// Полная переиндексация вместо проверки актуальности.
    #[serde(default)]
    pub full: bool,
    /// Согласие человека на переиндексацию после смены модели (Т-25). От `bars-mcp` не принимается.
    #[serde(default)]
    pub accept_model_change: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcceptedOutput {
    pub accepted: bool,
}

// --- register, unregister, session_start ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterParams {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterOutput {
    pub project: ProjectInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnregisterParams {
    #[serde(flatten)]
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnregisterOutput {
    pub removed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionStartParams {
    pub cwd: String,
    /// Откуда началась сессия: `startup`, `resume`, `clear` и так далее — как сообщил клиент.
    pub source: String,
}

// --- install, uninstall ---

/// Клиент, в который bars устанавливается.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallClient {
    #[serde(rename = "claude-code")]
    ClaudeCode,
    #[serde(rename = "codex")]
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallParams {
    pub client: InstallClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallOutput {
    pub changed_files: Vec<String>,
    /// Хук нужно одобрить в самом клиенте (Codex).
    pub needs_hook_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallParams {
    pub client: InstallClient,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UninstallOutput {
    pub changed_files: Vec<String>,
}

macro_rules! methods {
    ($($variant:ident: $params:ty => $output:ty, $name:literal, $mcp:literal;)*) => {
        $(impl Method for $params {
            const NAME: &'static str = $name;
            const FROM_MCP: bool = $mcp;
            type Output = $output;
        })*

        /// Разобранный вызов: по одному варианту на метод.
        #[derive(Debug, Clone, PartialEq)]
        pub enum Call {
            $($variant($params),)*
        }

        impl Call {
            /// Разбирает параметры по имени метода. Неизвестный метод — `-32601`, параметры, которые
            /// не разбираются, — `-32602`.
            pub fn from_request(request: &Request) -> Result<Call, RpcError> {
                let params = request.params.clone().unwrap_or_else(empty_params);
                match request.method.as_str() {
                    $(<$params as Method>::NAME => serde_json::from_value(params)
                        .map(Call::$variant)
                        .map_err(|error| {
                            RpcError::standard(
                                INVALID_PARAMS,
                                format!("параметры метода {}: {error}", <$params as Method>::NAME),
                            )
                        }),)*
                    other => Err(RpcError::standard(
                        METHOD_NOT_FOUND,
                        format!("нет метода {other}"),
                    )),
                }
            }

            pub fn method(&self) -> &'static str {
                match self {
                    $(Call::$variant(_) => <$params as Method>::NAME,)*
                }
            }

            /// Доступен ли вызов клиенту такого рода.
            pub fn allowed_for(&self, client: ClientKind) -> bool {
                match self {
                    $(Call::$variant(_) => client == ClientKind::Cli || <$params as Method>::FROM_MCP,)*
                }
            }
        }
    };
}

methods! {
    Hello: HelloParams => HelloOutput, "hello", true;
    Search: SearchParams => SearchOutput, "search", true;
    Outline: OutlineParams => OutlineOutput, "outline", true;
    Read: ReadParams => ReadOutput, "read", true;
    Projects: ProjectsParams => ProjectsOutput, "projects", true;
    Status: StatusParams => StatusOutput, "status", true;
    Reindex: ReindexParams => AcceptedOutput, "reindex", true;
    Register: RegisterParams => RegisterOutput, "register", false;
    Unregister: UnregisterParams => UnregisterOutput, "unregister", false;
    SessionStart: SessionStartParams => AcceptedOutput, "session_start", false;
    Install: InstallParams => InstallOutput, "install", false;
    Uninstall: UninstallParams => UninstallOutput, "uninstall", false;
}

/// Ответ метода как JSON-значение.
pub fn output_value<T: Serialize>(output: &T) -> Value {
    serde_json::to_value(output).expect("ответы методов сериализуются в JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target() -> Target {
        Target {
            cwd: "C:\\work\\proj".to_string(),
            project: None,
        }
    }

    fn roundtrip<M: Method + PartialEq + std::fmt::Debug + Clone>(params: M) -> Call
    where
        Call: From<M>,
    {
        let request = params.request(Id::Number(1));
        assert_eq!(request.method, M::NAME);
        let call = Call::from_request(&request).unwrap();
        assert_eq!(call, Call::from(params));
        call
    }

    macro_rules! from_params {
        ($($variant:ident($ty:ty)),*) => {
            $(impl From<$ty> for Call {
                fn from(params: $ty) -> Call {
                    Call::$variant(params)
                }
            })*
        };
    }

    from_params!(
        Hello(HelloParams),
        Search(SearchParams),
        Outline(OutlineParams),
        Read(ReadParams),
        Projects(ProjectsParams),
        Status(StatusParams),
        Reindex(ReindexParams),
        Register(RegisterParams),
        Unregister(UnregisterParams),
        SessionStart(SessionStartParams),
        Install(InstallParams),
        Uninstall(UninstallParams)
    );

    #[test]
    fn every_method_roundtrips_through_a_request() {
        roundtrip(HelloParams {
            protocol: 1,
            client: ClientKind::Mcp,
            client_version: "0.0.0".into(),
        });
        roundtrip(SearchParams {
            target: Target {
                cwd: "c".into(),
                project: Some("bars".into()),
            },
            query: "настройки поиска".into(),
            modules: vec![".".into(), "search".into()],
            files: vec!["a/b.md".into()],
            limit: Some(3),
            candidates: Some(20),
            min_score: Some(0.25),
        });
        roundtrip(OutlineParams {
            target: target(),
            path: "docs/a.md".into(),
            levels: vec![2, 3],
            under: Some("Заголовок 1/Заголовок 2".into()),
            under_line: None,
        });
        roundtrip(ReadParams {
            target: target(),
            path: "a.md".into(),
        });
        roundtrip(ProjectsParams {});
        roundtrip(StatusParams { target: target() });
        roundtrip(ReindexParams {
            target: target(),
            full: true,
            accept_model_change: true,
        });
        roundtrip(RegisterParams { path: "p".into() });
        roundtrip(UnregisterParams { target: target() });
        roundtrip(SessionStartParams {
            cwd: "c".into(),
            source: "startup".into(),
        });
        roundtrip(InstallParams {
            client: InstallClient::ClaudeCode,
        });
        roundtrip(UninstallParams {
            client: InstallClient::Codex,
        });
    }

    #[test]
    fn optional_params_are_omitted_and_default_when_missing() {
        let search = SearchParams {
            target: target(),
            query: "q".into(),
            modules: vec![],
            files: vec![],
            limit: None,
            candidates: None,
            min_score: None,
        };
        let value = serde_json::to_value(&search).unwrap();
        assert_eq!(value, json!({ "cwd": "C:\\work\\proj", "query": "q" }));

        let reindex: ReindexParams = serde_json::from_value(json!({ "cwd": "c" })).unwrap();
        assert!(!reindex.full && !reindex.accept_model_change);
    }

    #[test]
    fn projects_accepts_missing_params() {
        let request = Request {
            jsonrpc: "2.0".into(),
            id: Id::Number(1),
            method: "projects".into(),
            params: None,
        };
        assert_eq!(
            Call::from_request(&request).unwrap(),
            Call::Projects(ProjectsParams {})
        );
    }

    #[test]
    fn unknown_method_and_bad_params_have_standard_codes() {
        let unknown = Request::new(Id::Number(1), "nope", json!({}));
        assert_eq!(
            Call::from_request(&unknown).unwrap_err().code,
            METHOD_NOT_FOUND
        );
        let bad = Request::new(Id::Number(1), "search", json!({ "cwd": "c" }));
        assert_eq!(Call::from_request(&bad).unwrap_err().code, INVALID_PARAMS);
        let wrong_type = Request::new(
            Id::Number(1),
            "outline",
            json!({ "cwd": "c", "path": "a", "levels": "2" }),
        );
        assert_eq!(
            Call::from_request(&wrong_type).unwrap_err().code,
            INVALID_PARAMS
        );
    }

    #[test]
    fn install_client_uses_hyphenated_name() {
        let value = serde_json::to_value(InstallParams {
            client: InstallClient::ClaudeCode,
        })
        .unwrap();
        assert_eq!(value, json!({ "client": "claude-code" }));
    }

    #[test]
    fn only_the_documented_methods_are_open_to_mcp() {
        let mcp_methods = [
            "hello", "search", "outline", "read", "projects", "status", "reindex",
        ];
        let calls = [
            Call::from(HelloParams {
                protocol: 1,
                client: ClientKind::Mcp,
                client_version: String::new(),
            }),
            Call::from(RegisterParams { path: "p".into() }),
            Call::from(UnregisterParams { target: target() }),
            Call::from(SessionStartParams {
                cwd: "c".into(),
                source: "s".into(),
            }),
            Call::from(InstallParams {
                client: InstallClient::Codex,
            }),
            Call::from(UninstallParams {
                client: InstallClient::Codex,
            }),
            Call::from(ProjectsParams {}),
            Call::from(ReindexParams {
                target: target(),
                full: false,
                accept_model_change: false,
            }),
        ];
        for call in calls {
            assert_eq!(
                call.allowed_for(ClientKind::Mcp),
                mcp_methods.contains(&call.method()),
                "{}",
                call.method()
            );
            assert!(call.allowed_for(ClientKind::Cli));
        }
    }

    #[test]
    fn heading_line_matches_the_outline_format() {
        let heading = Heading {
            line: 7,
            level: 2,
            text: "Стадия".into(),
        };
        assert_eq!(heading.to_line(), "7 ## Стадия");
    }

    #[test]
    fn search_output_roundtrips() {
        let output = SearchOutput {
            results: vec![SearchHit {
                path: "docs/a.md".into(),
                module: ".".into(),
                title: "A".into(),
                heading_chain: "A/Б".into(),
                first_line: 3,
                last_line: 9,
                text: "текст".into(),
                score: 0.5,
                score_kind: ScoreKind::Cosine,
            }],
            applied: AppliedParams {
                results: 5,
                candidates: 30,
                rerank_candidates: 15,
                min_score: 0.3,
                score_kind: ScoreKind::Cosine,
                clamped: true,
            },
            indexing: true,
            pending: 4,
            rerank_error: None,
        };
        let value = output_value(&output);
        assert_eq!(value["results"][0]["score_kind"], "cosine");
        assert!(value.get("rerank_error").is_none());
        let back: SearchOutput = serde_json::from_value(value).unwrap();
        assert_eq!(back, output);
    }

    #[test]
    fn project_state_uses_snake_case() {
        let value = serde_json::to_value(ProjectState::ReindexRequired).unwrap();
        assert_eq!(value, json!("reindex_required"));
    }
}
