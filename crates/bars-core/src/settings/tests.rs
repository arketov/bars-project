use std::fs;
use std::path::Path;

use super::*;

const USER: &str = "settings.toml";
const PROJECT: &str = "config.toml";

fn user(text: &str) -> Loaded<UserSettings> {
    UserSettings::parse(text, Path::new(USER)).unwrap()
}

fn user_err(text: &str) -> SettingsError {
    UserSettings::parse(text, Path::new(USER)).unwrap_err()
}

fn project(text: &str) -> Loaded<ProjectConfig> {
    ProjectConfig::parse(text, Path::new(PROJECT)).unwrap()
}

fn project_err(text: &str) -> SettingsError {
    ProjectConfig::parse(text, Path::new(PROJECT)).unwrap_err()
}

#[test]
fn factory_values_match_the_settings_doc() {
    let defaults = UserSettings::default();
    assert_eq!(defaults.embed.source, ModelSource::Builtin);
    assert_eq!(defaults.embed.model, "BAAI/bge-m3");
    assert_eq!(defaults.embed.batch_size, 32);
    assert!(!defaults.rerank.enabled);
    assert_eq!(defaults.rerank.source, ModelSource::Builtin);
    assert_eq!(defaults.rerank.model, "BAAI/bge-reranker-v2-m3");
    assert_eq!(defaults.embed.accelerator, Accelerator::Auto);
    assert_eq!(
        (defaults.chunking.min_chars, defaults.chunking.max_chars),
        (250, 6000)
    );
    assert_eq!(defaults.search, SearchSettings::default());
}

#[test]
fn empty_files_give_the_factory_values() {
    let loaded = user("");
    assert_eq!(loaded.value, UserSettings::default());
    assert!(loaded.warnings.is_empty());
    let settings = Settings::resolve(&loaded.value, &project("").value);
    assert_eq!(settings.docs_root, "docs");
    assert_eq!(settings.chunking, ChunkingParams::default());
}

#[test]
fn independent_devices_migrate_without_overriding_explicit_preferences() {
    for (embed, rerank) in [
        ("auto", "auto"),
        ("auto", "cpu"),
        ("cpu", "auto"),
        ("cpu", "cpu"),
    ] {
        let loaded = user(&format!(
            "[embed]\naccelerator = '{embed}'\n[rerank]\naccelerator = '{rerank}'\n"
        ));
        assert_eq!(
            loaded.value.embed.accelerator,
            if embed == "cpu" {
                Accelerator::Cpu
            } else {
                Accelerator::Auto
            }
        );
        assert_eq!(
            loaded.value.rerank.accelerator,
            if rerank == "cpu" {
                Accelerator::Cpu
            } else {
                Accelerator::Auto
            }
        );
    }
    let loaded = user("[device]\naccelerator='cpu'\n[embed]\naccelerator='auto'\n");
    assert_eq!(loaded.value.embed.accelerator, Accelerator::Auto);
    assert_eq!(loaded.value.rerank.accelerator, Accelerator::Cpu);
    assert!(loaded.warnings.iter().any(|w| w.key.contains("legacy")));
    assert!(UserSettings::parse("[embed]\naccelerator='gpu'", Path::new(USER)).is_err());
    assert!(UserSettings::parse("[rerank]\naccelerator='gpu'", Path::new(USER)).is_err());
}

#[test]
fn user_file_overrides_only_the_keys_it_sets() {
    let loaded = user(
        r#"
        [embed]
        source = "remote"
        model = "text-embedding-3-large"
        base_url = "https://api.example.com/v1"
        api_key_name = "provider"
        batch_size = 8

        [rerank]
        enabled = true

        [search]
        results = 7
        min_score_rerank = 0.25
        agent_min_score = [0.2, 0.8]

        [device]
        accelerator = "cpu"

        [chunking]
        max_chars = 4000
        "#,
    );
    let settings = loaded.value;
    assert_eq!(settings.embed.source, ModelSource::Remote);
    assert_eq!(settings.embed.model, "text-embedding-3-large");
    assert_eq!(
        settings.embed.base_url.as_deref(),
        Some("https://api.example.com/v1")
    );
    assert_eq!(settings.embed.api_key_name.as_deref(), Some("provider"));
    assert_eq!(settings.embed.api_key_env, None);
    assert_eq!(settings.embed.batch_size, 8);
    assert!(settings.rerank.enabled);
    assert_eq!(settings.rerank.model, "BAAI/bge-reranker-v2-m3");
    assert_eq!(settings.search.results, 7);
    assert_eq!(settings.search.min_score_rerank, 0.25);
    assert_eq!(settings.search.agent_min_score, (0.2, 0.8));
    assert_eq!(settings.search.results_reranked, 3);
    assert_eq!(settings.embed.accelerator, Accelerator::Cpu);
    assert_eq!(settings.chunking.min_chars, 250);
    assert_eq!(settings.chunking.max_chars, 4000);
}

#[test]
fn local_openai_source_is_read_from_its_kebab_name() {
    let settings = user(
        "[embed]\nsource = \"local-openai\"\nmodel = \"nomic\"\nbase_url = \"http://localhost:8080\"\n",
    )
    .value;
    assert_eq!(settings.embed.source, ModelSource::LocalOpenai);
    assert_eq!(settings.embed.source.as_str(), "local-openai");
}

#[test]
fn integers_are_accepted_where_scores_are_floats() {
    let settings = user("[search]\nmin_score_vector = 0\nagent_min_score = [0, 1]\n").value;
    assert_eq!(settings.search.min_score_vector, 0.0);
    assert_eq!(settings.search.agent_min_score, (0.0, 1.0));
}

#[test]
fn project_keys_override_user_keys_one_by_one() {
    let user = user("[search]\nresults = 7\ncandidates = 40\n[chunking]\nmin_chars = 100\n").value;
    let project = project(
        r#"
        docs_root = "documentation"

        [search]
        results = 9

        [chunking]
        max_chars = 3000
        "#,
    )
    .value;
    let settings = Settings::resolve(&user, &project);
    assert_eq!(settings.docs_root, "documentation");
    assert_eq!(settings.search.results, 9);
    assert_eq!(settings.search.candidates, 40);
    assert_eq!(settings.chunking.min_chars, 100);
    assert_eq!(settings.chunking.max_chars, 3000);
    assert_eq!(settings.index_settings().docs_root, "documentation");
    assert_eq!(settings.index_settings().chunking, settings.chunking);
}

#[test]
fn project_does_not_change_models_or_device() {
    let user = user("[rerank]\nenabled = true\n[device]\naccelerator = \"cpu\"\n").value;
    let loaded = project("[embed]\nmodel = \"other\"\n[rerank]\nenabled = false\n");
    let settings = Settings::resolve(&user, &loaded.value);
    assert!(settings.rerank.enabled);
    assert_eq!(settings.embed.model, "BAAI/bge-m3");
    assert_eq!(settings.embed.accelerator, Accelerator::Cpu);
    let keys: Vec<_> = loaded.warnings.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(keys, ["embed", "rerank"]);
}

#[test]
fn unknown_keys_are_warned_about_and_ignored() {
    let loaded = user(
        r#"
        future = 1
        [search]
        results = 6
        min_score = 0.5
        [chunking]
        min_char = 10
        [embed]
        colour = "red"
        [rerank]
        speed = 1
        [device]
        gpu = true
        "#,
    );
    assert_eq!(loaded.value.search.results, 6);
    assert_eq!(loaded.value.chunking, ChunkingParams::default());
    let mut keys: Vec<_> = loaded.warnings.iter().map(|w| w.key.clone()).collect();
    keys.sort();
    assert_eq!(
        keys,
        [
            "chunking.min_char",
            "device.gpu",
            "embed.colour",
            "future",
            "rerank.speed",
            "search.min_score",
        ]
    );
    assert!(loaded.warnings.iter().all(|w| w.file == Path::new(USER)));
    let text = loaded.warnings[0].to_string();
    assert!(
        text.contains(USER) && text.contains("игнорируется"),
        "{text}"
    );
}

#[test]
fn broken_toml_is_a_parse_error_naming_the_file() {
    let error = user_err("[search\nresults = 1");
    assert!(matches!(error, SettingsError::Parse { .. }));
    assert!(error.to_string().starts_with(USER), "{error}");
}

#[test]
fn wrong_types_and_unknown_choices_are_parse_errors() {
    for text in [
        "[search]\nresults = \"five\"\n",
        "[search]\nresults = -1\n",
        "[search]\nagent_min_score = [0.1]\n",
        "[embed]\nsource = \"cloud\"\n",
        "[device]\naccelerator = \"gpu\"\n",
        "[rerank]\nenabled = \"yes\"\n",
        "[chunking]\nmin_chars = 1.5\n",
    ] {
        assert!(
            matches!(user_err(text), SettingsError::Parse { .. }),
            "{text}"
        );
    }
}

#[test]
fn zero_counts_and_sizes_are_invalid() {
    for (text, key) in [
        ("[search]\ncandidates = 0\n", "search.candidates"),
        (
            "[search]\nrerank_candidates = 0\n",
            "search.rerank_candidates",
        ),
        ("[search]\nresults = 0\n", "search.results"),
        (
            "[search]\nresults_reranked = 0\n",
            "search.results_reranked",
        ),
        ("[search]\nmax_results = 0\n", "search.max_results"),
        ("[search]\nmax_candidates = 0\n", "search.max_candidates"),
        (
            "[search]\nmax_rerank_candidates = 0\n",
            "search.max_rerank_candidates",
        ),
        ("[embed]\nbatch_size = 0\n", "embed.batch_size"),
        ("[chunking]\nmin_chars = 0\n", "chunking.min_chars"),
        ("[chunking]\nmax_chars = 0\n", "chunking.max_chars"),
    ] {
        match user_err(text) {
            SettingsError::Invalid { key: got, file, .. } => {
                assert_eq!(got, key);
                assert_eq!(file, Path::new(USER));
            }
            other => panic!("{text}: {other}"),
        }
    }
}

#[test]
fn scores_outside_zero_to_one_are_invalid() {
    for (text, key) in [
        (
            "[search]\nmin_score_vector = 1.5\n",
            "search.min_score_vector",
        ),
        (
            "[search]\nmin_score_rerank = -0.1\n",
            "search.min_score_rerank",
        ),
        (
            "[search]\nagent_min_score = [0.5, 1.2]\n",
            "search.agent_min_score",
        ),
    ] {
        match user_err(text) {
            SettingsError::Invalid { key: got, .. } => assert_eq!(got, key, "{text}"),
            other => panic!("{text}: {other}"),
        }
    }
}

#[test]
fn agent_limits_the_wrong_way_round_are_invalid() {
    match user_err("[search]\nagent_min_score = [0.8, 0.2]\n") {
        SettingsError::Invalid { key, reason, .. } => {
            assert_eq!(key, "search.agent_min_score");
            assert!(reason.contains("нижний"), "{reason}");
        }
        other => panic!("{other}"),
    }
}

#[test]
fn project_file_is_validated_like_the_user_file() {
    match project_err("[chunking]\nmax_chars = 0\n") {
        SettingsError::Invalid { key, file, .. } => {
            assert_eq!(key, "chunking.max_chars");
            assert_eq!(file, Path::new(PROJECT));
        }
        other => panic!("{other}"),
    }
    assert!(matches!(
        project_err("docs_root = 5\n"),
        SettingsError::Parse { .. }
    ));
}

#[test]
fn missing_files_mean_factory_values_and_are_not_created() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    let loaded = Settings::load(home.path(), root.path()).unwrap();
    assert_eq!(loaded.value.docs_root, "docs");
    assert_eq!(loaded.value.search, SearchSettings::default());
    assert!(loaded.warnings.is_empty());
    assert!(!user_settings_path(home.path()).exists());
    assert!(!root.path().join(".bars").exists());
}

#[test]
fn load_reads_both_files_and_merges_their_warnings() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::write(
        user_settings_path(home.path()),
        "[search]\nresults = 7\nresult = 1\n",
    )
    .unwrap();
    fs::create_dir(root.path().join(".bars")).unwrap();
    fs::write(
        project_config_path(root.path()),
        "docs_root = \"manual\"\n[chunking]\nmin_chars = 50\nmax = 1\n",
    )
    .unwrap();
    let loaded = Settings::load(home.path(), root.path()).unwrap();
    assert_eq!(loaded.value.docs_root, "manual");
    assert_eq!(loaded.value.search.results, 7);
    assert_eq!(loaded.value.chunking.min_chars, 50);
    let keys: Vec<_> = loaded.warnings.iter().map(|w| w.key.as_str()).collect();
    assert_eq!(keys, ["search.result", "chunking.max"]);
}

#[test]
fn a_broken_project_file_fails_the_whole_load() {
    let home = tempfile::tempdir().unwrap();
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".bars")).unwrap();
    fs::write(project_config_path(root.path()), "[search]\nresults = 0\n").unwrap();
    let error = Settings::load(home.path(), root.path()).unwrap_err();
    assert!(matches!(error, SettingsError::Invalid { .. }), "{error}");
}

#[test]
fn an_unreadable_path_is_a_read_error() {
    let home = tempfile::tempdir().unwrap();
    fs::create_dir(user_settings_path(home.path())).unwrap();
    let error = UserSettings::load(home.path()).unwrap_err();
    assert!(matches!(error, SettingsError::Read { .. }), "{error}");
}

fn invalid_key(text: &str) -> String {
    match user_err(text) {
        SettingsError::Invalid { key, .. } => key,
        other => panic!("{text}: {other}"),
    }
}

#[test]
fn embed_provider_fields_are_checked_on_read() {
    for (text, key) in [
        // Модель не из каталога.
        (
            "[embed]
model = \"gpt-4\"
",
            "embed.model",
        ),
        // Источник без имени модели у провайдера: умолчание bge-m3 ему не подходит.
        (
            "[embed]
source = \"local-openai\"
base_url = \"http://localhost:1\"
",
            "embed.model",
        ),
        (
            "[embed]
source = \"local-openai\"
model = \"m\"
",
            "embed.base_url",
        ),
        (
            "[embed]
source = \"local-openai\"
model = \"m\"
base_url = \"localhost:8080\"
",
            "embed.base_url",
        ),
        (
            "[embed]
source = \"local-openai\"
model = \"m\"
base_url = \"http://u:p@host\"
",
            "embed.base_url",
        ),
        // Удалённому провайдеру нужен ключ, локальному — нет.
        (
            "[embed]
source = \"remote\"
model = \"m\"
base_url = \"https://api.example\"
",
            "embed.api_key_name",
        ),
        (
            "[embed]
api_key_name = \" \"
",
            "embed.api_key_name",
        ),
    ] {
        assert_eq!(invalid_key(text), key, "{text}");
    }
}

#[test]
fn every_catalog_model_and_both_remote_kinds_are_accepted() {
    for id in [
        "BAAI/bge-m3",
        "Qwen/Qwen3-Embedding-0.6B",
        "Qwen/Qwen3-Embedding-4B",
        "nomic-ai/nomic-embed-text-v2-moe",
    ] {
        let settings = user(&format!(
            "[embed]
model = \"{id}\"
"
        ))
        .value;
        assert_eq!(settings.embed.model, id);
    }
    let remote = user(
        "[embed]
source = \"remote\"
model = \"text-embedding-3-small\"
base_url = \"https://api.example/v1\"
api_key_env = \"EXAMPLE_KEY\"
",
    )
    .value;
    assert_eq!(remote.embed.source, ModelSource::Remote);
}

#[test]
fn rerank_provider_fields_are_checked_only_when_rerank_is_enabled() {
    // Выключенный rerank с незаконченной таблицей не ошибка.
    user(
        "[rerank]
source = \"remote\"
",
    );
    for (text, key) in [
        (
            "[rerank]
enabled = true
model = \"other\"
",
            "rerank.model",
        ),
        (
            "[rerank]
enabled = true
source = \"remote\"
model = \"rerank-v3\"
",
            "rerank.base_url",
        ),
        (
            "[rerank]
enabled = true
source = \"remote\"
model = \"rerank-v3\"
base_url = \"https://api.example\"
",
            "rerank.api_key_name",
        ),
    ] {
        assert_eq!(invalid_key(text), key, "{text}");
    }
    let ok = user(
        "[rerank]
enabled = true
",
    )
    .value;
    assert!(ok.rerank.enabled);
    user(
        "[rerank]
enabled = true
source = \"local-openai\"
model = \"bge\"
base_url = \"http://localhost:8081\"
",
    );
}
