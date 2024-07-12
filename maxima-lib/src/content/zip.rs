use anyhow::{bail, Result};
use bytebuffer::{ByteBuffer, Endian};
use derive_getters::Getters;
use encoding::{all::WINDOWS_1252, DecoderTrap, Encoding};
use log::{debug, warn};
use reqwest::Client;
use std::cmp;

/// This module is based on https://users.cs.jmu.edu/buchhofp/forensics/formats/pkzip.html

const ZIP_EOCD_SIGNATURE: u32 = 0x06054b50;

const ZIP64_EOCD_SIGNATURE: u32 = 0x06064b50;
const ZIP64_EOCD_LOCATOR_SIGNATURE: u32 = 0x07064b50;
const ZIP64_SIGNATURE: i64 = 0xFFFFFFFF;

const ZIP_EOCD_FIXED_PART_SIZE: u32 = 22;
const ZIP64_EOCD_FIXED_PART_SIZE: u32 = 56;

const ZIP_FILE_HEADER_SIGNATURE: u32 = 0x02014b50;

const MAX_BACKSCAN_OFFSET: usize = 6 * 1024 * 1024;

fn signature_scan_rev(data: &[u8], signature: u32) -> Option<usize> {
    let signature_bytes = signature.to_le_bytes();
    let signature_len = signature_bytes.len();

    for (i, window) in data.windows(signature_len).enumerate().rev() {
        if window != signature_bytes {
            continue;
        }

        return Some(i);
    }

    None
}

#[derive(Default, Clone, Debug)]
pub enum CompressionType {
    #[default]
    None = 0,
    Deflate = 8,
}

impl CompressionType {
    pub fn from_num(num: u16) -> CompressionType {
        match num {
            8 => CompressionType::Deflate,
            0 | _ => CompressionType::None,
        }
    }
}

#[derive(Default, Clone, Getters)]
pub struct ZipFileEntry {
    name: String,
    crc32: u32,
    compression_type: CompressionType,
    compressed_size: i64,
    uncompressed_size: i64,
    disk_number_start: u16,
    local_header_offset: i64,
    data_offset: i64,

    #[getter(skip)]
    extra_field: Vec<u8>,
}

impl ZipFileEntry {
    pub fn parse(data: &mut ByteBuffer) -> Result<ZipFileEntry> {
        let mut entry = Self::default();

        let signature = data.read_u32()?;
        if signature != ZIP_FILE_HEADER_SIGNATURE {
            bail!("Invalid zip file entry signature: {:#10x}", signature);
        }

        data.read_u16()?; // Version
        data.read_u16()?; // Vers. needed
        let flags = data.read_u16()?;
        let use_utf8 = flags & (1 << 11) != 0;

        entry.compression_type = CompressionType::from_num(data.read_u16()?);

        data.read_u16()?; // Modified time
        data.read_u16()?; // Modified date

        entry.crc32 = data.read_u32()?;
        entry.compressed_size = data.read_u32()? as i64;
        entry.uncompressed_size = data.read_u32()? as i64;

        let file_name_len = data.read_u16()?;
        let extra_field_len = data.read_u16()?;
        let file_comment_len = data.read_u16()?;

        entry.disk_number_start = data.read_u16()?;

        data.read_u16()?; // Internal attr.
        data.read_u32()?; // External attr.

        entry.local_header_offset = data.read_u32()? as i64;

        let name_bytes = data.read_bytes(file_name_len as usize)?;
        entry.name = if use_utf8 {
            debug!("Using UTF-8...");
            String::from_utf8(name_bytes)?
        } else {
            match WINDOWS_1252.decode(&name_bytes, DecoderTrap::Strict) {
                Ok(s) => s,
                Err(e) => {
                    bail!(
                        "Failed to decode file name with Windows-1252 encoding: {}",
                        e
                    );
                }
            }
        };
        entry.extra_field = data.read_bytes(extra_field_len as usize)?;

        if let Ok(data) = entry.extra_field(0x01) {
            let mut data = ByteBuffer::from_vec(data);
            data.set_endian(Endian::LittleEndian);

            if entry.uncompressed_size == 0xFFFFFFFF {
                entry.uncompressed_size = data.read_i64()?;
            }

            if entry.compressed_size == 0xFFFFFFFF {
                entry.compressed_size = data.read_i64()?;
            }

            if entry.local_header_offset == 0xFFFFFFFF {
                entry.local_header_offset = data.read_i64()?;
            }
        }

        data.set_rpos(data.get_rpos() + file_comment_len as usize);

        Ok(entry)
    }

    fn extra_field(&self, id: u16) -> Result<Vec<u8>> {
        let mut data = ByteBuffer::from_vec(self.extra_field.clone());
        data.set_endian(Endian::LittleEndian);

        loop {
            let id2 = data.read_u16()?;
            let size = data.read_u16()? as usize;

            if id == id2 {
                if data.len() - data.get_rpos() >= size {
                    return Ok(data.read_bytes(size)?);
                }

                break;
            }

            data.set_rpos(size);

            if data.len() - data.get_rpos() > 0 {
                break;
            }
        }

        bail!(
            "Failed to find extra field {} for zip entry {}",
            id,
            self.name
        );
    }
}

#[derive(Default, Getters)]
pub struct ZipFile {
    entries: Vec<ZipFileEntry>,
}

#[derive(Default)]
struct EndOfCentralDirectory {
    pub disk_number: u32,
    pub disk_number_with_cd: u32,
    pub disk_entries: u64,
    pub total_entries: u64,
    pub cd_size: u64,
    pub cd_offset: i64,
    pub comment_length: u16,
}

impl EndOfCentralDirectory {
    pub fn parse(&mut self, data: &mut ByteBuffer) -> Result<()> {
        if data.len() - data.get_rpos() < ZIP_EOCD_FIXED_PART_SIZE as usize {
            bail!("Not enough space for end of central directory to be read");
        }

        let signature = data.read_u32()?;
        if signature as u32 != ZIP_EOCD_SIGNATURE {
            bail!("Invalid signature: {}", signature);
        }

        self.disk_number = data.read_u16()? as u32;
        self.disk_number_with_cd = data.read_u16()? as u32;
        self.disk_entries = data.read_u16()? as u64;
        self.total_entries = data.read_u16()? as u64;
        self.cd_size = data.read_u32()? as u64;
        self.cd_offset = data.read_u32()? as i64;
        self.comment_length = data.read_u16()?;

        // Discard the comment
        data.set_rpos(data.get_rpos() + self.comment_length as usize);

        Ok(())
    }

    pub fn parse64(&mut self, data: &mut ByteBuffer) -> Result<()> {
        if data.len() - data.get_rpos() < ZIP64_EOCD_FIXED_PART_SIZE as usize {
            bail!("Not enough space for end of central directory to be read");
        }

        let signature = data.read_u32()?;
        if signature as u32 != ZIP64_EOCD_SIGNATURE {
            bail!("Invalid signature: {}", signature);
        }

        let size_of_record = data.read_i64()?;
        if size_of_record < (ZIP64_EOCD_FIXED_PART_SIZE - 12) as i64 {
            return Ok(());
        }

        data.read_u16()?;
        data.read_u16()?;

        self.disk_number = data.read_u32()? as u32;
        self.disk_number_with_cd = data.read_u32()? as u32;
        self.disk_entries = data.read_i64()? as u64;
        self.total_entries = data.read_i64()? as u64;
        self.cd_size = data.read_i64()? as u64;
        self.cd_offset = data.read_i64()? as i64;
        self.comment_length = 0;

        Ok(())
    }
}

impl ZipFile {
    pub async fn fetch(url: &str) -> Result<Self> {
        let client = Client::new();

        let response = client.head(url).send().await?;
        let content_length = response.headers().get("content-length");
        if content_length.is_none() {
            bail!("No content length found in response");
        }

        let content_length = content_length.unwrap().to_str()?.parse::<i64>().unwrap_or(0);

        let mut data: Vec<u8> = Vec::with_capacity(MAX_BACKSCAN_OFFSET);
        let mut offset = content_length - 8 * 1024;
        if offset < 0 {
            bail!("Something went wrong while requesting a zip manifest");
        }

        let mut zip = Self::default();

        while offset > 0 && data.len() < MAX_BACKSCAN_OFFSET {
            let read = content_length - offset - data.len() as i64;
            let start_offset = content_length - data.len() as i64 - read;
            let end_offset = start_offset + read;

            let range_header = format!("bytes={}-{}", start_offset, end_offset - 1);
            let response = client
                .get(url)
                .header("range", &range_header)
                .send()
                .await?;
            let this_data = response.bytes().await?.to_vec();
            data = [this_data, data].concat();

            offset = zip.load(&mut ByteBuffer::from_vec(data.clone()), content_length)?;
            if offset > content_length {
                bail!("Requested read was too big");
            }
        }

        Ok(zip)
    }

    fn load(&mut self, data: &mut ByteBuffer, total_size: i64) -> Result<i64> {
        data.set_endian(Endian::LittleEndian);

        let pos = signature_scan_rev(data.as_bytes(), ZIP_EOCD_SIGNATURE);
        if pos.is_none() {
            let amt = cmp::min(total_size - data.len() as i64, 1024);
            return Ok(total_size - data.len() as i64 - amt);
        }

        data.set_rpos(pos.unwrap());

        let mut eocd = EndOfCentralDirectory::default();
        let result = eocd.parse(data);
        if result.is_err() {
            bail!(
                "Failed to read end of central directory: {}",
                result.err().unwrap()
            );
        }

        // Check if we're Zip64
        if eocd.cd_offset == ZIP64_SIGNATURE {
            let pos = signature_scan_rev(data.as_bytes(), ZIP64_EOCD_LOCATOR_SIGNATURE);
            if pos.is_none() {
                let amt = cmp::min(total_size - data.len() as i64, 1024);
                return Ok(total_size - data.len() as i64 - amt);
            }

            data.set_rpos(pos.unwrap());

            let signature = data.read_u32()?;
            if signature != ZIP64_EOCD_LOCATOR_SIGNATURE {
                bail!("Invalid Zip64 end of central directory signature");
            }

            data.read_u32()?; // Disk that contains EOCD
            let zip64_eocd_offset = data.read_i64()?;
            data.read_u32()?; // Disk count

            let pos2 = zip64_eocd_offset - (total_size - data.len() as i64);
            if pos2 < 0 {
                return Ok(zip64_eocd_offset);
            }

            data.set_rpos(pos2 as usize);
            if eocd.parse64(data).is_err() {
                warn!("Failed to read ZIP64 end of central directory");
                eocd.cd_offset = 0;
            }
        }

        if eocd.cd_offset < 0 || eocd.cd_offset == ZIP64_SIGNATURE {
            bail!("Failed to read end of central directory");
        }

        if data.len() < (total_size - eocd.cd_offset) as usize {
            if eocd.cd_offset < total_size {
                return Ok(eocd.cd_offset);
            }

            warn!("Something went wrong while parsing the end of central directory");
            return Ok(0);
        }

        let pos = eocd.cd_offset - (total_size - data.len() as i64);
        data.set_rpos(pos as usize);

        self.load_central_directory(data, eocd)?;

        Ok(0)
    }

    fn load_central_directory(
        &mut self,
        data: &mut ByteBuffer,
        eocd: EndOfCentralDirectory,
    ) -> Result<()> {
        for i in 0..eocd.total_entries {
            let result = ZipFileEntry::parse(data);
            if let Some(err) = result.as_ref().err() {
                bail!(
                    "Failed to load central directory entry {} (out of {}): {}",
                    i,
                    eocd.total_entries,
                    err
                );
            }

            let entry = result.unwrap();
            self.entries.push(entry.clone());

            if i == 0 {
                continue;
            }

            let prev = self.entries.get_mut(i as usize - 1);
            if prev.is_none() {
                continue;
            }

            let prev = prev.unwrap();
            if prev.disk_number_start != entry.disk_number_start {
                warn!("Data offset could not be calculated");
                continue;
            }

            prev.data_offset = entry.local_header_offset - prev.compressed_size;
        }

        if let Some(last_entry) = self.entries.last_mut() {
            last_entry.data_offset = eocd.cd_offset - last_entry.compressed_size;
        }

        Ok(())
    }
}
