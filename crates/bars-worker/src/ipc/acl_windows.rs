//! Дескриптор безопасности канала: доступ только у пользователя, запустившего воркер.

use std::io;

use bars_protocol::winsid::current_user_sid;
use interprocess::os::windows::security_descriptor::SecurityDescriptor;
use widestring::U16CString;
#[cfg(test)]
use windows_sys::Win32::Foundation::HANDLE;

/// SDDL защищённого DACL с одной записью: полный доступ только для `sid`. Без наследования, без
/// Everyone, Users и анонимных. Алиасы `OW` и `CO` не годятся: владельцем объекта может оказаться
/// группа администраторов.
pub fn owner_only_sddl(sid: &str) -> String {
    format!("D:P(A;;GA;;;{sid})")
}

/// Дескриптор безопасности канала: доступ только у текущего пользователя.
pub fn owner_only_descriptor() -> io::Result<SecurityDescriptor> {
    let sddl = U16CString::from_str(owner_only_sddl(&current_user_sid()?))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    SecurityDescriptor::deserialize(&sddl)
}

/// Проверяет по открытому описателю объекта, что его DACL защищён и состоит из одной записи
/// «разрешить» для текущего пользователя.
#[cfg(test)]
pub fn assert_owner_only(handle: HANDLE) {
    use bars_protocol::winsid::sid_to_string;
    use std::ffi::c_void;
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT};
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetSecurityDescriptorControl,
        PSECURITY_DESCRIPTOR, SE_DACL_PROTECTED,
    };

    // SAFETY: вызовы Win32 по документации; дескриптор из `GetSecurityInfo` освобождается `LocalFree`.
    unsafe {
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        let rc = GetSecurityInfo(
            handle,
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        );
        assert_eq!(rc, 0, "GetSecurityInfo");
        let mut control = 0u16;
        let mut revision = 0u32;
        assert_ne!(
            GetSecurityDescriptorControl(descriptor, &mut control, &mut revision),
            0
        );
        assert_ne!(
            control & SE_DACL_PROTECTED,
            0,
            "DACL не защищён от наследования"
        );
        let mut info: ACL_SIZE_INFORMATION = std::mem::zeroed();
        assert_ne!(
            GetAclInformation(
                dacl,
                (&raw mut info).cast(),
                size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            ),
            0
        );
        assert_eq!(info.AceCount, 1, "в DACL должна быть одна запись");
        let mut ace: *mut c_void = null_mut();
        assert_ne!(GetAce(dacl, 0, &mut ace), 0);
        assert_eq!(
            (*(ace as *const ACE_HEADER)).AceType,
            0u8,
            "ACCESS_ALLOWED_ACE_TYPE"
        );
        let sid = (&raw mut (*(ace as *mut ACCESS_ALLOWED_ACE)).SidStart).cast::<c_void>();
        assert_eq!(sid_to_string(sid).unwrap(), current_user_sid().unwrap());
        LocalFree(descriptor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_sid_looks_like_a_sid() {
        let sid = current_user_sid().unwrap();
        assert!(sid.starts_with("S-1-"), "{sid}");
        assert!(sid.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    }

    #[test]
    fn sddl_has_exactly_one_protected_entry() {
        assert_eq!(owner_only_sddl("S-1-5-21-1"), "D:P(A;;GA;;;S-1-5-21-1)");
        owner_only_descriptor().unwrap();
    }
}
