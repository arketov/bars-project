//! Автозапуск Windows: значение `bars` в `HKCU\...\Run` и флаг «Диспетчера задач»
//! в `HKCU\...\Explorer\StartupApproved\Run`.

use std::io;
use std::ptr;

use widestring::{U16CStr, U16CString};
use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_SZ, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

use super::{Autostart, AutostartState};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const VALUE_NAME: &str = "bars";

/// Автозапуск через реестр пользователя; прав администратора не нужно.
#[derive(Debug)]
pub struct RunKey {
    command: String,
}

impl RunKey {
    pub fn new(command: String) -> Self {
        Self { command }
    }
}

struct Key(HKEY);

impl Key {
    /// Открывает ключ; `None`, если его нет.
    fn open(path: &str, access: u32) -> io::Result<Option<Self>> {
        let path = wide(path)?;
        let mut key: HKEY = ptr::null_mut();
        // SAFETY: `path` — нуль-терминированная строка, `key` — допустимый указатель на HKEY.
        let status =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, access, &raw mut key) };
        match status {
            ERROR_SUCCESS => Ok(Some(Self(key))),
            ERROR_FILE_NOT_FOUND => Ok(None),
            other => Err(os_error(other)),
        }
    }

    /// Значение и его тип; `None`, если значения нет.
    fn query(&self, name: &U16CStr) -> io::Result<Option<(u32, Vec<u8>)>> {
        let mut kind = 0u32;
        let mut len = 0u32;
        // SAFETY: первый вызов только спрашивает размер.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &raw mut kind,
                ptr::null_mut(),
                &raw mut len,
            )
        };
        match status {
            ERROR_SUCCESS => {}
            ERROR_FILE_NOT_FOUND => return Ok(None),
            other => return Err(os_error(other)),
        }
        let mut data = vec![0u8; len as usize];
        // SAFETY: буфер на `len` байт, `len` передаётся как его размер.
        let status = unsafe {
            RegQueryValueExW(
                self.0,
                name.as_ptr(),
                ptr::null(),
                &raw mut kind,
                data.as_mut_ptr(),
                &raw mut len,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(os_error(status));
        }
        data.truncate(len as usize);
        Ok(Some((kind, data)))
    }

    fn set_string(&self, name: &U16CStr, text: &str) -> io::Result<()> {
        let text = wide(text)?;
        let bytes = u32::try_from((text.len() + 1) * 2)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "слишком длинная команда"))?;
        // SAFETY: `text` нуль-терминирована, `bytes` — её длина вместе с нулём.
        let status = unsafe {
            RegSetValueExW(
                self.0,
                name.as_ptr(),
                0,
                REG_SZ,
                text.as_ptr().cast::<u8>(),
                bytes,
            )
        };
        check(status)
    }

    fn delete(&self, name: &U16CStr) -> io::Result<()> {
        // SAFETY: `name` — нуль-терминированная строка.
        let status = unsafe { RegDeleteValueW(self.0, name.as_ptr()) };
        match status {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            other => Err(os_error(other)),
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: ключ открыт в `open` и закрывается один раз.
        unsafe { RegCloseKey(self.0) };
    }
}

fn wide(text: &str) -> io::Result<U16CString> {
    U16CString::from_str(text)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "нуль внутри строки"))
}

fn check(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(os_error(status))
    }
}

fn os_error(status: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(status).unwrap_or(i32::MAX))
}

fn decode_string(data: &[u8]) -> String {
    let units: Vec<u16> = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .take_while(|&unit| unit != 0)
        .collect();
    String::from_utf16_lossy(&units)
}

impl RunKey {
    fn stored(&self) -> io::Result<Option<String>> {
        let Some(key) = Key::open(RUN_KEY, KEY_QUERY_VALUE)? else {
            return Ok(None);
        };
        let name = wide(VALUE_NAME)?;
        Ok(key
            .query(&name)?
            .filter(|(kind, _)| *kind == REG_SZ)
            .map(|(_, data)| decode_string(&data)))
    }

    /// «Диспетчер задач» отключил запись: первый байт значения нечётный.
    fn disabled_by_user(&self) -> io::Result<bool> {
        let Some(key) = Key::open(APPROVED_KEY, KEY_QUERY_VALUE)? else {
            return Ok(false);
        };
        let name = wide(VALUE_NAME)?;
        Ok(key
            .query(&name)?
            .filter(|(kind, _)| *kind == REG_BINARY)
            .is_some_and(|(_, data)| data.first().is_some_and(|byte| byte & 1 == 1)))
    }

    fn forget_user_choice(&self) -> io::Result<()> {
        if let Some(key) = Key::open(APPROVED_KEY, KEY_SET_VALUE)? {
            key.delete(&wide(VALUE_NAME)?)?;
        }
        Ok(())
    }
}

impl Autostart for RunKey {
    fn state(&self) -> io::Result<AutostartState> {
        let command = self.stored()?;
        let present = command.is_some();
        Ok(AutostartState {
            supported: true,
            enabled: present && !self.disabled_by_user()?,
            stale: command
                .as_ref()
                .is_some_and(|stored| !stored.eq_ignore_ascii_case(&self.command)),
            command,
        })
    }

    fn set(&self, enabled: bool) -> io::Result<AutostartState> {
        let name = wide(VALUE_NAME)?;
        if enabled {
            let key = Key::open(RUN_KEY, KEY_SET_VALUE)?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "в реестре нет раздела Run")
            })?;
            key.set_string(&name, &self.command)?;
            self.forget_user_choice()?;
        } else if let Some(key) = Key::open(RUN_KEY, KEY_SET_VALUE)? {
            key.delete(&name)?;
        }
        self.state()
    }
}
