//! Ограниченный файловый журнал; контракт: docs/features/window/logs.md.
use std::collections::VecDeque;
use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MESSAGE: usize = 8192;
const ARCHIVES: usize = 3;
const LIMIT: usize = 500;
static JOURNAL: OnceLock<Journal> = OnceLock::new();
static LOGGER: FileLogger = FileLogger;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Entry {
    pub time: String,
    pub pid: u32,
    pub level: Level,
    pub target: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub directory: String,
    pub entries: Vec<Entry>,
    pub skipped: usize,
    pub total: usize,
    pub write_error: bool,
}

struct Journal {
    directory: PathBuf,
    max_bytes: u64,
    // Нет открытого дескриптора: ротация работает и на Windows.
    state: Mutex<bool>,
}

impl Journal {
    fn open(home: &Path, max_bytes: u64) -> io::Result<Self> {
        let directory = home.join("logs");
        fs::create_dir_all(&directory)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(directory.join("worker.jsonl"))?;
        // Незаконченная строка после сбоя не поглощает следующее событие.
        if file.metadata()?.len() > 0 {
            file.seek(SeekFrom::End(-1))?;
            let mut last = [0];
            file.read_exact(&mut last)?;
            if last[0] != b'\n' {
                file.write_all(b"\n")?;
            }
        }
        Ok(Self {
            directory,
            max_bytes,
            state: Mutex::new(false),
        })
    }

    fn file(&self, number: usize) -> PathBuf {
        self.directory.join(if number == 0 {
            "worker.jsonl".into()
        } else {
            format!("worker.{number}.jsonl")
        })
    }

    fn append(&self, entry: &Entry) -> io::Result<()> {
        let mut failed = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let result = (|| {
            let mut bytes = serde_json::to_vec(entry)?;
            bytes.push(b'\n');
            let path = self.file(0);
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            if size > 0 && size.saturating_add(bytes.len() as u64) > self.max_bytes {
                for n in (1..=ARCHIVES).rev() {
                    let destination = self.file(n);
                    if n == ARCHIVES {
                        match fs::remove_file(&destination) {
                            Ok(()) => {}
                            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                            Err(e) => return Err(e),
                        }
                    }
                    match fs::rename(self.file(n - 1), destination) {
                        Ok(()) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                        Err(e) => return Err(e),
                    }
                }
            }
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?
                .write_all(&bytes)
        })();
        *failed = result.is_err();
        result
    }

    fn snapshot(&self, level: Level) -> io::Result<Snapshot> {
        let failed = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries = VecDeque::with_capacity(LIMIT);
        let mut skipped = 0;
        let mut total = 0;
        for n in (0..=ARCHIVES).rev() {
            let file = match fs::File::open(self.file(n)) {
                Ok(file) => file,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            // Даже изменённый извне журнал не даёт неограниченное чтение/аллокацию.
            for line in BufReader::new(file.take(self.max_bytes + 65536)).split(b'\n') {
                let line = line?;
                if line.is_empty() {
                    continue;
                }
                let parsed = serde_json::from_slice::<Entry>(&line);
                if parsed.is_ok() {
                    total += 1;
                }
                match parsed {
                    Ok(entry) if entry.level >= level => {
                        if entries.len() == LIMIT {
                            entries.pop_front();
                        }
                        entries.push_back(entry);
                    }
                    Ok(_) => {}
                    Err(_) => skipped += 1,
                }
            }
        }
        Ok(Snapshot {
            directory: self.directory.display().to_string(),
            entries: entries.into_iter().rev().collect(),
            skipped,
            total,
            write_error: *failed,
        })
    }
}

struct FileLogger;
impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
            && matches!(
                metadata.target().split("::").next(),
                Some("bars" | "bars_core" | "bars_worker")
            )
    }
    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Some(journal) = JOURNAL.get() else {
            return;
        };
        let level = match record.level() {
            log::Level::Error => Level::Error,
            log::Level::Warn => Level::Warn,
            log::Level::Info => Level::Info,
            _ => Level::Debug,
        };
        let mut message = record.args().to_string();
        if message.len() > MAX_MESSAGE {
            let mut end = MAX_MESSAGE;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
            message.push('…');
        }
        let entry = Entry {
            time: crate::worker::rfc3339(SystemTime::now()),
            pid: std::process::id(),
            level,
            target: record.target().into(),
            message,
        };
        if journal.append(&entry).is_err() {
            // Не повторяем сообщение: оно может быть большим, диск уже неисправен.
            eprintln!("bars: не удалось записать журнал воркера");
        }
    }
    fn flush(&self) {}
}

/// Вызывается приложением после проверки единственного экземпляра. Тестовые воркеры не ставят
/// глобальный logger и не пишут в настоящий профиль.
pub fn init(home: &Path) -> io::Result<()> {
    if JOURNAL.get().is_some() {
        return Err(io::Error::other("журнал уже запущен"));
    }
    let journal = Journal::open(home, MAX_BYTES)?;
    log::set_logger(&LOGGER).map_err(|_| io::Error::other("logger уже установлен"))?;
    JOURNAL
        .set(journal)
        .map_err(|_| io::Error::other("журнал уже запущен"))?;
    log::set_max_level(log::LevelFilter::Debug);
    Ok(())
}

pub fn snapshot(level: Level) -> io::Result<Snapshot> {
    JOURNAL
        .get()
        .ok_or_else(|| io::Error::other("журнал воркера не запущен"))?
        .snapshot(level)
}

#[cfg(test)]
mod tests;
