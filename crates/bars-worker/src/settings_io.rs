//! Запись настроек из окна: `~/.bars/settings.toml` и `.bars/config.toml` проекта
//! (`docs/features/settings/settings.md`, `docs/features/window/settings-window.md`).
//!
//! Файл правится на месте через `toml_edit`: комментарии и порядок ключей сохраняются, меняются
//! только ключи, значения которых отличаются от прочитанных. Результат проверяется тем же разбором,
//! что при чтении, и пишется атомарно; не прошедший проверку файл не пишется (OQ-32).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use bars_core::models::Accelerator;
use bars_core::settings::{
    DEFAULT_EMBED_UNLOAD_MINUTES, DEFAULT_RERANK_UNLOAD_MINUTES, ProjectConfig, UserSettings,
    project_config_path, user_settings_path,
};
use bars_protocol::{ChunkingDoc, EmbedDoc, RerankDoc, SearchDoc, SettingsDoc};
use toml_edit::{DocumentMut, Item, Table, value};

// Все окна одного воркера сравнивают и записывают настройки последовательно.
static SETTINGS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn snapshot(bars_home: &Path) -> Result<bars_protocol::SettingsOutput, SaveError> {
    let file = user_settings_path(bars_home);
    let bytes = match fs::read(&file) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(source) => return Err(SaveError::Read { file, source }),
    };
    let revision = bytes
        .as_ref()
        .map(|bytes| format!("{}-{}", bytes.len(), blake3::hash(bytes).to_hex()));
    let text = String::from_utf8(bytes.unwrap_or_default())
        .map_err(|error| SaveError::Invalid(error.to_string()))?;
    let loaded =
        UserSettings::parse(&text, &file).map_err(|error| SaveError::Invalid(error.to_string()))?;
    Ok(bars_protocol::SettingsOutput {
        file: file.display().to_string(),
        revision,
        settings: to_doc(&loaded.value),
        warnings: loaded.warnings.iter().map(ToString::to_string).collect(),
    })
}

pub fn load_user_settings(bars_home: &Path) -> Result<bars_protocol::SettingsOutput, SaveError> {
    let _guard = SETTINGS_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    snapshot(bars_home)
}

pub fn save_changes(
    bars_home: &Path,
    params: bars_protocol::SettingsSaveParams,
) -> Result<bars_protocol::SettingsSaveOutput, SaveError> {
    let _guard = SETTINGS_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let current = snapshot(bars_home)?;
    let base = serde_json::to_value(&params.base).expect("settings serialize");
    let mut merged = serde_json::to_value(&current.settings).expect("settings serialize");
    let mut conflicts = Vec::new();
    for (path, mine) in &params.changes {
        let (section, key) = path
            .split_once('.')
            .ok_or_else(|| SaveError::Invalid(format!("Неизвестное поле настроек: {path}")))?;
        let disk = merged
            .get_mut(section)
            .and_then(|table| table.get_mut(key))
            .ok_or_else(|| SaveError::Invalid(format!("Неизвестное поле настроек: {path}")))?;
        if current.revision != params.expected_revision
            && *disk != base[section][key]
            && disk != mine
        {
            conflicts.push(bars_protocol::SettingsConflict {
                key: path.clone(),
                base: base[section][key].clone(),
                disk: disk.clone(),
                mine: mine.clone(),
            });
        }
        *disk = mine.clone();
    }
    let settings: SettingsDoc =
        serde_json::from_value(merged).map_err(|error| SaveError::Invalid(error.to_string()))?;
    if !conflicts.is_empty() {
        return Ok(bars_protocol::SettingsSaveOutput {
            status: bars_protocol::SettingsSaveStatus::Conflict,
            conflicts,
            current,
        });
    }
    write_user_settings(bars_home, &settings, Some(&current))?;
    Ok(bars_protocol::SettingsSaveOutput {
        status: bars_protocol::SettingsSaveStatus::Saved,
        conflicts,
        current: snapshot(bars_home)?,
    })
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    /// Значения не прошли проверку чтения: файл не записан.
    #[error("{0}")]
    Invalid(String),
    #[error("{}: файл не прочитан: {source}", file.display())]
    Read {
        file: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{}: файл не разбирается ({message}): bars его не менял", file.display())]
    Parse { file: PathBuf, message: String },
    #[error("{}: файл не записан: {source}", file.display())]
    Write {
        file: PathBuf,
        #[source]
        source: io::Error,
    },
}

fn accelerator_name(accelerator: Accelerator) -> bars_protocol::AcceleratorDoc {
    match accelerator {
        Accelerator::Auto => bars_protocol::AcceleratorDoc::Auto,
        Accelerator::Cpu => bars_protocol::AcceleratorDoc::Cpu,
    }
}

fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Настройки пользователя в том виде, в каком их показывает окно.
pub fn to_doc(user: &UserSettings) -> SettingsDoc {
    let embed = &user.embed;
    let rerank = &user.rerank;
    let search = &user.search;
    SettingsDoc {
        embed: EmbedDoc {
            accelerator: accelerator_name(embed.accelerator),
            source: embed.source.as_str().to_string(),
            model: embed.model.clone(),
            base_url: embed.base_url.clone(),
            api_key_name: embed.api_key_name.clone(),
            api_key_env: embed.api_key_env.clone(),
            batch_size: count(embed.batch_size),
            unload_after_minutes: user
                .unload
                .embed_minutes
                .unwrap_or(DEFAULT_EMBED_UNLOAD_MINUTES),
        },
        rerank: RerankDoc {
            accelerator: accelerator_name(rerank.accelerator),
            enabled: rerank.enabled,
            source: rerank.source.as_str().to_string(),
            model: rerank.model.clone(),
            base_url: rerank.base_url.clone(),
            api_key_name: rerank.api_key_name.clone(),
            api_key_env: rerank.api_key_env.clone(),
            parallel_requests: count(rerank.parallel_requests),
            context_tokens: count(rerank.context_tokens),
            unload_after_minutes: user
                .unload
                .rerank_minutes
                .unwrap_or(DEFAULT_RERANK_UNLOAD_MINUTES),
        },
        search: SearchDoc {
            candidates: count(search.candidates),
            rerank_candidates: count(search.rerank_candidates),
            results: count(search.results),
            results_reranked: count(search.results_reranked),
            min_score: search.min_score,
            agent_min_score: [search.agent_min_score.0, search.agent_min_score.1],
            max_results: count(search.max_results),
            max_candidates: count(search.max_candidates),
            max_rerank_candidates: count(search.max_rerank_candidates),
        },
        chunking: ChunkingDoc {
            min_chars: count(user.chunking.min_chars),
            max_chars: count(user.chunking.max_chars),
        },
        indexing: bars_protocol::IndexingDoc {
            edit_pause_seconds: user.indexing.edit_pause_seconds,
        },
        mcp: bars_protocol::McpDoc {
            graph_tools: user.mcp.graph_tools,
        },
    }
}

fn read_text(file: &Path) -> Result<String, SaveError> {
    match fs::read_to_string(file) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(source) => Err(SaveError::Read {
            file: file.to_path_buf(),
            source,
        }),
    }
}

fn write_atomic(file: &Path, text: &str) -> Result<(), SaveError> {
    let fail = |source| SaveError::Write {
        file: file.to_path_buf(),
        source,
    };
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(fail)?;
    }
    let mut name = file.file_name().unwrap_or_default().to_os_string();
    name.push(".bars-tmp");
    let temp = file.with_file_name(name);
    fs::write(&temp, text).map_err(fail)?;
    fs::rename(&temp, file).map_err(|error| {
        let _ = fs::remove_file(&temp);
        fail(error)
    })
}

fn parse_document(file: &Path, text: &str) -> Result<DocumentMut, SaveError> {
    text.parse::<DocumentMut>()
        .map_err(|error| SaveError::Parse {
            file: file.to_path_buf(),
            message: error.to_string(),
        })
}

fn table<'a>(doc: &'a mut DocumentMut, name: &str) -> &'a mut Table {
    let item = doc.entry(name).or_insert_with(|| Item::Table(Table::new()));
    if !item.is_table() {
        *item = Item::Table(Table::new());
    }
    item.as_table_mut().expect("только что стала таблицей")
}

/// Число с четырьмя знаками: `0.3f32` не должно попасть в файл как `0.30000001192…`.
fn round4(value: f32) -> f64 {
    (f64::from(value) * 10_000.0).round() / 10_000.0
}

fn set_text(table: &mut Table, key: &str, new: &str, old: &str) {
    if new != old {
        table.insert(key, value(new));
    }
}

fn set_optional(table: &mut Table, key: &str, new: &Option<String>, old: &Option<String>) {
    let new = new.as_deref().filter(|text| !text.trim().is_empty());
    if new != old.as_deref() {
        match new {
            Some(text) => {
                table.insert(key, value(text));
            }
            None => {
                table.remove(key);
            }
        }
    }
}

fn set_count(table: &mut Table, key: &str, new: u32, old: u32) {
    if new != old {
        table.insert(key, value(i64::from(new)));
    }
}

fn set_score(table: &mut Table, key: &str, new: f32, old: f32) {
    if (new - old).abs() > f32::EPSILON {
        table.insert(key, value(round4(new)));
    }
}

fn set_flag(table: &mut Table, key: &str, new: bool, old: bool) {
    if new != old {
        table.insert(key, value(new));
    }
}

/// Записывает `settings` в `<bars_home>/settings.toml`. Возвращает `true`, если файл изменился.
#[cfg(test)]
fn save_user_settings(bars_home: &Path, settings: &SettingsDoc) -> Result<bool, SaveError> {
    write_user_settings(bars_home, settings, None)
}

fn write_user_settings(
    bars_home: &Path,
    settings: &SettingsDoc,
    expected: Option<&bars_protocol::SettingsOutput>,
) -> Result<bool, SaveError> {
    let file = user_settings_path(bars_home);
    let text = read_text(&file)?;
    if let Some(expected) = expected {
        let revision = file
            .exists()
            .then(|| format!("{}-{}", text.len(), blake3::hash(text.as_bytes()).to_hex()));
        if revision != expected.revision {
            return Err(SaveError::Invalid(
                "Настройки снова изменились на диске. Повторите сохранение".into(),
            ));
        }
    }
    let current = UserSettings::parse(&text, &file)
        .map_err(|error| SaveError::Invalid(error.to_string()))?
        .value;
    let old = to_doc(&current);
    let mut doc = parse_document(&file, &text)?;

    let embed = table(&mut doc, "embed");
    set_text(embed, "source", &settings.embed.source, &old.embed.source);
    set_text(embed, "model", &settings.embed.model, &old.embed.model);
    // У внешнего источника model обязателен, даже если имя совпало с builtin default.
    if settings.embed.source != "builtin" && !embed.contains_key("model") {
        embed.insert("model", value(&settings.embed.model));
    }
    set_optional(
        embed,
        "base_url",
        &settings.embed.base_url,
        &old.embed.base_url,
    );
    set_optional(
        embed,
        "api_key_name",
        &settings.embed.api_key_name,
        &old.embed.api_key_name,
    );
    set_optional(
        embed,
        "api_key_env",
        &settings.embed.api_key_env,
        &old.embed.api_key_env,
    );
    set_count(
        embed,
        "batch_size",
        settings.embed.batch_size,
        old.embed.batch_size,
    );
    set_count(
        embed,
        "unload_after_minutes",
        settings.embed.unload_after_minutes,
        old.embed.unload_after_minutes,
    );

    let rerank = table(&mut doc, "rerank");
    set_flag(
        rerank,
        "enabled",
        settings.rerank.enabled,
        old.rerank.enabled,
    );
    set_text(
        rerank,
        "source",
        &settings.rerank.source,
        &old.rerank.source,
    );
    set_text(rerank, "model", &settings.rerank.model, &old.rerank.model);
    set_count(
        rerank,
        "parallel_requests",
        settings.rerank.parallel_requests,
        old.rerank.parallel_requests,
    );
    set_count(
        rerank,
        "context_tokens",
        settings.rerank.context_tokens,
        old.rerank.context_tokens,
    );
    if settings.rerank.source != "builtin" && !rerank.contains_key("model") {
        rerank.insert("model", value(&settings.rerank.model));
    }
    set_optional(
        rerank,
        "base_url",
        &settings.rerank.base_url,
        &old.rerank.base_url,
    );
    set_optional(
        rerank,
        "api_key_name",
        &settings.rerank.api_key_name,
        &old.rerank.api_key_name,
    );
    set_optional(
        rerank,
        "api_key_env",
        &settings.rerank.api_key_env,
        &old.rerank.api_key_env,
    );
    set_count(
        rerank,
        "unload_after_minutes",
        settings.rerank.unload_after_minutes,
        old.rerank.unload_after_minutes,
    );

    let (new, was) = (&settings.search, &old.search);
    let search = table(&mut doc, "search");
    set_count(search, "candidates", new.candidates, was.candidates);
    set_count(
        search,
        "rerank_candidates",
        new.rerank_candidates,
        was.rerank_candidates,
    );
    set_count(search, "results", new.results, was.results);
    set_count(
        search,
        "results_reranked",
        new.results_reranked,
        was.results_reranked,
    );
    // Прежние пороги переносятся в единый `min_score`: при сохранении ключи убираются, а значение,
    // которое загрузка уже вывела из них, записывается явно.
    if search.contains_key("min_score_vector") || search.contains_key("min_score_rerank") {
        search.insert("min_score", value(round4(new.min_score)));
        search.remove("min_score_vector");
        search.remove("min_score_rerank");
    } else {
        set_score(search, "min_score", new.min_score, was.min_score);
    }
    if new.agent_min_score != was.agent_min_score {
        let mut range = toml_edit::Array::new();
        range.push(round4(new.agent_min_score[0]));
        range.push(round4(new.agent_min_score[1]));
        search.insert("agent_min_score", value(range));
    }
    set_count(search, "max_results", new.max_results, was.max_results);
    set_count(
        search,
        "max_candidates",
        new.max_candidates,
        was.max_candidates,
    );
    set_count(
        search,
        "max_rerank_candidates",
        new.max_rerank_candidates,
        was.max_rerank_candidates,
    );

    // Materialize both values before removing the legacy shared preference.
    let migrate = doc
        .get("device")
        .and_then(Item::as_table)
        .is_some_and(|table| table.contains_key("accelerator"));
    for (role, preference, previous) in [
        ("embed", &settings.embed.accelerator, &old.embed.accelerator),
        (
            "rerank",
            &settings.rerank.accelerator,
            &old.rerank.accelerator,
        ),
    ] {
        if migrate || preference != previous {
            table(&mut doc, role).insert("accelerator", value(preference.as_str()));
        }
    }
    if let Some(device) = doc.get_mut("device").and_then(Item::as_table_mut) {
        device.remove("accelerator");
    }

    let chunking = table(&mut doc, "chunking");
    set_count(
        chunking,
        "min_chars",
        settings.chunking.min_chars,
        old.chunking.min_chars,
    );
    set_count(
        chunking,
        "max_chars",
        settings.chunking.max_chars,
        old.chunking.max_chars,
    );

    set_count(
        table(&mut doc, "indexing"),
        "edit_pause_seconds",
        settings.indexing.edit_pause_seconds,
        old.indexing.edit_pause_seconds,
    );

    set_flag(
        table(&mut doc, "mcp"),
        "graph_tools",
        settings.mcp.graph_tools,
        old.mcp.graph_tools,
    );

    // Пустые таблицы, которые мы сами завели, в файле не нужны.
    for name in [
        "embed", "rerank", "search", "device", "chunking", "indexing", "mcp",
    ] {
        if doc
            .get(name)
            .and_then(Item::as_table)
            .is_some_and(Table::is_empty)
        {
            doc.remove(name);
        }
    }

    let new_text = doc.to_string();
    UserSettings::parse(&new_text, &file).map_err(|error| SaveError::Invalid(error.to_string()))?;
    if new_text == text {
        return Ok(false);
    }
    if read_text(&file)? != text
        || expected.is_some_and(|snapshot| snapshot.revision.is_some() != file.exists())
    {
        return Err(SaveError::Invalid(
            "Настройки снова изменились на диске. Повторите сохранение".into(),
        ));
    }
    write_atomic(&file, &new_text)?;
    Ok(true)
}

/// Записывает `docs_root` в `.bars/config.toml` проекта; остальное в файле не трогается.
pub fn save_docs_root(project: &Path, docs_root: &str) -> Result<(), SaveError> {
    let file = project_config_path(project);
    let text = read_text(&file)?;
    let mut doc = parse_document(&file, &text)?;
    doc.insert("docs_root", value(docs_root));
    let new_text = doc.to_string();
    ProjectConfig::parse(&new_text, &file)
        .map_err(|error| SaveError::Invalid(error.to_string()))?;
    write_atomic(&file, &new_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_patch_merges_external_fields_and_preserves_comments() {
        let home = home();
        let file = user_settings_path(home.path());
        let base = load_user_settings(home.path()).unwrap();
        assert_eq!(base.revision, None);
        let mut mine = base.settings.clone();
        mine.search.results = 9;
        fs::write(
            &file,
            "# outside\n[search]\ncandidates = 40 # keep\n[future]\nkey = 7\n",
        )
        .unwrap();
        let result = save_changes(
            home.path(),
            bars_protocol::SettingsSaveParams::between(base, mine),
        )
        .unwrap();
        assert_eq!(result.status, bars_protocol::SettingsSaveStatus::Saved);
        assert_eq!(result.current.settings.search.results, 9);
        assert_eq!(result.current.settings.search.candidates, 40);
        assert!(result.current.revision.is_some());
        let text = fs::read_to_string(file).unwrap();
        assert!(text.contains("# outside"));
        assert!(text.contains("candidates = 40 # keep"));
        assert!(text.contains("key = 7"));
    }

    #[test]
    fn settings_conflict_writes_nothing_and_retry_still_checks_revision() {
        let home = home();
        let file = user_settings_path(home.path());
        let base = load_user_settings(home.path()).unwrap();
        let mut mine = base.settings.clone();
        mine.search.results = 9;
        mine.search.candidates = 41;
        let mut params = bars_protocol::SettingsSaveParams::between(base, mine);
        fs::write(&file, "[search]\nresults = 8\n").unwrap();
        let conflict = save_changes(home.path(), params.clone()).unwrap();
        assert_eq!(conflict.status, bars_protocol::SettingsSaveStatus::Conflict);
        assert_eq!(conflict.conflicts.len(), 1);
        assert_eq!(conflict.conflicts[0].key, "search.results");
        assert_eq!(conflict.conflicts[0].disk, 8);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "[search]\nresults = 8\n"
        );
        params.base = conflict.current.settings;
        params.expected_revision = conflict.current.revision;
        fs::write(&file, "[search]\nresults = 7\n").unwrap();
        let second = save_changes(home.path(), params.clone()).unwrap();
        assert_eq!(second.status, bars_protocol::SettingsSaveStatus::Conflict);
        params.base = second.current.settings;
        params.expected_revision = second.current.revision;
        let saved = save_changes(home.path(), params).unwrap();
        assert_eq!(saved.status, bars_protocol::SettingsSaveStatus::Saved);
        assert_eq!(saved.current.settings.search.results, 9);
        assert_eq!(saved.current.settings.search.candidates, 41);
    }

    #[test]
    fn settings_patch_deletion_identical_edits_and_invalid_values() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "[search]\nresults = 8\n").unwrap();
        let base = load_user_settings(home.path()).unwrap();
        let mut mine = base.settings.clone();
        mine.search.results = 9;
        let params = bars_protocol::SettingsSaveParams::between(base, mine);
        fs::remove_file(&file).unwrap();
        assert_eq!(
            save_changes(home.path(), params.clone()).unwrap().status,
            bars_protocol::SettingsSaveStatus::Conflict
        );
        assert!(!file.exists());
        fs::write(&file, "# same change\n[search]\nresults = 9\n").unwrap();
        assert_eq!(
            save_changes(home.path(), params).unwrap().status,
            bars_protocol::SettingsSaveStatus::Saved
        );
        let base = load_user_settings(home.path()).unwrap();
        let mut mine = base.settings.clone();
        mine.search.results = 0;
        let before = fs::read(&file).unwrap();
        assert!(
            save_changes(
                home.path(),
                bars_protocol::SettingsSaveParams::between(base.clone(), mine)
            )
            .is_err()
        );
        let mut params =
            bars_protocol::SettingsSaveParams::between(base.clone(), base.settings.clone());
        params
            .changes
            .insert("search.typo".into(), serde_json::json!(7));
        assert!(save_changes(home.path(), params).is_err());
        assert_eq!(fs::read(file).unwrap(), before);
    }

    #[test]
    fn settings_two_writers_cannot_silently_replace_each_other() {
        let home = home();
        let base = load_user_settings(home.path()).unwrap();
        fs::write(user_settings_path(home.path()), "").unwrap();
        assert!(load_user_settings(home.path()).unwrap().revision.is_some());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = [8, 9]
                .into_iter()
                .map(|value| {
                    let barrier = barrier.clone();
                    let base = base.clone();
                    let path = home.path();
                    scope.spawn(move || {
                        let mut mine = base.settings.clone();
                        mine.search.results = value;
                        barrier.wait();
                        save_changes(path, bars_protocol::SettingsSaveParams::between(base, mine))
                            .unwrap()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            results
                .iter()
                .filter(|result| result.status == bars_protocol::SettingsSaveStatus::Saved)
                .count(),
            1
        );
        assert_eq!(
            results
                .iter()
                .filter(|result| result.status == bars_protocol::SettingsSaveStatus::Conflict)
                .count(),
            1
        );
    }

    #[test]
    fn shared_device_is_migrated_once_without_losing_other_keys() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "# user choice\n[device]\naccelerator = 'cpu'\nother = 7\n[embed]\naccelerator = 'auto'\n").unwrap();
        let doc = current(home.path());
        assert_eq!(doc.embed.accelerator.as_str(), "auto");
        assert_eq!(doc.rerank.accelerator.as_str(), "cpu");
        save_user_settings(home.path(), &doc).unwrap();
        let saved = fs::read_to_string(&file).unwrap();
        assert!(saved.contains("# user choice"));
        assert!(saved.contains("other = 7"));
        let parsed: toml::Value = toml::from_str(&saved).unwrap();
        assert!(parsed["device"].get("accelerator").is_none());
        assert_eq!(parsed["rerank"]["accelerator"].as_str(), Some("cpu"));
        assert!(!save_user_settings(home.path(), &doc).unwrap());
    }

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn current(home: &Path) -> SettingsDoc {
        to_doc(&UserSettings::load(home).unwrap().value)
    }

    #[test]
    fn defaults_roundtrip_without_creating_a_file() {
        let home = home();
        let doc = current(home.path());
        assert_eq!(doc.embed.source, "builtin");
        assert_eq!(doc.search.agent_min_score, [0.0, 1.0]);
        assert!(!save_user_settings(home.path(), &doc).unwrap());
        assert!(!user_settings_path(home.path()).exists());
    }

    #[test]
    fn tei_settings_roundtrip_both_models() {
        let home = home();
        let mut doc = current(home.path());
        doc.embed.source = "tei".into();
        doc.embed.model = "BAAI/bge-m3".into();
        doc.embed.base_url = Some("http://127.0.0.1:8080".into());
        doc.rerank.enabled = true;
        doc.rerank.source = "tei".into();
        doc.rerank.model = "BAAI/bge-reranker-v2-m3".into();
        doc.rerank.base_url = Some("http://127.0.0.1:8081".into());
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert_eq!(current(home.path()), doc);
    }

    #[test]
    fn ollama_and_lmstudio_embed_rerank_roundtrip_and_validate_without_writing() {
        external_settings_roundtrip("ollama");
        external_settings_roundtrip("lmstudio");
    }

    fn external_settings_roundtrip(source: &str) {
        let home = home();
        let mut doc = current(home.path());
        doc.embed.source = source.into();
        doc.embed.model = "qwen3-embedding:0.6b".into();
        doc.embed.base_url = Some("http://127.0.0.1:11434".into());
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert_eq!(current(home.path()), doc);
        doc.rerank.enabled = true;
        doc.rerank.source = source.into();
        doc.rerank.model = "Qwen3-Reranker-0.6B".into();
        doc.rerank.base_url = doc.embed.base_url.clone();
        doc.rerank.parallel_requests = 2;
        doc.rerank.context_tokens = 16384;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert_eq!(current(home.path()), doc);
        let before = fs::read_to_string(user_settings_path(home.path())).unwrap();
        doc.rerank.parallel_requests = 17;
        assert!(save_user_settings(home.path(), &doc).is_err());
        assert_eq!(
            fs::read_to_string(user_settings_path(home.path())).unwrap(),
            before
        );
    }

    #[test]
    fn indexing_pause_roundtrips_and_rejects_out_of_range_values() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(
            &file,
            "# моя пауза\n[indexing]\nedit_pause_seconds = 30 # секунд\n",
        )
        .unwrap();
        let mut doc = current(home.path());
        doc.indexing.edit_pause_seconds = 0;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert_eq!(current(home.path()).indexing.edit_pause_seconds, 0);
        assert!(fs::read_to_string(&file).unwrap().contains("# моя пауза"));
        let before = fs::read_to_string(&file).unwrap();
        doc.indexing.edit_pause_seconds = 3601;
        assert!(save_user_settings(home.path(), &doc).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), before);
        doc.indexing.edit_pause_seconds = 3600;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert_eq!(current(home.path()).indexing.edit_pause_seconds, 3600);
    }

    #[test]
    fn graph_tools_flag_roundtrips_and_leaves_no_empty_table() {
        let home = home();
        let file = user_settings_path(home.path());
        let mut doc = current(home.path());
        assert!(!doc.mcp.graph_tools);
        assert!(!save_user_settings(home.path(), &doc).unwrap());
        assert!(!file.exists(), "умолчание файл не создаёт");

        doc.mcp.graph_tools = true;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert!(fs::read_to_string(&file).unwrap().contains("[mcp]"));
        assert!(current(home.path()).mcp.graph_tools);

        doc.mcp.graph_tools = false;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        assert!(!current(home.path()).mcp.graph_tools);
    }

    #[test]
    fn only_changed_keys_are_written_and_comments_survive() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "# мои заметки\n[search]\nresults = 7 # люблю семь\n").unwrap();
        let mut doc = current(home.path());
        doc.search.min_score = 0.35;
        doc.rerank.enabled = true;
        assert!(save_user_settings(home.path(), &doc).unwrap());

        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("# мои заметки"));
        assert!(text.contains("results = 7 # люблю семь"));
        assert!(text.contains("min_score = 0.35"), "{text}");
        assert!(text.contains("enabled = true"));
        assert!(
            !text.contains("candidates"),
            "умолчания в файл не пишутся: {text}"
        );
        let again = current(home.path());
        assert!(again.rerank.enabled);
        assert_eq!(again.search.results, 7);
    }

    #[test]
    fn remote_provider_is_saved_and_cleared_keys_are_removed() {
        let home = home();
        let mut doc = current(home.path());
        doc.embed.source = "remote".into();
        doc.embed.model = "text-embedding-3-small".into();
        doc.embed.base_url = Some("https://api.example.com/v1".into());
        doc.embed.api_key_name = Some("bars-embed".into());
        save_user_settings(home.path(), &doc).unwrap();
        let saved = current(home.path());
        assert_eq!(saved.embed.source, "remote");
        assert_eq!(saved.embed.api_key_name.as_deref(), Some("bars-embed"));

        doc.embed.source = "builtin".into();
        doc.embed.model = "BAAI/bge-m3".into();
        doc.embed.base_url = Some("  ".into());
        doc.embed.api_key_name = None;
        save_user_settings(home.path(), &doc).unwrap();
        let text = fs::read_to_string(user_settings_path(home.path())).unwrap();
        assert!(
            !text.contains("base_url") && !text.contains("api_key_name"),
            "{text}"
        );
        assert_eq!(current(home.path()).embed.source, "builtin");
    }

    #[test]
    fn invalid_values_are_rejected_and_the_file_stays() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "[search]\nresults = 4\n").unwrap();
        let mut doc = current(home.path());
        doc.embed.source = "remote".into();
        doc.embed.model = "m".into();
        // Нет адреса и ключа: чтение такой файл не примет.
        let error = save_user_settings(home.path(), &doc).unwrap_err();
        assert!(matches!(error, SaveError::Invalid(_)), "{error}");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "[search]\nresults = 4\n"
        );

        let mut doc = current(home.path());
        doc.search.candidates = 0;
        assert!(matches!(
            save_user_settings(home.path(), &doc).unwrap_err(),
            SaveError::Invalid(_)
        ));
    }

    #[test]
    fn unload_minutes_are_written_per_model_and_checked() {
        let home = home();
        let mut doc = current(home.path());
        assert_eq!(doc.embed.unload_after_minutes, 15);
        assert_eq!(doc.rerank.unload_after_minutes, 5);
        doc.embed.unload_after_minutes = 0;
        doc.rerank.unload_after_minutes = 30;
        assert!(save_user_settings(home.path(), &doc).unwrap());
        let parsed: toml::Value =
            toml::from_str(&fs::read_to_string(user_settings_path(home.path())).unwrap()).unwrap();
        assert_eq!(
            parsed["embed"]["unload_after_minutes"].as_integer(),
            Some(0)
        );
        assert_eq!(
            parsed["rerank"]["unload_after_minutes"].as_integer(),
            Some(30)
        );
        let again = current(home.path());
        assert_eq!(again.embed.unload_after_minutes, 0);
        assert_eq!(again.rerank.unload_after_minutes, 30);

        doc.embed.unload_after_minutes = 20_000;
        assert!(matches!(
            save_user_settings(home.path(), &doc).unwrap_err(),
            SaveError::Invalid(_)
        ));
    }

    #[test]
    fn a_broken_file_is_not_replaced() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "[search\n").unwrap();
        let doc = to_doc(&UserSettings::default());
        assert!(save_user_settings(home.path(), &doc).is_err());
        assert_eq!(fs::read_to_string(&file).unwrap(), "[search\n");
    }

    #[test]
    fn docs_root_is_written_next_to_other_keys() {
        let project = tempfile::tempdir().unwrap();
        let file = project_config_path(project.path());
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "# проект\n[search]\nresults = 2\n").unwrap();
        save_docs_root(project.path(), "documentation").unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("# проект") && text.contains("results = 2"));
        let loaded = ProjectConfig::load(project.path()).unwrap().value;
        let settings = bars_core::settings::Settings::resolve(&UserSettings::default(), &loaded);
        assert_eq!(settings.docs_root, "documentation");
        assert_eq!(settings.search.results, 2);
    }
}
