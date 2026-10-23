//! Настройки bars: `~/.bars/settings.toml` и `.bars/config.toml` проекта
//! (`docs/features/settings/settings.md`, `docs/architecture/files.md`, Т-16, Т-18, Т-24).
//!
//! Значение берётся по первому найденному: `config.toml` проекта → `settings.toml` пользователя →
//! заводское значение. Вызов агента накладывается поверх них в [`SearchSettings::resolve`].
//! Файлы только читаются; запись из окна — дело воркера и окна.
//!
//! Ошибка в файле (битый TOML, неверный тип, нулевой размер, пределы наоборот) не превращается в
//! умолчания: чтение возвращает [`SettingsError`], а воркер оставляет прежние настройки (OQ-32).
//! Неизвестный ключ не ошибка: он попадает в [`Loaded::warnings`] (OQ-33).

mod raw;

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::indexing::chunking::ChunkingParams;
use crate::indexing::indexer::IndexSettings;
use crate::models::{Accelerator, BGE_M3_ID, BGE_RERANKER_V2_M3_ID};
use crate::search::SearchSettings;

/// Имя файла настроек пользователя в `~/.bars`.
pub const USER_SETTINGS_FILE: &str = "settings.toml";
/// Имя файла настроек проекта в `<проект>/.bars`.
pub const PROJECT_CONFIG_FILE: &str = "config.toml";
/// Корень документации по умолчанию (Т-7).
pub const DEFAULT_DOCS_ROOT: &str = "docs";

/// Путь к `settings.toml` в домашней папке bars (`~/.bars`).
pub fn user_settings_path(bars_home: &Path) -> PathBuf {
    bars_home.join(USER_SETTINGS_FILE)
}

/// Путь к `config.toml` проекта.
pub fn project_config_path(project_root: &Path) -> PathBuf {
    project_root.join(".bars").join(PROJECT_CONFIG_FILE)
}

/// Откуда берётся модель (Т-16, Т-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelSource {
    /// Встроенная модель bars.
    Builtin,
    /// Удалённый провайдер.
    Remote,
    /// Локальный OpenAI-совместимый адрес.
    LocalOpenai,
    /// Нативный API Ollama (только embedding).
    Ollama,
}

impl ModelSource {
    /// Имя источника, как оно записано в файле и в отпечатке модели.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Remote => "remote",
            Self::LocalOpenai => "local-openai",
            Self::Ollama => "ollama",
        }
    }
}

/// Таблица `[embed]` (Т-16, Т-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbedSettings {
    pub source: ModelSource,
    /// Для `builtin` — модель из каталога; для остальных — имя модели у провайдера.
    pub model: String,
    pub accelerator: Accelerator,
    pub base_url: Option<String>,
    /// Имя записи в хранилище ОС (`docs/architecture/secrets.md`).
    pub api_key_name: Option<String>,
    /// Переменная окружения, которая перекрывает ключ из хранилища.
    pub api_key_env: Option<String>,
    /// Сколько чанков уходит в одном запросе к внешнему провайдеру.
    pub batch_size: usize,
}

impl Default for EmbedSettings {
    fn default() -> Self {
        Self {
            source: ModelSource::Builtin,
            accelerator: Accelerator::Auto,
            model: BGE_M3_ID.to_string(),
            base_url: None,
            api_key_name: None,
            api_key_env: None,
            batch_size: 32,
        }
    }
}

/// Таблица `[rerank]` (Т-16, Т-17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RerankSettings {
    /// Включён ли реранк. По умолчанию выключен: на прототипе он тратил секунды на запрос.
    pub enabled: bool,
    pub source: ModelSource,
    pub model: String,
    pub accelerator: Accelerator,
    pub base_url: Option<String>,
    pub api_key_name: Option<String>,
    pub api_key_env: Option<String>,
}

impl Default for RerankSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            source: ModelSource::Builtin,
            accelerator: Accelerator::Auto,
            model: BGE_RERANKER_V2_M3_ID.to_string(),
            base_url: None,
            api_key_name: None,
            api_key_env: None,
        }
    }
}

/// Сколько минут простоя до выгрузки встроенной embed-модели по умолчанию.
pub const DEFAULT_EMBED_UNLOAD_MINUTES: u32 = 15;
/// То же для rerank-модели.
pub const DEFAULT_RERANK_UNLOAD_MINUTES: u32 = 5;
/// Наибольший срок выгрузки, минут (неделя); «никогда» — это `0`.
pub const MAX_UNLOAD_MINUTES: u32 = 10_080;

/// Ключи `unload_after_minutes` из `[embed]` и `[rerank]`: через сколько минут простоя воркер
/// выгружает встроенную модель, `0` — не выгружать (ISS-18). `None` — ключа в файле нет, срок
/// задаёт воркер. Лежит отдельно от [`EmbedSettings`] и [`RerankSettings`]: срок не входит в
/// описание модели, и его смена не должна перезагружать её.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelUnload {
    pub embed_minutes: Option<u32>,
    pub rerank_minutes: Option<u32>,
}

/// Сколько секунд после правки документа bars ждёт, прежде чем пересчитывать его (Т-41).
pub const DEFAULT_EDIT_PAUSE_SECONDS: u32 = 30;
/// Наибольшая пауза, секунд (час); «без паузы» — это `0`.
pub const MAX_EDIT_PAUSE_SECONDS: u32 = 3_600;

/// Таблица `[indexing]`: поведение очереди документов (`docs/features/indexing/document-queue.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexingParams {
    /// Пауза после правки до embed документа, секунд; `0` — без паузы.
    pub edit_pause_seconds: u32,
}

impl Default for IndexingParams {
    fn default() -> Self {
        Self {
            edit_pause_seconds: DEFAULT_EDIT_PAUSE_SECONDS,
        }
    }
}

/// Настройки пользователя: всё содержимое `settings.toml` с умолчаниями на месте пропущенного.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserSettings {
    pub embed: EmbedSettings,
    pub rerank: RerankSettings,
    pub unload: ModelUnload,
    pub search: SearchSettings,
    pub chunking: ChunkingParams,
    pub indexing: IndexingParams,
}

/// Настройки проекта: только то, что `.bars/config.toml` может перекрыть (Т-5, Т-7).
#[derive(Debug, Clone, Default)]
pub struct ProjectConfig {
    docs_root: Option<String>,
    search: Option<raw::SearchOverrides>,
    chunking: Option<raw::ChunkingOverrides>,
    indexing: Option<raw::IndexingOverrides>,
}

/// Настройки, с которыми работает один проект: пользовательские, перекрытые его `config.toml`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub embed: EmbedSettings,
    pub rerank: RerankSettings,
    pub search: SearchSettings,
    pub chunking: ChunkingParams,
    pub indexing: IndexingParams,
    /// Корень документации относительно корня проекта (Т-7).
    pub docs_root: String,
}

impl Settings {
    /// Что нужно индексатору: нарезка и корень документации.
    pub fn index_settings(&self) -> IndexSettings {
        IndexSettings {
            chunking: self.chunking,
            docs_root: self.docs_root.clone(),
        }
    }

    /// Настройки пользователя, перекрытые `config.toml` проекта: ключ проекта выигрывает.
    pub fn resolve(user: &UserSettings, project: &ProjectConfig) -> Self {
        let mut search = user.search.clone();
        if let Some(overrides) = &project.search {
            overrides.apply(&mut search);
        }
        let mut chunking = user.chunking;
        if let Some(overrides) = &project.chunking {
            overrides.apply(&mut chunking);
        }
        let mut indexing = user.indexing;
        if let Some(overrides) = &project.indexing {
            overrides.apply(&mut indexing);
        }
        Self {
            embed: user.embed.clone(),
            rerank: user.rerank.clone(),
            search,
            chunking,
            indexing,
            docs_root: project
                .docs_root
                .clone()
                .unwrap_or_else(|| DEFAULT_DOCS_ROOT.to_string()),
        }
    }

    /// Читает оба файла и складывает их. `bars_home` — `~/.bars`, его определяет вызывающий.
    pub fn load(bars_home: &Path, project_root: &Path) -> Result<Loaded<Self>, SettingsError> {
        let user = UserSettings::load(bars_home)?;
        let project = ProjectConfig::load(project_root)?;
        let value = Self::resolve(&user.value, &project.value);
        let mut warnings = user.warnings;
        warnings.extend(project.warnings);
        Ok(Loaded { value, warnings })
    }
}

impl UserSettings {
    /// Разбирает текст `settings.toml`; `file` нужен для сообщений.
    pub fn parse(text: &str, file: &Path) -> Result<Loaded<Self>, SettingsError> {
        let parsed: raw::UserFile = raw::parse(text, file)?;
        parsed.validate(file)?;
        let warnings = parsed.warnings(file);
        Ok(Loaded {
            value: parsed.into_settings(),
            warnings,
        })
    }

    /// Читает `<bars_home>/settings.toml`. Нет файла — умолчания; файл при этом не создаётся.
    pub fn load(bars_home: &Path) -> Result<Loaded<Self>, SettingsError> {
        let file = user_settings_path(bars_home);
        match read_optional(&file)? {
            Some(text) => Self::parse(&text, &file),
            None => Ok(Loaded::clean(Self::default())),
        }
    }
}

impl ProjectConfig {
    /// Разбирает текст `.bars/config.toml`; `file` нужен для сообщений.
    pub fn parse(text: &str, file: &Path) -> Result<Loaded<Self>, SettingsError> {
        let parsed: raw::ProjectFile = raw::parse(text, file)?;
        parsed.validate(file)?;
        let warnings = parsed.warnings(file);
        Ok(Loaded {
            value: Self {
                docs_root: parsed.docs_root,
                search: parsed.search,
                chunking: parsed.chunking,
                indexing: parsed.indexing,
            },
            warnings,
        })
    }

    /// Читает `<проект>/.bars/config.toml`. Нет файла — проект без перекрытий.
    pub fn load(project_root: &Path) -> Result<Loaded<Self>, SettingsError> {
        let file = project_config_path(project_root);
        match read_optional(&file)? {
            Some(text) => Self::parse(&text, &file),
            None => Ok(Loaded::clean(Self::default())),
        }
    }
}

fn read_optional(file: &Path) -> Result<Option<String>, SettingsError> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SettingsError::Read {
            file: file.to_path_buf(),
            source,
        }),
    }
}

/// Прочитанное значение и предупреждения, которые файл заслужил.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded<T> {
    pub value: T,
    pub warnings: Vec<SettingsWarning>,
}

impl<T> Loaded<T> {
    fn clean(value: T) -> Self {
        Self {
            value,
            warnings: Vec::new(),
        }
    }
}

/// В файле есть ключ, которого bars не знает: опечатка или ключ из другой версии. Он игнорируется.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsWarning {
    pub file: PathBuf,
    /// Ключ вместе с таблицей: `search.min_char`.
    pub key: String,
}

impl fmt::Display for SettingsWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: неизвестный ключ `{}`, он игнорируется",
            self.file.display(),
            self.key
        )
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{}: файл не прочитан: {source}", file.display())]
    Read {
        file: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{}: {message}", file.display())]
    Parse { file: PathBuf, message: String },
    #[error("{}: ключ `{key}`: {reason}", file.display())]
    Invalid {
        file: PathBuf,
        key: String,
        reason: String,
    },
}

#[cfg(test)]
mod tests;
