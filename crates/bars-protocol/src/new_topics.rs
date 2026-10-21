//! Внутренний API окна для «Новых тем» (`docs/features/new-topics/design.md`, «Внутренние операции»).
//!
//! Все методы только для окна: канал закрыт им для `bars-mcp` и `bars-cli`, список тем агенту не
//! отдаётся (NT-01, NT-10). Проект — абсолютный путь корня, как у `project_delete`.

use serde::{Deserialize, Serialize};

/// Список тем проекта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsListParams {
    pub project: String,
}

/// Строка списка тем: без вариантов и проверок.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicSummary {
    pub id: i64,
    /// Ревизия, которую окно возвращает в операциях: изменённая тема даёт конфликт, а не перезапись.
    pub revision: i64,
    pub title: String,
    /// `open`, `materials_found`, `resolved`.
    pub status: String,
    pub first_seen: i64,
    pub last_seen: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<i64>,
    /// Число самостоятельных обращений, не людей и не задач.
    pub observations: i64,
    /// `window`, `mcp`.
    pub sources: Vec<String>,
    /// `empty_result`, `manual_missing`.
    pub reasons: Vec<String>,
    pub variants: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTopicsListOutput {
    /// Сбор включён.
    pub enabled: bool,
    /// Поколение списка: растёт при очистке и переключении сбора.
    pub generation: i64,
    pub topics: Vec<NewTopicSummary>,
    /// Где лежит БД: пользователь видит место хранения (NT-12).
    pub store_path: String,
    /// Известные потери сбора с запуска воркера; тексты вопросов в причину не попадают.
    pub losses: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loss_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsGetParams {
    pub project: String,
    pub id: i64,
}

/// Формулировка темы и область, в которой она искалась.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicVariant {
    pub id: i64,
    pub text: String,
    /// `project` или `m=<модули>;f=<файлы>`.
    pub scope: String,
    /// Параметры первого сохранённого обращения, JSON.
    pub original_params: String,
}

/// Найденный фрагмент: путь, заголовок, строки и оценка; текст документа не сохраняется.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTopicHit {
    pub path: String,
    pub title: String,
    pub heading_chain: String,
    pub first_line: u32,
    pub last_line: u32,
    pub score: f32,
    pub score_kind: String,
}

/// Итог проверки одного варианта.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTopicCheckResult {
    pub variant_id: i64,
    /// Область, по которой искали: исходная либо `project` при явном расширении.
    pub scope: String,
    /// `found`, `empty`, `incomplete`.
    pub outcome: String,
    /// Причина неполноты: `indexing`, `model_loading`, `reindex_required`, `scope_missing`, `error`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_stamp: Option<String>,
    pub hits: Vec<NewTopicHit>,
}

/// Проверка темы: поиск материалов по её вариантам текущим поиском.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTopicCheck {
    pub id: i64,
    /// Ревизия темы, с которой началась проверка. Если она меньше текущей, тема менялась после.
    pub topic_revision: i64,
    pub at: i64,
    /// Проверка расширена на весь проект по явному действию.
    pub expanded: bool,
    pub model: String,
    /// Применённые параметры поиска, JSON.
    pub params: String,
    /// `true`, если все варианты завершились без неполноты и по одной ревизии индекса. Иначе
    /// общего вывода нет (`design.md`, «Жизненный цикл темы и проверки»).
    pub conclusive: bool,
    pub results: Vec<NewTopicCheckResult>,
}

/// Тема с вариантами и двумя последними проверками (новая первой).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NewTopicDetail {
    #[serde(flatten)]
    pub topic: NewTopicSummary,
    pub variants_list: Vec<NewTopicVariant>,
    pub checks: Vec<NewTopicCheck>,
}

/// Ручная отметка «Не хватило знаний» по отображённой выдаче.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsMarkMissingParams {
    pub project: String,
    /// ID отображённого обращения: повторная отметка того же обращения идемпотентна.
    pub request_id: String,
    /// Текст запроса именно отображаемого результата, а не изменённого поля ввода.
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsMarkMissingOutput {
    pub topic_id: i64,
    /// `true`, если тема создана; `false` — объединена с имеющейся или отметка уже была.
    pub created: bool,
}

/// Проверка темы. `expand` — явное расширение исходных областей на весь проект.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsCheckParams {
    pub project: String,
    pub id: i64,
    pub revision: i64,
    #[serde(default)]
    pub expand: bool,
}

/// Решение пользователя: `resolved` («Знаний достаточно») или `open` («Всё ещё не хватает»).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsSetStatusParams {
    pub project: String,
    pub id: i64,
    pub revision: i64,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsRenameParams {
    pub project: String,
    pub id: i64,
    pub revision: i64,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsDeleteParams {
    pub project: String,
    pub id: i64,
    pub revision: i64,
}

/// Результат изменения темы: её новая ревизия.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsChangedOutput {
    pub revision: i64,
}

/// Очистка списка; `confirmed` — подтверждение пользователя в окне.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsClearParams {
    pub project: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsGenerationOutput {
    pub generation: i64,
}

/// Сбор вкл/выкл для проекта.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsSettingsParams {
    pub project: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewTopicsSettingsOutput {
    pub enabled: bool,
    pub generation: i64,
}
