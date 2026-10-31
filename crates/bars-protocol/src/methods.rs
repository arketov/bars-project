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
    /// Метод только для окна: по каналу его не вызывает никто, ни `bars-mcp`, ни `bars-cli`.
    const WINDOW_ONLY: bool = false;
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
    /// Сборка воркера (`BUILD_ID`). У воркера старой сборки поля нет — приходит пустая строка.
    #[serde(default)]
    pub worker_build: String,
}

// --- search ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchParams {
    #[serde(flatten)]
    pub target: Target,
    pub query: String,
    /// Служебная проверка: результаты обычные, спрос в «Новых темах» не записывается.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub diagnostic: bool,
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
    #[serde(default)]
    pub source_hash: String,
    /// Непрозрачный ID чанка для `similar`; действует, пока не изменился документ чанка
    /// (`docs/features/search/maintenance-tools.md`).
    #[serde(default)]
    pub chunk_id: String,
    /// Путь документа от корня проекта.
    pub path: String,
    pub module: String,
    pub title: String,
    /// Цепочка заголовков в виде `Заголовок 1/Заголовок 2`.
    pub heading_chain: String,
    /// Заголовки разделов, приклеенных к чанку после первого, строками `## Заголовок` (ISS-28).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<String>,
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

// --- similar ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimilarParams {
    #[serde(flatten)]
    pub target: Target,
    /// ID чанка из выдачи поиска.
    pub chunk_id: String,
    /// Модули (корневой — `.`), из которых брать похожие; пусто — весь проект.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Похожие чанки: оценка каждого — косинусная близость к исходному чанку.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SimilarOutput {
    /// Исходный чанк (его текст и положение), результаты без него.
    pub source: SearchHit,
    pub results: Vec<SearchHit>,
    /// Сколько результатов запрошено после ограничения пределом.
    pub limit: u32,
    pub clamped: bool,
    pub indexing: bool,
    pub pending: u32,
}

// --- duplicates ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicatesParams {
    #[serde(flatten)]
    pub target: Target,
    /// Модули (корневой — `.`); пусто — весь проект.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Вид повтора: точный, близкая копия, раздел внутри другого, частичная копия, смысловой повтор.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateKind {
    Exact,
    Near,
    Contained,
    Partial,
    Semantic,
}

/// Чанк, входящий в повтор.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicateChunk {
    pub chunk_id: String,
    pub path: String,
    pub module: String,
    pub heading_chain: String,
    pub first_line: u32,
    pub last_line: u32,
}

/// Замена слов между двумя копиями; пустая строка — слов с этой стороны нет.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateChange {
    pub first: String,
    pub second: String,
}

/// Различия двух разделов: изменённые слова, числа и слова отрицания сохраняются в сравнении.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateDifferences {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<DuplicateChange>,
    /// Различий больше, чем показано, или текст слишком длинный для сравнения.
    #[serde(default)]
    pub changes_truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numbers_only_first: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub numbers_only_second: Vec<String>,
    /// Сколько слов отрицания («не», «нет», «без»…) в первом и во втором разделе.
    pub negations_first: u32,
    pub negations_second: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicateItem {
    pub kind: DuplicateKind,
    /// Две и больше записи; у пары — две.
    pub chunks: Vec<DuplicateChunk>,
    /// Мера совпадения: у точных 1; у близких — Жаккар общих окон из пяти слов; у содержащихся и
    /// частичных — доля общих окон меньшего раздела; у смысловых — косинус.
    pub score: f32,
    /// Самые длинные совпавшие отрывки текста.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shared: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub differences: Option<DuplicateDifferences>,
    /// Одинаковый блок в четырёх и больше разделах: скорее шаблон, чем случайная копия.
    #[serde(default)]
    pub repeated_block: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateCounts {
    pub exact: u32,
    pub near: u32,
    pub contained: u32,
    pub partial: u32,
    pub semantic: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicatesOutput {
    pub results: Vec<DuplicateItem>,
    /// Сколько найдено по видам до ограничения числа записей.
    pub counts: DuplicateCounts,
    pub total: u32,
    /// Применённое число записей.
    pub limit: u32,
    pub clamped: bool,
    pub chunks_checked: u32,
    /// Короткие разделы не сравнивались.
    pub chunks_skipped_short: u32,
    pub indexing: bool,
    pub pending: u32,
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
    #[serde(default)]
    pub revision: String,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<crate::GraphStatus>,
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
pub struct ProjectObserveParams {
    pub window: String,
    pub project: Option<String>,
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
pub struct ProjectDeleteParams {
    pub project: String,
    pub confirmed: bool,
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

// --- методы окна: index_pause, index_resume, index_cancel, activity, tree ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexPauseParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexResumeParams {}

/// Очередь индексации приостановлена целиком (OQ-55).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PausedOutput {
    pub paused: bool,
}

/// Остановка индексации проекта: идущее задание прерывается, ждущее снимается с очереди.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexCancelParams {
    /// Абсолютный путь к корню проекта или однозначное имя.
    pub project: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CancelOutput {
    /// Задание было идущим или ждущим.
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityParams {}

/// Что делает задание очереди.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    /// Полная переиндексация в новом файле с подменой.
    Rebuild,
    /// Проверка актуальности всего проекта.
    Check,
    /// Проверка перечисленных файлов (слежение).
    Files,
}

/// Откуда пришло задание.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobPriority {
    /// Явная переиндексация, хук, регистрация.
    Explicit,
    /// Правка документа в окне bars.
    Edit,
    /// Слежение за файлами.
    Watch,
    /// Старт воркера и часовая проверка.
    Periodic,
}

/// Идущее задание и его ход.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunningJob {
    pub embed_device: Option<String>,
    pub attempt: u32,
    pub project: String,
    pub name: String,
    pub kind: JobKind,
    pub priority: JobPriority,
    /// Документов в обходе; 0, пока обход не закончился.
    pub docs_total: u32,
    pub docs_done: u32,
    /// Чанков обработано: посчитано моделью или взято из индекса.
    pub chunks_done: u32,
    /// Чанков, обработанных за секунду с начала задания.
    pub chunks_per_second: f32,
    pub elapsed_seconds: u32,
    /// Документ, который обрабатывается сейчас.
    pub current_file: Option<String>,
}

/// Ждущее задание.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedJob {
    pub project: String,
    pub name: String,
    pub kind: JobKind,
    pub priority: JobPriority,
    /// Для `files` — сколько документов названо.
    pub files: u32,
}

/// Файл, который не удалось проиндексировать.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileProblem {
    pub stage: String,
    pub attempts: u32,
    pub previous_version: bool,
    pub path: String,
    pub reason: String,
}

/// Проблемы проекта по итогу последней проверки; проекты без проблем в список не попадают.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectProblems {
    pub rebuild_not_applied: bool,
    pub project: String,
    pub name: String,
    pub files: Vec<FileProblem>,
    pub last_error: Option<String>,
}

/// Состояние документа в очереди.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentState {
    /// Ждёт, пока пройдёт пауза после правки.
    Waiting,
    /// Готов, ждёт своей очереди.
    Queued,
    /// Идёт эмбеддинг.
    Indexing,
    /// Не проиндексирован после повторов.
    Error,
}

/// Откуда пришло изменение документа.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentSource {
    Explicit,
    Edit,
    Watch,
    Periodic,
}

/// Запись очереди документов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedDocument {
    /// Путь от корня проекта.
    pub path: String,
    pub state: DocumentState,
    pub source: DocumentSource,
    /// Сколько секунд осталось до конца паузы; только у `waiting`.
    pub due_in: Option<u32>,
    /// Причина ошибки без ключей; только у `error`.
    pub error: Option<String>,
}

/// Очередь документов проекта для страницы индексации (`document-queue.md`); проекты без
/// записей в список не попадают.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectDocuments {
    pub project: String,
    /// Записи в порядке выполнения, не больше 200.
    pub documents: Vec<QueuedDocument>,
    /// Сколько записей всего (включая не вошедшие в список).
    pub total: u32,
    pub waiting: u32,
    pub queued: u32,
    pub indexing: u32,
    pub error: u32,
}

/// «Повторить» для документа с ошибкой.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexRetryParams {
    pub project: String,
    /// Путь документа от корня проекта.
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetryOutput {
    /// У документа была запись с ошибкой, теперь он снова в очереди.
    pub retried: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityOutput {
    pub paused: bool,
    pub running: Option<RunningJob>,
    pub queue: Vec<QueuedJob>,
    pub problems: Vec<ProjectProblems>,
    /// Очередь документов по проектам: паузы после правок, состояния, ошибки.
    #[serde(default)]
    pub documents: Vec<ProjectDocuments>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeParams {
    #[serde(flatten)]
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeDocument {
    /// Путь от корня проекта, как в выдаче поиска.
    pub path: String,
    pub module: String,
    pub title: String,
    pub chunks: u32,
}

/// Документы проекта из индекса: основа дерева с галочками (Т-14).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeOutput {
    pub docs_root: String,
    pub documents: Vec<TreeDocument>,
}

// --- методы окна: настройки, модели, ключи, клиенты, корень документации ---

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsGetParams {}

/// Таблица `[embed]` как её показывает и правит окно.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AcceleratorDoc {
    Auto,
    Cpu,
}

impl AcceleratorDoc {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Cpu => "cpu",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmbedDoc {
    pub accelerator: AcceleratorDoc,
    /// `builtin`, `remote`, `local-openai`, `tei`; `ollama`/`lmstudio` только embed.
    pub source: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_name: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    pub batch_size: u32,
    /// Минут простоя до выгрузки встроенной модели, `0` — не выгружать.
    pub unload_after_minutes: u32,
}

/// Таблица `[rerank]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RerankDoc {
    pub accelerator: AcceleratorDoc,
    pub enabled: bool,
    pub source: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key_name: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Минут простоя до выгрузки встроенной модели, `0` — не выгружать.
    pub unload_after_minutes: u32,
}

/// Таблица `[search]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchDoc {
    pub candidates: u32,
    pub rerank_candidates: u32,
    pub results: u32,
    pub results_reranked: u32,
    pub min_score_vector: f32,
    pub min_score_rerank: f32,
    pub agent_min_score: [f32; 2],
    pub max_results: u32,
    pub max_candidates: u32,
    pub max_rerank_candidates: u32,
}

/// Таблица `[chunking]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkingDoc {
    pub min_chars: u32,
    pub max_chars: u32,
}

/// Таблица `[indexing]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexingDoc {
    pub edit_pause_seconds: u32,
}

/// Таблица `[mcp]`: что `bars-mcp` показывает агентам.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpDoc {
    /// Показывать ли шесть графовых инструментов; по умолчанию скрыты (OQ-162).
    pub graph_tools: bool,
}

/// Настройки пользователя (`~/.bars/settings.toml`) целиком, с умолчаниями на месте пропущенного.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsDoc {
    pub embed: EmbedDoc,
    pub rerank: RerankDoc,
    pub search: SearchDoc,
    pub chunking: ChunkingDoc,
    pub indexing: IndexingDoc,
    #[serde(default)]
    pub mcp: McpDoc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsOutput {
    pub revision: Option<String>,
    /// Путь к `settings.toml`.
    pub file: String,
    pub settings: SettingsDoc,
    /// Предупреждения чтения: неизвестные ключи.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsSaveParams {
    pub base: SettingsDoc,
    pub changes: std::collections::BTreeMap<String, serde_json::Value>,
    pub expected_revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsConflict {
    pub key: String,
    pub base: serde_json::Value,
    pub disk: serde_json::Value,
    pub mine: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsSaveOutput {
    pub status: SettingsSaveStatus,
    pub conflicts: Vec<SettingsConflict>,
    #[serde(flatten)]
    pub current: SettingsOutput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSaveStatus {
    Saved,
    Conflict,
}

impl SettingsSaveParams {
    pub fn between(base: SettingsOutput, settings: SettingsDoc) -> Self {
        let old = serde_json::to_value(&base.settings).expect("settings serialize");
        let new = serde_json::to_value(settings).expect("settings serialize");
        let mut changes = std::collections::BTreeMap::new();
        for (section, fields) in new.as_object().expect("settings object") {
            for (key, value) in fields.as_object().expect("settings table") {
                if value != &old[section][key] {
                    changes.insert(format!("{section}.{key}"), value.clone());
                }
            }
        }
        Self {
            base: base.settings,
            expected_revision: base.revision,
            changes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedOutput {
    pub saved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsInfoParams {}

/// Встроенная модель каталога и состояние её файлов.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    /// Размерность вектора; у rerank-модели 0.
    pub dim: u32,
    /// `onnx` (процессор или видеокарта) или `candle` (процессор).
    pub backend: String,
    /// Предупреждения окна: скорость, память, длина входа.
    pub warnings: Vec<String>,
    pub downloaded: bool,
    pub downloading: bool,
    /// Ход файлов модели; 100 при downloading означает подготовку сессии.
    #[serde(default)]
    pub download_percent: Option<u8>,
    /// Безопасное сообщение последней ошибки ручного скачивания.
    #[serde(default)]
    pub download_error: Option<String>,
    /// Вид ошибки встроенной embed-модели: `network`, `files_missing`, `out_of_memory`, `runtime`.
    /// У rerank-модели вида нет (OQ-218).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_error_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsInfoOutput {
    pub runtime: Option<ModelsRuntime>,
    pub embed: Vec<ModelInfo>,
    pub rerank: Vec<ModelInfo>,
    /// Папка моделей (`~/.bars/models`).
    pub models_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRuntime {
    pub model: String,
    pub source: String,
    pub requested: String,
    pub state: String,
    pub backend: String,
    pub actual: Option<String>,
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelsRuntime {
    pub embed: ModelRuntime,
    pub rerank: ModelRuntime,
}

/// Скачивание встроенной модели в фоне; ход виден по `downloading` в `models_info`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelDownloadParams {
    pub id: String,
}

/// Кнопка «Повторить» на экране ошибки модели: снимает паузу перед повтором фоновой загрузки
/// всех моделей, следующий поиск загружает модель сразу
/// (`docs/features/models/local-models.md`, ISS-71).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRetryParams {}

/// Запись ключа провайдера в хранилище ОС. Значение никогда не попадает в лог и в `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretSetParams {
    pub name: String,
    pub value: String,
}

impl std::fmt::Debug for SecretSetParams {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretSetParams")
            .field("name", &self.name)
            .field("value", &"<скрыто>")
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretStatusParams {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretOutput {
    /// Запись с таким именем есть в хранилище ОС. Сам ключ окну не отдаётся.
    pub present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientsStatusParams {}

/// Состояние установки bars в клиент.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientStatus {
    pub client: InstallClient,
    /// В конфиге клиента есть MCP-запись bars.
    pub installed: bool,
    /// Записи указывают не на текущие `bars-mcp` и `bars-cli`: нужна переустановка.
    pub stale: bool,
    /// Файлы, которые bars правит у этого клиента.
    pub files: Vec<String>,
    /// Хук нужно одобрить в самом клиенте (Codex, `/hooks`).
    pub needs_hook_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientsStatusOutput {
    pub clients: Vec<ClientStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionParams {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionOutput {
    /// Текстовая инструкция по установке для агента (Т-21).
    pub text: String,
    /// Конфиг MCP-сервера для клиентов, которых bars не умеет ставить сам.
    pub mcp_config: String,
}

/// Корень документации проекта: запись в `.bars/config.toml` и полная проверка.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetDocsRootParams {
    pub project: String,
    /// Относительно корня проекта; `.` — сам корень.
    pub docs_root: String,
}

/// Проверка всех проектов сейчас: пропавшие возвращаются в работу, если папка на месте.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckAllParams {}

/// Состояние автозапуска `bars` при входе в систему.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutostartGetParams {}

/// Включить или выключить автозапуск.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutostartSetParams {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutostartOutput {
    /// ОС и сборка умеют автозапуск; без этого галочки в окне нет.
    pub supported: bool,
    /// Запись есть, и ОС её не отключила.
    pub enabled: bool,
    /// Запись указывает не на текущий `bars`.
    pub stale: bool,
    /// Команда из записи.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

/// Кто может вызвать метод: все клиенты канала, только `bars-cli` или только окно.
macro_rules! audience {
    (All) => {
        (true, false)
    };
    (Cli) => {
        (false, false)
    };
    (Window) => {
        (false, true)
    };
}

macro_rules! methods {
    ($($variant:ident: $params:ty => $output:ty, $name:literal, $audience:ident;)*) => {
        $(impl Method for $params {
            const NAME: &'static str = $name;
            const FROM_MCP: bool = audience!($audience).0;
            const WINDOW_ONLY: bool = audience!($audience).1;
            type Output = $output;
        })*

        /// Разобранный вызов: по одному варианту на метод.
        #[derive(Debug, Clone, PartialEq)]
        #[allow(clippy::large_enum_variant)] // короткоживущее значение: упаковка в Box меняет код всех обработчиков
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

            /// Доступен ли вызов клиенту канала такого рода. Методы окна по каналу закрыты всем.
            pub fn allowed_for(&self, client: ClientKind) -> bool {
                match self {
                    $(Call::$variant(_) => {
                        !<$params as Method>::WINDOW_ONLY
                            && (client == ClientKind::Cli || <$params as Method>::FROM_MCP)
                    })*
                }
            }

            /// Метод только для окна: окно вызывает обработчик напрямую, без канала.
            pub fn window_only(&self) -> bool {
                match self {
                    $(Call::$variant(_) => <$params as Method>::WINDOW_ONLY,)*
                }
            }
        }
    };
}

methods! {
    GraphSearch: crate::GraphSearchParams => crate::GraphSearchOutput, "graph_search", All;
    GraphQuery: crate::GraphQueryParams => crate::GraphReply, "graph_query", All;
    GraphStatus: crate::GraphStatusParams => crate::GraphStatus, "graph_status", All;
    GraphPhysical: crate::GraphPhysicalParams => crate::PhysicalGraphSlice, "graph_physical", Window;
    GraphQuerySlice: crate::GraphQuerySliceParams => crate::QueryGraphSlice, "graph_query_slice", Window;
    GraphBuild: crate::GraphBuildParams => crate::GraphAcceptedOutput, "graph_build", Cli;
    GraphCancel: crate::GraphCancelParams => CancelOutput, "graph_cancel", Cli;
    OperationEvents: crate::OperationEventsParams => crate::OperationEventsOutput, "operation_events", Window;
    OperationRetry: crate::OperationRetryParams => AcceptedOutput, "operation_retry", Window;
    Hello: HelloParams => HelloOutput, "hello", All;
    Search: SearchParams => SearchOutput, "search", All;
    Similar: SimilarParams => SimilarOutput, "similar", All;
    Duplicates: DuplicatesParams => DuplicatesOutput, "duplicates", All;
    Outline: OutlineParams => OutlineOutput, "outline", All;
    Read: ReadParams => ReadOutput, "read", All;
    Projects: ProjectsParams => ProjectsOutput, "projects", All;
    Status: StatusParams => StatusOutput, "status", All;
    Reindex: ReindexParams => AcceptedOutput, "reindex", All;
    Register: RegisterParams => RegisterOutput, "register", Cli;
    Unregister: UnregisterParams => UnregisterOutput, "unregister", Cli;
    ProjectDelete: ProjectDeleteParams => UnregisterOutput, "project_delete", Window;
    ProjectObserve: ProjectObserveParams => SavedOutput, "project_observe", Window;
    SessionStart: SessionStartParams => AcceptedOutput, "session_start", Cli;
    Install: InstallParams => InstallOutput, "install", Cli;
    Uninstall: UninstallParams => UninstallOutput, "uninstall", Cli;
    IndexPause: IndexPauseParams => PausedOutput, "index_pause", Window;
    IndexResume: IndexResumeParams => PausedOutput, "index_resume", Window;
    IndexCancel: IndexCancelParams => CancelOutput, "index_cancel", Window;
    Activity: ActivityParams => ActivityOutput, "activity", Window;
    IndexRetry: IndexRetryParams => RetryOutput, "index_retry", Window;
    Tree: TreeParams => TreeOutput, "tree", Window;
    SettingsGet: SettingsGetParams => SettingsOutput, "settings_get", Window;
    SettingsSave: SettingsSaveParams => SettingsSaveOutput, "settings_save", Window;
    ModelsInfo: ModelsInfoParams => ModelsInfoOutput, "models_info", Window;
    ModelDownload: ModelDownloadParams => AcceptedOutput, "model_download", Window;
    ModelRetry: ModelRetryParams => AcceptedOutput, "model_retry", Window;
    SecretSet: SecretSetParams => SecretOutput, "secret_set", Window;
    SecretStatus: SecretStatusParams => SecretOutput, "secret_status", Window;
    ClientsStatus: ClientsStatusParams => ClientsStatusOutput, "clients_status", Window;
    Instruction: InstructionParams => InstructionOutput, "instruction", Window;
    SetDocsRoot: SetDocsRootParams => AcceptedOutput, "set_docs_root", Window;
    CheckAll: CheckAllParams => AcceptedOutput, "check_all", Window;
    NewTopicsList: crate::NewTopicsListParams => crate::NewTopicsListOutput, "new_topics_list", All;
    NewTopicsGet: crate::NewTopicsGetParams => crate::NewTopicDetail, "new_topics_get", All;
    NewTopicsMarkMissing: crate::NewTopicsMarkMissingParams => crate::NewTopicsMarkMissingOutput, "new_topics_mark_missing", Window;
    NewTopicsCheck: crate::NewTopicsCheckParams => crate::NewTopicDetail, "new_topics_check", All;
    NewTopicsSetStatus: crate::NewTopicsSetStatusParams => crate::NewTopicsChangedOutput, "new_topics_set_status", Window;
    NewTopicsRename: crate::NewTopicsRenameParams => crate::NewTopicsChangedOutput, "new_topics_rename", Window;
    NewTopicsMerge: crate::NewTopicsMergeParams => crate::NewTopicsChangedOutput, "new_topics_merge", Window;
    NewTopicsDelete: crate::NewTopicsDeleteParams => AcceptedOutput, "new_topics_delete", Window;
    NewTopicsClear: crate::NewTopicsClearParams => crate::NewTopicsGenerationOutput, "new_topics_clear", Window;
    NewTopicsSettings: crate::NewTopicsSettingsParams => crate::NewTopicsSettingsOutput, "new_topics_settings", Window;
    LinksOf: crate::LinksOfParams => crate::LinksOfOutput, "links_of", Window;
    LinksInto: crate::LinksIntoParams => crate::LinksIntoOutput, "links_into", Window;
    TextSearch: crate::TextSearchParams => crate::TextSearchOutput, "text_search", Window;
    DiskText: crate::DiskTextParams => crate::DiskTextOutput, "disk_text", All;
    DiskLinks: crate::DiskLinksParams => crate::DiskLinksOutput, "disk_links", All;
    AuditList: crate::AuditListParams => crate::AuditListOutput, "audit_list", All;
    AuditRun: crate::AuditRunParams => crate::AuditRunOutput, "audit_run", All;
    AuditCancel: crate::AuditCancelParams => crate::AuditCancelOutput, "audit_cancel", All;
    AuditFindings: crate::AuditFindingsParams => crate::AuditFindingsOutput, "audit_findings", All;
    AuditSchedule: crate::AuditScheduleParams => crate::AuditScheduleOutput, "audit_schedule", Window;
    AuditIgnore: crate::AuditIgnoreParams => crate::AuditIgnoreOutput, "audit_ignore", Window;
    AutostartGet: AutostartGetParams => AutostartOutput, "autostart_get", Cli;
    AutostartSet: AutostartSetParams => AutostartOutput, "autostart_set", Cli;
}

/// Ответ метода как JSON-значение.
pub fn output_value<T: Serialize>(output: &T) -> Value {
    serde_json::to_value(output).expect("ответы методов сериализуются в JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_delete_requires_confirmation_in_params_and_is_window_only() {
        let request = Request::new(
            Id::Number(1),
            "project_delete",
            json!({"project": "C:/проект", "confirmed": true}),
        );
        let call = Call::from_request(&request).unwrap();
        assert_eq!(
            call,
            Call::ProjectDelete(ProjectDeleteParams {
                project: "C:/проект".into(),
                confirmed: true
            })
        );
        assert!(call.window_only());
        assert!(!call.allowed_for(ClientKind::Cli));
        assert!(!call.allowed_for(ClientKind::Mcp));
        let missing = Request::new(
            Id::Number(2),
            "project_delete",
            json!({"project": "C:/проект"}),
        );
        assert!(Call::from_request(&missing).is_err());
    }

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
        Similar(SimilarParams),
        Duplicates(DuplicatesParams),
        Outline(OutlineParams),
        Read(ReadParams),
        Projects(ProjectsParams),
        Status(StatusParams),
        Reindex(ReindexParams),
        Register(RegisterParams),
        Unregister(UnregisterParams),
        SessionStart(SessionStartParams),
        Install(InstallParams),
        Uninstall(UninstallParams),
        IndexPause(IndexPauseParams),
        IndexResume(IndexResumeParams),
        IndexCancel(IndexCancelParams),
        Activity(ActivityParams),
        IndexRetry(IndexRetryParams),
        Tree(TreeParams),
        SettingsGet(SettingsGetParams),
        SettingsSave(SettingsSaveParams),
        ModelsInfo(ModelsInfoParams),
        ModelDownload(ModelDownloadParams),
        ModelRetry(ModelRetryParams),
        SecretSet(SecretSetParams),
        SecretStatus(SecretStatusParams),
        ClientsStatus(ClientsStatusParams),
        Instruction(InstructionParams),
        SetDocsRoot(SetDocsRootParams),
        CheckAll(CheckAllParams),
        AutostartGet(AutostartGetParams),
        AutostartSet(AutostartSetParams)
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
            diagnostic: true,
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
        roundtrip(IndexPauseParams {});
        roundtrip(IndexResumeParams {});
        roundtrip(IndexCancelParams {
            project: "bars".into(),
        });
        roundtrip(ActivityParams {});
        roundtrip(IndexRetryParams {
            project: "bars".into(),
            path: "docs/a.md".into(),
        });
        roundtrip(TreeParams { target: target() });
        roundtrip(AutostartGetParams {});
        roundtrip(AutostartSetParams { enabled: true });
    }

    #[test]
    fn window_methods_are_closed_on_the_channel() {
        let calls = [
            Call::from(IndexPauseParams {}),
            Call::from(IndexResumeParams {}),
            Call::from(IndexCancelParams {
                project: "p".into(),
            }),
            Call::from(ActivityParams {}),
            Call::from(TreeParams { target: target() }),
            Call::from(SettingsGetParams {}),
            Call::from(ModelsInfoParams {}),
            Call::from(ModelDownloadParams { id: "m".into() }),
            Call::from(ModelRetryParams {}),
            Call::from(SecretSetParams {
                name: "n".into(),
                value: "v".into(),
            }),
            Call::from(SecretStatusParams { name: "n".into() }),
            Call::from(ClientsStatusParams {}),
            Call::from(InstructionParams {}),
            Call::from(SetDocsRootParams {
                project: "p".into(),
                docs_root: ".".into(),
            }),
            Call::from(CheckAllParams {}),
        ];
        for call in calls {
            assert!(call.window_only(), "{}", call.method());
            assert!(!call.allowed_for(ClientKind::Mcp), "{}", call.method());
            assert!(!call.allowed_for(ClientKind::Cli), "{}", call.method());
        }
        assert!(!Call::from(ProjectsParams {}).window_only());
    }

    #[test]
    fn optional_params_are_omitted_and_default_when_missing() {
        let search = SearchParams {
            target: target(),
            query: "q".into(),
            diagnostic: false,
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
            "hello",
            "search",
            "similar",
            "duplicates",
            "outline",
            "read",
            "projects",
            "status",
            "reindex",
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
            Call::from(SimilarParams {
                target: target(),
                chunk_id: "1-00000000".into(),
                modules: Vec::new(),
                limit: None,
            }),
            Call::from(DuplicatesParams {
                target: target(),
                modules: Vec::new(),
                limit: None,
            }),
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
                source_hash: "hash".into(),
                chunk_id: "7-0a1b2c3d".into(),
                path: "docs/a.md".into(),
                module: ".".into(),
                title: "A".into(),
                heading_chain: "A/Б".into(),
                sections: vec!["## В".into()],
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
    fn model_runtime_carries_optional_precision_and_revision() {
        let old = json!({"model":"bge", "source":"builtin", "requested":"auto", "state":"ready", "backend":"onnx", "actual":"cpu", "fallback_reason":null});
        let mut runtime: ModelRuntime = serde_json::from_value(old.clone()).unwrap();
        assert_eq!(runtime.precision, None);
        assert_eq!(runtime.revision, None);
        assert_eq!(serde_json::to_value(&runtime).unwrap(), old);
        runtime.actual = Some("directml".into());
        runtime.precision = Some("fp16".into());
        runtime.revision = Some("pinned-revision".into());
        let value = serde_json::to_value(&runtime).unwrap();
        assert_eq!(value["precision"], "fp16");
        assert_eq!(value["revision"], "pinned-revision");
        assert_eq!(
            serde_json::from_value::<ModelRuntime>(value).unwrap(),
            runtime
        );
    }

    #[test]
    fn project_state_uses_snake_case() {
        let value = serde_json::to_value(ProjectState::ReindexRequired).unwrap();
        assert_eq!(value, json!("reindex_required"));
    }
}
