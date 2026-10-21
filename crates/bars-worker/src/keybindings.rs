//! Профиль клавиатуры: единственный писатель, ревизии, сохранение неизвестных ID.
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub code: String,
    pub modifiers: Vec<String>,
}
pub type Bindings = BTreeMap<String, Option<Binding>>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub version: u32,
    pub revision: u64,
    pub platforms: BTreeMap<String, Bindings>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            version: 1,
            revision: 0,
            platforms: BTreeMap::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Snapshot {
    pub profile: Profile,
    pub error: Option<String>,
    // Точный исходник нужен для явного сброса повреждённого файла без слепой перезаписи.
    pub recovery: Option<String>,
}
#[derive(Clone, Default)]
pub struct Store(Arc<Mutex<()>>);
fn read(home: &Path) -> Result<Snapshot, String> {
    let text = match fs::read_to_string(home.join("keybindings.json")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Snapshot {
                profile: Profile::default(),
                error: None,
                recovery: None,
            });
        }
        Err(e) => return Err(format!("Не прочитан профиль клавиатуры: {e}")),
    };
    match serde_json::from_str::<Profile>(&text).and_then(|profile| {
        if profile.version != 1 {
            return Err(serde::de::Error::custom("Неизвестная версия профиля"));
        }
        Ok(profile)
    }) {
        Ok(profile) => Ok(Snapshot {
            profile,
            error: None,
            recovery: None,
        }),
        Err(e) => Ok(Snapshot {
            profile: Profile::default(),
            error: Some(format!("Профиль клавиатуры повреждён: {e}")),
            recovery: Some(text),
        }),
    }
}
impl Store {
    pub fn get(&self, home: &Path) -> Result<Snapshot, String> {
        let _guard = self
            .0
            .lock()
            .map_err(|_| "Хранилище клавиатуры недоступно")?;
        read(home)
    }
    pub fn save(
        &self,
        home: &Path,
        platform: &str,
        bindings: Bindings,
        expected_revision: u64,
        recovery: Option<&str>,
    ) -> Result<Snapshot, String> {
        let _guard = self
            .0
            .lock()
            .map_err(|_| "Хранилище клавиатуры недоступно")?;
        if !matches!(platform, "windows" | "macos") {
            return Err("Неизвестная платформа".into());
        }
        let current = read(home)?;
        if current.error.is_some()
            && (recovery.is_none() || recovery != current.recovery.as_deref())
        {
            return Err("Повреждённый профиль: требуется явный сброс".into());
        }
        if current.profile.revision != expected_revision {
            return Err("Профиль изменён другим окном. Загрузите актуальную карту; черновик сохранён на экране".into());
        }
        for binding in bindings.values().flatten() {
            if binding.code.is_empty()
                || binding.code.len() > 40
                || binding
                    .modifiers
                    .iter()
                    .any(|m| !matches!(m.as_str(), "Alt" | "Control" | "Meta" | "Shift"))
            {
                return Err("Некорректное сочетание".into());
            }
            let mut normalized = binding.modifiers.clone();
            normalized.sort();
            normalized.dedup();
            if normalized != binding.modifiers {
                return Err("Модификаторы должны быть уникальны и отсортированы".into());
            }
        }
        let mut profile = current.profile;
        profile.revision = profile
            .revision
            .checked_add(1)
            .ok_or("Исчерпаны ревизии профиля")?;
        profile.platforms.insert(platform.into(), bindings);
        fs::create_dir_all(home).map_err(|e| e.to_string())?;
        let temp = home.join("keybindings.json.bars-tmp");
        let mut file = fs::File::create(&temp).map_err(|e| e.to_string())?;
        file.write_all(
            serde_json::to_string_pretty(&profile)
                .map_err(|e| e.to_string())?
                .as_bytes(),
        )
        .map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(&temp, home.join("keybindings.json")).map_err(|e| e.to_string())?;
        Ok(Snapshot {
            profile,
            error: None,
            recovery: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn revisions_platforms_unknown_ids_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        let map = BTreeMap::from([("future.command".into(), None)]);
        store
            .save(dir.path(), "windows", map.clone(), 0, None)
            .unwrap();
        assert!(
            store
                .save(dir.path(), "windows", Bindings::new(), 0, None)
                .is_err()
        );
        store
            .save(dir.path(), "macos", Bindings::new(), 1, None)
            .unwrap();
        assert_eq!(
            store.get(dir.path()).unwrap().profile.platforms["windows"],
            map
        );
        fs::write(dir.path().join("keybindings.json"), "broken").unwrap();
        let bad = store.get(dir.path()).unwrap();
        assert!(bad.error.is_some());
        assert!(
            store
                .save(dir.path(), "windows", Bindings::new(), 0, None)
                .is_err()
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("keybindings.json")).unwrap(),
            "broken"
        );
        store
            .save(
                dir.path(),
                "windows",
                Bindings::new(),
                0,
                bad.recovery.as_deref(),
            )
            .unwrap();
    }
    #[test]
    fn cloned_writers_do_not_lose_updates() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..2)
                .map(|_| {
                    let store = store.clone();
                    let home = dir.path();
                    scope.spawn(move || store.save(home, "windows", Bindings::new(), 0, None))
                })
                .collect();
            assert_eq!(
                jobs.into_iter()
                    .filter_map(|j| j.join().unwrap().ok())
                    .count(),
                1
            );
        });
    }
    #[test]
    fn write_failure_preserves_profile_and_unknown_version_requires_reset() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::default();
        store
            .save(dir.path(), "windows", Bindings::new(), 0, None)
            .unwrap();
        let original = fs::read(dir.path().join("keybindings.json")).unwrap();
        fs::create_dir(dir.path().join("keybindings.json.bars-tmp")).unwrap();
        assert!(
            store
                .save(dir.path(), "windows", Bindings::new(), 1, None)
                .is_err()
        );
        assert_eq!(
            fs::read(dir.path().join("keybindings.json")).unwrap(),
            original
        );
        fs::write(
            dir.path().join("keybindings.json"),
            r#"{"version":99,"revision":7,"platforms":{}}"#,
        )
        .unwrap();
        assert!(store.get(dir.path()).unwrap().error.is_some());
        assert!(
            store
                .save(dir.path(), "windows", Bindings::new(), 0, None)
                .is_err()
        );
    }
}
