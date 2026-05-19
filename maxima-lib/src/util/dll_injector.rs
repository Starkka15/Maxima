use std::ffi::{CString, OsStr};
use std::iter::once;
use std::mem;
use std::os::windows::ffi::OsStrExt;
use std::ptr;
use thiserror::Error;
use winapi::shared::minwindef::LPVOID;
use winapi::um::errhandlingapi::GetLastError;
use winapi::um::handleapi::CloseHandle;
use winapi::um::libloaderapi::{GetModuleHandleW, GetProcAddress};
use winapi::um::memoryapi::{VirtualAllocEx, VirtualFreeEx, WriteProcessMemory};
use winapi::um::processthreadsapi::{CreateRemoteThread, OpenProcess};
use winapi::um::synchapi::WaitForSingleObject;
use winapi::um::winbase::INFINITE;
use winapi::um::winnt::{
    HANDLE, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, PROCESS_ALL_ACCESS,
};

#[derive(Debug, Error)]
pub enum InjectionError {
    #[error("failed to create remote thread, error code: {0}")]
    CreateRemoteThreadFailed(u32),
    #[error("failed to get kernel32 handle, error code: {0}")]
    GetModuleHandleFailed(u32),
    #[error("failed to get LoadLibraryW address, error code: {0}")]
    GetProcAddressFailed(u32),
    #[error("invalid DLL path")]
    InvalidPath,
    #[error("failed to open process, error code: {0}")]
    OpenProcessFailed(u32),
    #[error("process not found")]
    ProcessNotFound,
    #[error("failed to write process memory, error code: {0}")]
    WriteProcessMemoryFailed(u32),
    #[error("failed to allocate memory, error code: {0}")]
    VirtualAllocFailed(u32),
}

pub struct DllInjector {
    target_pid: u32,
}

impl DllInjector {
    pub fn new(pid: u32) -> Self {
        Self { target_pid: pid }
    }

    pub fn inject(&self, dll_path: &str) -> Result<(), InjectionError> {
        unsafe {
            let process_handle = OpenProcess(PROCESS_ALL_ACCESS, 0, self.target_pid);
            if process_handle.is_null() {
                return Err(InjectionError::OpenProcessFailed(GetLastError()));
            }

            let _process_guard = ProcessHandleGuard(process_handle);
            let dll_path_wide: Vec<u16> =
                OsStr::new(dll_path).encode_wide().chain(once(0)).collect();
            let dll_path_size = dll_path_wide.len() * mem::size_of::<u16>();

            let remote_memory = VirtualAllocEx(
                process_handle,
                ptr::null_mut(),
                dll_path_size,
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            );

            if remote_memory.is_null() {
                return Err(InjectionError::VirtualAllocFailed(GetLastError()));
            }

            let _memory_guard = RemoteMemoryGuard {
                process_handle,
                address: remote_memory,
            };

            let mut bytes_written: usize = 0;
            let result = WriteProcessMemory(
                process_handle,
                remote_memory,
                dll_path_wide.as_ptr() as LPVOID,
                dll_path_size,
                &mut bytes_written as *mut usize,
            );

            if result == 0 {
                return Err(InjectionError::WriteProcessMemoryFailed(GetLastError()));
            }

            let kernel32_wide: Vec<u16> = OsStr::new("kernel32.dll")
                .encode_wide()
                .chain(once(0))
                .collect();
            let kernel32_handle = GetModuleHandleW(kernel32_wide.as_ptr());
            if kernel32_handle.is_null() {
                return Err(InjectionError::GetModuleHandleFailed(GetLastError()));
            }

            let load_library_cstring = CString::new("LoadLibraryW").unwrap();
            let load_library_addr = GetProcAddress(kernel32_handle, load_library_cstring.as_ptr());

            if load_library_addr.is_null() {
                return Err(InjectionError::GetProcAddressFailed(GetLastError()));
            }

            let thread_handle = CreateRemoteThread(
                process_handle,
                ptr::null_mut(),
                0,
                Some(mem::transmute(load_library_addr)),
                remote_memory,
                0,
                ptr::null_mut(),
            );

            if thread_handle.is_null() {
                return Err(InjectionError::CreateRemoteThreadFailed(GetLastError()));
            }

            WaitForSingleObject(thread_handle, INFINITE);
            CloseHandle(thread_handle);

            Ok(())
        }
    }
}

struct ProcessHandleGuard(HANDLE);

impl Drop for ProcessHandleGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                CloseHandle(self.0);
            }
        }
    }
}

struct RemoteMemoryGuard {
    process_handle: HANDLE,
    address: LPVOID,
}

impl Drop for RemoteMemoryGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.address.is_null() {
                VirtualFreeEx(self.process_handle, self.address, 0, MEM_RELEASE);
            }
        }
    }
}
