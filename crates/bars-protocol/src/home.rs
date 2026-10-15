//! Домашняя папка пользователя, от которой bars строит `~/.bars` и пути клиентов.

use std::path::PathBuf;

/// Переменная окружения, которая подменяет домашнюю папку: для проверок установщика и клиентов
/// на копиях, без настоящих `~/.bars`, `~/.claude` и `~/.codex` (`docs/features/distribution/`).
pub const USER_HOME_VAR: &str = "BARS_USER_HOME";

/// Подмена домашней папки из окружения; пустое значение не считается.
pub fn home_override() -> Option<PathBuf> {
    std::env::var_os(USER_HOME_VAR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Домашняя папка пользователя: `BARS_USER_HOME`, иначе папка профиля ОС.
pub fn user_home() -> Option<PathBuf> {
    home_override().or_else(dirs::home_dir)
}
