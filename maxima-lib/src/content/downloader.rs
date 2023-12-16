use anyhow::{bail, Result};
use reqwest::{header::HeaderValue, Client};

use super::zip::{ZipFile, ZipFileEntry};

pub struct ZipDownloadRequest {
    entries: Vec<ZipFileEntry>,
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

    pub async fn download_single_file(&self, entry: &ZipFileEntry) -> Result<()> {
        let offset = entry.data_offset();
        let range = format!("bytes={}-{}", offset, entry.compressed_size() - offset);
        let data = self
            .client
            .get(&self.url)
            .header("range", range)
            .send()
            .await?;
        Ok(())
    }
}
