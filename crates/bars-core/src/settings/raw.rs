//! Таблицы файлов настроек, как они записаны: каждый ключ необязателен, лишние ключи собираются
//! для предупреждения (`docs/features/settings/settings.md`, OQ-33).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{
    EmbedSettings, IndexingParams, MAX_EDIT_PAUSE_SECONDS, MAX_UNLOAD_MINUTES, ModelSource,
    RerankSettings, SettingsError, SettingsWarning, UserSettings,
};
use crate::indexing::chunking::ChunkingParams;
use crate::models::{Accelerator, BGE_RERANKER_V2_M3_ID, builtin_model, builtin_models};
use crate::search::SearchSettings;

/// Ключи, которых нет в схеме: имя и значение. Значение не нужно, но `flatten` требует тип.
pub(super) type Unknown = BTreeMap<String, toml::Value>;

/// `settings.toml` пользователя.
#[derive(Debug, Default, Deserialize)]
pub(super) struct UserFile {
    embed: Option<EmbedTable>,
    rerank: Option<RerankTable>,
    pub(super) search: Option<SearchOverrides>,
    device: Option<DeviceTable>,
    pub(super) chunking: Option<ChunkingOverrides>,
    pub(super) indexing: Option<IndexingOverrides>,
    mcp: Option<McpTable>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// Таблица `[mcp]`.
#[derive(Debug, Default, Deserialize)]
struct McpTable {
    graph_tools: Option<bool>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// `.bars/config.toml` проекта.
#[derive(Debug, Default, Deserialize)]
pub(super) struct ProjectFile {
    pub(super) docs_root: Option<String>,
    pub(super) search: Option<SearchOverrides>,
    pub(super) chunking: Option<ChunkingOverrides>,
    pub(super) indexing: Option<IndexingOverrides>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct EmbedTable {
    source: Option<ModelSource>,
    model: Option<String>,
    accelerator: Option<AcceleratorName>,
    base_url: Option<String>,
    api_key_name: Option<String>,
    api_key_env: Option<String>,
    batch_size: Option<usize>,
    unload_after_minutes: Option<u32>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct RerankTable {
    enabled: Option<bool>,
    source: Option<ModelSource>,
    model: Option<String>,
    accelerator: Option<AcceleratorName>,
    base_url: Option<String>,
    api_key_name: Option<String>,
    api_key_env: Option<String>,
    unload_after_minutes: Option<u32>,
    parallel_requests: Option<usize>,
    context_tokens: Option<usize>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct DeviceTable {
    accelerator: Option<AcceleratorName>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum AcceleratorName {
    Auto,
    Cpu,
}

impl From<AcceleratorName> for Accelerator {
    fn from(value: AcceleratorName) -> Self {
        match value {
            AcceleratorName::Auto => Self::Auto,
            AcceleratorName::Cpu => Self::Cpu,
        }
    }
}

/// Таблица `[search]`: в `settings.toml` и в `.bars/config.toml` ключи одни и те же.
#[derive(Debug, Default, Clone, Deserialize)]
pub(super) struct SearchOverrides {
    candidates: Option<usize>,
    rerank_candidates: Option<usize>,
    results: Option<usize>,
    results_reranked: Option<usize>,
    min_score_vector: Option<f32>,
    min_score_rerank: Option<f32>,
    agent_min_score: Option<[f32; 2]>,
    max_results: Option<usize>,
    max_candidates: Option<usize>,
    max_rerank_candidates: Option<usize>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// Таблица `[chunking]`.
#[derive(Debug, Default, Clone, Deserialize)]
pub(super) struct ChunkingOverrides {
    min_chars: Option<usize>,
    max_chars: Option<usize>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// Таблица `[indexing]`.
#[derive(Debug, Default, Clone, Deserialize)]
pub(super) struct IndexingOverrides {
    edit_pause_seconds: Option<u32>,
    #[serde(flatten)]
    unknown: Unknown,
}

/// Разбирает текст файла; битый TOML и неверный тип — ошибка с именем файла.
pub(super) fn parse<T: for<'de> Deserialize<'de>>(
    text: &str,
    file: &Path,
) -> Result<T, SettingsError> {
    toml::from_str(text).map_err(|error| SettingsError::Parse {
        file: file.to_path_buf(),
        message: error.to_string(),
    })
}

fn invalid(file: &Path, key: &str, reason: &str) -> SettingsError {
    SettingsError::Invalid {
        file: file.to_path_buf(),
        key: key.to_string(),
        reason: reason.to_string(),
    }
}

/// Срок выгрузки: `0` — «никогда», больше недели — опечатка, а не намерение.
fn unload_minutes(file: &Path, key: &str, value: Option<u32>) -> Result<(), SettingsError> {
    match value {
        Some(minutes) if minutes > MAX_UNLOAD_MINUTES => Err(invalid(
            file,
            key,
            "больше 10080 минут (неделя); чтобы не выгружать модель, укажите 0",
        )),
        _ => Ok(()),
    }
}

fn positive(file: &Path, key: &str, value: Option<usize>) -> Result<(), SettingsError> {
    match value {
        Some(0) => Err(invalid(file, key, "должно быть больше нуля")),
        _ => Ok(()),
    }
}

fn score(file: &Path, key: &str, value: Option<f32>) -> Result<(), SettingsError> {
    match value {
        Some(value) if !(0.0..=1.0).contains(&value) => {
            Err(invalid(file, key, "оценка лежит от 0 до 1"))
        }
        _ => Ok(()),
    }
}

fn note_unknown(unknown: &Unknown, table: &str, file: &Path, warnings: &mut Vec<SettingsWarning>) {
    for key in unknown.keys() {
        let key = if table.is_empty() {
            key.clone()
        } else {
            format!("{table}.{key}")
        };
        warnings.push(SettingsWarning {
            file: file.to_path_buf(),
            key,
        });
    }
}

impl SearchOverrides {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        for (key, value) in [
            ("search.candidates", self.candidates),
            ("search.rerank_candidates", self.rerank_candidates),
            ("search.results", self.results),
            ("search.results_reranked", self.results_reranked),
            ("search.max_results", self.max_results),
            ("search.max_candidates", self.max_candidates),
            ("search.max_rerank_candidates", self.max_rerank_candidates),
        ] {
            positive(file, key, value)?;
        }
        score(file, "search.min_score_vector", self.min_score_vector)?;
        score(file, "search.min_score_rerank", self.min_score_rerank)?;
        if let Some([low, high]) = self.agent_min_score {
            let key = "search.agent_min_score";
            score(file, key, Some(low))?;
            score(file, key, Some(high))?;
            if low > high {
                return Err(invalid(file, key, "нижний предел выше верхнего"));
            }
        }
        Ok(())
    }

    pub(super) fn apply(&self, target: &mut SearchSettings) {
        macro_rules! set {
            ($($field:ident),*) => {$(
                if let Some(value) = self.$field {
                    target.$field = value;
                }
            )*};
        }
        set!(
            candidates,
            rerank_candidates,
            results,
            results_reranked,
            min_score_vector,
            min_score_rerank,
            max_results,
            max_candidates,
            max_rerank_candidates
        );
        if let Some([low, high]) = self.agent_min_score {
            target.agent_min_score = (low, high);
        }
    }

    pub(super) fn note_unknown(&self, file: &Path, warnings: &mut Vec<SettingsWarning>) {
        note_unknown(&self.unknown, "search", file, warnings);
    }
}

impl ChunkingOverrides {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        positive(file, "chunking.min_chars", self.min_chars)?;
        positive(file, "chunking.max_chars", self.max_chars)
    }

    pub(super) fn apply(&self, target: &mut ChunkingParams) {
        if let Some(value) = self.min_chars {
            target.min_chars = value;
        }
        if let Some(value) = self.max_chars {
            target.max_chars = value;
        }
    }

    pub(super) fn note_unknown(&self, file: &Path, warnings: &mut Vec<SettingsWarning>) {
        note_unknown(&self.unknown, "chunking", file, warnings);
    }
}

impl IndexingOverrides {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        match self.edit_pause_seconds {
            Some(seconds) if seconds > MAX_EDIT_PAUSE_SECONDS => Err(invalid(
                file,
                "indexing.edit_pause_seconds",
                "больше 3600 секунд (час); чтобы не ждать, укажите 0",
            )),
            _ => Ok(()),
        }
    }

    pub(super) fn apply(&self, target: &mut IndexingParams) {
        if let Some(value) = self.edit_pause_seconds {
            target.edit_pause_seconds = value;
        }
    }

    pub(super) fn note_unknown(&self, file: &Path, warnings: &mut Vec<SettingsWarning>) {
        note_unknown(&self.unknown, "indexing", file, warnings);
    }
}

impl UserFile {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        if let Some(indexing) = &self.indexing {
            indexing.validate(file)?;
        }
        if let Some(embed) = &self.embed {
            positive(file, "embed.batch_size", embed.batch_size)?;
            unload_minutes(
                file,
                "embed.unload_after_minutes",
                embed.unload_after_minutes,
            )?;
            embed.validate(file)?;
        }
        if let Some(rerank) = &self.rerank {
            // Срок проверяется и у выключенного rerank: его validate смотрит только включённый.
            unload_minutes(
                file,
                "rerank.unload_after_minutes",
                rerank.unload_after_minutes,
            )?;
            rerank.validate(file)?;
        }
        if let Some(search) = &self.search {
            search.validate(file)?;
        }
        if let Some(chunking) = &self.chunking {
            chunking.validate(file)?;
        }
        Ok(())
    }

    pub(super) fn into_settings(self) -> UserSettings {
        let mut settings = UserSettings::default();
        if let Some(device) = self.device.and_then(|d| d.accelerator) {
            settings.embed.accelerator = device.into();
            settings.rerank.accelerator = device.into();
        }
        settings.unload.embed_minutes = self.embed.as_ref().and_then(|t| t.unload_after_minutes);
        settings.unload.rerank_minutes = self.rerank.as_ref().and_then(|t| t.unload_after_minutes);
        if let Some(embed) = self.embed {
            embed.apply(&mut settings.embed);
        }
        if let Some(rerank) = self.rerank {
            rerank.apply(&mut settings.rerank);
        }
        if let Some(search) = &self.search {
            search.apply(&mut settings.search);
        }
        if let Some(chunking) = &self.chunking {
            chunking.apply(&mut settings.chunking);
        }
        if let Some(indexing) = &self.indexing {
            indexing.apply(&mut settings.indexing);
        }
        if let Some(value) = self.mcp.as_ref().and_then(|table| table.graph_tools) {
            settings.mcp.graph_tools = value;
        }
        settings
    }

    pub(super) fn warnings(&self, file: &Path) -> Vec<SettingsWarning> {
        let mut warnings = Vec::new();
        note_unknown(&self.unknown, "", file, &mut warnings);
        if let Some(embed) = &self.embed {
            note_unknown(&embed.unknown, "embed", file, &mut warnings);
        }
        if let Some(rerank) = &self.rerank {
            note_unknown(&rerank.unknown, "rerank", file, &mut warnings);
        }
        if let Some(device) = &self.device {
            if device.accelerator.is_some() {
                warnings.push(SettingsWarning {
                    file: file.to_path_buf(),
                    key: "device.accelerator (legacy; save to migrate to embed/rerank.accelerator)"
                        .into(),
                });
            }
            note_unknown(&device.unknown, "device", file, &mut warnings);
        }
        if let Some(search) = &self.search {
            search.note_unknown(file, &mut warnings);
        }
        if let Some(chunking) = &self.chunking {
            chunking.note_unknown(file, &mut warnings);
        }
        if let Some(indexing) = &self.indexing {
            indexing.note_unknown(file, &mut warnings);
        }
        if let Some(mcp) = &self.mcp {
            note_unknown(&mcp.unknown, "mcp", file, &mut warnings);
        }
        warnings
    }
}

impl ProjectFile {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        if let Some(indexing) = &self.indexing {
            indexing.validate(file)?;
        }
        if let Some(search) = &self.search {
            search.validate(file)?;
        }
        if let Some(chunking) = &self.chunking {
            chunking.validate(file)?;
        }
        Ok(())
    }

    pub(super) fn warnings(&self, file: &Path) -> Vec<SettingsWarning> {
        let mut warnings = Vec::new();
        note_unknown(&self.unknown, "", file, &mut warnings);
        if let Some(search) = &self.search {
            search.note_unknown(file, &mut warnings);
        }
        if let Some(chunking) = &self.chunking {
            chunking.note_unknown(file, &mut warnings);
        }
        if let Some(indexing) = &self.indexing {
            indexing.note_unknown(file, &mut warnings);
        }
        warnings
    }
}

/// Поля провайдера, как они записаны в таблице (`docs/features/settings/settings.md`,
/// «Как bars читает файлы настроек»).
struct ProviderFields<'a> {
    table: &'static str,
    source: Option<ModelSource>,
    model: Option<&'a str>,
    base_url: Option<&'a str>,
    api_key_name: Option<&'a str>,
    api_key_env: Option<&'a str>,
}

impl ProviderFields<'_> {
    fn validate(
        &self,
        file: &Path,
        builtin_ok: impl Fn(&str) -> Result<(), String>,
    ) -> Result<(), SettingsError> {
        let key = |name: &str| format!("{}.{name}", self.table);
        let invalid = |name: &str, reason: String| SettingsError::Invalid {
            file: file.to_path_buf(),
            key: key(name),
            reason,
        };
        for (name, value) in [
            ("model", self.model),
            ("api_key_name", self.api_key_name),
            ("api_key_env", self.api_key_env),
        ] {
            if value.is_some_and(|text| text.trim().is_empty()) {
                return Err(invalid(name, "значение пустое".into()));
            }
        }
        match self.source.unwrap_or(ModelSource::Builtin) {
            ModelSource::Builtin => {
                if let Some(model) = self.model {
                    builtin_ok(model).map_err(|reason| invalid("model", reason))?;
                }
                Ok(())
            }
            source => {
                if self.model.is_none() {
                    return Err(invalid(
                        "model",
                        format!(
                            "для источника `{}` нужно имя модели у провайдера",
                            source.as_str()
                        ),
                    ));
                }
                let Some(base_url) = self.base_url else {
                    return Err(invalid(
                        "base_url",
                        format!("для источника `{}` нужен адрес провайдера", source.as_str()),
                    ));
                };
                check_base_url(base_url).map_err(|reason| invalid("base_url", reason))?;
                if source == ModelSource::Remote
                    && self.api_key_name.is_none()
                    && self.api_key_env.is_none()
                {
                    return Err(invalid(
                        "api_key_name",
                        "удалённому провайдеру нужен ключ: `api_key_name` (запись в хранилище ОС) или `api_key_env` (переменная окружения)".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Адрес провайдера: `http://` или `https://`, хост, без пробелов и без логина с паролем.
fn check_base_url(url: &str) -> Result<(), String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .ok_or("адрес должен начинаться с http:// или https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() || url.chars().any(char::is_whitespace) {
        return Err("в адресе нет хоста или есть пробелы".into());
    }
    if authority.contains('@') {
        return Err("логин и пароль в адресе не поддерживаются: ключ задаётся отдельно".into());
    }
    Ok(())
}

impl EmbedTable {
    fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        ProviderFields {
            table: "embed",
            source: self.source,
            model: self.model.as_deref(),
            base_url: self.base_url.as_deref(),
            api_key_name: self.api_key_name.as_deref(),
            api_key_env: self.api_key_env.as_deref(),
        }
        .validate(file, |model| {
            builtin_model(model).map(|_| ()).ok_or_else(|| {
                let ids: Vec<_> = builtin_models().iter().map(|m| m.id).collect();
                format!(
                    "`{model}` нет во встроенном каталоге; доступны: {}",
                    ids.join(", ")
                )
            })
        })
    }

    fn apply(self, target: &mut EmbedSettings) {
        if let Some(value) = self.accelerator {
            target.accelerator = value.into();
        }
        if let Some(value) = self.source {
            target.source = value;
        }
        if let Some(value) = self.model {
            target.model = value;
        }
        target.base_url = self.base_url.or(target.base_url.take());
        target.api_key_name = self.api_key_name.or(target.api_key_name.take());
        target.api_key_env = self.api_key_env.or(target.api_key_env.take());
        if let Some(value) = self.batch_size {
            target.batch_size = value;
        }
    }
}

impl RerankTable {
    /// Выключенный rerank не проверяется: окно может сохранять таблицу по шагам.
    fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        if self
            .parallel_requests
            .is_some_and(|n| !(1..=16).contains(&n))
        {
            return Err(invalid(
                file,
                "rerank.parallel_requests",
                "целое число от 1 до 16",
            ));
        }
        if self
            .context_tokens
            .is_some_and(|n| !(512..=131072).contains(&n))
        {
            return Err(invalid(
                file,
                "rerank.context_tokens",
                "целое число от 512 до 131072",
            ));
        }
        if self.enabled != Some(true) {
            return Ok(());
        }
        ProviderFields {
            table: "rerank",
            source: self.source,
            model: self.model.as_deref(),
            base_url: self.base_url.as_deref(),
            api_key_name: self.api_key_name.as_deref(),
            api_key_env: self.api_key_env.as_deref(),
        }
        .validate(file, |model| {
            if model == BGE_RERANKER_V2_M3_ID {
                Ok(())
            } else {
                Err(format!(
                    "встроенный rerank один: {BGE_RERANKER_V2_M3_ID}, а не `{model}`"
                ))
            }
        })
    }

    fn apply(self, target: &mut RerankSettings) {
        if let Some(value) = self.parallel_requests {
            target.parallel_requests = value;
        }
        if let Some(value) = self.context_tokens {
            target.context_tokens = value;
        }
        if let Some(value) = self.enabled {
            target.enabled = value;
        }
        if let Some(value) = self.accelerator {
            target.accelerator = value.into();
        }
        if let Some(value) = self.source {
            target.source = value;
        }
        if let Some(value) = self.model {
            target.model = value;
        }
        target.base_url = self.base_url.or(target.base_url.take());
        target.api_key_name = self.api_key_name.or(target.api_key_name.take());
        target.api_key_env = self.api_key_env.or(target.api_key_env.take());
    }
}
