use crate::util::hash::hash_fnv1a;
use gethostname::gethostname;
use hex::ToHex;
use regex::Regex;
use ring::digest::SHA1_FOR_LEGACY_USE_ONLY;
use std::arch::x86_64::CpuidResult;
use thiserror::Error;

#[derive(Debug)]
pub struct CpuDetails {
    pub flags: CpuidResult,
    pub manufacturer: String,
    pub brand_name: String,
}

impl Default for CpuDetails {
    fn default() -> Self {
        Self {
            flags: CpuidResult {
                eax: 0,
                ebx: 0,
                ecx: 0,
                edx: 0,
            },
            manufacturer: String::default(),
            brand_name: String::default(),
        }
    }
}

#[derive(Debug, Default)]
pub struct HardwareInfo {
    pub version: u32,
    pub board_manufacturer: String,
    pub board_sn: String,
    pub bios_manufacturer: String,
    pub bios_sn: String,
    pub os_install_date: String,
    pub os_sn: String,
    pub disk_sn: String,
    pub volume_sn: String,
    pub gpu_pnp_id: Option<String>,
    pub mac: Option<String>,
    pub cpu_details: CpuDetails,
    pub hostname: String,
}

#[derive(Error, Debug)]
pub enum HardwareHashError {
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl HardwareInfo {
    #[cfg(windows)]
    pub fn new(version: u32) -> Self {
        use std::collections::HashMap;

        use log::warn;
        use wmi::{COMLibrary, FilterValue, WMIConnection};

        use crate::util::wmi_utils;

        let wmi_thread = std::thread::spawn(move || {
            let com_con = COMLibrary::new().unwrap();
            let wmi_con = WMIConnection::new(com_con).unwrap();

            let os_data: Vec<wmi_utils::Win32OperatingSystem> = wmi_con.query().unwrap();
            let bios_data: Vec<wmi_utils::Win32BIOS> = wmi_con.query().unwrap();
            let board_data: Vec<wmi_utils::Win32BaseBoard> = wmi_con.query().unwrap();
            let gpu_data: Vec<wmi_utils::Win32VideoController> = wmi_con.query().unwrap();
            let disk_data: Vec<wmi_utils::Win32DiskDrive> = wmi_con
                .filtered_query(&{
                    let mut filters = HashMap::new();
                    filters.insert(String::from("Index"), FilterValue::Number(0));

                    filters
                })
                .unwrap();
            Box::new((os_data, bios_data, board_data, gpu_data, disk_data))
        });

        let wmi_data = wmi_thread.join();
        if wmi_data.is_err() {
            warn!(
                "WMI call failed, using dummy hardware info. Please report this! {:?}",
                wmi_data.err().unwrap()
            );
            return Self::default();
        }

        let (os_data, bios_data, board_data, gpu_data, disk_data) = *wmi_data.unwrap();

        let mut board_manufacturer = "Microsoft Corporation";
        let mut board_sn = "None";
        if let Some(board_info) = board_data.get(0) {
            board_manufacturer = board_info.manufacturer.as_str();
            board_sn = board_info.serial_number.as_str();
        }

        let mut bios_manufacturer = "Microsoft Corporation";
        let mut bios_sn = "None";
        if let Some(bios_info) = bios_data.get(0) {
            bios_manufacturer = bios_info.manufacturer.as_str();
            bios_sn = bios_info.serial_number.as_str();
        }

        let mut os_install_date = "1970-01-0100:00:00.000000000+0000";
        let mut os_sn = "None";
        if let Some(os_info) = os_data.get(0) {
            os_install_date = os_info.install_date.as_str();
            os_sn = os_info.serial_number.as_str();
        }

        let mut disk_sn = "None";
        if let Some(disk_info) = disk_data.get(0) {
            disk_sn = disk_info.serial_number.as_str();
        }

        let volume_sn = format!("{:08x}", get_c_drive_volume_serial());

        let mut gpu_pnp_id: Option<String> = None;
        if let Some(gpu_info) = gpu_data.get(0) {
            let device_id = &gpu_info.pnp_device_id;
            let device_id =
                device_id[..device_id.rfind('\\').unwrap_or(device_id.len())].to_string();
            gpu_pnp_id = Some(device_id);
        }

        let mac = get_ea_mac_address();
        let cpu_details = Self::get_cpu_details();
        let hostname = gethostname().to_string_lossy().to_string();

        Self {
            version,
            bios_manufacturer: bios_manufacturer.to_owned(),
            bios_sn: bios_sn.to_owned(),
            board_manufacturer: board_manufacturer.to_owned(),
            board_sn: board_sn.to_owned(),
            os_install_date: os_install_date.to_owned(),
            os_sn: os_sn.to_owned(),
            disk_sn: disk_sn.to_owned(),
            volume_sn,
            gpu_pnp_id,
            mac,
            cpu_details,
            hostname,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn new(version: u32) -> Self {
        use std::{fs, path::Path, process::Command};

        let board_manufacturer = match fs::read_to_string("/sys/class/dmi/id/board_vendor") {
            Ok(vendor) => vendor.trim().to_owned(),
            Err(_) => String::from("Linux Foundation"),
        };

        let board_sn = match fs::read_to_string("/var/lib/dbus/machine-id") {
            Ok(machine_id) => machine_id.trim().to_ascii_uppercase().to_owned(),
            Err(_) => String::from("None"),
        };
        let bios_manufacturer = match fs::read_to_string("/sys/class/dmi/id/bios_vendor") {
            Ok(vendor) => vendor.trim().to_owned(),
            Err(_) => String::from("Linux Foundation"),
        };

        let bios_sn = String::from("Serial number");
        let os_install_date = get_root_creation_str();
        let os_sn = String::from("00330-50000-00000-AAOEM");

        let mut gpu_pnp_id: Option<String> = None;
        let output = Command::new("lspci").args(["-Dd", "::0300"]).output();
        if let Ok(output) = output {
            if output.status.success() {
                let output = String::from_utf8_lossy(&output.stdout);
                let lines: Vec<&str> = output
                    .lines()
                    .take(1)
                    .map(|line| line.split_whitespace().next().unwrap_or_default())
                    .collect();

                if let Some(address) = lines.first() {
                    let path = format!("/sys/bus/pci/devices/{}", address);
                    let path_str = path.as_str();

                    if Path::new(path_str).exists() {
                        let vendor_id = read_file_hex_contents(format!("{}/{}", path, "vendor"));
                        let device_id = read_file_hex_contents(format!("{}/{}", path, "device"));
                        let rev_id = Some(0u16);

                        gpu_pnp_id =
                            Some(generate_pci_pnp_id(version, vendor_id, device_id, rev_id));
                    }
                }
            }
        }

        // TODO: Maybe, in the future, look for a good way to get the actual disk serial number
        // instead of using the partition UUID
        let mut disk_sn = String::from("None");
        let fstab = fs::read_to_string("/etc/fstab");
        if let Ok(fstab) = fstab {
            for line in fstab.lines() {
                // Skip comments and empty lines
                if !line.starts_with('#') && !line.is_empty() {
                    // Split the line into fields
                    let fields: Vec<&str> = line.split_whitespace().collect();

                    // Check if the line corresponds to the root filesystem ("/")
                    if fields.len() >= 2 && fields[1] == "/" {
                        // Extract the UUID
                        if let Some(uuid_field) =
                            fields.iter().find(|&&field| field.starts_with("UUID="))
                        {
                            disk_sn = uuid_field
                                .trim_start_matches("UUID=")
                                .trim_matches('"')
                                .to_owned();
                        }
                    }
                }
            }
        }

        let mac = get_ea_mac_address();
        let cpu_details = Self::get_cpu_details();
        let hostname = gethostname().to_string_lossy().to_string();

        Self {
            version,
            bios_manufacturer,
            bios_sn,
            board_manufacturer,
            board_sn,
            os_install_date,
            os_sn,
            disk_sn,
            volume_sn: String::from("43000000"),
            gpu_pnp_id,
            mac,
            cpu_details,
            hostname,
        }
    }

    #[cfg(target_os = "macos")]
    pub fn new(version: u32) -> Self {
        use std::process::Command;

        use smbioslib::{
            table_load_from_device, SMBiosBaseboardInformation, SMBiosSystemInformation,
        };

        use crate::util::system_profiler_utils::SPDisplaysDataType;

        let smbios_data = table_load_from_device().unwrap();
        let bios_data = smbios_data.first::<SMBiosSystemInformation>();
        let board_data = smbios_data.first::<SMBiosBaseboardInformation>();

        let mut board_manufacturer = String::from("Apple Inc.");
        let mut board_sn = String::from("None");
        if let Some(board) = board_data {
            board_manufacturer = board.manufacturer().to_string();
            board_sn = board.serial_number().to_string();
        }

        let mut bios_manufacturer = String::from("Apple Inc.");
        let mut bios_sn = String::from("None");
        if let Some(bios) = bios_data.as_ref() {
            bios_manufacturer = bios.manufacturer().to_string();
            bios_sn = bios.serial_number().to_string();
        }

        let os_install_date = get_root_creation_str();
        let mut os_sn = String::from("None");
        if let Some(uuid) = bios_data.and_then(|bios| bios.uuid()) {
            os_sn = uuid.to_string();
        }

        let mut gpu_pnp_id: Option<String> = None;
        let output = Command::new("system_profiler")
            .args(["SPDisplaysDataType", "-json"])
            .output()
            .unwrap();
        if output.status.success() {
            let json = String::from_utf8_lossy(&output.stdout);
            let result: SPDisplaysDataType = serde_json::from_str(&json).unwrap();

            if let Some(gpu) = result.items.first() {
                gpu_pnp_id = Some(generate_pci_pnp_id(
                    version,
                    None,
                    Some(gpu.device_id),
                    Some(gpu.revision_id),
                ));
            }
        }

        let mut disk_sn = String::from("None");
        let output = Command::new("diskutil")
            .args(["info", "/"])
            .output()
            .unwrap();
        // Check if the command was successful
        if output.status.success() {
            // Convert the output bytes to a UTF-8 string
            let output_str = String::from_utf8_lossy(&output.stdout);

            // Search for the line containing the serial number
            if let Some(uuid) = extract_diskutil_volume_uuid(&output_str) {
                disk_sn = uuid.to_owned();
            }
        }

        let mac = get_ea_mac_address();
        let cpu_details = Self::get_cpu_details();
        let hostname = gethostname().to_string_lossy().to_string();

        Self {
            version,
            bios_manufacturer,
            bios_sn,
            board_manufacturer,
            board_sn,
            os_install_date,
            os_sn,
            gpu_pnp_id,
            disk_sn,
            volume_sn: String::from("43000000"),
            mac,
            cpu_details,
            hostname,
        }
    }

    pub fn get_gpu_id(&self) -> u32 {
        let re = Regex::new(r"DEV_(\w+)").unwrap();

        match &self.gpu_pnp_id {
            Some(gpu_id) => match re.captures(gpu_id) {
                Some(captures) => captures
                    .get(1)
                    .map_or(0, |m| u32::from_str_radix(m.as_str(), 16).unwrap()),
                None => 0,
            },
            None => 0,
        }
    }

    pub fn get_cpu_details() -> CpuDetails {
        use core::arch::x86_64::__cpuid;

        let m = unsafe { __cpuid(0) };
        let flags = unsafe { __cpuid(1) };

        let mut man = Vec::new();
        for val in [m.ebx, m.edx, m.ecx] {
            for b in val.to_ne_bytes() {
                man.push(b);
            }
        }

        let mut brand_name = Vec::with_capacity(47);
        'outer: for eax in [0x80000002, 0x80000003, 0x80000004] {
            let part = unsafe { __cpuid(eax) };
            for val in [part.eax, part.ebx, part.ecx, part.edx] {
                for b in val.to_ne_bytes() {
                    if b == 0 {
                        break 'outer;
                    }
                    brand_name.push(b);
                }
            }
        }
        brand_name.resize(47, 0);
        let brand_name = std::str::from_utf8(&brand_name)
            .unwrap_or("Unknown")
            .to_string();
        let manufacturer = std::str::from_utf8(&man).unwrap_or("Unknown").to_string();

        CpuDetails {
            flags,
            manufacturer,
            brand_name,
        }
    }

    pub fn generate_mid(&self) -> Result<String, HardwareHashError> {
        let mut buffer = String::new();
        buffer += &self.board_manufacturer;
        buffer += &self.board_sn;
        buffer += &self.bios_manufacturer;
        buffer += &self.bios_sn;
        buffer += &self.os_install_date;
        buffer += &self.os_sn;

        if let Some(mac) = get_ea_mac_address() {
            buffer += mac.as_str();
        }

        Ok(hash_fnv1a(buffer.as_bytes()).to_string())
    }

    pub fn generate_hardware_hash(&self) -> String {
        let mut buffer: Vec<&str> = Vec::new();
        let gpu = self.gpu_pnp_id.clone().unwrap_or("None".to_string());
        let cpu_edx = format!("{:08x}", self.cpu_details.flags.edx);
        let cpu_edx_eax = format!(
            "{:08X}{:08X}",
            self.cpu_details.flags.edx, self.cpu_details.flags.eax
        );
        let cpu_ecx = format!("{:08x}", self.cpu_details.flags.ecx);

        buffer.push(&self.board_manufacturer);
        buffer.push(&self.board_sn);
        match self.version {
            0 | 1 => {
                buffer.push(&self.hostname);
                buffer.push(&self.bios_manufacturer);
                buffer.push(&self.bios_sn);
                buffer.push(&self.os_install_date);
                buffer.push(&self.os_sn)
            }
            2 => {
                buffer.push(&self.bios_manufacturer);
                buffer.push(&self.bios_sn);
                buffer.push(&self.os_install_date);
                buffer.push(&self.os_sn);
                buffer.push(&self.volume_sn);
                buffer.push(&gpu);
                buffer.push(&self.cpu_details.manufacturer);
                buffer.push(&cpu_edx);
                buffer.push(&cpu_ecx);
            }
            3 => {
                buffer.push(&self.bios_manufacturer);
                buffer.push(&self.bios_sn);
                buffer.push(&self.volume_sn);
                buffer.push(&gpu);
                buffer.push(&self.cpu_details.manufacturer);
                buffer.push(&cpu_edx);
                buffer.push(&cpu_ecx);
            }
            4_u32..=u32::MAX => {
                buffer.push(&self.bios_manufacturer);
                buffer.push(&self.bios_sn);
                buffer.push(&self.volume_sn);
                buffer.push(&gpu); // THIS IS EXTENDED IN generate_pci_pnp_id
                buffer.push(&self.cpu_details.manufacturer);
                buffer.push(&cpu_edx_eax);
            }
        }

        let mut final_data = buffer.join(";").to_string();
        final_data.push(';');
        if self.version >= 2 {
            final_data.push_str(&self.cpu_details.brand_name);
            final_data.push(';');
        }
        log::debug!("Hardware hash string \"{}\"", final_data);
        let digest = ring::digest::digest(&SHA1_FOR_LEGACY_USE_ONLY, final_data.as_bytes());
        if self.version < 4 {
            // they fucked up the format and used :x instead of :02x
            digest
                .as_ref()
                .iter()
                .map(|byte| format!("{:x}", byte))
                .collect::<Vec<String>>()
                .join("")
        } else {
            digest.encode_hex()
        }
    }
}

#[cfg(unix)]
fn get_root_creation_str() -> String {
    use crate::unix::wine::wine_prefix_dir;
    use chrono::{TimeZone, Utc};
    use std::{fs, os::unix::fs::MetadataExt};

    let date_str = String::from("1970010100:00:00.000000000+0000");
    let wine_prefix = wine_prefix_dir();
    if wine_prefix.is_err() {
        return date_str;
    }
    let wine_prefix = wine_prefix.unwrap();
    let date_str = match fs::metadata(wine_prefix.join("drive_c")) {
        Ok(metadata) => {
            let nsec = (metadata.mtime_nsec() / 1_000_000) * 1_000_000;
            // Convert Unix timestamp to a DateTime
            let datetime = Utc.timestamp_nanos((metadata.mtime() * 1_000_000_000) + nsec);
            // Format the DateTime
            return datetime.format("%Y%m%d%H%M%S%.6f+000").to_string();
        }
        Err(_) => date_str,
    };

    date_str
}

#[cfg(unix)]
fn generate_pci_pnp_id(
    version: u32,
    vendor: Option<u16>,
    device: Option<u16>,
    revision: Option<u16>,
) -> String {
    let mut sections = vec![];

    sections.push(format!("VEN_{:04X}", vendor.unwrap_or(0)));
    sections.push(format!("DEV_{:04X}", device.unwrap_or(0)));
    sections.push(format!("SUBSYS_{:08X}", 0));
    if version < 4 {
        sections.push(format!("REV_{:02X}", revision.unwrap_or(0)));
    } else {
        // TODO: check how this looks on windows
        sections.push(format!("REV_{:02X}\\0", revision.unwrap_or(0)));
        sections.push(format!("{:08X}", 0xDEADBEEFu32));
        sections.push(format!("{:X}", 0));
        sections.push(format!("{:04X}", 0xDEAD));
    }

    format!("PCI\\{}", sections.join("&"))
}

#[cfg(target_os = "linux")]
fn read_file_hex_contents(path: String) -> Option<u16> {
    use std::fs;

    match fs::read_to_string(path) {
        Ok(hex_str) => Some(u16::from_str_radix(&hex_str.trim()[2..], 16).unwrap()),
        Err(_) => None,
    }
}

#[cfg(target_os = "macos")]
fn extract_diskutil_volume_uuid(output: &str) -> Option<&str> {
    for line in output.lines() {
        if line.trim().starts_with("Volume UUID:") {
            // Extract the serial number from the line
            let parts: Vec<&str> = line.split_whitespace().collect();
            if let Some(uuid) = parts.get(2) {
                return Some(uuid);
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn get_c_drive_volume_serial() -> u32 {
    use std::ptr;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::fileapi::GetVolumeInformationW;

    let mut serial: DWORD = 0;
    let res = unsafe {
        GetVolumeInformationW(
            "C:\\"
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<u16>>()
                .as_ptr(),
            ptr::null_mut(),
            0,
            &mut serial,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            0,
        )
    };

    if res == 0 {
        return 0;
    }

    serial
}

fn get_ea_mac_address() -> Option<String> {
    let mac = match mac_address::get_mac_address() {
        Ok(addr) => addr,
        Err(_) => return None,
    };

    match mac {
        Some(address) => {
            let mac = hex::encode(&address.bytes());
            Some("$".to_owned() + &mac)
        }
        None => None,
    }
}
