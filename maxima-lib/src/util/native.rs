use std::ffi::CString;

use anyhow::{bail, Result};
#[cfg(target_family = "windows")]
use winapi::{
    shared::windef::HWND,
    um::{
        wincon::GetConsoleWindow,
        winuser::{FindWindowA, SetForegroundWindow, EnumWindows, GetWindowThreadProcessId, IsWindowVisible},
    },
};

#[cfg(target_family = "windows")]
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
#[cfg(target_family = "windows")]
pub fn get_hwnd() -> Result<HWND> {
    unsafe {
        EnumWindows(Some(enum_windows_proc), 0);

        let window_name = CString::new("Maxima").expect("Failed to create native string");
        let mut hwnd = FindWindowA(std::ptr::null(), window_name.as_ptr());
        if !hwnd.is_null() {
            println!("Is not null");
            Ok(hwnd)
        } else {
            hwnd = GetConsoleWindow();
            if !hwnd.is_null() {
                //bail!("Failed to find native window");
                Ok(hwnd)
            } else {
                bail!("Failed to find native window");
            }
        }
    }
}

pub fn take_foreground_focus() -> Result<()> {
    unsafe {
        #[cfg(target_family = "unix")]
        //TODO: Figure it out
        #[cfg(target_family = "windows")]
        EnumWindows(Some(enum_windows_proc), 0);
    }

    Ok(())
}
