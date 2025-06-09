#[cfg(windows)]
#[macro_use]
extern crate windows_service;

#[cfg(windows)]
mod service;

#[cfg(windows)]
fn main() -> Result<(), service::error::ServerError> {
    service::start_service()?;
    Ok(())
}

#[cfg(not(windows))]
fn main() {}
