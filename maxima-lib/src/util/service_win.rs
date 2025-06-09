use log::{debug, info};
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

use super::BackgroundServiceControlError;
use super::native::{module_path, NativeError, SafeStr};
use super::registry::launch_bootstrap;
use is_elevated::is_elevated;

pub const SERVICE_NAME: &str = "MaximaBackgroundService";
pub const SERVICE_DISPLAY_NAME: &str = "Maxima Background Service";


pub fn register_service() -> Result<(), BackgroundServiceControlError> {
    let service_manager = service_manager(true)?;

    let service_info = ServiceInfo {
        name: OsString::from(SERVICE_NAME),
        display_name: OsString::from(SERVICE_DISPLAY_NAME),
        service_type: ServiceType::OWN_PROCESS | ServiceType::INTERACTIVE_PROCESS,
        start_type: ServiceStartType::OnDemand,
        error_control: ServiceErrorControl::Normal,
        executable_path: service_path()?,
        launch_arguments: vec![],
        dependencies: vec![],
        account_name: None,
        account_password: None,
    };

    // Update the existing service, if it exists
    let existing_service = service_manager.open_service(
        OsString::from(SERVICE_NAME),
        ServiceAccess::START
            | ServiceAccess::STOP
            | ServiceAccess::CHANGE_CONFIG
            | ServiceAccess::QUERY_STATUS,
    );

    if let Ok(service) = existing_service {
        info!("Updating existing service...");

        let state = service.query_status()?.current_state;
        if state == ServiceState::Running {
            let result = service.stop();
            if result.is_err() {
                // Noop
            }
        }

        service.change_config(&service_info)?;
        service.start(&[OsStr::new("")])?;
        return Ok(());
    }

    let service = service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
    service.set_description(SERVICE_DISPLAY_NAME)?;

    // Allow the service to be started without administrative rights
    unsafe { init_service_security()? };

    Ok(())
}

pub unsafe fn init_service_security() -> Result<(), BackgroundServiceControlError> {
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
        return Err(BackgroundServiceControlError::Absent);
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
        let last_error = std::io::Error::last_os_error();
        let raw_error = (&last_error).raw_os_error().unwrap() as u32;
        if raw_error != winapi::shared::winerror::ERROR_INSUFFICIENT_BUFFER {
            return Err(BackgroundServiceControlError::ServiceObjectSecurity(last_error));
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
        return Err(BackgroundServiceControlError::ServiceObjectSecurity(
            std::io::Error::last_os_error(),
        ));
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
        return Err(BackgroundServiceControlError::SecurityDescriptorToString(
            std::io::Error::last_os_error(),
        ));
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
        U16CString::from_str(amended_sddl.as_str()).unwrap_or(Err(NativeError::Stringify)?)
            .as_ptr(),
        SDDL_REVISION_1.into(),
        &mut amended_security_descriptor,
        &mut amended_security_descriptor_len,
    );

    if result == 0 {
        return Err(BackgroundServiceControlError::StringToSecurityDescriptor(
            std::io::Error::last_os_error(),
        ));
    }

    // Set the service object security with the amended security descriptor
    let result = SetServiceObjectSecurity(
        hservice,
        DACL_SECURITY_INFORMATION,
        amended_security_descriptor,
    );

    if result == 0 {
        return Err(BackgroundServiceControlError::SecurityAttributes(
            std::io::Error::last_os_error(),
        ));
    }

    Ok(())
}

pub fn is_service_valid() -> Result<bool, BackgroundServiceControlError> {
    let service_manager = service_manager(false)?;

    let result =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::QUERY_CONFIG);
    let service = match result {
        Ok(result) => result,
        Err(_) => return Ok(false)
    };

    debug!("Verifying service config");

    let config = service.query_config()?;
    let config_path = PathBuf::from({
        let path = config.executable_path.safe_str()?;
        if path.starts_with("\"") && path.ends_with("\"") {
            &path[1..path.len() - 1]
        } else {
            path
        }
    });

    if config_path != service_path()? {
        debug!(
            "Service config invalid: {:?}/{:?}",
            config.executable_path,
            service_path()?
        );
        return Ok(false);
    }

    Ok(true)
}

pub fn is_service_running() -> Result<bool, BackgroundServiceControlError> {
    let service_manager = service_manager(false)?;

    let service =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::QUERY_STATUS)?;
    let state = service.query_status()?.current_state;

    Ok(state == ServiceState::Running)
}

pub async fn start_service() -> Result<(), BackgroundServiceControlError> {
    let service_manager = service_manager(false)?;

    let service_result =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::START)?;

    service_result.start(&[OsStr::new("")])?;

    while !is_service_running()? {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

pub async fn stop_service() -> Result<(), BackgroundServiceControlError> {
    let service_manager = service_manager(false)?;

    let service_result =
        service_manager.open_service(OsString::from(SERVICE_NAME), ServiceAccess::STOP)?;

    service_result.stop()?;

    while is_service_running()? {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Ok(())
}

pub fn register_service_user() -> Result<(), BackgroundServiceControlError> {
    if !is_elevated() {
        launch_bootstrap()?;
        return Ok(());
    }

    register_service()?;

    Ok(())
}

fn service_manager(create: bool) -> Result<ServiceManager, BackgroundServiceControlError> {
    let mut manager_access = ServiceManagerAccess::CONNECT;
    if create {
        manager_access |= ServiceManagerAccess::CREATE_SERVICE;
    }

    Ok(ServiceManager::local_computer(
        None::<&str>,
        manager_access,
    )?)
}

fn service_path() -> Result<PathBuf, NativeError> {
    Ok(module_path()?.with_file_name("maxima-service.exe"))
}
