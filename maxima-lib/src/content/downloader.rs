use std::cmp;
use std::path::Path;

use anyhow::{bail, Result};
use async_compression::tokio::bufread::DeflateDecoder;
use futures::TryStreamExt;
use log::{debug, info};
use reqwest::Client;
use tokio::{
    fs::{create_dir, create_dir_all, File},
    io::BufReader,
};
use tokio::io::AsyncReadExt;

use tokio_util::compat::FuturesAsyncReadCompatExt;

use crate::content::zip::CompressionType;

use super::zip::{ZipFile, ZipFileEntry};

pub struct ZipDownloadRequest {
    _entries: Vec<ZipFileEntry>,
}

pub struct ZipDownloader {
    url: String,
    client: Client,
    manifest: ZipFile,
}

impl ZipDownloader {
    pub async fn new(url: &str) -> Result<Self> {
        let manifest = ZipFile::fetch(url).await?;

        Ok(Self {
            url: url.to_owned(),
            client: Client::builder().build()?,
            manifest,
        })
    }

    pub fn manifest(&self) -> &ZipFile {
        &self.manifest
    }

    pub async fn read_zip_entry_bytes(&self, entry: &ZipFileEntry, bytes: i64) -> Result<Vec<u8>> {
        if entry.name().ends_with("/") {
            bail!("{} is a directory", entry.name());
        }

        if *entry.uncompressed_size() == 0 {
            bail!("{} is empty", entry.name());
        }

        let offset = entry.data_offset();
        debug!("Type: {:?}", entry.compression_type());
        debug!("Compressed Size: {}", entry.compressed_size());
        debug!("Offset: {}", offset);

        let range = format!("bytes={}-{}", offset, offset + cmp::min(bytes, *entry.compressed_size()) - 1);
        let data = self
            .client
            .get(&self.url)
            .header("range", range)
            .send()
            .await.unwrap();


        let stream = data.bytes_stream();
        let stream = stream
            .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e))
            .into_async_read();

        match entry.compression_type() {
            CompressionType::None => {
                bail!("Compression type not supported");
            }
            CompressionType::Deflate => {
                let stream_reader = BufReader::new(stream.compat());

                let mut decoder = DeflateDecoder::new(stream_reader);

                let mut x = decoder.take(cmp::min(bytes, *entry.compressed_size()) as u64);
                let mut buffer = Vec::new();
                let result = x.read_to_end(&mut buffer).await;
                if result.is_err() {
                    bail!("Failed to read bytes");
                }

                return Ok(buffer);
            }
        };
    }

    pub async fn download_single_file(&self, entry: &ZipFileEntry) -> Result<()> {
        let dir_path = Path::new("GameTest");
        let file_path = dir_path.join(entry.name());

        if !file_path.parent().unwrap().exists() {
            create_dir_all(&file_path.parent().unwrap()).await?;
        }

        if entry.name().ends_with("/") && !file_path.exists() {
            // This is a folder, create the dir
            info!("{} is a directory", entry.name());
            create_dir(file_path).await?;
            return Ok(());
        }

        let mut file = File::create(file_path).await?;

        if *entry.uncompressed_size() == 0 {
            info!("{} is empty", entry.name());
            return Ok(());
        }

        let offset = entry.data_offset();
        debug!("Type: {:?}", entry.compression_type());
        debug!("Compressed Size: {}", entry.compressed_size());
        debug!("Offset: {}", offset);

        let range = format!("bytes={}-{}", offset, offset + entry.compressed_size() - 1);
        let data = self
            .client
            .get(&self.url)
            .header("range", range)
            .send()
            .await?;

        let stream = data.bytes_stream();
        let stream = stream
            .map_err(|e| futures::io::Error::new(futures::io::ErrorKind::Other, e))
            .into_async_read();

        match entry.compression_type() {
            CompressionType::None => {
                tokio::io::copy(&mut stream.compat(), &mut file).await?;
            }
            CompressionType::Deflate => {
                let stream_reader = BufReader::new(stream.compat());

                let mut decoder = DeflateDecoder::new(stream_reader);
                tokio::io::copy(&mut decoder, &mut file).await?;
            }
        };

        Ok(())
    }
}
