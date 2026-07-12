use std::{
    collections::HashMap,
    env,
    ffi::OsStr,
    fs::{create_dir_all, remove_dir_all, remove_file, File},
    io::Read,
    path::PathBuf,
    process::{ExitStatus, Stdio},
};

use flate2::read::GzDecoder;
use lazy_static::lazy_static;
use log::{info, warn};
use regex::Regex;
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tar::Archive;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::Mutex,
};
use xz2::read::XzDecoder;

use crate::util::{
    github::{fetch_github_release, fetch_github_releases, github_download_asset, GithubRelease},
    native::{maxima_dir, maxima_cache_dir, DownloadError, NativeError, SafeParent, SafeStr, WineError},
    registry::RegistryError,
};

lazy_static! {
    static ref PROTON_PATTERN: Regex = Regex::new(r"GE-Proton\d+-\d+\.tar\.gz").unwrap();
}

// A Proton verb to use
pub enum CommandType {
    // Set the prefix up and runs the command
    Run,
    // Waits for any hanging wineserver instances and runs the command
    WaitForExitAndRun,
    // Directly calls the command, doesn't setup the prefix (use with caution)
    RunInPrefix,
}

impl std::fmt::Display for CommandType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display = match self {
            Self::RunInPrefix => "runinprefix",
            Self::Run => "run",
            Self::WaitForExitAndRun => "waitforexitandrun",
        };
        f.write_str(display)
    }
}

const VERSION_FILE: &str = "dependency-versions.toml";

#[derive(Deserialize, Default)]
pub(crate) struct LutrisRuntime {
    name: String,
    created_at: String,
    url: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct Versions {
    proton: String,
    eac_runtime: String,
    umu: String,
}

/// Returns internal prtoton pfx path
pub fn wine_prefix_dir() -> Result<PathBuf, NativeError> {
    Ok(maxima_dir()?.join("wine/prefix"))
}

pub fn proton_dir() -> Result<PathBuf, NativeError> {
    Ok(maxima_dir()?.join("wine/proton"))
}

pub fn wine_dir() -> Result<PathBuf, NativeError> {
    Ok(maxima_dir()?.join("wine"))
}

pub fn eac_dir() -> Result<PathBuf, NativeError> {
    Ok(maxima_dir()?.join("wine/eac_runtime"))
}

pub fn umu_bin() -> Result<PathBuf, NativeError> {
    Ok(maxima_dir()?.join("wine/umu/umu-run"))
}

fn versions() -> Result<Versions, NativeError> {
    let file = maxima_dir()?.join(VERSION_FILE);
    if !file.exists() {
        return Ok(Versions::default());
    }

    let data = std::fs::read_to_string(file)?;
    Ok(toml::from_str(&data).unwrap_or_default())
}

fn set_versions(versions: Versions) -> Result<(), NativeError> {
    let file = maxima_dir()?.join(VERSION_FILE);
    std::fs::write(file, toml::to_string(&versions)?)?;
    Ok(())
}

pub(crate) async fn check_wine_validity() -> Result<bool, NativeError> {
    if !proton_dir()?.exists() {
        return Ok(false);
    }

    let version = versions()?.proton;

    let release = get_wine_release();
    if let Err(err) = release {
        if !version.is_empty() {
            warn!("Failed to check wine release, rate limited?");
            return Ok(true);
        }

        return Err(NativeError::Wine(err));
    }

    Ok(version == release?.tag_name)
}

pub(crate) async fn get_lutris_runtimes() -> Result<Vec<LutrisRuntime>, WineError> {
    let client = reqwest::Client::builder()
        .user_agent("ArmchairDevelopers/Maxima")
        .build()?;
    let res = client.get("https://lutris.net/api/runtimes").send().await?;
    let res = res.error_for_status()?;
    let data = res.json().await?;
    Ok(data)
}

pub(crate) async fn check_runtime_validity(
    key: &str,
    runtimes: &[LutrisRuntime],
) -> Result<bool, NativeError> {
    let versions = versions()?;
    let version = match key {
        "umu" => &versions.umu,
        "eac_runtime" => &versions.eac_runtime,
        _ => {
            return Err(NativeError::Wine(WineError::UnimplementedRuntime(
                key.to_string(),
            )))
        }
    };
    let path = wine_dir()?.join(key);
    if !path.exists() {
        return Ok(false);
    }
    let runtime_version = runtimes.iter().find(|r| r.name == key);

    Ok(runtime_version.is_some_and(|r| &r.created_at == version))
}

pub(crate) async fn install_runtime(
    key: &str,
    runtimes: &[LutrisRuntime],
) -> Result<(), NativeError> {
    info!("Downloading {key}");
    let runtime = runtimes
        .iter()
        .find(|r| r.name == key)
        .ok_or(NativeError::Wine(WineError::UnimplementedRuntime(
            key.to_string(),
        )))?;
    let mut versions = versions()?;
    let path = wine_dir()?.join(key);
    let runtime_ver = match key {
        "umu" => &mut versions.umu,
        "eac_runtime" => &mut versions.eac_runtime,
        _ => {
            return Err(NativeError::Wine(WineError::UnimplementedRuntime(
                key.to_string(),
            )))
        }
    };

    let res = match ureq::get(&runtime.url)
        .set("User-Agent", "ArmchairDevelopers/Maxima")
        .call()
    {
        Err(err) => return Err(NativeError::Download(DownloadError::Request1(err))),
        Ok(res) => res,
    };

    if res.status() != StatusCode::OK {
        return Err(NativeError::Download(DownloadError::Http(key.to_string())));
    }

    let mut body: Vec<u8> = vec![];
    res.into_reader().read_to_end(&mut body)?;

    if path.exists() {
        remove_dir_all(&path)?;
    }

    create_dir_all(&path)?;

    let data: Box<dyn std::io::Read> = if runtime.url.ends_with(".xz") {
        Box::new(XzDecoder::new(&body[..]))
    } else {
        Box::new(&body[..])
    };

    let archive = Archive::new(data);
    extract_archive(path, archive)?;

    let created_at = runtime.created_at.clone();
    *runtime_ver = created_at;
    set_versions(versions)
}

fn get_wine_release() -> Result<GithubRelease, WineError> {
    let releases = fetch_github_releases("GloriousEggroll", "proton-ge-custom")?;

    let mut release = None;
    for r in releases {
        if r.tag_name.ends_with("LoL") {
            continue;
        }

        release = Some(r);
        break;
    }

    release.ok_or(WineError::Fetch)
}

pub async fn run_wine_command<I: IntoIterator<Item = T>, T: AsRef<OsStr>>(
    arg: T,
    args: Option<I>,
    cwd: Option<PathBuf>,
    want_output: bool,
    command_type: CommandType,
) -> Result<String, NativeError> {
    let proton_path = proton_dir()?;
    let proton_prefix_path = wine_prefix_dir()?;
    let eac_path = eac_dir()?;
    let umu_bin = umu_bin()?;

    let wine_path =
        env::var("MAXIMA_WINE_COMMAND").unwrap_or_else(|_| umu_bin.to_string_lossy().to_string());

    // Create command with all necessary wine env variables
    let mut binding = Command::new(wine_path.clone());
    let mut child = binding
        .env("WINEPREFIX", proton_prefix_path)
        .env("GAMEID", "umu-0")
        .env("PROTON_VERB", &command_type.to_string())
        .env("PROTONPATH", proton_path)
        .env("STORE", "ea")
        .env("PROTON_EAC_RUNTIME", eac_path)
        .env("UMU_ZENITY", "1")
        .env("WINEDEBUG", "fixme-all")
        .env("LD_PRELOAD", "") // Fixes some log errors for some games
        .arg(arg);

    if !wine_path.ends_with("umu-run") {
        // wsock32 is used as a proxy for Northstar (Titanfall 2). TODO: provide user-facing option for this!
        child = child.env(
            "WINEDLLOVERRIDES",
            "CryptBase,wsock32,bcrypt,dxgi,d3d11,d3d12,d3d12core=n,b;winemenubuilder.exe=d",
        );
    }

    if let Some(arguments) = args {
        child = child.args(arguments);
    }

    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }

    let status: ExitStatus;
    let mut output_str = String::new();

    if want_output {
        let output = child
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?
            .wait_with_output()
            .await?;
        // Capture both streams. stderr is appended (labelled) so Wine diagnostics
        // surface in WineError::Command instead of being silently dropped.
        output_str = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.stderr.is_empty() {
            let stderr_str = String::from_utf8_lossy(&output.stderr);
            if !output_str.is_empty() {
                output_str.push('\n');
            }
            output_str.push_str("[stderr] ");
            output_str.push_str(&stderr_str);
        }
        status = output.status;
    } else {
        status = child.spawn()?.wait().await?;
    };

    if !status.success() {
        return Err(NativeError::Wine(WineError::Command {
            output: output_str,
            exit: status,
        }));
    }

    Ok(output_str.to_string())
}

pub(crate) async fn install_wine() -> Result<(), NativeError> {
    let release = get_wine_release()?;
    let asset = match release
        .assets
        .iter()
        .find(|x| PROTON_PATTERN.captures(&x.name).is_some())
    {
        Some(asset) => asset,
        None => return Err(NativeError::Wine(WineError::Fetch)),
    };

    let dir = maxima_cache_dir()?.join("downloads");
    create_dir_all(&dir)?;

    let archive_path = dir.join(&asset.name);
    github_download_asset(asset, &archive_path)?;
    extract_wine(&archive_path)?;

    let mut versions = versions()?;
    versions.proton = release.tag_name;
    set_versions(versions)?;

    if let Err(err) = remove_file(&archive_path) {
        warn!("Failed to delete {:?} - {:?}", archive_path, err);
    }

    let _ = run_wine_command("", None::<[&str; 0]>, None, false, CommandType::Run).await;

    Ok(())
}

fn extract_wine(archive_path: &PathBuf) -> Result<(), NativeError> {
    info!("Extracting proton...");

    let dir = proton_dir()?;
    if dir.exists() {
        remove_dir_all(&dir)?;
    }

    create_dir_all(&dir)?;

    let archive_file = File::open(archive_path)?;
    let archive_decoder = GzDecoder::new(archive_file);
    let archive = Archive::new(archive_decoder);
    extract_archive(dir, archive)
}

fn extract_archive<R: Read + Sized>(
    dir: PathBuf,
    mut archive: Archive<R>,
) -> Result<(), NativeError> {
    for entry in archive.entries()? {
        let mut entry = entry?;
        let entry_path = entry.path()?;

        let next = match entry_path.components().next() {
            Some(next) => next,
            None => {
                return Err(NativeError::PathComponentNext(entry_path.clone().into()));
            }
        };
        let destination_path = dir.join(entry_path.strip_prefix(next)?);
        if let Some(parent_dir) = destination_path.parent() {
            create_dir_all(parent_dir)?;
        }

        entry.unpack(destination_path)?;
    }

    Ok(())
}

/// Clean up any interrupted WiX Burn-engine installations (vcredist etc.) that
/// left a `/burn.runonce` entry in Wine's RunOnce registry key and a `state.rsm`
/// checkpoint in the Package Cache. A Burn install that is killed mid-run sets
/// `Resume=dword:00000001` in its Uninstall key and adds a RunOnce entry; the
/// next invocation then tries to resume from the (potentially corrupt) checkpoint
/// and exits with code 1 instead of installing fresh.
pub async fn cleanup_interrupted_burn_installs() -> Result<(), NativeError> {
    let registry = parse_mx_wine_registry().await?;

    let runonce_prefix_wow =
        "software\\wow6432node\\microsoft\\windows\\currentversion\\runonce\\";
    let runonce_prefix_plain = "software\\microsoft\\windows\\currentversion\\runonce\\";

    let mut to_clean: Vec<(String, String)> = Vec::new();
    for (full_key, value) in &registry {
        if (!full_key.starts_with(runonce_prefix_wow)
            && !full_key.starts_with(runonce_prefix_plain))
            || !value.contains("burn.runonce")
        {
            continue;
        }
        let value_name = full_key.rsplit('\\').next().unwrap_or("").to_string();
        if let Some(guid) = extract_burn_guid_from_command(value) {
            info!(
                "Found interrupted Burn install: RunOnce='{}' GUID={}",
                value_name, guid
            );
            to_clean.push((value_name, guid));
        }
    }

    if to_clean.is_empty() {
        return Ok(());
    }

    // Delete state.rsm checkpoint files directly on the host filesystem
    let prefix = wine_prefix_dir()?;
    for (_, guid) in &to_clean {
        let state_rsm = prefix
            .join("drive_c")
            .join("ProgramData")
            .join("Package Cache")
            .join(guid)
            .join("state.rsm");
        if state_rsm.exists() {
            match tokio::fs::remove_file(&state_rsm).await {
                Ok(()) => info!("Deleted Burn checkpoint: {}", state_rsm.display()),
                Err(err) => warn!("Could not delete {}: {}", state_rsm.display(), err),
            }
        }
    }

    // Build a .reg file to clear Resume flags and delete RunOnce entries
    let mut reg = "Windows Registry Editor Version 5.00\n\n".to_string();
    for (value_name, guid) in &to_clean {
        for key_prefix in &[
            "HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
            "HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall",
        ] {
            reg.push_str(&format!("[{}\\{}]\n", key_prefix, guid));
            reg.push_str("\"Resume\"=dword:00000000\n\n");
        }
        for key_prefix in &[
            "HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
            "HKEY_LOCAL_MACHINE\\Software\\Microsoft\\Windows\\CurrentVersion\\RunOnce",
        ] {
            reg.push_str(&format!("[{}]\n", key_prefix));
            // "ValueName"=- is the .reg syntax for deleting a value
            reg.push_str(&format!("\"{}\"=-\n\n", value_name));
        }
    }

    let reg_path = maxima_dir()?.join("temp").join("burn_cleanup.reg");
    tokio::fs::create_dir_all(reg_path.safe_parent()?).await?;
    tokio::fs::write(&reg_path, reg.as_bytes()).await?;

    run_wine_command(
        "regedit",
        Some(vec!["/S", reg_path.safe_str()?]),
        None,
        true,
        CommandType::Run,
    )
    .await?;

    tokio::fs::remove_file(&reg_path).await?;
    invalidate_mx_wine_registry().await;

    info!("Cleaned up {} interrupted Burn installation(s)", to_clean.len());
    Ok(())
}

fn extract_burn_guid_from_command(command: &str) -> Option<String> {
    let start = command.find('{')?;
    let end = command[start..].find('}')? + start + 1;
    let candidate = &command[start..end];
    // A GUID with braces is {xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx} = 38 chars
    if candidate.len() == 38 {
        Some(candidate.to_string())
    } else {
        None
    }
}

pub async fn setup_wine_registry() -> Result<(), NativeError> {
    let mut reg_content = "Windows Registry Editor Version 5.00\n\n".to_string();
    // This supports text values only at the moment
    // if you need a dword - implement it
    let entries: &[(&str, &[(&str, &str)])] = &[
        (
            "HKEY_LOCAL_MACHINE\\Software\\Electronic Arts\\EA Desktop",
            &[("InstallSuccessful", "true")],
        ),
        (
            "HKEY_LOCAL_MACHINE\\Software\\Origin",
            &[
                ("InstallSuccessful", "true"),
                ("ClientPath", "C:/Windows/System32/conhost.exe"),
            ],
        ),
        (
            "HKEY_LOCAL_MACHINE\\Software\\Electronic Arts\\Origin",
            &[
                ("InstallSuccessful", "true"),
                ("ClientPath", "C:/Windows/System32/conhost.exe"),
            ],
        ),
        (
            "HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Electronic Arts\\EA Desktop",
            &[("InstallSuccessful", "true")],
        ),
        (
            "HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Electronic Arts\\Origin",
            &[
                ("InstallSuccessful", "true"),
                ("ClientPath", "C:/Windows/System32/conhost.exe"),
            ],
        ),
        // Plain `Origin` under Wow6432Node. Origin's install check runs in a
        // 32-bit context, so a game reading HKLM\Software\Origin is redirected
        // by the WoW64 registry to HKLM\Software\Wow6432Node\Origin. We already
        // write the 64-bit `Software\Origin`, but titles like Mirror's Edge
        // Catalyst read the 32-bit view and abort with "Origin is not
        // installed" when this key is absent. OriginPath/ClientPath point at a
        // real existing exe so any path check the game does still succeeds.
        (
            "HKEY_LOCAL_MACHINE\\Software\\Wow6432Node\\Origin",
            &[
                ("InstallSuccessful", "true"),
                ("ClientPath", "C:/Windows/System32/conhost.exe"),
                ("OriginPath", "C:/Windows/System32/conhost.exe"),
            ],
        ),
    ];

    for (key, values) in entries.into_iter() {
        reg_content.push_str(&format!("[{}]\n", key));
        for (name, value) in values.into_iter() {
            let value = value.replace("\\", "\\\\");
            reg_content.push_str(&format!("\"{}\"=\"{}\"\n\n", name, value));
        }
    }

    let path = maxima_cache_dir()?.join("wine.reg");
    tokio::fs::create_dir_all(path.safe_parent()?).await?;

    {
        let mut reg_file = tokio::fs::File::create(&path).await?;
        reg_file.write_all(reg_content.as_bytes()).await?;
    }

    run_wine_command(
        "regedit",
        Some(vec!["/S", path.safe_str()?]),
        None,
        true,
        CommandType::Run,
    )
    .await?;

    tokio::fs::remove_file(path).await?;

    Ok(())
}

/// Write an EA game's `[HKLM\...\Install Dir]` registry key directly into the
/// wine prefix, so `is_installed()` / `execute_path()` resolve the game without
/// depending on its own `Touchup.exe` having succeeded. EA's touchup runs
/// `DXSETUP.exe`, which fails under wine ("DirectX Setup Error: An internal
/// error occurred") and aborts touchup BEFORE it writes this key -- yet the
/// DirectX DLLs register fine via regsvr32 and Proton ships DirectX anyway, so
/// the only thing actually lost is this key. We already know the install path
/// and the key (from the manifest launcher template), so we just write it.
///
/// `registry_template` is the bracketed launcher path from the manifest, e.g.
/// `[HKEY_LOCAL_MACHINE\SOFTWARE\EA Games\BFH\Install Dir]BFHWebHelper.exe`.
/// Launchers that aren't a registry template (no brackets) are a no-op.
pub async fn write_install_dir_registry(
    registry_template: &str,
    install_path: &std::path::Path,
) -> Result<(), NativeError> {
    let (Some(start), Some(end)) =
        (registry_template.find('['), registry_template.find(']'))
    else {
        return Ok(());
    };
    let full = &registry_template[start + 1..end]; // HKLM\SOFTWARE\EA Games\BFH\Install Dir
    let Some(sep) = full.rfind('\\') else {
        return Ok(());
    };
    let key = &full[..sep]; // HKEY_LOCAL_MACHINE\SOFTWARE\EA Games\BFH
    let value_name = &full[sep + 1..]; // Install Dir

    let Some(unix) = install_path.to_str() else {
        return Ok(());
    };
    // Exact form Touchup.exe writes: Z:<unix path>, backslashes, trailing sep.
    let win_path = format!("Z:{}\\", unix.trim_end_matches('/').replace('/', "\\"));

    // Also write the Wow6432Node sibling so 32-bit lookups resolve.
    let wow_key = match key.strip_prefix("HKEY_LOCAL_MACHINE\\SOFTWARE\\") {
        Some(rest) => format!("HKEY_LOCAL_MACHINE\\SOFTWARE\\Wow6432Node\\{}", rest),
        None => key.to_string(),
    };

    let mut reg_content = "Windows Registry Editor Version 5.00\n\n".to_string();
    for k in [key.to_string(), wow_key] {
        reg_content.push_str(&format!("[{}]\n", k));
        reg_content.push_str(&format!(
            "\"{}\"=\"{}\"\n\n",
            value_name,
            win_path.replace('\\', "\\\\")
        ));
    }

    let path = maxima_cache_dir()?.join("install_dir.reg");
    {
        let mut reg_file = tokio::fs::File::create(&path).await?;
        reg_file.write_all(reg_content.as_bytes()).await?;
    }

    run_wine_command(
        "regedit",
        Some(vec!["/S", path.safe_str()?]),
        None,
        true,
        CommandType::Run,
    )
    .await?;

    tokio::fs::remove_file(path).await?;
    invalidate_mx_wine_registry().await;

    Ok(())
}

pub type WineRegistry = HashMap<String, String>;

lazy_static! {
    static ref MX_WINE_REGISTRY: Mutex<WineRegistry> = Mutex::new(WineRegistry::new());
}

async fn parse_wine_registry(file_path: &str) -> WineRegistry {
    let mut registry_map = MX_WINE_REGISTRY.lock().await;
    if !registry_map.is_empty() {
        return registry_map.clone();
    }

    let file = tokio::fs::File::open(file_path)
        .await
        .expect("Could not open file");
    let reader = BufReader::new(file);
    let mut current_section = String::new();

    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await.expect("Failed to read file") {
        let trimmed_line = line.trim();

        if trimmed_line.starts_with('[') && trimmed_line.contains(']') {
            if let Some(end) = trimmed_line.find(']') {
                current_section = trimmed_line[1..end].to_string();
            }
        } else if trimmed_line.contains('=') && trimmed_line.starts_with('"') {
            let parts: Vec<&str> = trimmed_line.splitn(2, '=').collect();
            if parts.len() == 2 {
                let key = parts[0].trim_matches('"').to_string();
                let value = parts[1].trim_matches('"').to_string();
                let full_key = format!("{}\\{}", current_section, key).replace("\\\\", "\\");
                registry_map.insert(full_key.to_lowercase(), value);
            }
        }
    }

    registry_map.clone()
}

pub async fn parse_mx_wine_registry() -> Result<WineRegistry, NativeError> {
    let path = wine_prefix_dir()?.join("system.reg");
    if !path.exists() {
        return Ok(HashMap::new());
    }

    Ok(parse_wine_registry(path.safe_str()?).await)
}

pub async fn invalidate_mx_wine_registry() {
    MX_WINE_REGISTRY.lock().await.clear();
}

fn normalize_key(key: &str) -> String {
    let lower_key = key.to_lowercase();
    if lower_key.starts_with("hkey_local_machine\\") {
        lower_key
            .trim_start_matches("hkey_local_machine\\")
            .to_string()
    } else {
        lower_key
    }
}

pub async fn get_mx_wine_registry_value(query_key: &str) -> Result<Option<String>, RegistryError> {
    let registry_map = parse_mx_wine_registry().await?;
    let normalized_query_key = normalize_key(query_key);

    let value = if let Some(value) = registry_map.get(&normalized_query_key) {
        Some(value.clone())
    } else {
        let wow6432_query_key =
            normalized_query_key.replace("software\\", "software\\wow6432node\\");
        registry_map.get(&wow6432_query_key).cloned()
    };

    Ok(value.map(|x| x.replace("Z:", "").replace("\\", "/")))
}
