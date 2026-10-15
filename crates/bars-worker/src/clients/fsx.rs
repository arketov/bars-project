//! Файловые операции над чужими конфигами: чтение, копия, атомарная запись.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{ClientsError, Result, io as io_err};

/// Текст файла; `None`, если файла нет.
pub fn read_text(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ClientsError::Io {
            file: path.to_path_buf(),
            source: error,
        }),
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Пишет файл через временный файл рядом и переименование. Существующий файл при `backup`
/// сначала копируется в `<файл>.bars-backup`.
pub fn write_atomic(path: &Path, text: &str, backup: bool) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    if backup && path.exists() {
        fs::copy(path, with_suffix(path, ".bars-backup")).map_err(io_err(path))?;
    }
    let temp = with_suffix(path, ".bars-tmp");
    fs::write(&temp, text).map_err(io_err(&temp))?;
    fs::rename(&temp, path).map_err(|error| {
        let _ = fs::remove_file(&temp);
        ClientsError::Io {
            file: path.to_path_buf(),
            source: error,
        }
    })
}

/// Удаляет папку целиком; `false`, если её не было.
pub fn remove_dir(path: &Path) -> Result<bool> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(io_err(path)(error)),
    }
}
