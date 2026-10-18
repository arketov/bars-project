//! Закреплённая цель папки; файловый allowlist остаётся отдельным.
use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, PoisonError};

#[derive(Clone)]
pub struct Target {
    pub project: PathBuf,
    pub identity: String,
    pub root: PathBuf,
    pub path: PathBuf,
    pub canonical_root: PathBuf,
    pub absolute: PathBuf,
    pub id: u64,
}

#[derive(Default)]
pub struct Targets {
    active: Mutex<(u64, Option<Target>)>,
    trees: Mutex<Vec<TreeProof>>,
}

struct TreeProof {
    project: PathBuf,
    identity: String,
    docs_root: String,
    documents: Vec<bars_protocol::TreeDocument>,
}

impl Targets {
    /// Два последних дерева сохраняют допуск удалённой папки при фоновом обновлении.
    pub fn remember(&self, project: PathBuf, identity: String, tree: bars_protocol::TreeOutput) {
        let mut trees = self.trees.lock().unwrap_or_else(PoisonError::into_inner);
        trees.push(TreeProof {
            project,
            identity,
            docs_root: tree.docs_root,
            documents: tree.documents,
        });
        if trees.len() > 2 {
            trees.remove(0);
        }
    }

    pub fn documents(
        &self,
        project: &Path,
        identity: &str,
        docs_root: &str,
    ) -> Vec<bars_protocol::TreeDocument> {
        self.trees
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|tree| {
                tree.project == project && tree.identity == identity && tree.docs_root == docs_root
            })
            .flat_map(|tree| tree.documents.clone())
            .collect()
    }

    pub fn begin(&self) -> u64 {
        let mut state = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        state.0 += 1;
        state.1 = None;
        state.0
    }

    pub fn put(&self, id: u64, mut target: Target) -> Result<(), String> {
        let mut state = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        if state.0 != id {
            return Err("Цель устарела; откройте меню заново".into());
        }
        target.id = id;
        state.1 = Some(target);
        Ok(())
    }

    pub fn get(&self, id: u64) -> Result<Target, String> {
        self.active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .1
            .as_ref()
            .filter(|target| target.id == id)
            .cloned()
            .ok_or_else(|| "Цель устарела; откройте меню заново".into())
    }
}

fn relative(value: &str) -> Result<PathBuf, String> {
    if value == "." {
        return Ok(PathBuf::new());
    }
    // Проверяем сегменты до Path::components: он нормализует внутренние `.`.
    if value.is_empty()
        || value
            .split(['/', '\\'])
            .any(|s| s.is_empty() || s == "." || s == ".." || s.contains(':'))
    {
        return Err("Недопустимый путь папки".into());
    }
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if !path.components().all(|c| matches!(c, Component::Normal(_))) {
        return Err("Недопустимый путь папки".into());
    }
    Ok(path.to_path_buf())
}

pub fn target(
    project: &Path,
    docs_root: &str,
    folder: &str,
    documents: &[bars_protocol::TreeDocument],
    identity: String,
) -> Result<Target, String> {
    let root = project.join(relative(docs_root)?);
    let relative_folder = relative(folder)?;
    let path = root.join(&relative_folder);
    if !relative_folder.as_os_str().is_empty()
        && !documents.iter().any(|doc| {
            let document = project.join(doc.path.replace('\\', "/"));
            document
                .strip_prefix(&path)
                .is_ok_and(|tail| tail.components().count() >= 1)
        })
    {
        return Err("Папка отсутствует в дереве проекта; обновите дерево".into());
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "Корень документации недоступен")?;
    let (absolute, _) = checked_location(project, &root, &path)?;
    Ok(Target {
        project: project.to_path_buf(),
        identity,
        root,
        path,
        canonical_root,
        absolute,
        id: 0,
    })
}

/// Не пропускаем повреждённые ссылки, отсутствующие предки или выход через junction.
pub fn checked_path(target: &Target) -> Result<(PathBuf, bool), String> {
    if target
        .root
        .canonicalize()
        .map_err(|_| "Корень документации недоступен")?
        != target.canonical_root
    {
        return Err("Корень документации изменился; откройте меню заново".into());
    }
    let checked = checked_location(&target.project, &target.root, &target.path)?;
    if checked.0 != target.absolute {
        return Err("Путь папки изменился; откройте меню заново".into());
    }
    Ok(checked)
}

fn checked_location(project: &Path, root: &Path, path: &Path) -> Result<(PathBuf, bool), String> {
    let project = project.canonicalize().map_err(|_| "Проект недоступен")?;
    let root = root
        .canonicalize()
        .map_err(|_| "Корень документации недоступен")?;
    if !root.starts_with(&project) || !root.is_dir() {
        return Err("Корень документации вне проекта".into());
    }
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::symlink_metadata(ancestor) {
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .ok_or("Недопустимый путь папки")?
                        .to_os_string(),
                );
                ancestor = ancestor.parent().ok_or("Недопустимый путь папки")?;
            }
            Err(_) => return Err("Нет доступа к папке".into()),
        }
    }
    let mut checked = ancestor
        .canonicalize()
        .map_err(|_| "Ссылка папки недоступна")?;
    if !checked.starts_with(&root) || !checked.is_dir() {
        return Err("Папка вне корня документации".into());
    }
    let absent = !missing.is_empty();
    for segment in missing.iter().rev() {
        checked.push(segment);
    }
    Ok((checked, absent))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn document(path: &str) -> bars_protocol::TreeDocument {
        bars_protocol::TreeDocument {
            path: path.into(),
            module: "модуль".into(),
            title: "Тема".into(),
            chunks: 1,
        }
    }

    #[test]
    fn roots_nested_docs_and_virtual_group_work_without_documents() {
        let temp = tempfile::tempdir().unwrap();
        for docs in ["docs", "manual/docs", "."] {
            fs::create_dir_all(temp.path().join(docs)).unwrap();
            let target = target(temp.path(), docs, ".", &[], "1".into()).unwrap();
            assert_eq!(
                checked_path(&target).unwrap(),
                (temp.path().join(docs).canonicalize().unwrap(), false)
            );
        }
    }

    #[test]
    fn folders_are_confirmed_by_components_and_copy_survives_removal() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("manual/docs/модуль/Папка с пробелами");
        fs::create_dir_all(&folder).unwrap();
        let docs = [document("manual/docs/модуль/Папка с пробелами/Тема.md")];
        let pinned = target(
            temp.path(),
            "manual/docs",
            "модуль/Папка с пробелами",
            &docs,
            "1".into(),
        )
        .unwrap();
        let absolute = checked_path(&pinned).unwrap().0;
        fs::remove_dir(&folder).unwrap();
        assert_eq!(checked_path(&pinned).unwrap(), (absolute, true));
        for path in [
            "моду",
            "модуль/Папка",
            "модуль/Папка с пробелами/Тема.md",
            "../docs",
            "docs/../docs",
            "./модуль",
            "/docs",
            "C:/docs",
            "\\\\server\\share",
            "",
            "модуль//папка",
        ] {
            assert!(
                target(temp.path(), "manual/docs", path, &docs, "1".into()).is_err(),
                "{path}"
            );
        }
    }

    #[test]
    fn target_ids_do_not_accept_replaced_or_forged_targets() {
        let temp = tempfile::tempdir().unwrap();
        let targets = Targets::default();
        let first = targets.begin();
        targets
            .put(
                first,
                target(temp.path(), ".", ".", &[], "1".into()).unwrap(),
            )
            .unwrap();
        let second = targets.begin();
        assert!(
            targets
                .put(
                    first,
                    target(temp.path(), ".", ".", &[], "1".into()).unwrap()
                )
                .is_err()
        );
        targets
            .put(
                second,
                target(temp.path(), ".", ".", &[], "1".into()).unwrap(),
            )
            .unwrap();
        assert!(targets.get(first).is_err());
        assert!(targets.get(second + 1).is_err());
        assert_eq!(targets.get(second).unwrap().identity, "1");
    }

    #[test]
    fn tree_proof_preserves_removed_node_but_never_another_registration_or_root() {
        let targets = Targets::default();
        let project = Path::new("C:/project");
        targets.remember(
            project.to_path_buf(),
            "generation-1".into(),
            bars_protocol::TreeOutput {
                docs_root: "manual/docs".into(),
                documents: vec![document("manual/docs/module/a.md")],
            },
        );
        targets.remember(
            project.to_path_buf(),
            "generation-1".into(),
            bars_protocol::TreeOutput {
                docs_root: "manual/docs".into(),
                documents: vec![],
            },
        );
        assert_eq!(
            targets
                .documents(project, "generation-1", "manual/docs")
                .len(),
            1
        );
        assert!(
            targets
                .documents(project, "generation-2", "manual/docs")
                .is_empty()
        );
        assert!(
            targets
                .documents(project, "generation-1", "docs")
                .is_empty()
        );
        assert!(
            targets
                .documents(Path::new("C:/other"), "generation-1", "manual/docs")
                .is_empty()
        );
    }

    #[cfg(windows)]
    fn junction(link: &Path, destination: &Path) {
        let result = std::process::Command::new("cmd.exe")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(destination)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }

    #[cfg(windows)]
    #[test]
    fn junction_escape_and_retarget_after_open_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let docs = temp.path().join("docs");
        fs::create_dir_all(docs.join("inside")).unwrap();
        let link = docs.join("link");
        junction(&link, outside.path());
        let documents = [document("docs/link/a.md")];
        assert!(target(temp.path(), "docs", "link", &documents, "1".into()).is_err());
        fs::remove_dir(&link).unwrap();
        junction(&link, &docs.join("inside"));
        let pinned = target(temp.path(), "docs", "link", &documents, "1".into()).unwrap();
        fs::remove_dir(&link).unwrap();
        junction(&link, outside.path());
        assert!(checked_path(&pinned).is_err());
        fs::remove_dir(&link).unwrap();
        // Даже другой допустимый каталог не заменяет закреплённый абсолютный путь.
        fs::create_dir_all(docs.join("second")).unwrap();
        junction(&link, &docs.join("second"));
        assert!(checked_path(&pinned).is_err());
        fs::remove_dir(&link).unwrap();
    }
}
