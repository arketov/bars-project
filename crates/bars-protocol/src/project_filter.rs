//! Фильтр проектов (Т-50): одно правило совпадения для воркера, `bars_status` и CLI.
//! Окно повторяет его в `ui/src/lib/projects.ts` (`matchProject`); таблица случаев общая
//! (`docs/features/projects/subprojects.md`, «Тесты»).

fn normalize(value: &str) -> String {
    value.replace('\\', "/").to_lowercase()
}

/// Текст фильтра после обрезки пробелов; пустой — `None` (фильтра нет).
pub fn filter_text(filter: Option<&str>) -> Option<&str> {
    filter.map(str::trim).filter(|text| !text.is_empty())
}

/// Подстрока без учёта регистра (Unicode lowercase) хотя бы в одной из строк записи: имя,
/// составное имя, путь; `\` равен `/`. Пустой текст подходит всем.
pub fn project_matches_filter(fields: &[&str], text: &str) -> bool {
    let needle = normalize(text.trim());
    needle.is_empty()
        || fields
            .iter()
            .any(|field| normalize(field).contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Таблица общая с `ui/src/lib/project-switcher.test.ts` («matchProject»): те же данные
    /// и те же ответы.
    #[test]
    fn shared_case_table_with_the_window() {
        let api = [
            "Api",
            "tmc-opovechenie/backend/api",
            "C:\\Work\\Бэкенд\\api",
        ];
        let plain = ["x", "C:/x"];
        let cases: [(&[&str], &str, bool); 8] = [
            (&api, "", true),
            (&api, "   ", true),
            (&api, "API", true),
            (&api, "opovechenie/BACKEND", true),
            (&api, "work/бэкенд", true),
            (&api, "work\\БЭКЕНД", true),
            (&plain, "backend", false),
            (&plain, "x", true),
        ];
        for (fields, text, expected) in cases {
            assert_eq!(
                project_matches_filter(fields, text),
                expected,
                "{fields:?} / {text:?}"
            );
        }
    }

    #[test]
    fn empty_filter_is_absent() {
        assert_eq!(filter_text(None), None);
        assert_eq!(filter_text(Some("  \t")), None);
        assert_eq!(filter_text(Some(" web ")), Some("web"));
    }
}
