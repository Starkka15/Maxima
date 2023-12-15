use std::path::PathBuf;

use anyhow::{bail, Result};

#[cfg(windows)]
use std::{
    os::windows::prelude::{OsStrExt, OsStringExt},
    ffi::{CString, OsString},
};

#[cfg(windows)]
use winapi::{
    shared::windef::HWND,
    um::{
        libloaderapi::{GetModuleFileNameW, GetModuleHandleW},
        wincon::GetConsoleWindow,
        winuser::{
            EnumWindows, FindWindowA, GetWindowThreadProcessId, IsWindowVisible,
            SetForegroundWindow,
        },
    },
};

use std::fs;

#[cfg(windows)]
unsafe extern "system" fn enum_windows_proc(
    hwnd: HWND,
    _l_param: winapi::shared::minwindef::LPARAM,
) -> winapi::shared::minwindef::BOOL {
    let mut window_process_id: u32 = 0;

    GetWindowThreadProcessId(hwnd, &mut window_process_id);

    if window_process_id != std::process::id() || IsWindowVisible(hwnd) == 0 {
        return winapi::shared::minwindef::TRUE;
    }

    if IsWindowVisible(hwnd) != 0 {
        SetForegroundWindow(hwnd);
    }

    winapi::shared::minwindef::TRUE
}
#[cfg(windows)]
pub fn get_hwnd() -> Result<HWND> {
    unsafe {
        EnumWindows(Some(enum_windows_proc), 0);

        let window_name = CString::new("Maxima").expect("Failed to create native string");
        let mut hwnd = FindWindowA(std::ptr::null(), window_name.as_ptr());
        if !hwnd.is_null() {
            return Ok(hwnd);
        }

        hwnd = GetConsoleWindow();
        if hwnd.is_null() {
            bail!("Failed to find native window");
        }

        Ok(hwnd)
    }
}

#[cfg(windows)]
pub fn take_foreground_focus() -> Result<()> {
    unsafe {
        EnumWindows(Some(enum_windows_proc), 0);
    }

    Ok(())
}

#[cfg(unix)]
pub fn take_foreground_focus() -> Result<()> {
    // TODO
    Ok(())
}

#[cfg(windows)]
pub fn get_module_path() -> Result<PathBuf> {
    // Get a handle to the DLL
    let mut maxima_mod_name = OsString::from("maxima.dll")
        .encode_wide()
        .collect::<Vec<_>>();
    maxima_mod_name.push(0);

    let mut hmodule = unsafe { GetModuleHandleW(maxima_mod_name.as_mut_ptr()) };
    if hmodule.is_null() {
        hmodule = unsafe { GetModuleHandleW(std::ptr::null_mut()) };
    }

    if hmodule.is_null() {
        bail!("Failed to find module");
    }

    // Create a buffer to hold the DLL path
    let mut buffer: [u16; 260] = [0; 260];

    // Get the DLL path
    let length = unsafe { GetModuleFileNameW(hmodule, buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        bail!("Failed to get module length");
    }

    // Convert buffer to a Rust String
    let os_string = OsString::from_wide(&buffer[0..length as usize]);
    Ok(os_string.to_string_lossy().into_owned().into())
}

#[cfg(unix)]
pub fn module_path() -> Result<PathBuf> {
    let path = fs::read_link("/proc/self/exe");
    if path.is_err() {
        bail!("Invalid module path!");
    }

    Ok(path.unwrap())
}

#[cfg(not(unix))]
pub fn maxima_dir() -> Result<PathBuf> {
    use directories::ProjectDirs;

    let dirs = ProjectDirs::from("com", "Maxima", "Maxima");
    Ok(dirs.unwrap().data_dir().to_path_buf())
}

#[cfg(unix)]
pub fn maxima_dir() -> Result<PathBuf> {
    use std::{env, fs::create_dir_all};

    let home = env::var("HOME")?;
    let path = PathBuf::from(format!("{}/.maxima", home));

    create_dir_all(&path)?;
    Ok(path)
}
