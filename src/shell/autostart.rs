//! "Start with Windows" via the current user's own Run key (no admin rights needed).

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteValueW,
    RegGetValueW, RegOpenKeyExW, RegSetValueExW,
};
use windows::core::{HSTRING, PCWSTR, Result, w};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");

/// Value name under the Run key; differs in installer test mode (see `install::name`).
fn value() -> HSTRING {
    HSTRING::from(crate::shell::install::name())
}

pub fn is_enabled() -> bool {
    unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, &value(), RRF_RT_REG_SZ, None, None, None).is_ok() }
}

pub fn set_enabled(enable: bool) -> Result<()> {
    if enable {
        return set_command(&std::env::current_exe().unwrap_or_default());
    }
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key).ok()?;
        let status = match RegDeleteValueW(key, &value()) {
            ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
            other => other,
        };
        let _ = RegCloseKey(key);
        status.ok()
    }
}

/// Run `exe` at Windows startup (also used by the installer, for Crest in the install folder).
pub fn set_command(exe: &std::path::Path) -> Result<()> {
    unsafe {
        let mut key = HKEY::default();
        RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key).ok()?;
        let command: Vec<u16> = format!("\"{}\"", exe.display()).encode_utf16().chain([0]).collect();
        let bytes = std::slice::from_raw_parts(command.as_ptr().cast::<u8>(), command.len() * 2);
        let status = RegSetValueExW(key, &value(), None, REG_SZ, Some(bytes));
        let _ = RegCloseKey(key);
        status.ok()
    }
}
