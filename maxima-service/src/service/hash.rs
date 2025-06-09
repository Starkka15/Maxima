use maxima::util::native::NativeError;
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStringExt;
use std::ptr::null_mut;
use winapi::um::handleapi::CloseHandle;
use winapi::um::processthreadsapi::OpenProcess;
use winapi::um::psapi::GetModuleFileNameExW;
use winapi::um::winnt::PROCESS_QUERY_INFORMATION;

pub fn get_sha256_hash_of_pid(pid: u32) -> Result<[u8; 32], NativeError> {
    unsafe {
        let process_handle = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if process_handle.is_null() {
            return Err(NativeError::CantFindProcess);
        }

        let mut buffer = [0u16; 4096];
        let result = GetModuleFileNameExW(
            process_handle,
            null_mut(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
        );
        CloseHandle(process_handle);
        if result == 0 {
            return Err(NativeError::CantFindModuleFileName);
        }

        let path = OsString::from_wide(&buffer[..result as usize])
            .into_string()
            .expect("Failed to convert path to String.");

        let binary = fs::read(&path)?;
        let mut hasher = Sha256::new();
        hasher.update(&binary);
        let hash = hasher.finalize();

        Ok(hash.into())
    }
}
