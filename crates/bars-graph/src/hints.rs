//! Подсказки об области поиска: пути и модули в ошибках, единая форма пути файла (OQ-169).
//!
//! Пути здесь — от корня документации (как их печатают и принимают `files` и `modules`).

/// Путь из вызова без `\`, ведущего `./` и пустых сегментов.
pub fn normalize_file(input: &str) -> String {
    input
        .trim()
        .replace('\\', "/")
        .split('/')
        .filter(|part| !matches!(*part, "" | "."))
        .collect::<Vec<_>>()
        .join("/")
}

/// Лексическая форма входа документа; сравнение с известными источниками не обращается к диску.
pub fn normalize_document_input(input: &str) -> Option<String> {
    let normalized = normalize_file(input);
    let mut parts = Vec::new();
    for part in normalized.trim_start_matches("?/").split('/') {
        if part == ".." {
            parts.pop()?;
        } else {
            parts.push(part);
        }
    }
    let result = parts.join("/");
    Some(if cfg!(windows) {
        result.to_lowercase()
    } else {
        result
    })
}

/// До трёх документов, путь которых оканчивается на введённый или совпадает с ним по имени файла.
pub fn suggest_files<'a>(
    docs_paths: impl IntoIterator<Item = &'a str>,
    input: &str,
) -> Vec<String> {
    let wanted = normalize_file(input).to_lowercase();
    let wanted_name = wanted.rsplit('/').next().unwrap_or("").to_string();
    if wanted_name.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<String> = docs_paths
        .into_iter()
        .filter(|shown| {
            let lower = normalize_file(shown).to_lowercase();
            lower == wanted
                || lower.ends_with(&format!("/{wanted}"))
                || lower.rsplit('/').next() == Some(wanted_name.as_str())
        })
        .map(str::to_string)
        .collect();
    found.sort();
    found.dedup();
    found.truncate(3);
    found
}

/// Текст ошибки неизвестного файла: с кандидатами, если они есть.
pub fn unknown_file_message<'a>(
    docs_paths: impl IntoIterator<Item = &'a str>,
    input: &str,
) -> String {
    let suggestions = suggest_files(docs_paths, input);
    if suggestions.is_empty() {
        format!("неизвестный файл `{input}`")
    } else {
        format!(
            "неизвестный файл `{input}`; возможно: {}",
            suggestions.join(", ")
        )
    }
}

/// Текст ошибки неизвестного модуля: все модули первого уровня и, если имя — подпапка внутри
/// модуля, подсказка про `files`. `docs` — пары (модуль, путь документа от корня документации).
pub fn unknown_module_message(name: &str, docs: &[(&str, &str)]) -> String {
    let mut modules: Vec<&str> = docs.iter().map(|(module, _)| *module).collect();
    modules.sort_unstable();
    modules.dedup();
    let mut message = format!("неизвестный модуль `{name}`");
    if !modules.is_empty() {
        message.push_str(&format!("; есть: {}", modules.join(", ")));
    }
    let parent = docs.iter().find_map(|(module, shown)| {
        let parts: Vec<&str> = shown.split('/').collect();
        let folders = &parts[..parts.len().saturating_sub(1)];
        folders
            .iter()
            .skip(1)
            .any(|part| *part == name)
            .then_some(*module)
    });
    if let Some(module) = parent {
        message.push_str(&format!(
            "; {name} — подпапка {module}, модуль — папка первого уровня; сузить область можно путём в files"
        ));
    }
    message
}
