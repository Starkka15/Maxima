#[cfg(target_family = "windows")]
extern crate winapi;

use anyhow::{bail, Result};
use std::{env::current_exe, path::PathBuf};

#[cfg(target_family = "windows")]
use winapi::{
    shared::winerror::ERROR_CANCELLED,
    um::{
        errhandlingapi::GetLastError,
        shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE},
    },
};

#[cfg(target_family = "windows")]
use winreg::{
    enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_WRITE},
    RegKey,
};

#[cfg(target_pointer_width = "64")]
pub const REG_ARCH_PATH: &str = "SOFTWARE\\WOW6432Node";
#[cfg(target_pointer_width = "32")]
pub const REG_ARCH_PATH: &str = "SOFTWARE";

pub const REG_EAX32_PATH: &str = "SOFTWARE\\Electronic Arts\\EA Desktop";

#[cfg(target_family = "windows")]
pub fn check_registry_validity() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let origin = hklm.open_subkey(format!("{}\\Origin", REG_ARCH_PATH))?;

    let path: String = origin.get_value("ClientPath")?;
    let valid = path == get_bootstrap_path()?.to_str().unwrap();
    if !valid {
        bail!("Invalid stored client path");
    }

    let eax32 = hklm.open_subkey(REG_EAX32_PATH)?;
    let install_succesful: String = eax32.get_value("InstallSuccessful")?;
    if install_succesful != "true" {
        bail!("Install key is invalid");
    }

    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let qrc = hkcr.open_subkey("qrc");
    if qrc.is_err() {
        bail!("Invalid qrc protocol");
    }

    Ok(())
}

#[cfg(target_family = "windows")]
pub fn read_game_path(name: String) -> Result<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    let mut key = hklm.open_subkey(format!("SOFTWARE\\EA Games\\{}", name));
    if key.is_err() {
        key = hklm.open_subkey(format!("SOFTWARE\\WOW6432Node\\EA Games\\{}", name));
    }

    if key.is_err() {
        bail!("Failed to find game path!");
    }

    let path: String = key.unwrap().get_value("Install Dir")?;
    Ok(PathBuf::from(path))
}

#[cfg(target_family = "windows")]
pub fn get_bootstrap_path() -> Result<PathBuf> {
    let path = current_exe()?
        .parent()
        .unwrap()
        .join("maxima-bootstrap.exe");

    Ok(path)
}

#[cfg(target_family = "windows")]
pub fn launch_bootstrap() -> Result<()> {
    let path = get_bootstrap_path()?;

    let verb = "runas";
    let file = path.to_str().unwrap();
    let parameters = "";

    let verb = verb.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let file = file.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let parameters = parameters.encode_utf16().chain(Some(0)).collect::<Vec<_>>();

    let mut shell_execute_info = winapi::um::shellapi::SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<winapi::um::shellapi::SHELLEXECUTEINFOW>() as u32,
        lpVerb: verb.as_ptr(),
        lpFile: file.as_ptr(),
        lpParameters: parameters.as_ptr(),
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NO_CONSOLE,
        ..Default::default()
    };

    unsafe {
        ShellExecuteExW(&mut shell_execute_info);

        let err = GetLastError();
        if err == ERROR_CANCELLED {
            bail!("Failed to elevate process");
        }
    }

    Ok(())
}

#[cfg(target_family = "windows")]
pub fn set_up_registry() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (origin, _) =
        hklm.create_subkey_with_flags(format!("{}\\Origin", REG_ARCH_PATH), KEY_WRITE)?;

    let bootstrap_path = &get_bootstrap_path()?.to_str().unwrap().to_string();
    origin.set_value("ClientPath", bootstrap_path)?;

    let (eax_32, _) = hklm.create_subkey_with_flags(REG_EAX32_PATH, KEY_WRITE)?;
    eax_32.set_value("InstallSuccessful", &"true")?;

    // Hijack Qt's protocol for our login redirection
    register_custom_protocol(
        "qrc".to_string(),
        "Maxima Protocol".to_string(),
        bootstrap_path,
    )?;

    // We link2maxima now
    register_custom_protocol(
        "link2ea".to_string(),
        "Maxima Launcher".to_string(),
        bootstrap_path,
    )?;

    // maxima2
    register_custom_protocol(
        "origin2".to_string(),
        "Maxima Launcher".to_string(),
        bootstrap_path,
    )?;

    Ok(())
}

#[cfg(target_family = "windows")]
fn register_custom_protocol(protocol: String, name: String, executable: &str) -> Result<()> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let (protocol, _) = hkcr.create_subkey_with_flags(protocol, KEY_WRITE)?;

    protocol.set_value("", &format!("URL:{}", name))?;
    protocol.set_value("URL Protocol", &"")?;

    let (command, _) = protocol.create_subkey_with_flags("shell\\open\\command", KEY_WRITE)?;
    command.set_value("", &format!("\"{}\" \"%1\"", executable))?;

    Ok(())
}

#[cfg(target_family = "unix")]
pub fn set_up_registry() -> Result<()> {
    todo!()
}

#[cfg(target_family = "unix")]
fn register_custom_protocol(protocol: String, name: String, executable: &str) -> Result<()> {
    todo!()
}

#[cfg(target_family = "unix")]
pub fn check_registry_validity() -> Result<()> {
    println!("[DONOTSHIP] make sure to fix the xdg handler before release!");
    Ok(())
}

#[cfg(target_family = "unix")]
pub fn read_game_path(name: String) -> Result<PathBuf> {
    todo!();
}

#[cfg(target_family = "unix")]
pub fn get_bootstrap_path() -> Result<PathBuf> {
    todo!()
}

#[cfg(target_family = "unix")]
pub fn launch_bootstrap() -> Result<()> {
    todo!()
}
