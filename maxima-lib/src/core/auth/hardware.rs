use regex::Regex;

use crate::util::simple_crypto::hash_fnv1a;

#[derive(Debug)]
pub struct HardwareInfo {
    pub board_manufacturer: String,
    pub board_sn: String,
    pub bios_manufacturer: String,
    pub bios_sn: String,
    pub os_install_date: String,
    pub os_sn: String,
    pub disk_sn: String,
    pub gpu_pnp_id: Option<String>,
    pub mac: Option<String>,
}

impl HardwareInfo {
    #[cfg(windows)]
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        use std::collections::HashMap;

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

        let wmi_data = wmi_thread.join().expect("Failed to get data from wmi");

        let (os_data, bios_data, board_data, gpu_data, disk_data) = *wmi_data;

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

        let mut gpu_pnp_id: Option<String> = None;
        if let Some(gpu_info) = gpu_data.get(0) {
            gpu_pnp_id = Some(gpu_info.pnp_device_id.clone());
        }

        let mac = get_ea_mac_address();

        Ok(Self {
            bios_manufacturer: bios_manufacturer.to_owned(),
            bios_sn: bios_sn.to_owned(),
            board_manufacturer: board_manufacturer.to_owned(),
            board_sn: board_sn.to_owned(),
            os_install_date: os_install_date.to_owned(),
            os_sn: os_sn.to_owned(),
            disk_sn: disk_sn.to_owned(),
            gpu_pnp_id,
            mac,
        })
    }

    #[cfg(target_os = "linux")]
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        use std::{fs, path::Path, process::Command};

        let board_manufacturer = match fs::read_to_string("/sys/class/dmi/id/board_vendor") {
            Ok(vendor) => vendor.trim().to_owned(),
            Err(_) => String::from("Linux Foundation"),
        };
        let board_sn = String::from("None");
        let bios_manufacturer = match fs::read_to_string("/sys/class/dmi/id/bios_vendor") {
            Ok(vendor) => vendor.trim().to_owned(),
            Err(_) => String::from("Linux Foundation"),
        };
        let bios_sn = String::from("None");
        let os_install_date = get_root_creation_str();
        let os_sn = match fs::read_to_string("/etc/machine-id") {
            Ok(machine_id) => machine_id.trim().to_owned(),
            Err(_) => String::from("None"),
        };

        let mut gpu_pnp_id: Option<String> = None;
        let output = Command::new("lspci").args(["-Dd", "*:*:0300"]).output()?;
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
                    let rev_id = read_file_hex_contents(format!("{}/{}", path, "revision"));

                    gpu_pnp_id = Some(generate_pci_pnp_id(vendor_id, device_id, rev_id));
                }
            }
        }

        // TODO: Maybe, in the future, look for a good way to get the actual disk serial number
        // instead of using the partition UUID
        let mut disk_sn = String::from("None");
        let fstab = fs::read_to_string("/etc/fstab")?;
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

        let mac = get_ea_mac_address();

        Ok(Self {
            bios_manufacturer,
            bios_sn,
            board_manufacturer,
            board_sn,
            os_install_date,
            os_sn,
            disk_sn,
            gpu_pnp_id,
            mac,
        })
    }

    #[cfg(target_os = "macos")]
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        use std::process::Command;

        use smbioslib::{
            table_load_from_device, SMBiosBaseboardInformation, SMBiosSystemInformation,
        };

        use crate::util::system_profiler_utils::SPDisplaysDataType;

        let smbios_data = table_load_from_device()?;
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
            .output()?;
        if output.status.success() {
            let json = String::from_utf8_lossy(&output.stdout);
            let result: SPDisplaysDataType = serde_json::from_str(&json).unwrap();

            if let Some(gpu) = result.items.first() {
                gpu_pnp_id = Some(generate_pci_pnp_id(
                    None,
                    Some(gpu.device_id),
                    Some(gpu.revision_id),
                ));
            }
        }

        let mut disk_sn = String::from("None");
        let output = Command::new("diskutil").args(["info", "/"]).output()?;
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

        Ok(Self {
            bios_manufacturer,
            bios_sn,
            board_manufacturer,
            board_sn,
            os_install_date,
            os_sn,
            gpu_pnp_id,
            disk_sn,
            mac,
        })
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

    pub fn generate_mid(&self) -> Result<String, Box<dyn std::error::Error>> {
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
}

#[cfg(unix)]
fn get_root_creation_str() -> String {
    use std::fs;

    use chrono::{TimeZone, Utc};
    use filetime::FileTime;

    let date_str = String::from("1970-01-0100:00:00.000000000+0000");
    let date_str = match fs::metadata("/") {
        Ok(metadata) => {
            if let Some(creation_time) = FileTime::from_creation_time(&metadata) {
                // Convert Unix timestamp to a DateTime
                let datetime = Utc.timestamp_nanos(creation_time.unix_seconds() * 1_000_000_000);
                // Format the DateTime
                return datetime.format("%Y-%m-%d%H:%M:%S%.9f%z").to_string();
            }

            date_str
        }
        Err(_) => date_str,
    };

    date_str
}

#[cfg(unix)]
fn generate_pci_pnp_id(vendor: Option<u16>, device: Option<u16>, revision: Option<u16>) -> String {
    let mut sections = vec![];
    if let Some(vendor) = vendor {
        sections.push(format!("VEN_{:04X}", vendor));
    }
    if let Some(device) = device {
        sections.push(format!("DEV_{:04X}", device));
    }
    if let Some(revision) = revision {
        sections.push(format!("REV_{:02X}", revision));
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
