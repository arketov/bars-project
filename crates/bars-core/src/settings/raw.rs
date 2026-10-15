//! Таблицы файлов настроек, как они записаны: каждый ключ необязателен, лишние ключи собираются
//! для предупреждения (`docs/features/settings/settings.md`, OQ-33).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{
    EmbedSettings, ModelSource, RerankSettings, SettingsError, SettingsWarning, UserSettings,
};
use crate::indexing::chunking::ChunkingParams;
use crate::models::Accelerator;
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
    #[serde(flatten)]
    unknown: Unknown,
}

/// `.bars/config.toml` проекта.
#[derive(Debug, Default, Deserialize)]
pub(super) struct ProjectFile {
    pub(super) docs_root: Option<String>,
    pub(super) search: Option<SearchOverrides>,
    pub(super) chunking: Option<ChunkingOverrides>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct EmbedTable {
    source: Option<ModelSource>,
    model: Option<String>,
    base_url: Option<String>,
    api_key_name: Option<String>,
    api_key_env: Option<String>,
    batch_size: Option<usize>,
    #[serde(flatten)]
    unknown: Unknown,
}

#[derive(Debug, Default, Deserialize)]
struct RerankTable {
    enabled: Option<bool>,
    source: Option<ModelSource>,
    model: Option<String>,
    base_url: Option<String>,
    api_key_name: Option<String>,
    api_key_env: Option<String>,
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

impl UserFile {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
        if let Some(embed) = &self.embed {
            positive(file, "embed.batch_size", embed.batch_size)?;
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
        if let Some(embed) = self.embed {
            embed.apply(&mut settings.embed);
        }
        if let Some(rerank) = self.rerank {
            rerank.apply(&mut settings.rerank);
        }
        if let Some(search) = &self.search {
            search.apply(&mut settings.search);
        }
        if let Some(accelerator) = self.device.and_then(|device| device.accelerator) {
            settings.accelerator = match accelerator {
                AcceleratorName::Auto => Accelerator::Auto,
                AcceleratorName::Cpu => Accelerator::Cpu,
            };
        }
        if let Some(chunking) = &self.chunking {
            chunking.apply(&mut settings.chunking);
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
            note_unknown(&device.unknown, "device", file, &mut warnings);
        }
        if let Some(search) = &self.search {
            search.note_unknown(file, &mut warnings);
        }
        if let Some(chunking) = &self.chunking {
            chunking.note_unknown(file, &mut warnings);
        }
        warnings
    }
}

impl ProjectFile {
    pub(super) fn validate(&self, file: &Path) -> Result<(), SettingsError> {
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
        warnings
    }
}

impl EmbedTable {
    fn apply(self, target: &mut EmbedSettings) {
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
    fn apply(self, target: &mut RerankSettings) {
        if let Some(value) = self.enabled {
            target.enabled = value;
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
