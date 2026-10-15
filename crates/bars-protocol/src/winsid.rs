//! SID пользователя процесса и проверка владельца сервера канала (Windows, OQ-38).

use std::io;
use std::ptr::null_mut;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, LocalFree};
use windows_sys::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows_sys::Win32::Security::{GetTokenInformation, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// SID текущего пользователя в виде строки `S-1-5-21-…`.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: вызовы Win32 по документации; токен закрывается, строка SID освобождается `LocalFree`.
    unsafe {
        let mut token: HANDLE = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = token_user_sid(token);
        CloseHandle(token);
        result
    }
}

unsafe fn token_user_sid(token: HANDLE) -> io::Result<String> {
    // SAFETY: см. `current_user_sid`; буфер выровнен по 8 байт, как требует `TOKEN_USER`.
    unsafe {
        let mut len = 0u32;
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut len);
        if len == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut buf = vec![0u64; (len as usize).div_ceil(8)];
        if GetTokenInformation(token, TokenUser, buf.as_mut_ptr().cast(), len, &mut len) == 0 {
            return Err(io::Error::last_os_error());
        }
        let user = &*(buf.as_ptr() as *const TOKEN_USER);
        sid_to_string(user.User.Sid)
    }
}

/// Строка `S-1-…` для SID.
///
/// # Safety
/// `sid` — действующий указатель на SID.
pub unsafe fn sid_to_string(sid: PSID) -> io::Result<String> {
    // SAFETY: строку выделяет `ConvertSidToStringSidW`, освобождаем `LocalFree`.
    unsafe {
        let mut text: *mut u16 = null_mut();
        if ConvertSidToStringSidW(sid, &mut text) == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut end = 0;
        while *text.add(end) != 0 {
            end += 1;
        }
        let sid = String::from_utf16_lossy(std::slice::from_raw_parts(text, end));
        LocalFree(text.cast());
        Ok(sid)
    }
}

/// SID владельца процесса с номером `pid`. Нет доступа к процессу — ошибка.
pub fn process_user_sid(pid: u32) -> io::Result<String> {
    // SAFETY: вызовы Win32 по документации; описатели закрываются.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let mut token: HANDLE = null_mut();
        let opened = OpenProcessToken(process, TOKEN_QUERY, &mut token);
        let result = if opened == 0 {
            Err(io::Error::last_os_error())
        } else {
            let sid = token_user_sid(token);
            CloseHandle(token);
            sid
        };
        CloseHandle(process);
        result
    }
}

/// Принадлежит ли процесс `pid` текущему пользователю. Не удалось узнать — ошибка: клиент не
/// доверяет серверу, чьего владельца не видит.
pub fn owned_by_current_user(pid: u32) -> io::Result<bool> {
    Ok(process_user_sid(pid)? == current_user_sid()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_belongs_to_the_current_user() {
        let own = process_user_sid(std::process::id()).unwrap();
        assert_eq!(own, current_user_sid().unwrap());
    }

    #[test]
    fn system_process_is_not_ours_or_not_readable() {
        // Процесс 4 (System) принадлежит не пользователю: либо SID другой, либо доступа нет.
        if let Ok(sid) = process_user_sid(4) {
            assert_ne!(sid, current_user_sid().unwrap());
        }
    }
}
