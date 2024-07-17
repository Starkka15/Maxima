#![allow(non_snake_case)]

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use derive_getters::Getters;
use serde::Deserialize;

pub const DIP_RELATIVE_PATH: &str = "__Installer/installerdata.xml";

macro_rules! dip_type {
    (
        $(#[$message_attr:meta])*
        $message_name:ident;
        attr {
            $(
                $(#[$attr_field_attr:meta])*
                $attr_field:ident: $attr_field_type:ty
            ),* $(,)?
        },
        data {
            $(
                $(#[$field_attr:meta])*
                $field:ident: $field_type:ty
            ),* $(,)?
        }
    ) => {
        paste::paste! {
            // Main struct definition
            $(#[$message_attr])*
            #[derive(Default, Debug, Clone, Deserialize, PartialEq, Getters)]
            #[serde(rename_all = "camelCase")]
            pub struct [<DiP $message_name>] {
                $(
                    $(#[$attr_field_attr])*
                    #[serde(rename = "@" $attr_field)]
                    pub [<attr_ $attr_field>]: $attr_field_type,
                )*
                $(
                    $(#[$field_attr])*
                    pub $field: $field_type,
                )*
            }
        }
    }
}

dip_type!(
    Launcher;
    attr {
        uid: String,
    },
    data {
        file_path: String,
        execute_elevated: Option<bool>,
        trial: bool,
    }
);

dip_type!(
    Runtime;
    attr {},
    data {
        launcher: Vec<DiPLauncher>,
    }
);

dip_type!(
    Touchup;
    attr {},
    data {
        file_path: String,
        parameters: String,
    }
);

fn remove_leading_slash(path: &str) -> &str {
    path.strip_prefix('/').unwrap_or(path)
}

fn remove_trailing_slash(path: &str) -> &str {
    path.strip_suffix('/').unwrap_or(path)
}

fn remove_trailing_backslash(path: &str) -> &str {
    path.strip_suffix('\\').unwrap_or(path)
}

impl DiPTouchup {
    pub fn path(&self) -> &str {
        remove_leading_slash(&self.file_path)
    }
}

dip_type!(
    Manifest;
    attr {
        version: String,
    },
    data {
        runtime: DiPRuntime,
        touchup: DiPTouchup,
    }
);

dip_type!(
    LegacyManifest;
    attr {},
    data {
        executable: DiPTouchup,
    }
);

/// https://www.reddit.com/r/rust/comments/11co87m/comment/ja4sy88
fn bytes_to_string(bytes: Vec<u8>) -> Option<String> {
    if let Ok(v) = String::from_utf8(bytes.clone()) {
        return Some(v);
    }

    let u16_bytes: Vec<u16> = bytes
        .chunks_exact(2)
        .into_iter()
        .map(|a| u16::from_ne_bytes([a[0], a[1]]))
        .collect();

    if let Ok(v) = String::from_utf16(&u16_bytes) {
        return Some(v);
    }

    None
}

impl DiPManifest {
    pub async fn read(path: &PathBuf) -> Result<Self> {
        let bytes = tokio::fs::read(path)
            .await
            .context("Failed to read DiP manifest file")
            .unwrap();
        let string = bytes_to_string(bytes);
        if string.is_none() {
            bail!("Failed to decode DiPManifest file. Weird encoding?");
        }

        Ok(quick_xml::de::from_str(&string.unwrap())?)
    }

    pub fn execute_path(&self, trial: bool) -> Option<String> {
        let launcher = self.runtime.launcher.iter().find(|l| l.trial == trial);
        launcher.map(|l| l.file_path.clone())
    }

    #[cfg(unix)]
    pub async fn run_touchup(&self, install_path: &PathBuf) -> Result<()> {
        use crate::unix::wine::{invalidate_mx_wine_registry, run_wine_command};

        let install_path = PathBuf::from(remove_trailing_slash(install_path.to_str().unwrap()));
        let args = self.collect_touchup_args(&install_path);
        let path = install_path.join(&self.touchup.path());
        run_wine_command("wine64", path, Some(args), None, true)?;

        invalidate_mx_wine_registry().await;
        Ok(())
    }

    #[cfg(windows)]
    pub async fn run_touchup(&self, install_path: &PathBuf) -> Result<()> {
        let args = self.collect_touchup_args(install_path);
        let path = install_path.join(&self.touchup.path());
        // TODO
        Ok(())
    }

    fn collect_touchup_args(&self, install_path: &PathBuf) -> Vec<PathBuf> {
        let mut args = Vec::new();
        for arg in self.touchup.parameters.split(" ") {
            let arg = arg.replace("{locale}", "en_US").replace(
                "\"{installLocation}\"",
                &format!(
                    "Z:{}",
                    remove_trailing_backslash(install_path.to_str().unwrap()).replace("/", "\\")
                ),
            );

            args.push(PathBuf::from(arg));
        }
        args
    }
}
