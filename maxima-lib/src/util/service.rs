use anyhow::{bail, Result};
use std::ffi::{CString, OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;
use widestring::U16CString;
use winapi::shared::sddl::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW,
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use winapi::um::winnt::{DACL_SECURITY_INFORMATION, LPWSTR, PSECURITY_DESCRIPTOR};
use winapi::um::winsvc::{
    OpenSCManagerA, OpenServiceA, QueryServiceObjectSecurity, SetServiceObjectSecurity,
    SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS,
};
use windows_service::service::{
    ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType, ServiceState, ServiceType,
};
use windows_service::service_manager::{ServiceManager, ServiceManagerAccess};

use is_elevated::is_elevated;

use super::registry::launch_bootstrap;

const SERVICE_NAME: &str = "MaximaBackgroundService";

pub fn register_service() -> Result<()> {
    let service_manager = get_service_manager(true)?;

    // Delete the existing service, if it exists
    let existing_service = service_manager.open_service(SERVICE_NAME, ServiceAccess::DELETE);
    if existing_service.is_ok() {
        let service = existing_service.unwrap();
        service.stop()?;
        service.delete()?;
    }

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from("Maxima Background service"),
        service_type: ServiceType::OWN_PROCESS | ServiceType::INTERACTIVE_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: get_service_path()?,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    let service = service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    service.set_description("Maxima Background Service")?;

    // Allow the service to be started without administrative rights
    unsafe { init_service_security()? };

    Ok(())
}

pub unsafe fn init_service_security() -> Result<()> {
    let hscm = OpenSCManagerA(
        std::ptr::null(),
        CString::new("ServicesActive")?.as_ptr(),
        SC_MANAGER_ALL_ACCESS,
    );

    let hservice = OpenServiceA(
        hscm,
        CString::new(SERVICE_NAME)?.as_ptr(),
        SERVICE_ALL_ACCESS,
    );

    if hservice.is_null() {
        bail!("Failed to find service when configuring security");
    }

    // Query the service object security
    let mut bytes_required: u32 = 0;
    let result = QueryServiceObjectSecurity(
        hservice,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        0,
        &mut bytes_required,
    );

    if result == 0 {
        // The initial call failed; check if the error was related to buffer size
        let last_error = std::io::Error::last_os_error().raw_os_error().unwrap() as u32;
        if last_error != winapi::shared::winerror::ERROR_INSUFFICIENT_BUFFER {
            bail!(
                "Unable to query service object security. Error: {}",
                last_error
            );
        }
    }

    // Allocate a buffer for the security descriptor
    let mut security_descriptor_buffer: Vec<u8> = vec![0; bytes_required as usize];
    let security_descriptor = security_descriptor_buffer.as_mut_ptr() as PSECURITY_DESCRIPTOR;

    // Query the service object security again with the correct buffer
    let result = QueryServiceObjectSecurity(
        hservice,
        DACL_SECURITY_INFORMATION,
        security_descriptor,
        bytes_required,
        &mut bytes_required,
    );

    if result == 0 {
        bail!(
            "Unable to query service object security. Error: {}",
            std::io::Error::last_os_error()
        );
    }

    // Convert the security descriptor to a string
    let mut sddl_string: LPWSTR = std::ptr::null_mut();
    let mut sddl_string_len: u32 = 0;
    let result = ConvertSecurityDescriptorToStringSecurityDescriptorW(
        security_descriptor,
        SDDL_REVISION_1.into(),
        DACL_SECURITY_INFORMATION,
        &mut sddl_string,
        &mut sddl_string_len,
    );

    if result == 0 {
        bail!(
            "Unable to convert security descriptor to string. Error: {}",
            std::io::Error::last_os_error()
        );
    }

    let sddl = U16CString::from_ptr_str(sddl_string).to_string_lossy();
    let sddl_to_add = "(A;;RPWPCR;;;BU)";
    if sddl.contains(sddl_to_add) {
        return Ok(());
    }

    let mut amended_sddl = sddl.clone();
    amended_sddl.push_str(sddl_to_add);

    let mut amended_security_descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let mut amended_security_descriptor_len: u32 = 0;
    let result = ConvertStringSecurityDescriptorToSecurityDescriptorW(
        U16CString::from_str(amended_sddl.as_str())
            .unwrap()
            .as_ptr(),
        SDDL_REVISION_1.into(),
        &mut amended_security_descriptor,
        &mut amended_security_descriptor_len,
    );

    if result == 0 {
        bail!(
            "Unable to convert SDDL string to security descriptor. Error: {}",
            std::io::Error::last_os_error()
        );
    }

    // Set the service object security with the amended security descriptor
    let result = SetServiceObjectSecurity(
        hservice,
        DACL_SECURITY_INFORMATION,
        amended_security_descriptor,
    );

    if result == 0 {
        bail!(
            "Failed to set service security attributes. Error: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(())
}

pub fn is_service_valid() -> Result<bool> {
    let service_manager = get_service_manager(false)?;

    let result =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::QUERY_CONFIG);
    if result.is_err() {
        return Ok(false);
    }

    log::debug!("Verifying service config");

    let service = result.unwrap();
    let config = service.query_config()?;
    if config.executable_path != get_service_path()? {
        log::debug!("Service config invalid: {:?}/{:?}", config.executable_path, get_service_path()?);
        return Ok(false);
    }

    Ok(true)
}

pub fn is_service_running() -> Result<bool> {
    let service_manager = get_service_manager(false)?;

    let service =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::QUERY_STATUS)?;
    let state = service.query_status()?.current_state;

    Ok(state == ServiceState::Running)
}

pub async fn start_service() -> Result<()> {
    let service_manager = get_service_manager(false)?;

    let service_result =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::START);
    if let Some(windows_service::Error::Winapi(code)) = service_result.as_ref().err() {
        bail!("Failed to start background service! {}", code);
    }

    service_result.unwrap().start(&[OsStr::new("test")])?;

    while !is_service_running()? {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

pub fn register_service_user() -> Result<()> {
    if !is_elevated() {
        launch_bootstrap()?;
        return Ok(());
    }

    register_service()?;

    Ok(())
}

fn get_service_manager(create: bool) -> Result<ServiceManager> {
    let mut manager_access = ServiceManagerAccess::CONNECT;
    if create {
        manager_access |= ServiceManagerAccess::CREATE_SERVICE;
    }

    Ok(ServiceManager::local_computer(
        None::<&str>,
        manager_access,
    )?)
}

fn get_service_path() -> Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("maxima-service.exe"))
}
