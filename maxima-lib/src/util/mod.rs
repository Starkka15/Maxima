pub mod github;
pub mod hash;
pub mod log;
pub mod native;
pub mod registry;
pub mod simple_crypto;
pub mod system_profiler_utils;
pub mod wmi_utils;

#[derive(thiserror::Error, Debug)]
pub enum BackgroundServiceControlError {
    #[error(transparent)]
    Native(#[from] native::NativeError),
    #[cfg(windows)]
    #[error(transparent)]
    WindowsService(#[from] windows_service::Error),
    #[error(transparent)]
    Nul(#[from] prost::alloc::ffi::NulError),

    #[error("failed to find service when configuring security")]
    Absent,
    #[error("failed to set service security attributes: `{0}`")]
    SecurityAttributes(std::io::Error),
    #[error("unable to convert security descriptor to string: `{0}`")]
    SecurityDescriptorToString(std::io::Error),
    #[error("unable to query service object security: `{0}`")]
    ServiceObjectSecurity(std::io::Error),
    #[error("unable to convert SDDL string to security descriptor: `{0}`")]
    StringToSecurityDescriptor(std::io::Error),
}

#[cfg(windows)]
pub mod service {
    include!("service_win.rs");
}

#[cfg(unix)]
#[allow(dead_code)]
pub mod service {
    include!("service_nix.rs");
}
