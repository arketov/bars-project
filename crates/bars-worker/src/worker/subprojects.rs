//! Автопоиск подпроектов: обход дерева главного проекта, точечная проверка пути, ленивая
//! регистрация, первое обращение и уборка ушедших (`docs/features/projects/subprojects.md`, Т-49).
//!
//! Подпроект — вложенный репозиторий с каталогом `.git` и `docs/` хотя бы с одним `.md`, вне корня
//! документации главного. Воркер регистрирует такую папку найденной и ленивой: ни `.bars/`, ни
//! индекса, ни слежения, пока в неё не обратились (НФТ-7). Обход ничего не читает, кроме имён
//! записей каталогов и типа `.git`.

use std::ffi::OsStr;
use std::path::Component;

use super::registry::path_key;
use super::*;

/// Глубина обхода от главного проекта: дети главного — первый уровень.
pub(super) const MAX_DEPTH: usize = 6;
/// Записей каталогов (файлы и папки), прочитанных за один проход одного проекта.
pub(super) const MAX_ENTRIES: usize = 50_000;
/// Не чаще одного обхода проекта на открытие окна за это время.
const WINDOW_RESCAN: Duration = Duration::from_secs(300);
/// Предел цепочки вложенных подпроектов при точечной проверке одного пути.
const MAX_CHAIN: usize = 16;

/// Пределы стоимости обхода; в тестах уменьшаются.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    pub depth: usize,
    pub entries: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: MAX_DEPTH,
            entries: MAX_ENTRIES,
        }
    }
}

/// Итог одного обхода.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Discovery {
    /// Найденные подпроекты, по алфавиту путей.
    pub found: Vec<PathBuf>,
    /// Обход оборван пределом глубины или числа записей: найденное сохраняется, ничего не удаляется.
    pub truncated: bool,
    /// Сколько записей каталогов прочитано (для замера и тестов).
    pub entries: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Git {
    None,
    File,
    Dir,
}

/// Что лежит в `<dir>/.git`: каталог — репозиторий, файл — worktree или подмодуль.
fn git_kind(dir: &Path) -> Git {
    match fs::symlink_metadata(dir.join(".git")) {
        Ok(meta) if meta.is_dir() => Git::Dir,
        Ok(meta) if meta.is_file() => Git::File,
        _ => Git::None,
    }
}

/// Имя папки, в которую обход не заходит: скрытая, `node_modules`, `target`.
fn skipped_name(name: &OsStr) -> bool {
    let name = name.to_string_lossy();
    name.starts_with('.') || name == "node_modules" || name == "target"
}

/// Корень документации главного проекта как путь.
fn docs_dir(parent: &Path, docs_root: &str) -> PathBuf {
    normalize(&parent.join(docs_root)).unwrap_or_else(|_| parent.join(docs_root))
}

/// Одна папка лежит в другой (или это одна и та же папка).
fn overlaps(a: &Path, b: &Path) -> bool {
    let (a, b) = (path_key(a), path_key(b));
    a.starts_with(&b) || b.starts_with(&a)
}

/// Папка строго внутри `parent`.
fn strictly_inside(parent: &Path, child: &Path) -> bool {
    let (parent, child) = (path_key(parent), path_key(child));
    child.len() > parent.len() && child.starts_with(&parent)
}

/// Правило подпроекта (Т-49, OQ-309) без условия «папка не зарегистрирована»: его проверяет
/// вызывающий. Корень документации подпроекта по правилу — `docs`, содержимое файлов не читается.
pub(super) fn matches_rule(parent: &Path, docs_root: &str, candidate: &Path) -> bool {
    strictly_inside(parent, candidate)
        && !overlaps(candidate, &docs_dir(parent, docs_root))
        && git_kind(candidate) == Git::Dir
        && has_markdown(candidate, DEFAULT_DOCS_ROOT)
}

/// Обход дерева главного проекта: найденные подпроекты и признак неполного обхода. Без блокировок
/// и без чтения содержимого файлов; `registered` — корни зарегистрированных проектов (их
/// собственный проход обработает их вложенные).
pub(super) fn discover(
    parent: &Path,
    docs_root: &str,
    registered: &[PathBuf],
    limits: Limits,
) -> Discovery {
    let mut result = Discovery::default();
    let docs = docs_dir(parent, docs_root);
    let docs_key = path_key(&docs);
    // Корень документации — весь проект: вне его ничего нет, находить нечего.
    if docs_key == path_key(parent) {
        return result;
    }
    let registered: HashSet<Vec<String>> = registered.iter().map(|path| path_key(path)).collect();
    let mut stack = vec![(parent.to_path_buf(), 0_usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            result.entries += 1;
            if result.entries > limits.entries {
                result.truncated = true;
                result.found.sort();
                return result;
            }
            // `DirEntry::file_type` не идёт по ссылкам: символическая ссылка и junction папками
            // не считаются, петли невозможны.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() || skipped_name(&entry.file_name()) {
                continue;
            }
            let path = entry.path();
            let key = path_key(&path);
            if key == docs_key || registered.contains(&key) {
                continue;
            }
            match git_kind(&path) {
                // Worktree и подмодуль: папка пропускается целиком.
                Git::File => {}
                // Репозиторий — граница: внутрь обход не заходит ни с `docs/`, ни без неё.
                Git::Dir => {
                    if has_markdown(&path, DEFAULT_DOCS_ROOT) && !overlaps(&path, &docs) {
                        result.found.push(path);
                    }
                }
                Git::None => {
                    if depth + 1 >= limits.depth {
                        result.truncated = true;
                    } else {
                        stack.push((path, depth + 1));
                    }
                }
            }
        }
    }
    result.found.sort();
    result
}

/// Точечная проверка пути без обхода: первая папка от `parent` вниз к `target` (включая `target`),
/// которая отвечает правилу подпроекта. Идёт теми же шагами, что и обход: скрытые папки,
/// `node_modules`, `target`, ссылки, worktree и репозиторий без `docs/` — граница.
pub(super) fn candidate_on_path(parent: &Path, docs_root: &str, target: &Path) -> Option<PathBuf> {
    if !strictly_inside(parent, target) {
        return None;
    }
    let docs = docs_dir(parent, docs_root);
    let docs_key = path_key(&docs);
    if docs_key == path_key(parent) {
        return None;
    }
    let mut dir = parent.to_path_buf();
    for component in target.components().skip(parent.components().count()) {
        let Component::Normal(name) = component else {
            return None;
        };
        if skipped_name(name) {
            return None;
        }
        dir.push(name);
        let meta = fs::symlink_metadata(&dir).ok()?;
        if !meta.is_dir() || path_key(&dir).starts_with(&docs_key) {
            return None;
        }
        match git_kind(&dir) {
            Git::File => return None,
            Git::Dir => {
                return (has_markdown(&dir, DEFAULT_DOCS_ROOT) && !overlaps(&dir, &docs))
                    .then_some(dir);
            }
            Git::None => {}
        }
    }
    None
}

/// Куда указывает селектор `главный/путь`: имя главного и папка под его корнем. Только для
/// проекта без предка; путь с `.`/`..`/пустым сегментом не годится.
fn composite_target(registry: &Registry, selector: &str) -> Option<(String, PathBuf)> {
    let key = selector.replace('\\', "/");
    let key = key.trim_end_matches('/');
    let (first, rest) = key.split_once('/')?;
    if rest
        .split('/')
        .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return None;
    }
    let main = registry
        .entries()
        .iter()
        .find(|entry| registry.parent_of(&entry.path).is_none() && entry.display_name() == first)?;
    let target = rest
        .split('/')
        .fold(main.path.clone(), |path, part| path.join(part));
    Some((main.display_name(), target))
}

/// Папка, которую называет селектор: абсолютный путь или `главный/путь`.
fn selector_target(registry: &Registry, selector: &str) -> Option<PathBuf> {
    let as_path = Path::new(selector);
    if as_path.is_absolute() {
        return normalize(as_path).ok();
    }
    composite_target(registry, selector).map(|(_, target)| target)
}

/// Состояние автопоиска: один проход на процесс, слияние ожидающих запросов, пределы частоты и
/// проекты с оборванным обходом.
#[derive(Default)]
pub(super) struct Gate {
    /// Проход идёт под этим замком: одновременно не больше одного (НТР-3).
    pass: Mutex<()>,
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    /// Проекты, на которые уже поставлен фоновый обход и он ещё не начался.
    scheduled: HashSet<PathBuf>,
    /// Начало последнего обхода проекта по открытию окна.
    last_window: HashMap<PathBuf, Instant>,
    /// Проекты, чей последний обход оборван пределом.
    truncated: HashSet<PathBuf>,
}

impl Worker {
    /// Подпроект по селектору, которого реестр ещё не знает: абсолютный путь или `главный/путь`
    /// внутри папки зарегистрированного проекта. Отвечает правилу — регистрируется найденным
    /// ленивым. `Some` — папка теперь в реестре.
    pub(super) fn register_for_selector(&self, selector: &str) -> Option<PathBuf> {
        let target = selector_target(&lock(&self.inner.registry), selector)?;
        self.register_chain(&target);
        lock(&self.inner.registry)
            .find(&target)
            .map(|entry| entry.path.clone())
    }

    /// Точечная проверка начала сессии: подпроект выше `cwd` и ниже зарегистрированного проекта
    /// регистрируется найденным ленивым (две-три проверки существования, без обхода).
    pub(crate) fn register_for_cwd(&self, cwd: &Path) -> Vec<PathBuf> {
        match normalize(cwd) {
            Ok(cwd) => self.register_chain(&cwd),
            Err(_) => Vec::new(),
        }
    }

    /// Регистрирует найденными ленивыми подпроекты на пути от ближайшего зарегистрированного
    /// предка к `target`, от внешнего к внутреннему; `target` зарегистрирован — ничего не делает.
    /// Возвращает вновь зарегистрированные папки.
    fn register_chain(&self, target: &Path) -> Vec<PathBuf> {
        // Идёт удаление проекта — реестр не трогаем, как `register`.
        let Ok(_request) = self.inner.requests.try_read() else {
            return Vec::new();
        };
        let mut added: Vec<PathBuf> = Vec::new();
        let mut parents: Vec<PathBuf> = Vec::new();
        for _ in 0..MAX_CHAIN {
            let parent = {
                let registry = lock(&self.inner.registry);
                if registry.contains(target) {
                    break;
                }
                let Some(parent) = target.ancestors().find_map(|dir| registry.find(dir)) else {
                    break;
                };
                parent.path.clone()
            };
            if !parent.is_dir() {
                break;
            }
            let docs_root = self.inner.docs_root_of(&parent);
            let Some(found) = candidate_on_path(&parent, &docs_root, target) else {
                break;
            };
            match lock(&self.inner.registry).add_discovered(&found) {
                Ok(true) => {
                    added.push(found);
                    parents.push(parent);
                }
                Ok(false) => break,
                Err(error) => {
                    log::warn!("Подпроект {} не записан в реестр: {error}", found.display());
                    break;
                }
            }
        }
        if !added.is_empty() {
            self.inner.after_discovery(&parents, &added);
        }
        added
    }

    /// Ответ об неизвестном имени, когда папка `<корень главного>/<остаток>` есть, но правилу не
    /// отвечает: причина вместо перечня подпроектов.
    pub(super) fn not_a_subproject(&self, selector: &str, error: ResolveError) -> ResolveError {
        let ResolveError::UnknownSubproject { name, .. } = &error else {
            return error;
        };
        let found = composite_target(&lock(&self.inner.registry), selector);
        let Some((main, target)) = found else {
            return error;
        };
        if !target.is_dir() {
            return error;
        }
        ResolveError::UnknownSubproject {
            name: name.clone(),
            message: format!(
                "проект `{name}` не зарегистрирован: папка есть, но это не подпроект — нужны свой \
                 каталог `.git` (не файл worktree) и `docs/` хотя бы с одним `.md`, папка вне \
                 корня документации `{main}`; зарегистрировать вручную — bars-cli register <путь>"
            ),
        }
    }
}

impl Inner {
    /// Запись реестра ленивая (найденный подпроект, которого ещё не требовали).
    pub(super) fn is_lazy(&self, path: &Path) -> bool {
        lock(&self.registry)
            .find(path)
            .is_some_and(|entry| entry.lazy)
    }

    /// Первое обращение к ленивому проекту: `lazy` снимается (файл реестра переписывается),
    /// `.bars/config.toml` с `docs_root = docs` создаётся, если его нет, состояние `Lazy`
    /// становится `Ready`; дальше проект идёт обычным путём. `true` — проект разбужен сейчас.
    /// Повторное обращение ничего не переписывает. Папки нет — проект `unavailable`, `lazy` остаётся.
    pub(super) fn wake_lazy(&self, path: &Path) -> bool {
        if !self.is_lazy(path) {
            return false;
        }
        if lock(&self.projects)
            .get(path)
            .is_some_and(|runtime| runtime.deleting)
        {
            return false;
        }
        if !path.is_dir() {
            drop(self.mark_unavailable(path));
            return false;
        }
        let _publication = self.updates.after(path, updates::STATUS);
        match lock(&self.registry).clear_lazy(path) {
            Ok(_) => {}
            Err(error) => {
                log::warn!("Не удалось снять lazy у {}: {error}", path.display());
                if let Some(runtime) = lock(&self.projects).get_mut(path) {
                    runtime.status.last_error = Some(error.to_string());
                }
                return false;
            }
        }
        // Умолчание Т-7; существующий `config.toml` не трогается.
        if let Err(error) = bars_protocol::prepare_project(path, None) {
            log::warn!("Не удалось создать .bars у {}: {error}", path.display());
            if let Some(runtime) = lock(&self.projects).get_mut(path) {
                runtime.status.last_error = Some(error.to_string());
            }
        }
        let mut projects = lock(&self.projects);
        let runtime = projects
            .entry(path.to_path_buf())
            .or_insert_with(Runtime::new);
        if matches!(
            runtime.status.state,
            ProjectState::Lazy | ProjectState::Unavailable
        ) {
            runtime.status.state = ProjectState::Ready;
        }
        true
    }

    /// Обход этого проекта оборван пределом.
    pub(super) fn discovery_truncated(&self, path: &Path) -> bool {
        lock(&self.discovery.state).truncated.contains(path)
    }

    /// Ставит фоновый обход проекта; повторный запрос того же проекта до начала обхода сливается
    /// с ожидающим. `window` — запрос от открытия окна, не чаще раза в пять минут.
    pub(super) fn schedule_discovery(self: &Arc<Self>, path: &Path, window: bool) {
        let path = path.to_path_buf();
        {
            let mut state = lock(&self.discovery.state);
            if window
                && state
                    .last_window
                    .get(&path)
                    .is_some_and(|started| started.elapsed() < WINDOW_RESCAN)
            {
                return;
            }
            if !state.scheduled.insert(path.clone()) {
                return;
            }
            if window {
                state.last_window.insert(path.clone(), Instant::now());
            }
        }
        let inner = Arc::clone(self);
        self.handle.spawn_blocking(move || {
            lock(&inner.discovery.state).scheduled.remove(&path);
            inner.discover_chain(&path);
        });
    }

    /// Старт воркера: обход каждого зарегистрированного проекта в фоне, канал не ждёт.
    pub(super) fn schedule_discovery_all(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        self.handle.spawn_blocking(move || inner.discover_all());
    }

    /// Обход одного проекта и найденных в нём (транзитивно); ждёт своей очереди на проход.
    pub(super) fn discover_chain(self: &Arc<Self>, start: &Path) {
        let _pass = lock(&self.discovery.pass);
        self.run_discovery(vec![start.to_path_buf()]);
    }

    /// Обход всех зарегистрированных проектов по очереди: старт и часовая проверка. Сначала
    /// убираются найденные записи без предка.
    pub(super) fn discover_all(self: &Arc<Self>) {
        let _pass = lock(&self.discovery.pass);
        if self.remove_orphans() {
            self.refresh_nested();
        }
        let roots = lock(&self.registry).paths();
        self.run_discovery(roots);
    }

    /// Проход по списку проектов; найденные подпроекты добавляются в список и обходятся сами.
    /// Вызывается под замком `pass`.
    fn run_discovery(self: &Arc<Self>, roots: Vec<PathBuf>) {
        let mut queue = roots;
        queue.reverse();
        let mut seen: HashSet<PathBuf> = HashSet::new();
        while let Some(project) = queue.pop() {
            if self.stop.load(Ordering::SeqCst) {
                return;
            }
            if !seen.insert(project.clone()) {
                continue;
            }
            queue.extend(self.discover_project(&project));
        }
    }

    /// Один проход автопоиска по проекту: регистрирует найденное и убирает найденные записи, не
    /// отвечающие правилу. Недоступный проект не обходится и не проверяется: отсутствие диска не
    /// доказывает, что подпроект исчез. Возвращает вновь зарегистрированное.
    fn discover_project(self: &Arc<Self>, project: &Path) -> Vec<PathBuf> {
        let registered = {
            let registry = lock(&self.registry);
            if !registry.contains(project) {
                return Vec::new();
            }
            registry.paths()
        };
        if !project.is_dir() {
            return Vec::new();
        }
        let docs_root = self.docs_root_of(project);
        let found = discover(project, &docs_root, &registered, Limits::default());
        let truncated_changed = {
            let mut state = lock(&self.discovery.state);
            if found.truncated {
                state.truncated.insert(project.to_path_buf())
            } else {
                state.truncated.remove(project)
            }
        };
        if found.truncated && truncated_changed {
            log::warn!(
                "Автопоиск подпроектов {} оборван пределом ({} записей, глубина {}): найденное сохранено",
                project.display(),
                found.entries,
                MAX_DEPTH
            );
        }
        let mut added: Vec<PathBuf> = Vec::new();
        {
            let mut registry = lock(&self.registry);
            if registry.contains(project) {
                for path in &found.found {
                    match registry.add_discovered(path) {
                        Ok(true) => added.push(path.clone()),
                        Ok(false) => {}
                        Err(error) => {
                            log::warn!("Подпроект {} не записан в реестр: {error}", path.display());
                            break;
                        }
                    }
                }
            }
        }
        let removed = self.remove_vanished(project, &docs_root);
        if !added.is_empty() {
            self.after_discovery(&[project.to_path_buf()], &added);
        } else if removed || truncated_changed {
            self.refresh_nested();
            self.updates.changed(project, updates::STATUS, false);
        }
        added
    }

    /// Найденные записи, чей ближайший предок — `parent`, проверяются правилом по самой записи (не
    /// по результату обхода): папка есть, в ней `.git`-каталог и `docs/` с `.md`, она вне корня
    /// документации предка. Не прошла — запись убирается тем же кодом, что снятие с регистрации;
    /// с диска не удаляется ничего. Самостоятельные записи не трогаются.
    fn remove_vanished(&self, parent: &Path, docs_root: &str) -> bool {
        let candidates: Vec<PathBuf> = {
            let registry = lock(&self.registry);
            registry
                .entries()
                .iter()
                .filter(|entry| {
                    entry.discovered
                        && registry
                            .parent_of(&entry.path)
                            .is_some_and(|found| found.path == parent)
                })
                .map(|entry| entry.path.clone())
                .collect()
        };
        let mut removed = false;
        for path in candidates {
            let deleting = lock(&self.projects)
                .get(&path)
                .is_some_and(|runtime| runtime.deleting);
            if deleting || matches_rule(parent, docs_root, &path) {
                continue;
            }
            match self.remove_entry(&path) {
                Ok(true) => removed = true,
                Ok(false) => {}
                Err(error) => {
                    log::warn!("Подпроект {} не убран: {error}", path.display());
                }
            }
        }
        removed
    }

    /// Убирает найденные записи без зарегистрированного предка (транзитивно по цепочке).
    /// Самостоятельные подпроекты остаются обычными проектами. `true` — что-то убрано.
    pub(super) fn remove_orphans(&self) -> bool {
        let mut removed = false;
        let limit = lock(&self.registry).entries().len();
        for _ in 0..=limit {
            let orphan = {
                let registry = lock(&self.registry);
                registry
                    .entries()
                    .iter()
                    .find(|entry| entry.discovered && registry.parent_of(&entry.path).is_none())
                    .map(|entry| entry.path.clone())
            };
            let Some(orphan) = orphan else {
                break;
            };
            match self.remove_entry(&orphan) {
                Ok(true) => removed = true,
                _ => break,
            }
        }
        removed
    }

    /// Убирает запись из реестра и останавливает всё, что с ней связано: слежение, очередь, scan,
    /// граф. Данные проекта на диске не трогаются (Т-5). `Ok(false)` — записи не было.
    pub(super) fn remove_entry(&self, path: &Path) -> Result<bool, WorkerError> {
        let _publication = self.updates.after(path, updates::ALL);
        let removed = lock(&self.registry).remove(path)?;
        let runtime = lock(&self.projects).remove(path);
        self.graph_cache.remove(path);
        self.forget_worktrees(path);
        self.cancel_deltas(path);
        self.cancel_queued(path);
        self.cancel_scan(path);
        // Слежение останавливается вне блокировок: поток `notify` может ждать их.
        drop(runtime);
        Ok(removed)
    }

    /// После регистрации найденных: ленивые `Runtime` (иначе проект показан как `unavailable`),
    /// области вложенных у предков и обновление списка в окне.
    fn after_discovery(&self, parents: &[PathBuf], added: &[PathBuf]) {
        {
            let mut projects = lock(&self.projects);
            for path in added {
                projects
                    .entry(path.clone())
                    .or_insert_with(Runtime::new_lazy);
            }
        }
        self.refresh_nested();
        // Новые подпроекты — тоже владельцы worktree; ленивым дельта не ставится.
        if !added.is_empty() {
            self.discover_worktrees(Some(added));
        }
        for parent in parents {
            self.updates.changed(parent, updates::STATUS, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Репозиторий: каталог `.git` и, при `docs`, `docs/x.md`.
    fn repo(root: &Path, rel: &str, docs: bool) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.join(".git")).unwrap();
        if docs {
            fs::create_dir_all(path.join("docs")).unwrap();
            fs::write(path.join("docs/x.md"), "# x\n").unwrap();
        }
        path
    }

    /// Worktree: файл `.git`, документы на месте.
    fn worktree(root: &Path, rel: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.join("docs")).unwrap();
        fs::write(path.join(".git"), "gitdir: /nowhere/.git/worktrees/wt\n").unwrap();
        fs::write(path.join("docs/x.md"), "# x\n").unwrap();
        path
    }

    /// Главный проект по образцу `tmc-opovechenie`: свои `docs`, четыре подпроекта, `deploy` без
    /// `docs/` и worktree рядом с `backend/api`.
    fn opovechenie() -> (TempDir, PathBuf) {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("tmc-opovechenie");
        fs::create_dir_all(main.join("docs")).unwrap();
        fs::write(main.join("docs/index.md"), "# главный\n").unwrap();
        repo(&main, "backend/api", true);
        repo(&main, "backend/worker", true);
        repo(&main, "frontend/web-angular", true);
        repo(&main, "frontend/web-design", true);
        repo(&main, "deploy", false);
        worktree(&main, "backend/api-wt-quorum");
        (temp, main)
    }

    fn found(main: &Path, docs_root: &str, registered: &[PathBuf]) -> Vec<PathBuf> {
        discover(main, docs_root, registered, Limits::default()).found
    }

    fn relative(main: &Path, paths: &[PathBuf]) -> Vec<String> {
        paths
            .iter()
            .map(|path| {
                path.strip_prefix(main)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn finds_exactly_the_four_repositories_of_the_sample_tree() {
        let (_temp, main) = opovechenie();
        let result = discover(&main, "docs", &[main.clone()], Limits::default());
        assert_eq!(
            relative(&main, &result.found),
            [
                "backend/api",
                "backend/worker",
                "frontend/web-angular",
                "frontend/web-design"
            ]
        );
        assert!(!result.truncated);
    }

    #[test]
    fn docs_root_dot_finds_nothing() {
        let (_temp, main) = opovechenie();
        let result = discover(&main, ".", &[], Limits::default());
        assert!(result.found.is_empty());
        assert_eq!(result.entries, 0, "каталоги даже не читаются");
    }

    #[test]
    fn repository_inside_the_docs_root_of_the_main_project_is_not_found() {
        let (_temp, main) = opovechenie();
        repo(&main, "docs/vendored", true);
        assert!(!found(&main, "docs", &[]).contains(&main.join("docs/vendored")));
        assert!(!matches_rule(&main, "docs", &main.join("docs/vendored")));
    }

    #[test]
    fn hidden_node_modules_and_target_folders_are_skipped() {
        let (_temp, main) = opovechenie();
        repo(&main, ".claude/inner", true);
        repo(&main, "node_modules/pkg", true);
        repo(&main, "target/debug/inner", true);
        let result = relative(&main, &found(&main, "docs", &[]));
        assert_eq!(result.len(), 4, "{result:?}");
    }

    #[test]
    fn git_file_skips_the_folder_without_entering_it() {
        let (_temp, main) = opovechenie();
        // Репозиторий внутри worktree обходу не виден.
        repo(&main, "backend/api-wt-quorum/inner", true);
        let result = relative(&main, &found(&main, "docs", &[]));
        assert!(!result.iter().any(|path| path.contains("api-wt-quorum")));
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn repository_without_docs_is_a_boundary() {
        let (_temp, main) = opovechenie();
        repo(&main, "deploy/inner", true);
        let result = relative(&main, &found(&main, "docs", &[]));
        assert!(!result.iter().any(|path| path.starts_with("deploy")));
    }

    #[test]
    fn nested_repository_is_not_seen_from_the_main_project_but_found_from_the_subproject() {
        let (_temp, main) = opovechenie();
        let api = main.join("backend/api");
        repo(&api, "vendor/x", true);
        assert!(!found(&main, "docs", &[]).contains(&api.join("vendor/x")));
        assert_eq!(found(&api, "docs", &[]), vec![api.join("vendor/x")]);
    }

    #[test]
    fn registered_projects_are_not_entered() {
        let (_temp, main) = opovechenie();
        let api = main.join("backend/api");
        repo(&api, "vendor/x", true);
        let result = relative(&main, &found(&main, "docs", &[api.clone()]));
        assert_eq!(
            result,
            [
                "backend/worker",
                "frontend/web-angular",
                "frontend/web-design"
            ]
        );
    }

    #[test]
    fn docs_with_markdown_only_in_a_dotted_folder_do_not_count() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        fs::create_dir_all(main.join("docs")).unwrap();
        let hidden = repo(&main, "svc", false);
        fs::create_dir_all(hidden.join("docs/.cache")).unwrap();
        fs::write(hidden.join("docs/.cache/x.md"), "# x\n").unwrap();
        assert!(found(&main, "docs", &[]).is_empty());
        assert!(!matches_rule(&main, "docs", &hidden));
    }

    #[test]
    fn depth_limit_marks_the_pass_truncated_and_keeps_what_was_found() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        fs::create_dir_all(main.join("docs")).unwrap();
        repo(&main, "a/near", true);
        repo(&main, "a/b/far", true);
        let limits = Limits {
            depth: 2,
            entries: MAX_ENTRIES,
        };
        let result = discover(&main, "docs", &[], limits);
        assert_eq!(relative(&main, &result.found), ["a/near"]);
        assert!(result.truncated);
        // Тот же обход с запасом глубины находит обе и неполным не считается.
        let deep = discover(&main, "docs", &[], Limits::default());
        assert_eq!(deep.found.len(), 2);
        assert!(!deep.truncated);
    }

    #[test]
    fn entry_limit_marks_the_pass_truncated_and_stops_reading() {
        let temp = TempDir::new().unwrap();
        let main = temp.path().join("main");
        fs::create_dir_all(main.join("docs")).unwrap();
        for index in 0..20 {
            fs::create_dir_all(main.join(format!("folder{index:02}"))).unwrap();
        }
        let limits = Limits {
            depth: MAX_DEPTH,
            entries: 5,
        };
        let result = discover(&main, "docs", &[], limits);
        assert!(result.truncated);
        assert!(result.entries <= 6, "прочитано {}", result.entries);
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_link_to_a_repository_is_not_followed() {
        let (temp, main) = opovechenie();
        let outside = repo(temp.path(), "outside", true);
        std::os::unix::fs::symlink(&outside, main.join("link")).unwrap();
        let result = relative(&main, &found(&main, "docs", &[]));
        assert!(!result.iter().any(|path| path == "link"));
    }

    #[test]
    fn rule_needs_git_directory_markdown_and_a_place_inside_the_main_project() {
        let (temp, main) = opovechenie();
        assert!(matches_rule(&main, "docs", &main.join("backend/api")));
        assert!(
            !matches_rule(&main, "docs", &main.join("deploy")),
            "нет docs/"
        );
        assert!(
            !matches_rule(&main, "docs", &main.join("backend/api-wt-quorum")),
            "файл .git"
        );
        assert!(!matches_rule(&main, "docs", &main), "не строго внутри");
        let outside = repo(temp.path(), "other", true);
        assert!(!matches_rule(&main, "docs", &outside));
        assert!(!matches_rule(&main, ".", &main.join("backend/api")));
    }

    #[test]
    fn point_check_finds_the_outermost_repository_on_the_way_to_the_folder() {
        let (_temp, main) = opovechenie();
        let api = main.join("backend/api");
        repo(&api, "vendor/x", true);
        fs::create_dir_all(api.join("src/deep")).unwrap();
        for target in [api.clone(), api.join("src/deep"), api.join("vendor/x")] {
            assert_eq!(
                candidate_on_path(&main, "docs", &target),
                Some(api.clone()),
                "{}",
                target.display()
            );
        }
        // Изнутри самого подпроекта вложенный репозиторий — следующий найденный.
        assert_eq!(
            candidate_on_path(&api, "docs", &api.join("vendor/x")),
            Some(api.join("vendor/x"))
        );
    }

    #[test]
    fn point_check_stops_at_boundaries() {
        let (_temp, main) = opovechenie();
        fs::create_dir_all(main.join("deploy/sub")).unwrap();
        repo(&main, "deploy/inner", true);
        fs::create_dir_all(main.join("docs/inside")).unwrap();
        fs::create_dir_all(main.join(".claude/worktrees/wt")).unwrap();
        for target in [
            main.join("deploy"),
            main.join("deploy/inner"),
            main.join("backend/api-wt-quorum"),
            main.join("backend/api-wt-quorum/docs"),
            main.join("docs/inside"),
            main.join(".claude/worktrees/wt"),
            main.join("backend"),
            main.clone(),
            main.join("missing/folder"),
        ] {
            assert_eq!(
                candidate_on_path(&main, "docs", &target),
                None,
                "{}",
                target.display()
            );
        }
        assert_eq!(
            candidate_on_path(&main, ".", &main.join("backend/api")),
            None
        );
    }
}
