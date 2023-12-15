#[cfg(windows)]
extern crate winapi;

use anyhow::{bail, Result};
use std::path::PathBuf;

#[cfg(windows)]
use winapi::{
    shared::winerror::ERROR_CANCELLED,
    um::{
        errhandlingapi::GetLastError,
        shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE},
    },
};

#[cfg(windows)]
use winreg::{
    enums::{HKEY_CLASSES_ROOT, HKEY_LOCAL_MACHINE, KEY_WRITE},
    RegKey,
};

#[cfg(unix)]
use std::{collections::HashMap, fs};

use super::native::module_path;

#[cfg(target_pointer_width = "64")]
pub const REG_ARCH_PATH: &str = "SOFTWARE\\WOW6432Node";
#[cfg(target_pointer_width = "32")]
pub const REG_ARCH_PATH: &str = "SOFTWARE";

pub const REG_EAX32_PATH: &str = "SOFTWARE\\Electronic Arts\\EA Desktop";

#[cfg(windows)]
pub fn check_registry_validity() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let origin = hklm.open_subkey(format!("{}\\Origin", REG_ARCH_PATH))?;

    let path: String = origin.get_value("ClientPath")?;
    let valid = path == bootstrap_path()?.to_str().unwrap();
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

#[cfg(windows)]
pub fn read_game_path(name: &str) -> Result<PathBuf> {
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

#[cfg(windows)]
pub fn get_bootstrap_path() -> Result<PathBuf> {
    let path = module_path()?
        .parent()
        .unwrap()
        .join("maxima-bootstrap.exe");

    Ok(path)
}

#[cfg(windows)]
pub fn launch_bootstrap() -> Result<()> {
    let path = bootstrap_path()?;

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

#[cfg(windows)]
pub fn set_up_registry() -> Result<()> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (origin, _) =
        hklm.create_subkey_with_flags(format!("{}\\Origin", REG_ARCH_PATH), KEY_WRITE)?;

    let bootstrap_path = &bootstrap_path()?.to_str().unwrap().to_string();
    origin.set_value("ClientPath", bootstrap_path)?;

    let (eax_32, _) = hklm.create_subkey_with_flags(REG_EAX32_PATH, KEY_WRITE)?;
    eax_32.set_value("InstallSuccessful", &"true")?;

    // Hijack Qt's protocol for our login redirection
    register_custom_protocol(
        "qrc",
        "Maxima Protocol",
        bootstrap_path,
    )?;

    // We link2maxima now
    register_custom_protocol(
        "link2ea",
        "Maxima Launcher",
        bootstrap_path,
    )?;

    // maxima2
    register_custom_protocol(
        "origin2",
        "Maxima Launcher",
        bootstrap_path,
    )?;

    Ok(())
}

#[cfg(windows)]
fn register_custom_protocol(protocol: &str, name: &str, executable: &str) -> Result<()> {
    let hkcr = RegKey::predef(HKEY_CLASSES_ROOT);
    let (protocol, _) = hkcr.create_subkey_with_flags(protocol, KEY_WRITE)?;

    protocol.set_value("", &format!("URL:{}", name))?;
    protocol.set_value("URL Protocol", &"")?;

    let (command, _) = protocol.create_subkey_with_flags("shell\\open\\command", KEY_WRITE)?;
    command.set_value("", &format!("\"{}\" \"%1\"", executable))?;

    Ok(())
}

#[cfg(unix)]
pub fn set_up_registry() -> Result<()> {
    let bootstrap_path = &bootstrap_path()?.to_str().unwrap().to_string();

    // Hijack Qt's protocol for our login redirection
    register_custom_protocol(
        "qrc",
        "Maxima Launcher",
        bootstrap_path,
    )?;

    Ok(())
}

#[cfg(unix)]
fn register_custom_protocol(protocol: &str, name: &str, executable: &str) -> Result<()> {
    use std::env;

    let mut parts = HashMap::<&str, String>::new();
    parts.insert("Type", "Application".to_owned());
    parts.insert("Name", name.to_owned());
    parts.insert("MimeType", format!("x-scheme-handler/{}", protocol));
    parts.insert("Exec", format!("{} %u", executable));
    parts.insert("NoDisplay", "true".to_owned());
    parts.insert("StartupNotify", "true".to_owned());

    let mut desktop_file = String::from("[Desktop Entry]\n");
    for part in parts {
        desktop_file += &(part.0.to_owned() + "=" + &part.1 + "\n");
    }

    let home = env::var("HOME")?;
    let desktop_file_name = format!("maxima-{}.desktop", protocol);
    let desktop_file_path = format!("{}/.local/share/applications/{}", home, desktop_file_name);
    fs::write(desktop_file_path, desktop_file)?;

    set_mime_type(&format!("x-scheme-handler/{}", protocol), &desktop_file_name)?;
    Ok(())
}

#[cfg(unix)]
fn set_mime_type(mime_type: &str, desktop_file_path: &str) -> Result<()> {
    use std::process::Command;

    let xdg_mime_check = Command::new("xdg-mime").arg("--version").output();
    if xdg_mime_check.is_err() {
        bail!("xdg-mime command is not available. Please install xdg-utils.");
    }
    
    let output = Command::new("xdg-mime")
        .arg("default")
        .arg(desktop_file_path)
        .arg(mime_type)
        .output()?;
    
    if !output.status.success() {
        bail!(
            "Failed to set MIME type association for {}: {}",
            mime_type,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(())
}

#[cfg(unix)]
pub fn check_registry_validity() -> Result<()> {
    if !verify_protocol_handler("qrc")? {
        bail!("Protocol is not registered");
    }
    
    Ok(())
}

#[cfg(unix)]
fn verify_protocol_handler(protocol: &str) -> Result<bool> {
    use std::process::Command;

    let output = Command::new("xdg-mime")
        .arg("query")
        .arg("default")
        .arg(format!("x-scheme-handler/{}", protocol))
        .output()
        .expect("Failed to execute xdg-mime");

    if !output.status.success() {
        bail!("Failed to query mime status");
    }

    let output_str = String::from_utf8_lossy(&output.stdout);
    if output_str.is_empty() {
        return Ok(false);
    }

    let expected = format!("maxima-{}.desktop\n", protocol);
    return Ok(output_str == expected);
}

#[cfg(unix)]
pub fn read_game_path(name: &str) -> Result<PathBuf> {
    todo!();
}

#[cfg(unix)]
pub fn bootstrap_path() -> Result<PathBuf> {
    let path = module_path()?
        .parent()
        .unwrap()
        .join("maxima-bootstrap");

    Ok(path)
}

#[cfg(unix)]
pub fn launch_bootstrap() -> Result<()> {
    todo!()
}
