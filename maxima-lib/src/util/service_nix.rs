pub use super::BackgroundServiceControlError;

pub fn register_service() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

pub unsafe fn init_service_security() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

pub fn is_service_valid() -> Result<bool, BackgroundServiceControlError> {
    Ok(true)
}

pub fn is_service_running() -> Result<bool, BackgroundServiceControlError> {
    Ok(true)
}

pub async fn start_service() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

pub async fn stop_service() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}

pub fn register_service_user() -> Result<(), BackgroundServiceControlError> {
    Ok(())
}
