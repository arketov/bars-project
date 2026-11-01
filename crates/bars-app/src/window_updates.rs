//! Общие снимки наблюдаемых проектов; IO вне замков, один flight на проект.
use bars_protocol::{Call, StatusParams, Target, TreeParams};
use bars_worker::{
    handler::WorkerHandler,
    worker::updates::{ALL, Revisions},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{OnceCell, Semaphore};

const NAMES: [&str; 3] = ["disk", "index", "status"];
const CACHE_BYTES: usize = 32 * 1024 * 1024;
const CACHE_PROJECTS: usize = 8;
const CONTROL: Duration = Duration::from_secs(30);
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}
type Sink = Arc<dyn Fn(&str, Value) + Send + Sync>;
type Visible = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone, Default)]
struct Part {
    revision: u64,
    hash: Option<blake3::Hash>,
    error: Option<String>,
    value: Option<Arc<Value>>,
    bytes: usize,
}
#[derive(Clone)]
struct Snapshot {
    epoch: u64,
    identity: String,
    parts: [Part; 3],
    built: [u64; 3],
}
type Flight = OnceCell<Result<Arc<Snapshot>, String>>;
struct Entry {
    path: PathBuf,
    epoch: AtomicU64,
    identity: Mutex<String>,
    required: AtomicU8,
    wanted: [AtomicU64; 3],
    source: Mutex<Revisions>,
    meta: Mutex<[Part; 3]>,
    built: Mutex<[u64; 3]>,
    cache: Mutex<Option<Arc<Snapshot>>>,
    flight: Mutex<Option<Arc<Flight>>>,
    background: AtomicBool,
    next_check: Mutex<Instant>,
    failures: Mutex<usize>,
    touched: Mutex<Instant>,
    emitted: Mutex<(u64, [u64; 3])>,
    debounce: Mutex<Option<(Instant, Instant)>>,
    retry_at: Mutex<Option<Instant>>,
}
impl Entry {
    fn invalidate(&self, mask: u8) {
        for (i, revision) in self.wanted.iter().enumerate() {
            if mask & (1 << i) != 0 {
                revision.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    fn dirty(&self) -> u8 {
        let built = lock(&self.built);
        (0..3).fold(0, |mask, i| {
            mask | if built[i] != self.wanted[i].load(Ordering::SeqCst) {
                1 << i
            } else {
                0
            }
        }) & self.required.load(Ordering::SeqCst)
    }
}
#[derive(Clone)]
struct Subscription {
    token: String,
    entry: Arc<Entry>,
    parts: u8,
}
#[derive(Default)]
struct State {
    windows: HashMap<String, Subscription>,
    entries: HashMap<PathBuf, Arc<Entry>>,
}
impl State {
    fn retain_observed(&mut self) {
        let live: Vec<_> = self.windows.values().map(|s| s.entry.clone()).collect();
        self.entries
            .retain(|_, e| live.iter().any(|v| Arc::ptr_eq(e, v)));
        for entry in self.entries.values() {
            entry.required.store(
                self.windows
                    .values()
                    .filter(|s| Arc::ptr_eq(&s.entry, entry))
                    .fold(0, |mask, s| mask | s.parts),
                Ordering::SeqCst,
            );
        }
    }
}
struct Inner {
    state: Mutex<State>,
    clock: AtomicU64,
    started: AtomicBool,
    generation: AtomicU64,
    io: Arc<Semaphore>,
    budget: Mutex<()>,
    sink: Mutex<Option<Sink>>,
    pub builds: [AtomicU64; 3],
}
#[derive(Clone)]
pub struct Coordinator(Arc<Inner>);
impl Default for Coordinator {
    fn default() -> Self {
        Self(Arc::new(Inner {
            state: Mutex::default(),
            clock: AtomicU64::new(1),
            started: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            io: Arc::new(Semaphore::new(2)),
            budget: Mutex::new(()),
            sink: Mutex::new(None),
            builds: Default::default(),
        }))
    }
}

pub fn parts(names: &[String]) -> Result<u8, String> {
    let mut mask = 0;
    for name in names {
        mask |= 1
            << NAMES
                .iter()
                .position(|n| n == name)
                .ok_or("Неизвестная часть снимка")?;
    }
    if mask == 0 {
        return Err("Не выбраны части снимка".into());
    }
    Ok(mask)
}

impl Coordinator {
    pub fn observed(&self, window: &str) -> bool {
        lock(&self.0.state).windows.contains_key(window)
    }
    pub fn statistics(&self) -> Value {
        let entries: Vec<_> = lock(&self.0.state).entries.values().cloned().collect();
        json!({"builds":self.0.builds.each_ref().map(|v|v.load(Ordering::SeqCst)),"projects":entries.len(),"cache_bytes":entries.iter().map(|e|lock(&e.cache).as_ref().map_or(0,|s|snapshot_bytes(s))).sum::<usize>()})
    }
    pub fn reset(&self) {
        self.0.generation.fetch_add(1, Ordering::SeqCst);
        *lock(&self.0.state) = State::default();
        self.0.started.store(false, Ordering::SeqCst);
    }
    pub fn start(&self, handler: WorkerHandler, sink: Sink, visible: Visible) {
        if self.0.started.swap(true, Ordering::SeqCst) {
            return;
        }
        *lock(&self.0.sink) = Some(sink);
        let weak = Arc::downgrade(&self.0);
        let generation = self.0.generation.load(Ordering::SeqCst);
        let mut updates = handler.worker().window_updates().subscribe();
        tauri::async_runtime::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_millis(250));
            loop {
                tokio::select! { _ = timer.tick() => {}, changed = updates.changed() => { if changed.is_err() { break; } } }
                let Some(inner) = weak.upgrade() else {
                    break;
                };
                if inner.generation.load(Ordering::SeqCst) != generation {
                    break;
                }
                let coordinator = Coordinator(inner);
                let windows: Vec<_> = lock(&coordinator.0.state)
                    .windows
                    .iter()
                    .map(|(w, s)| (w.clone(), s.entry.path.clone()))
                    .collect();
                for (window, path) in windows {
                    if !visible(&window) || !handler.worker().window_is_observing(&window, &path) {
                        coordinator.release(&window);
                    }
                }
                let entries: Vec<_> = lock(&coordinator.0.state)
                    .entries
                    .values()
                    .cloned()
                    .collect();
                for entry in entries {
                    coordinator.source(&handler, &entry);
                    let now = Instant::now();
                    if lock(&entry.retry_at).is_some_and(|retry| now < retry) {
                        continue;
                    }
                    {
                        let mut next = lock(&entry.next_check);
                        if now >= *next {
                            entry.invalidate(entry.required.load(Ordering::SeqCst));
                            *next = now + CONTROL;
                        }
                    }
                    if lock(&entry.debounce).is_some_and(|(first, last)| {
                        now < (last + Duration::from_millis(250))
                            .min(first + Duration::from_secs(1))
                    }) {
                        continue;
                    }
                    if entry.dirty() == 0 || entry.background.swap(true, Ordering::SeqCst) {
                        continue;
                    }
                    let coordinator = coordinator.clone();
                    let handler = handler.clone();
                    tauri::async_runtime::spawn(async move {
                        // Watcher уже объединяет пачку 500 мс; здесь не добавляется второй debounce.
                        let _ = coordinator.collect(&handler, &entry, 0).await;
                        entry.background.store(false, Ordering::SeqCst);
                    });
                }
            }
        });
    }
    fn source(&self, handler: &WorkerHandler, entry: &Arc<Entry>) {
        let revisions = handler.worker().window_updates().revisions(&entry.path);
        let mut previous = lock(&entry.source);
        let mut mask = 0;
        for i in 0..3 {
            if previous.parts[i] != revisions.parts[i] {
                mask |= 1 << i;
            }
        }
        let recovery = previous.recovery != revisions.recovery;
        if recovery {
            mask = ALL;
        }
        *previous = revisions;
        drop(previous);
        entry.invalidate(mask);
        if mask != 0 {
            *lock(&entry.retry_at) = None;
            let now = Instant::now();
            let mut debounce = lock(&entry.debounce);
            let first = debounce.map_or(now, |(first, _)| first);
            *debounce = Some((first, now));
        }
        if recovery {
            let sink = lock(&self.0.sink).clone();
            if let Some(sink) = sink {
                let revisions = lock(&entry.meta).each_ref().map(|p| p.revision);
                let windows: Vec<_> = lock(&self.0.state)
                    .windows
                    .iter()
                    .filter(|(_, s)| Arc::ptr_eq(&s.entry, entry))
                    .map(|(w, s)| (w.clone(), s.token.clone()))
                    .collect();
                for (window, token) in windows {
                    sink(
                        &window,
                        json!({"project":entry.path,"subscription":token,"epoch":entry.epoch.load(Ordering::SeqCst),"revisions":revisions,"recovery":true}),
                    );
                }
            }
            *lock(&entry.debounce) = None;
        }
    }
    pub fn observe(
        &self,
        handler: &WorkerHandler,
        window: &str,
        project: &str,
        mask: u8,
    ) -> Result<Value, String> {
        let path = handler
            .worker()
            .resolve(project)
            .map_err(|e| e.to_string())?;
        let identity = handler
            .worker()
            .folder_identity(&path)
            .map_err(|e| e.to_string())?;
        handler
            .worker()
            .observe_window(window, Some(&path))
            .map_err(|e| e.to_string())?;
        let mut state = lock(&self.0.state);
        let entry = state
            .entries
            .entry(path.clone())
            .or_insert_with(|| {
                Arc::new(Entry {
                    path: path.clone(),
                    epoch: AtomicU64::new(self.0.clock.fetch_add(1, Ordering::SeqCst)),
                    identity: Mutex::new(identity),
                    required: AtomicU8::new(0),
                    wanted: Default::default(),
                    source: Mutex::new(handler.worker().window_updates().revisions(&path)),
                    meta: Default::default(),
                    built: Default::default(),
                    cache: Default::default(),
                    flight: Default::default(),
                    background: AtomicBool::new(false),
                    next_check: Mutex::new(Instant::now() + CONTROL),
                    failures: Mutex::new(0),
                    touched: Mutex::new(Instant::now()),
                    emitted: Default::default(),
                    debounce: Default::default(),
                    retry_at: Default::default(),
                })
            })
            .clone();
        entry.invalidate(mask);
        let token = self.0.clock.fetch_add(1, Ordering::SeqCst).to_string();
        state.windows.insert(
            window.to_owned(),
            Subscription {
                token: token.clone(),
                entry: entry.clone(),
                parts: mask,
            },
        );
        state.retain_observed();
        Ok(
            json!({"subscription":token,"project":path,"epoch":entry.epoch.load(Ordering::SeqCst),"identity":*lock(&entry.identity),"revisions":lock(&entry.meta).each_ref().map(|p|p.revision)}),
        )
    }
    pub fn release(&self, window: &str) {
        let mut state = lock(&self.0.state);
        state.windows.remove(window);
        state.retain_observed();
    }
    fn subscription(&self, window: &str, token: &str) -> Result<Subscription, String> {
        lock(&self.0.state)
            .windows
            .get(window)
            .filter(|s| s.token == token)
            .cloned()
            .ok_or_else(|| "Подписка окна истекла".into())
    }
    fn live(&self, entry: &Arc<Entry>) -> bool {
        lock(&self.0.state)
            .entries
            .get(&entry.path)
            .is_some_and(|e| Arc::ptr_eq(e, entry))
    }
    pub async fn snapshot(
        &self,
        handler: &WorkerHandler,
        window: &str,
        token: &str,
        known: Value,
        force: bool,
    ) -> Result<Value, String> {
        let subscription = self.subscription(window, token)?;
        let entry = &subscription.entry;
        self.source(handler, entry);
        if force {
            *lock(&entry.retry_at) = None;
            entry.invalidate(subscription.parts);
        }
        let required = std::array::from_fn::<_, 3, _>(|i| entry.wanted[i].load(Ordering::SeqCst));
        let mut needed = 0;
        {
            let meta = lock(&entry.meta);
            for i in 0..3 {
                if subscription.parts & (1 << i) != 0
                    && (known["epoch"].as_u64() != Some(entry.epoch.load(Ordering::SeqCst))
                        || known["revisions"][i].as_u64() != Some(meta[i].revision))
                {
                    needed |= 1 << i;
                }
            }
        }
        loop {
            let snapshot = self.collect(handler, entry, needed).await?;
            self.subscription(window, token)?;
            if (0..3).any(|i| subscription.parts & (1 << i) != 0 && snapshot.built[i] < required[i])
            {
                continue;
            }
            if (0..3).any(|i| {
                needed & (1 << i) != 0
                    && snapshot.parts[i].value.is_none()
                    && snapshot.parts[i].error.is_none()
            }) {
                continue;
            }
            let mut output = serde_json::Map::new();
            for (i, name) in NAMES.iter().enumerate() {
                if subscription.parts & (1 << i) == 0 {
                    continue;
                }
                let part = &snapshot.parts[i];
                let unchanged = known["epoch"].as_u64() == Some(snapshot.epoch)
                    && known["revisions"][i].as_u64() == Some(part.revision);
                output.insert((*name).into(), if unchanged { json!({"revision":part.revision,"state":if part.error.is_some(){"error"}else{"ready"},"unchanged":true,"error":part.error}) }
                    else { json!({"revision":part.revision,"state":if part.error.is_some(){"error"}else{"ready"},"error":part.error,"value":part.value.as_deref()}) });
            }
            return Ok(
                json!({"subscription":token,"project":entry.path,"epoch":snapshot.epoch,"identity":snapshot.identity,"revisions":snapshot.parts.each_ref().map(|p|p.revision),"updating":entry.dirty()!=0,"parts":output,"diagnostics":self.statistics()}),
            );
        }
    }
    async fn collect(
        &self,
        handler: &WorkerHandler,
        entry: &Arc<Entry>,
        needed: u8,
    ) -> Result<Arc<Snapshot>, String> {
        *lock(&entry.touched) = Instant::now();
        if entry.dirty() == 0
            && let Some(cache) = lock(&entry.cache).clone()
            && (0..3).all(|i| {
                needed & (1 << i) == 0
                    || cache.parts[i].value.is_some()
                    || cache.parts[i].error.is_some()
            })
        {
            return Ok(cache);
        }
        let flight = lock(&entry.flight)
            .get_or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();
        let result = flight
            .get_or_init(|| self.build(handler, entry, needed))
            .await
            .clone();
        if result.is_err() {
            let mut failures = lock(&entry.failures);
            let delay = [1, 2, 5, 10, 30][(*failures).min(4)];
            *failures += 1;
            *lock(&entry.retry_at) = Some(Instant::now() + Duration::from_secs(delay));
        }
        let mut current = lock(&entry.flight);
        if current.as_ref().is_some_and(|f| Arc::ptr_eq(f, &flight)) {
            *current = None;
        }
        result
    }
    async fn build(
        &self,
        handler: &WorkerHandler,
        entry: &Arc<Entry>,
        needed: u8,
    ) -> Result<Arc<Snapshot>, String> {
        let _permit = self
            .0
            .io
            .clone()
            .acquire_owned()
            .await
            .map_err(|e| e.to_string())?;
        if !self.live(entry) {
            return Err("Наблюдение завершено".into());
        }
        let identity = handler
            .worker()
            .folder_identity(&entry.path)
            .map_err(|e| e.to_string())?;
        if *lock(&entry.identity) != identity {
            *lock(&entry.identity) = identity.clone();
            entry.epoch.store(
                self.0.clock.fetch_add(1, Ordering::SeqCst),
                Ordering::SeqCst,
            );
            entry.invalidate(entry.required.load(Ordering::SeqCst));
            *lock(&entry.cache) = None;
        }
        let source: [u64; 3] = std::array::from_fn(|i| entry.wanted[i].load(Ordering::SeqCst));
        *lock(&entry.debounce) = None;
        let cached = lock(&entry.cache).clone();
        let mut parts = cached
            .as_ref()
            .map(|s| s.parts.clone())
            .unwrap_or_else(|| lock(&entry.meta).clone());
        let missing = (0..3).fold(0, |mask, i| {
            mask | if needed & (1 << i) != 0 && parts[i].value.is_none() {
                1 << i
            } else {
                0
            }
        });
        let mask = entry.dirty() | missing;
        let mut built = *lock(&entry.built);
        for i in 0..3 {
            if mask & (1 << i) == 0 {
                continue;
            }
            self.0.builds[i].fetch_add(1, Ordering::SeqCst);
            let result = read_part(handler, &entry.path, i).await;
            let (value, error) = match result {
                Ok(value) => (Some(Arc::new(value)), None),
                Err(error) => (parts[i].value.clone(), Some(error)),
            };
            if error.is_none() {
                parts[i].bytes = value.as_deref().map_or(0, value_bytes);
            }
            let hash = blake3::hash(
                &serde_json::to_vec(
                    &json!({"value":if error.is_none(){value.as_deref()}else{None},"error":error}),
                )
                .map_err(|e| e.to_string())?,
            );
            if parts[i].hash != Some(hash) {
                parts[i].revision = self.0.clock.fetch_add(1, Ordering::SeqCst);
            }
            parts[i].hash = Some(hash);
            parts[i].error = error;
            parts[i].value = value;
            built[i] = source[i];
        }
        if handler
            .worker()
            .folder_identity(&entry.path)
            .ok()
            .as_deref()
            != Some(&identity)
            || !self.live(entry)
        {
            return Err("Проект изменился во время чтения; снимок не опубликован".into());
        }
        let snapshot = Arc::new(Snapshot {
            epoch: entry.epoch.load(Ordering::SeqCst),
            identity,
            parts,
            built,
        });
        *lock(&entry.built) = built;
        *lock(&entry.meta) = snapshot.parts.clone().map(|mut p| {
            p.value = None;
            p
        });
        let failed = snapshot.parts.iter().any(|p| p.error.is_some());
        if failed {
            let mut failures = lock(&entry.failures);
            let seconds = [1, 2, 5, 10, 30][(*failures).min(4)];
            *failures += 1;
            *lock(&entry.next_check) = Instant::now() + Duration::from_secs(seconds);
        } else if mask & entry.required.load(Ordering::SeqCst)
            == entry.required.load(Ordering::SeqCst)
        {
            *lock(&entry.failures) = 0;
            *lock(&entry.next_check) = Instant::now() + CONTROL;
        }
        if snapshot_bytes(&snapshot) <= CACHE_BYTES {
            *lock(&entry.cache) = Some(snapshot.clone());
        } else {
            *lock(&entry.cache) = None;
        }
        self.trim();
        let sink = lock(&self.0.sink).clone();
        if let Some(sink) = sink {
            self.emit(entry, &snapshot, &sink);
        }
        Ok(snapshot)
    }
    fn trim(&self) {
        let _budget = lock(&self.0.budget);
        let mut entries: Vec<_> = lock(&self.0.state).entries.values().cloned().collect();
        entries.sort_by_key(|e| *lock(&e.touched));
        let mut count = 0;
        let mut bytes = 0;
        for entry in &entries {
            if let Some(cache) = lock(&entry.cache).as_ref() {
                count += 1;
                bytes += snapshot_bytes(cache);
            }
        }
        for entry in entries {
            if count <= CACHE_PROJECTS && bytes <= CACHE_BYTES {
                break;
            }
            if let Some(cache) = lock(&entry.cache).take() {
                count -= 1;
                bytes -= snapshot_bytes(&cache);
            }
        }
    }
    fn emit(&self, entry: &Arc<Entry>, snapshot: &Snapshot, sink: &Sink) {
        let revisions = snapshot.parts.each_ref().map(|p| p.revision);
        {
            let mut emitted = lock(&entry.emitted);
            if *emitted == (snapshot.epoch, revisions) {
                return;
            }
            *emitted = (snapshot.epoch, revisions);
        }
        let windows: Vec<_> = lock(&self.0.state)
            .windows
            .iter()
            .filter(|(_, s)| Arc::ptr_eq(&s.entry, entry))
            .map(|(w, s)| (w.clone(), s.token.clone()))
            .collect();
        for (window, token) in windows {
            sink(
                &window,
                json!({"subscription":token,"project":entry.path,"epoch":snapshot.epoch,"revisions":snapshot.parts.each_ref().map(|p|p.revision)}),
            );
        }
    }
}
fn value_bytes(value: &Value) -> usize {
    std::mem::size_of::<Value>()
        + match value {
            Value::String(s) => s.capacity(),
            Value::Array(a) => {
                a.iter().map(value_bytes).sum::<usize>()
                    + (a.capacity() - a.len()) * std::mem::size_of::<Value>()
            }
            Value::Object(o) => o
                .iter()
                .map(|(k, v)| k.capacity() + value_bytes(v) + 32)
                .sum(),
            _ => 0,
        }
}
fn snapshot_bytes(snapshot: &Snapshot) -> usize {
    std::mem::size_of::<Snapshot>()
        + snapshot.identity.capacity()
        + snapshot
            .parts
            .iter()
            .map(|p| {
                p.error.as_ref().map_or(0, String::capacity)
                    + if p.value.is_some() { p.bytes } else { 0 }
            })
            .sum::<usize>()
}
async fn read_part(
    handler: &WorkerHandler,
    path: &std::path::Path,
    part: usize,
) -> Result<Value, String> {
    let target = Target {
        cwd: path.display().to_string(),
        project: Some(path.display().to_string()),
    };
    match part {
        0 => {
            let docs_root = handler
                .worker()
                .docs_root(path)
                .map_err(|e| e.to_string())?;
            let path = path.to_owned();
            tokio::task::spawn_blocking(move || crate::tree_io::read_tree(&path,&docs_root).map(|tree|json!({"docs_root":tree.docs_root,"revision":tree.revision,"dirs":tree.dirs,"files":tree.files.iter().map(|f|json!({"path":f.path,"size":f.size,"modified":f.modified_ms})).collect::<Vec<_>>(),"problems":tree.problems.iter().map(|(path,reason)|json!({"path":path,"reason":reason})).collect::<Vec<_>>()})).map_err(|e|e.message())).await.map_err(|e|e.to_string())?
        }
        1 => handler
            .dispatch(Call::Tree(TreeParams { target }))
            .await
            .map_err(|e| e.message),
        _ => handler
            .dispatch(Call::Status(StatusParams { target }))
            .await
            .map_err(|e| e.message),
    }
}

#[cfg(test)]
#[path = "window_updates_tests.rs"]
mod tests;
