use chrono::Utc;
use serde::Serialize;

use super::hardware::HardwareInfo;

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PCSignVersion {
    V1,
    V2,
}

#[derive(Serialize)]
pub struct PCSign<'a> {
    av: &'a str,
    /// BIOS Serial Number
    bsn: String,
    /// GPU device ID
    gid: u32,
    /// Disk Serial Number
    hsn: String,
    /// MAC Address
    #[serde(skip_serializing_if = "Option::is_none")]
    mac: Option<String>,
    /// FNV1a hash of hardware info
    mid: String,
    /// Motherboard Serial Number
    msn: String,
    /// Secret Key Version
    sv: PCSignVersion,
    /// Timestamp
    ts: String,
}

impl PCSign<'_> {
    pub fn new() -> anyhow::Result<Self> {
        let hw_info = HardwareInfo::new(1)?;

        let timestamp = Utc::now();
        let formatted_timestamp = timestamp.format("%Y-%m-%d %H:%M:%S:%3f");
        let mid = hw_info.generate_mid()?;
        let gid = hw_info.get_gpu_id();
        let sv = match rand::random::<f64>() {
            n if n > 0.5 => PCSignVersion::V1,
            _ => PCSignVersion::V2,
        };

        Ok(Self {
            av: "v1",
            bsn: hw_info.bios_sn,
            gid,
            hsn: hw_info.disk_sn,
            sv,
            msn: hw_info.board_sn,
            mac: hw_info.mac,
            mid,
            ts: formatted_timestamp.to_string(),
        })
    }

    pub fn sign_key<'a>(&self) -> &'a [u8; 32] {
        match self.sv {
            PCSignVersion::V2 => b"nt5FfJbdPzNcl2pkC3zgjO43Knvscxft",
            PCSignVersion::V1 => b"ISa3dpGOc8wW7Adn4auACSQmaccrOyR2",
        }
    }
}
