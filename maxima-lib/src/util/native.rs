use std::{
    env,
    fs::create_dir_all,
    num::ParseIntError,
    path::{Path, PathBuf},
};
use thiserror::Error;

#[cfg(windows)]
use std::{
    ffi::{CString, OsString},
    os::windows::prelude::{OsStrExt, OsStringExt},
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

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Request1(#[from] ureq::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Serialization(#[from] serde_json::Error),

    #[error("failed to download: `{0}`")]
    Http(String),
}

#[derive(Error, Debug)]
pub enum WineError {
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Download(#[from] DownloadError),

    #[error("failed to run wine command: {output} ({exit:?})")]
    Command {
        output: String,
        exit: std::process::ExitStatus,
    },
    #[error("could not find runtime `{0}`")]
    MissingRuntime(String),
    #[error("runtime `{0}` is not implemented")]
    UnimplementedRuntime(String),
    #[error("couldn't find suitable wine release")]
    Fetch,
}
pub trait SafeParent {
    fn safe_parent(&self) -> Result<&Path, NativeError>;
}

pub trait SafeStr {
    fn safe_str(&self) -> Result<&str, NativeError>;
}

impl SafeParent for PathBuf {
    fn safe_parent(&self) -> Result<&Path, NativeError> {
        match self.parent() {
            Some(parent) => Ok(parent),
            None => Err(NativeError::Parent(self.safe_str()?.to_owned())),
        }
    }
}
impl SafeStr for Path {
    fn safe_str(&self) -> Result<&str, NativeError> {
        self.to_str()
            .ok_or(NativeError::StringifyPath(Box::from(self)))
    }
}

impl SafeParent for Path {
    fn safe_parent(&self) -> Result<&Path, NativeError> {
        self.parent()
            .ok_or(NativeError::Parent(self.safe_str()?.to_owned()))
    }
}

#[derive(Error, Debug)]
pub enum NativeError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Wine(#[from] WineError),
    #[error(transparent)]
    TomlSer(#[from] toml::ser::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    StripPrefix(#[from] std::path::StripPrefixError),
    #[error(transparent)]
    ParseInt(#[from] ParseIntError),

    #[error("missing `{0}` environment variable")]
    MissingEnvironmentVariable(String),
    #[error("could not get the parent directory of `{0:?}`")]
    Parent(String),
    #[error("could not convert `&str` to a `String`")]
    Stringify,
    #[error("could not convert `{0:?}` to a string")]
    StringifyPath(Box<Path>),
    #[error("could not get file name from path")]
    FileName,
    #[error("could not get the next path component of `{0}`")]
    PathComponentNext(Box<Path>),
    #[error("could not find PID of `{0}`")]
    Pid(String),
    #[error("could not find PID pattern")]
    PidPattern,

    // Windows
    #[error("failed to elevate `{0}`")]
    Elevation(String),
    #[error("could not get module file name")]
    CantFindModuleFileName,
    #[error("could not open process")]
    CantFindProcess,
    #[error("could not find window")]
    CantFindWindow,
    #[error("could not run command. exit code `{0}`")]
    Command(i32),
}

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
pub fn get_hwnd() -> Result<HWND, NativeError> {
    unsafe {
        EnumWindows(Some(enum_windows_proc), 0);

        let window_name = CString::new("Maxima").expect("Failed to create native string");
        let mut hwnd = FindWindowA(std::ptr::null(), window_name.as_ptr());
        if !hwnd.is_null() {
            return Ok(hwnd);
        }

        hwnd = GetConsoleWindow();
        if hwnd.is_null() {
            return Err(NativeError::CantFindWindow);
        }

        Ok(hwnd)
    }
}

#[cfg(windows)]
pub fn take_foreground_focus() -> Result<(), NativeError> {
    unsafe {
        EnumWindows(Some(enum_windows_proc), 0);
    }

    Ok(())
}

#[cfg(unix)]
pub fn take_foreground_focus() -> Result<(), NativeError> {
    // TODO
    Ok(())
}

#[cfg(windows)]
pub fn module_path() -> Result<PathBuf, NativeError> {
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
        panic!("Failed to find module");
    }

    // Create a buffer to hold the DLL path
    let mut buffer: [u16; 260] = [0; 260];

    // Get the DLL path
    let length = unsafe { GetModuleFileNameW(hmodule, buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        panic!("Failed to get module length");
    }

    // Convert buffer to a Rust String
    let os_string = OsString::from_wide(&buffer[0..length as usize]);
    Ok(os_string.to_string_lossy().into_owned().into())
}

#[cfg(target_os = "linux")]
pub fn module_path() -> Result<PathBuf, NativeError> {
    let path = std::fs::read_link("/proc/self/exe");

    Ok(path?)
}

#[cfg(target_os = "macos")]
pub fn module_path() -> Result<PathBuf, NativeError> {
    Ok(env::current_exe()?)
}

#[cfg(not(unix))]
pub fn maxima_dir() -> Result<PathBuf, NativeError> {
    use directories::ProjectDirs;

    let dirs = ProjectDirs::from("com", "ArmchairDevelopers", "Maxima");
    let path = dirs.unwrap().data_dir().to_path_buf();
    create_dir_all(&path)?;
    Ok(path)
}

#[cfg(unix)]
pub fn maxima_dir() -> Result<PathBuf, NativeError> {
    let home = if let Ok(home) = env::var("XDG_DATA_HOME") {
        home
    } else if let Ok(home) = env::var("HOME") {
        format!("{}/.local/share", home)
    } else {
        return Err(NativeError::MissingEnvironmentVariable("HOME".to_string()));
    };

    let path = PathBuf::from(format!("{}/maxima", home));
    create_dir_all(&path)?;
    Ok(path)
}

#[cfg(unix)]
pub fn platform_path<P: AsRef<Path>>(path: P) -> PathBuf {
    PathBuf::from(format!("Z:{}", path.as_ref().to_str().unwrap()))
}

#[cfg(windows)]
pub fn platform_path<P: AsRef<Path>>(path: P) -> PathBuf {
    PathBuf::from(path.as_ref())
}
