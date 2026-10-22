//! Install, update, and uninstall Crest without administrator rights. Crest is copied to the
//! user's own programs folder (`%LOCALAPPDATA%\Programs\Crest`), gets a Start menu shortcut,
//! is registered in Settings > Apps (and can be uninstalled from there), and starts with Windows.
//!
//! crest.exe run from anywhere else (e.g. the Downloads folder) offers to install itself or
//! update the installed version. Nothing is downloaded.

use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, ERROR_CANCELLED, HANDLE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, IPersistFile,
};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_DWORD, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegGetValueW, RegSetValueExW,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
};
use windows::Win32::UI::Shell::{
    FOLDERID_Programs, FOLDERID_UserProgramFiles, IShellLinkW, KNOWN_FOLDER_FLAG, SEE_MASK_NOASYNC,
    SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, SHGetKnownFolderPath, ShellExecuteExW, ShellLink,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowThreadProcessId, IDYES, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION,
    MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO, MB_YESNOCANCEL, MESSAGEBOX_RESULT, MESSAGEBOX_STYLE, MessageBoxW,
    PostMessageW, RegisterWindowMessageW, SW_HIDE,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, PWSTR, Result, w};

use crate::{shell::autostart, shell::identity};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const NAME: &str = "Crest";
/// Message asking the running Crest to exit cleanly (so its tray icon goes away too).
pub const QUIT_MESSAGE: PCWSTR = w!("Crest.Exit");
/// Crest's window class (see `app`), used to find a running copy.
const WINDOW_CLASS: PCWSTR = w!("CrestWindow");
const TITLE: PCWSTR = w!("Crest");

/// Debug builds can test installing into another folder (`CREST_INSTALL_ROOT`). In that test
/// mode the registry and Start menu names differ too, and only Crest copies from the test folder
/// are stopped, so the real installation is never touched.
fn test_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) { std::env::var_os("CREST_INSTALL_ROOT").map(PathBuf::from) } else { None }
}

/// Crest's name in the registry (Run at startup, Installed apps) and in the Start menu.
pub fn name() -> &'static str {
    if test_root().is_some() { "CrestTest" } else { NAME }
}

fn uninstall_key() -> HSTRING {
    HSTRING::from(format!("Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\{}", name()))
}

fn shortcut_path() -> Option<PathBuf> {
    known_folder(&FOLDERID_Programs).map(|programs| programs.join(format!("{}.lnk", name())))
}

/// Install folder: `%LOCALAPPDATA%\Programs\Crest`.
pub fn dir() -> Option<PathBuf> {
    match test_root() {
        Some(root) => Some(root.join(NAME)),
        None => known_folder(&FOLDERID_UserProgramFiles).map(|programs| programs.join(NAME)),
    }
}

pub fn exe() -> Option<PathBuf> {
    dir().map(|dir| dir.join("crest.exe"))
}

/// This copy of Crest is running from the install folder.
pub fn is_installed_copy() -> bool {
    matches!((std::env::current_exe(), exe()), (Ok(current), Some(installed)) if same_path(&current, &installed))
}

/// Runs first thing in `main`. `true` means this process is done: Crest was installed and
/// started from the install folder, or the user cancelled.
pub fn offer() -> bool {
    let (Ok(current), Some(installed)) = (std::env::current_exe(), exe()) else { return false };
    if same_path(&current, &installed) {
        refresh_entry(&installed);
        return false;
    }
    // Builds from the project folder (`cargo build`) run as-is, without an install offer.
    if current.components().any(|part| part.as_os_str().eq_ignore_ascii_case("target")) {
        return false;
    }
    let old = installed_version().filter(|_| installed.exists());
    let text = match &old {
        // The same version is already installed: just start that one.
        Some(version) if version == VERSION => {
            launch(&installed, None);
            return true;
        }
        Some(version) => format!(
            "Update Crest {version} to version {VERSION}?\n\n\
             Your settings and data are kept.\n\n\
             Yes: update.   No: run this version once without installing."
        ),
        None => format!(
            "Install Crest {VERSION}?\n\n\
             • Copied to your own programs folder, no administrator rights needed.\n\
             • Appears in the Start menu and starts with Windows.\n\
             • Can be uninstalled anytime from Settings > Apps.\n\n\
             Yes: install.   No: run once without installing."
        ),
    };
    match ask(&text, MB_YESNOCANCEL | MB_ICONQUESTION) {
        IDYES => {
            match install(&current, &installed, old.is_none()) {
                Ok(()) => launch(&installed, None),
                Err(e) => {
                    error_log!("install failed: {e}");
                    ask(&format!("Crest couldn't be installed:\n{e}"), MB_ICONERROR);
                }
            }
            true
        }
        windows::Win32::UI::WindowsAndMessaging::IDNO => false,
        _ => true,
    }
}

/// Installs without asking (`crest.exe --install`), then starts the installed Crest.
pub fn install_quietly() -> bool {
    let (Ok(current), Some(installed)) = (std::env::current_exe(), exe()) else { return false };
    if same_path(&current, &installed) {
        return true;
    }
    let fresh = installed_version().is_none() || !installed.exists();
    match install(&current, &installed, fresh) {
        Ok(()) => {
            launch(&installed, None);
            true
        }
        Err(e) => {
            error_log!("install failed: {e}");
            false
        }
    }
}

fn install(source: &Path, target: &Path, fresh: bool) -> std::io::Result<()> {
    let dir = target.parent().ok_or(std::io::ErrorKind::NotFound)?;
    stop_running();
    std::fs::create_dir_all(dir)?;
    // Only the contents are copied (not via CopyFile), so the "downloaded from the internet" mark
    // is not carried over: the SmartScreen warning shows once, when the download is first opened.
    let bytes = std::fs::read(source)?;
    let temp = dir.join("crest.exe.new");
    std::fs::write(&temp, bytes)?;
    retry(|| std::fs::rename(&temp, target))?;
    identity::write_assets(dir)?;
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if let Err(e) = shortcut(target) {
        error_log!("failed to create the Start menu shortcut: {e}");
    }
    write_entry(target).map_err(std::io::Error::other)?;
    // A fresh install starts with Windows right away; an update keeps the previous choice.
    if fresh || autostart::is_enabled() {
        autostart::set_command(target).map_err(std::io::Error::other)?;
    }
    Ok(())
}

/// Runs from Settings > Apps (`crest.exe --uninstall`). Without asking (`--uninstall --quiet`):
/// your data is kept and the certificate is not removed (that needs administrator rights).
pub fn uninstall(quiet: bool) {
    let question = "Uninstall Crest from this computer?\n\n\
                    Crest, its Start menu shortcut, and \"Start with Windows\" will be removed.";
    if !quiet && ask(question, MB_YESNO | MB_ICONQUESTION) != IDYES {
        return;
    }
    let _ = unsafe { CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED) };
    stop_running();
    if let Some(dir) = dir()
        && let Err(e) = identity::remove_package(&dir)
    {
        error_log!("failed to remove the identity package: {e}");
    }
    // Crest's certificate is removed from the trusted list (needs administrator rights) when no
    // Crest package uses it anymore. Declining is harmless: its key was deleted long ago.
    if !quiet && identity::certificate_trusted() && !identity::any_package() {
        let _ = run_elevated("--remove-certificates");
    }
    let _ = autostart::set_enabled(false);
    unsafe {
        let _ = RegDeleteTreeW(HKEY_CURRENT_USER, &uninstall_key());
    }
    if let Some(link) = shortcut_path() {
        let _ = std::fs::remove_file(link);
    }
    let data = "Delete your data too?\n\n\
                This includes settings, tasks, agenda, water log, and the shelf list. The original \
                files on the shelf are not deleted.";
    if let Some(folder) = crate::shell::store::dir()
        && !quiet
        && ask(data, MB_YESNO | MB_ICONQUESTION) == IDYES
    {
        let _ = std::fs::remove_dir_all(folder);
    }
    if let Some(dir) = dir() {
        remove_later(&dir);
    }
    if !quiet {
        ask("Crest has been uninstalled. Thanks for trying it!", MB_ICONINFORMATION);
    }
}

/// Deletes the install folder shortly after this process exits (a running crest.exe can't
/// delete itself).
fn remove_later(dir: &Path) {
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let command = format!("/c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
    let spawned = std::process::Command::new("cmd.exe").raw_arg(command).creation_flags(CREATE_NO_WINDOW).spawn();
    if let Err(e) = spawned {
        error_log!("failed to delete the install folder: {e}");
    }
}

/// Asks the running Crest to exit. Older versions that don't know the message are terminated.
pub fn stop_running() {
    unsafe {
        let Ok(hwnd) = FindWindowW(WINDOW_CLASS, PCWSTR::null()) else { return };
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return;
        }
        let access = PROCESS_SYNCHRONIZE | PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION;
        let Ok(process) = OpenProcess(access, false, pid) else { return };
        // Test mode: Crest outside the test folder (e.g. the copy in real use) is left alone.
        if let Some(root) = test_root()
            && !process_path(process).is_some_and(|path| path.starts_with(&root))
        {
            let _ = CloseHandle(process);
            return;
        }
        let _ = PostMessageW(Some(hwnd), RegisterWindowMessageW(QUIT_MESSAGE), WPARAM(0), LPARAM(0));
        if WaitForSingleObject(process, 3_000) == WAIT_TIMEOUT {
            let _ = TerminateProcess(process, 1);
            WaitForSingleObject(process, 3_000);
        }
        let _ = CloseHandle(process);
    }
}

fn process_path(process: HANDLE) -> Option<PathBuf> {
    let mut buffer = [0u16; 1024];
    let mut len = buffer.len() as u32;
    unsafe { QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buffer.as_mut_ptr()), &mut len).ok()? };
    Some(PathBuf::from(String::from_utf16_lossy(&buffer[..len as usize])))
}

/// Starts Crest (or another copy) with argument `arg`.
pub fn launch(exe: &Path, arg: Option<&str>) {
    let mut command = std::process::Command::new(exe);
    command.args(arg);
    if let Err(e) = command.spawn() {
        error_log!("failed to start Crest: {e}");
    }
}

/// Runs this crest.exe as administrator with argument `arg` and waits for it to finish.
/// `Ok(true)` = succeeded, `Ok(false)` = failed, `Err` = the user declined the UAC prompt.
pub fn run_elevated(arg: &str) -> Result<bool> {
    let exe = HSTRING::from(std::env::current_exe().unwrap_or_default().as_os_str());
    let arg = HSTRING::from(arg);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(exe.as_ptr()),
        lpParameters: PCWSTR(arg.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    unsafe {
        ShellExecuteExW(&mut info)?;
        if info.hProcess.is_invalid() {
            return Ok(false);
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        Ok(code == 0)
    }
}

/// The user declined the UAC prompt.
pub fn cancelled(e: &windows::core::Error) -> bool {
    e.code() == ERROR_CANCELLED.to_hresult()
}

/// The running installed Crest refreshes its Settings > Apps entry (e.g. after an update) and
/// creates the Start menu shortcut if it is missing.
fn refresh_entry(installed: &Path) {
    if installed_version().as_deref() != Some(VERSION)
        && let Err(e) = write_entry(installed)
    {
        error_log!("failed to update the Installed apps entry: {e}");
    }
    // The shortcut is created via COM; on the main thread COM is only initialized later by file
    // drag and drop (same apartment, so it is safe to initialize it here first).
    if shortcut_path().is_some_and(|link| !link.exists()) {
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if let Err(e) = shortcut(installed) {
            error_log!("failed to create the Start menu shortcut: {e}");
        }
    }
}

/// Entry in Settings > Apps (and Control Panel > Programs), including the uninstall command.
fn write_entry(exe: &Path) -> Result<()> {
    let dir = exe.parent().unwrap_or(exe);
    let size_kb = std::fs::metadata(exe).map_or(0, |m| m.len() / 1024) as u32;
    unsafe {
        let mut key = HKEY::default();
        RegCreateKeyExW(HKEY_CURRENT_USER, &uninstall_key(), None, None, REG_OPTION_NON_VOLATILE, KEY_WRITE, None, &mut key, None)
            .ok()?;
        let text = |name: PCWSTR, value: &str| {
            let wide: Vec<u16> = value.encode_utf16().chain([0]).collect();
            RegSetValueExW(key, name, None, REG_SZ, Some(std::slice::from_raw_parts(wide.as_ptr().cast(), wide.len() * 2)))
        };
        let number = |name: PCWSTR, value: u32| RegSetValueExW(key, name, None, REG_DWORD, Some(&value.to_le_bytes()));
        let results = [
            text(w!("DisplayName"), NAME),
            text(w!("DisplayVersion"), VERSION),
            text(w!("Publisher"), "Rinci Labs"),
            text(w!("DisplayIcon"), &format!("{},0", exe.display())),
            text(w!("InstallLocation"), &dir.display().to_string()),
            text(w!("UninstallString"), &format!("\"{}\" --uninstall", exe.display())),
            text(w!("QuietUninstallString"), &format!("\"{}\" --uninstall --quiet", exe.display())),
            number(w!("NoModify"), 1),
            number(w!("NoRepair"), 1),
            number(w!("EstimatedSize"), size_kb),
        ];
        let _ = RegCloseKey(key);
        results.into_iter().try_for_each(|status| status.ok())
    }
}

fn installed_version() -> Option<String> {
    let mut buffer = [0u16; 64];
    let mut size = (buffer.len() * 2) as u32;
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, &uninstall_key(), w!("DisplayVersion"), RRF_RT_REG_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size))
            .ok()
            .ok()?;
    }
    let len = (size as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buffer[..len]))
}

/// The "Crest" shortcut in the Start menu.
fn shortcut(exe: &Path) -> Result<()> {
    let link_path = shortcut_path().ok_or_else(windows::core::Error::empty)?;
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
        link.SetPath(&HSTRING::from(exe.as_os_str()))?;
        if let Some(dir) = exe.parent() {
            link.SetWorkingDirectory(&HSTRING::from(dir.as_os_str()))?;
        }
        link.SetDescription(w!("A top-of-screen panel for Windows"))?;
        link.SetIconLocation(&HSTRING::from(exe.as_os_str()), 0)?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(link_path.as_os_str()), true)
    }
}

fn known_folder(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let path = SHGetKnownFolderPath(id, KNOWN_FOLDER_FLAG(0), None).ok()?;
        let result = path.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(path.0 as *const _));
        result
    }
}

fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
}

/// A file just released by another process can stay locked for a moment.
fn retry(mut action: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = Ok(());
    for _ in 0..15 {
        last = action();
        if last.is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    last
}

pub fn ask(text: &str, style: MESSAGEBOX_STYLE) -> MESSAGEBOX_RESULT {
    unsafe { MessageBoxW(Some(HWND::default()), &HSTRING::from(text), TITLE, style | MB_TOPMOST | MB_SETFOREGROUND) }
}
