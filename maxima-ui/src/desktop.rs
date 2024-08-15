use log::{info, error};
use std::collections::HashMap;
use maxima::util::native::maxima_dir;
use std::process::Command;

use anyhow::{bail, Result};

#[cfg(target_os = "linux")]
pub fn check_desktop_icon() -> Result<()> {
    let maxima_dir = maxima_dir()?;
    let desktop_file_path = maxima_dir.parent().unwrap().join("applications").join("maxima.desktop");

    if let Err(err) = std::fs::metadata(&desktop_file_path) {
        info!("creating application shortcut");

        let xdg_desktop_check = Command::new("xdg-desktop-menu").arg("--version").output();
        if xdg_desktop_check.is_err() {
            bail!("xdg-desktop-menu command is not available. Please install xdg-utils.");
        }
        let xdg_icon_check = Command::new("xdg-icon-resource").arg("--version").output();
        if xdg_icon_check.is_err() {
            bail!("xdg-icon-resource command is not available. Please install xdg-utils.");
        }

        let png_path_temp = &maxima_dir.join("32.png");
        std::fs::write(png_path_temp, include_bytes!("../../maxima-resources/assets/logo.png"))?;
        let xdg_register_icon_check = Command::new("xdg-icon-resource")
        .arg("install")
        .arg("--size")
        .arg("32")
        .arg("--novendor")
        .arg(png_path_temp)
        .arg("maxima")
        .output();
        if xdg_register_icon_check.is_err() {
            error!("Failed to register icon, continuing");
        } else {
            info!("Registered icon.");
        }
        std::fs::remove_file(png_path_temp)?;

        let binary = std::fs::read_link("/proc/self/exe").expect("Couldn't get module path").parent().unwrap().join("maxima");

        let mut parts = HashMap::<&str, String>::new();
        parts.insert("Type", "Application".to_owned());
        parts.insert("Name", "Maxima Launcher".to_owned());
        parts.insert("Exec", format!("{} %u", binary.as_path().to_string_lossy()));
        parts.insert("Icon", "maxima".to_owned());
        parts.insert("NoDisplay", "true".to_owned());
        parts.insert("StartupNotify", "true".to_owned());

        let mut desktop_file = String::from("[Desktop Entry]\n");
        for part in parts {
            desktop_file += &(part.0.to_owned() + "=" + &part.1 + "\n");
        }

        std::fs::write(&desktop_file_path, desktop_file)?;
        let xdg_register_check = Command::new("xdg-desktop-menu").arg("install").arg("--novendor").arg(desktop_file_path).output();
        if xdg_register_check.is_err() {
            bail!("failed to install .desktop file.");
        }

    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn check_desktop_icon() -> Result<()> {
    Ok(())
}