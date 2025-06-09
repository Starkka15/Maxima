use log::{error, info};
use maxima::util::native::{maxima_dir, NativeError, SafeParent};
use std::collections::HashMap;
use std::process::{Command, Output};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DesktopError {
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("failed to install .desktop file")]
    Installation(std::io::Result<Output>),
    #[error("xdg-desktop-menu produced an error ({status:?}): {output}")]
    XdgDesktopMenu { status: Option<i32>, output: String },
    #[error("xdg-desktop-menu command failed to run: {0}")]
    XdgDesktopMenuIo(std::io::Error),
}

#[cfg(target_os = "linux")]
pub fn check_desktop_icon() -> Result<(), DesktopError> {
    let maxima_dir = maxima_dir()?;
    let desktop_file_path = maxima_dir
        .safe_parent()?
        .join("applications")
        .join("io.github.ArmchairDevelopers.Maxima.desktop");
    if desktop_file_path.exists() {
        return Ok(());
    }

    info!("creating application shortcut");

    if let Err(e) = Command::new("xdg-desktop-menu").arg("--version").output() {
        return Err(DesktopError::XdgDesktopMenuIo(e));
    }

    let icon = if let Ok(x) = Command::new("xdg-icon-resource").arg("--version").output() {
        x.status.success()
    } else {
        false
    };

    if icon {
        let png_path_temp = &maxima_dir.join("32.png");
        std::fs::write(
            png_path_temp,
            include_bytes!("../../maxima-resources/assets/logo.png"),
        )?;
        if let Err(err) = Command::new("xdg-icon-resource")
            .arg("install")
            .arg("--size")
            .arg("32")
            .arg("--novendor")
            .arg(png_path_temp)
            .arg("maxima")
            .output()
        {
            error!("Failed to register icon ({err:?})");
        } else {
            info!("Registered icon.");
        }
        std::fs::remove_file(png_path_temp)?;
    }

    let binary = std::fs::read_link("/proc/self/exe")?.safe_parent()?.join("maxima");

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
    let xdg_register_check = Command::new("xdg-desktop-menu")
        .arg("install")
        .arg("--novendor")
        .arg(desktop_file_path)
        .output();
    match xdg_register_check {
        Ok(output) => {
            if !output.status.success() {
                Err(DesktopError::XdgDesktopMenu {
                    status: output.status.code(),
                    output: String::from_utf8(output.stderr).unwrap_or(
                        String::from_utf8(output.stdout)
                            .unwrap_or("Unable to format string".to_string()),
                    ),
                })
            } else {
                Ok(())
            }
        }
        Err(e) => Err(DesktopError::XdgDesktopMenuIo(e)),
    }
}

#[cfg(target_os = "windows")]
pub fn check_desktop_icon() -> Result<(), DesktopError> {
    Ok(())
}
