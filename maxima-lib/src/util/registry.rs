#[cfg(windows)]
extern crate winapi;

use anyhow::{bail, Result};
use widestring::U16CString;

use std::{path::PathBuf, ptr};

#[cfg(windows)]
use winapi::{
    shared::{minwindef::HKEY, winerror::ERROR_CANCELLED},
    um::{
        errhandlingapi::GetLastError,
        shellapi::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SEE_MASK_NO_CONSOLE},
    },
    um::{
        winnt::KEY_QUERY_VALUE,
        winreg::{RegCloseKey, RegOpenKeyExW, RegQueryValueExW},
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
    let valid = path == bootstrap_path().to_str().unwrap();
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

fn read_reg_key(path: &str) -> Option<String> {
    if let (Some(hkey_segment), Some(value_segment)) = (path.find('\\'), path.rfind('\\')) {
        let sub_key = &path[(hkey_segment + 1)..value_segment];
        let value_name = &path[(value_segment + 1)..];

        let hkey = HKEY_LOCAL_MACHINE as HKEY;
        let mut handle = ptr::null_mut();

        unsafe {
            if RegOpenKeyExW(
                hkey,
                U16CString::from_str(sub_key).unwrap().as_ptr(),
                0,
                KEY_QUERY_VALUE,
                &mut handle,
            ) != 0
            {
                return None;
            }

            let dw_type = ptr::null_mut();
            let mut dw_size = 0;

            if RegQueryValueExW(
                handle,
                U16CString::from_str(value_name).unwrap().as_ptr(),
                ptr::null_mut(),
                dw_type,
                ptr::null_mut(),
                &mut dw_size,
            ) != 0
            {
                RegCloseKey(handle);
                return None;
            }

            if dw_size <= 0 {
                RegCloseKey(handle);
                return None;
            }

            let mut buf: Vec<u16> = vec![0; (dw_size as usize / 2) - 1];
            if RegQueryValueExW(
                handle,
                U16CString::from_str(value_name).unwrap().as_ptr(),
                ptr::null_mut(),
                dw_type,
                buf.as_mut_ptr() as *mut u8,
                &mut dw_size,
            ) != 0
            {
                RegCloseKey(handle);
                return None;
            }

            RegCloseKey(handle);
            return Some(String::from_utf16_lossy(&buf));
        }
    }

    None
}

pub fn parse_registry_path(key: &str) -> PathBuf {
    let mut parts = key
        .split(|c| c == '[' || c == ']')
        .filter(|s| !s.is_empty());

    if let (Some(first), Some(second)) = (parts.next(), parts.next()) {
        let path = read_reg_key(first);
        if path.is_none() {
            return PathBuf::from(key.to_owned());
        }

        let path = path.unwrap().replace("\\", "/");
        let second = second.replace("\\", "/");
        let second = second.strip_prefix("/").unwrap_or(&second);

        return [path, second.to_owned()].iter().collect();
    }

    PathBuf::from(key.to_owned())
}

#[cfg(windows)]
pub fn read_game_path(name: &str) -> Result<PathBuf> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);

    dbg!(name);

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
pub fn bootstrap_path() -> PathBuf {
    module_path().parent().unwrap().join("maxima-bootstrap.exe")
}

#[cfg(windows)]
pub fn launch_bootstrap() -> Result<()> {
    let path = bootstrap_path();

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

    let bootstrap_path = &bootstrap_path().to_str().unwrap().to_string();
    origin.set_value("ClientPath", bootstrap_path)?;

    let (eax_32, _) = hklm.create_subkey_with_flags(REG_EAX32_PATH, KEY_WRITE)?;
    eax_32.set_value("InstallSuccessful", &"true")?;

    // Hijack Qt's protocol for our login redirection
    register_custom_protocol("qrc", "Maxima Protocol", bootstrap_path)?;

    // We link2maxima now
    register_custom_protocol("link2ea", "Maxima Launcher", bootstrap_path)?;

    // maxima2
    register_custom_protocol("origin2", "Maxima Launcher", bootstrap_path)?;

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

#[cfg(target_os = "linux")]
pub fn set_up_registry() -> Result<()> {
    let bootstrap_path = &bootstrap_path().to_str().unwrap().to_string();

    // Hijack Qt's protocol for our login redirection
    register_custom_protocol("qrc", "Maxima Launcher", bootstrap_path)?;

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn set_up_registry() -> Result<()> {
    use std::process::Command;

    use log::warn;

    let bin = bootstrap_path();

    if !bin.try_exists()? {
        warn!(
            "{} does not exist. Did you run `cargo bundle` for `maxima-bootstrap`?",
            bin.display()
        );
    }

    Command::new(bin).arg("--noop").spawn()?;

    Ok(())
}

#[cfg(target_os = "linux")]
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

    set_mime_type(
        &format!("x-scheme-handler/{}", protocol),
        &desktop_file_name,
    )?;

    Ok(())
}

#[cfg(target_os = "linux")]
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
    if std::env::var("MAXIMA_DISABLE_QRC").is_ok() {
        return Ok(());
    }

    if !verify_protocol_handler("qrc")? {
        bail!("Protocol is not registered");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
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

#[cfg(target_os = "macos")]
fn verify_protocol_handler(protocol: &str) -> Result<bool> {
    use std::process::Command;

    let output = Command::new("open")
        .args([&format!("{}://", protocol), "--args", "--noop"])
        .output()
        .expect("Failed to call open");

    Ok(output.status.success())
}

#[cfg(unix)]
pub fn read_game_path(_name: &str) -> Result<PathBuf> {
    todo!("Cannot read game path on unix");
}

#[cfg(target_os = "linux")]
pub fn bootstrap_path() -> PathBuf {
    module_path().parent().unwrap().join("maxima-bootstrap")
}

#[cfg(target_os = "macos")]
pub fn bootstrap_path() -> PathBuf {
    module_path()
        .parent()
        .unwrap()
        .join("bundle")
        .join("osx")
        .join("MaximaBootstrap.app")
        .join("Contents")
        .join("MacOS")
        .join("maxima-bootstrap")
}

#[cfg(unix)]
pub fn launch_bootstrap() -> Result<()> {
    todo!()
}
