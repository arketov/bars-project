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
use bars_core::settings::{ProjectConfig, UserSettings, project_config_path, user_settings_path};
use bars_protocol::{ChunkingDoc, DeviceDoc, EmbedDoc, RerankDoc, SearchDoc, SettingsDoc};
use toml_edit::{DocumentMut, Item, Table, value};

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

fn accelerator_name(accelerator: Accelerator) -> &'static str {
    match accelerator {
        Accelerator::Auto => "auto",
        Accelerator::Cpu => "cpu",
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
            source: embed.source.as_str().to_string(),
            model: embed.model.clone(),
            base_url: embed.base_url.clone(),
            api_key_name: embed.api_key_name.clone(),
            api_key_env: embed.api_key_env.clone(),
            batch_size: count(embed.batch_size),
        },
        rerank: RerankDoc {
            enabled: rerank.enabled,
            source: rerank.source.as_str().to_string(),
            model: rerank.model.clone(),
            base_url: rerank.base_url.clone(),
            api_key_name: rerank.api_key_name.clone(),
            api_key_env: rerank.api_key_env.clone(),
        },
        search: SearchDoc {
            candidates: count(search.candidates),
            rerank_candidates: count(search.rerank_candidates),
            results: count(search.results),
            results_reranked: count(search.results_reranked),
            min_score_vector: search.min_score_vector,
            min_score_rerank: search.min_score_rerank,
            agent_min_score: [search.agent_min_score.0, search.agent_min_score.1],
            max_results: count(search.max_results),
            max_candidates: count(search.max_candidates),
            max_rerank_candidates: count(search.max_rerank_candidates),
        },
        device: DeviceDoc {
            accelerator: accelerator_name(user.accelerator).to_string(),
        },
        chunking: ChunkingDoc {
            min_chars: count(user.chunking.min_chars),
            max_chars: count(user.chunking.max_chars),
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
pub fn save_user_settings(bars_home: &Path, settings: &SettingsDoc) -> Result<bool, SaveError> {
    let file = user_settings_path(bars_home);
    let text = read_text(&file)?;
    let current = UserSettings::parse(&text, &file)
        .map_err(|error| SaveError::Invalid(error.to_string()))?
        .value;
    let old = to_doc(&current);
    let mut doc = parse_document(&file, &text)?;

    let embed = table(&mut doc, "embed");
    set_text(embed, "source", &settings.embed.source, &old.embed.source);
    set_text(embed, "model", &settings.embed.model, &old.embed.model);
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
    set_score(
        search,
        "min_score_vector",
        new.min_score_vector,
        was.min_score_vector,
    );
    set_score(
        search,
        "min_score_rerank",
        new.min_score_rerank,
        was.min_score_rerank,
    );
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

    set_text(
        table(&mut doc, "device"),
        "accelerator",
        &settings.device.accelerator,
        &old.device.accelerator,
    );

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

    // Пустые таблицы, которые мы сами завели, в файле не нужны.
    for name in ["embed", "rerank", "search", "device", "chunking"] {
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
    fn only_changed_keys_are_written_and_comments_survive() {
        let home = home();
        let file = user_settings_path(home.path());
        fs::write(&file, "# мои заметки\n[search]\nresults = 7 # люблю семь\n").unwrap();
        let mut doc = current(home.path());
        doc.search.min_score_rerank = 0.35;
        doc.rerank.enabled = true;
        assert!(save_user_settings(home.path(), &doc).unwrap());

        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("# мои заметки"));
        assert!(text.contains("results = 7 # люблю семь"));
        assert!(text.contains("min_score_rerank = 0.35"), "{text}");
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
