use std::ptr;

#[cfg(target_family = "windows")]
use winapi::{
    shared::{
        minwindef::{FALSE, DWORD},
        tcpmib::MIB_TCPTABLE_OWNER_PID,
        winerror::ERROR_SUCCESS,
    },
    um::iphlpapi::GetExtendedTcpTable,
};

/// Finds a process that has an open TCP connection from local_port.
#[cfg(target_family = "windows")]
pub fn get_process_id(local_port: u16) -> Option<u32> {
    unsafe {
        let mut table_size: DWORD = 0;
        let mut result = GetExtendedTcpTable(
            ptr::null_mut(),
            &mut table_size,
            FALSE,
            winapi::shared::ws2def::AF_INET as DWORD,
            5,
            0,
        );

        if result != winapi::shared::winerror::ERROR_INSUFFICIENT_BUFFER {
            return None;
        }

        let mut table_buffer: Vec<u8> = vec![0; table_size as usize];
        let table_ptr = table_buffer.as_mut_ptr() as *mut winapi::ctypes::c_void;

        result = GetExtendedTcpTable(
            table_ptr,
            &mut table_size,
            FALSE,
            winapi::shared::ws2def::AF_INET as DWORD,
            5,
            0,
        );

        if result != ERROR_SUCCESS {
            return None;
        }

        let table = table_ptr as *mut MIB_TCPTABLE_OWNER_PID;
        let num_entries = (*table).dwNumEntries;

        for i in 0..num_entries {
            let row = (*table).table.get_unchecked(i as usize);
            let pid = row.dwOwningPid;
            if pid == 0 || u16::from_be(row.dwLocalPort as u16) != local_port {
                continue;
            }

            return Some(pid);
        }

        None
    }
}

#[cfg(target_family = "unix")]
pub fn get_process_id(local_port: u16) -> Option<u32> {
    todo!()
}