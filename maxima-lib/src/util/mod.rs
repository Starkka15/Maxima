pub mod github;
pub mod log;
pub mod native;
pub mod registry;
pub mod simple_crypto;
pub mod system_profiler_utils;
pub mod wmi_utils;

#[cfg(windows)]
pub mod service {
    include!("service_win.rs");
}

#[cfg(unix)]
#[allow(dead_code)]
pub mod service {
    include!("service_nix.rs");
}
