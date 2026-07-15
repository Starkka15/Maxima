use std::{
    cmp,
    io::{self, Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    pin::Pin,
    prelude,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    task,
    time::Duration,
};

use crate::{
    content::{
        manager::DownloaderError,
        zip::{CompressionType, ZipFile, ZipFileEntry},
        zlib::{restore_zlib_state, write_zlib_state},
    },
    util::{
        hash::hash_file_crc32,
        native::{maxima_cache_dir, NativeError, SafeParent, SafeStr},
    },
};
use async_compression::tokio::write::DeflateDecoder;
use async_trait::async_trait;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use derive_getters::Getters;
use flate2::bufread::DeflateDecoder as BufreadDeflateDecoder;
use futures::{Stream, StreamExt, TryStreamExt};
use log::{debug, error, info, warn};
use reqwest::Client;
use strum_macros::Display;
use thiserror::Error;
use tokio::{
    fs::{create_dir, create_dir_all, File, OpenOptions},
    io::{AsyncReadExt, AsyncSeekExt, AsyncWrite, BufReader, BufWriter},
    runtime::Handle,
};
use tokio_util::compat::FuturesAsyncReadCompatExt;

fn zstate_path(id: &str, path: &str) -> Result<PathBuf, DownloaderError> {
    let mut path = maxima_cache_dir()?.join("downloader").join(id).join(path);
    path.set_extension("eazstate");
    std::fs::create_dir_all(path.safe_parent()?)?;
    Ok(path)
}

#[async_trait]
trait DownloadDecoder: Send {
    fn save_state(&mut self, buf: &mut BytesMut);
    fn restore_state(&mut self, buf: &mut Bytes);

    fn seek(&mut self, pos: SeekFrom) -> Result<(), DownloaderError>;

    fn write_in_pos(&self) -> u64;
    fn write_out_pos(&self) -> u64;

    fn get_mut<'b>(&mut self) -> Arc<Mutex<dyn AsyncWriteWrapper>>;
}

struct ZLibDeflateDecoder {
    decoder: Arc<Mutex<DeflateDecoder<BufWriter<File>>>>,
}

impl ZLibDeflateDecoder {
    fn new(writer: BufWriter<File>) -> Self {
        Self {
            decoder: Arc::new(Mutex::new(DeflateDecoder::new(writer))),
        }
    }
}

#[async_trait]
impl DownloadDecoder for ZLibDeflateDecoder {
    fn save_state(&mut self, buf: &mut BytesMut) {
        let mut decoder = self.decoder.lock().unwrap();
        let zstream = decoder.inner_mut().decoder_mut().inner.decompress.get_raw();
        write_zlib_state(buf, zstream);
    }

    fn restore_state(&mut self, buf: &mut Bytes) {
        let mut decoder = self.decoder.lock().unwrap();
        let decompress = &mut decoder.inner_mut().decoder_mut().inner.decompress;
        decompress.reset(false);
        let zstream = decompress.get_raw();
        restore_zlib_state(buf, zstream);
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<(), DownloaderError> {
        let mut decoder = self.decoder.lock().unwrap();
        let file = decoder.get_mut();

        let handle = Handle::current();
        let _ = handle.enter();
        futures::executor::block_on(file.seek(pos))?;

        Ok(())
    }

    fn write_in_pos(&self) -> u64 {
        let mut decoder = self.decoder.lock().unwrap();
        let decompress = &mut decoder.inner_mut().decoder_mut().inner.decompress;
        let zstream = decompress.get_raw();
        zstream.total_in as u64
    }

    fn write_out_pos(&self) -> u64 {
        let mut decoder = self.decoder.lock().unwrap();
        let decompress = &mut decoder.inner_mut().decoder_mut().inner.decompress;
        let zstream = decompress.get_raw();
        zstream.total_out as u64
    }

    fn get_mut(&mut self) -> Arc<Mutex<dyn AsyncWriteWrapper>> {
        self.decoder.clone()
    }
}

struct NoopDecoder {
    writer: Arc<Mutex<BufWriter<File>>>,
    pos: u64,
}

impl NoopDecoder {
    pub fn new(writer: BufWriter<File>) -> Self {
        Self {
            writer: Arc::new(Mutex::new(writer)),
            pos: 0,
        }
    }
}

#[async_trait]
impl DownloadDecoder for NoopDecoder {
    fn save_state(&mut self, buf: &mut BytesMut) {
        self.pos = self.writer.lock().unwrap().buffer().len() as u64;
        buf.put_u64(self.pos);
    }

    fn restore_state(&mut self, buf: &mut Bytes) {
        self.pos = buf.get_u64();
    }

    fn seek(&mut self, pos: SeekFrom) -> Result<(), DownloaderError> {
        let mut file = self.writer.lock().unwrap();

        let handle = Handle::current();
        let _ = handle.enter();
        futures::executor::block_on(file.seek(pos))?;

        Ok(())
    }

    fn write_in_pos(&self) -> u64 {
        self.pos
    }

    fn write_out_pos(&self) -> u64 {
        self.pos
    }

    fn get_mut<'b>(&mut self) -> Arc<Mutex<dyn AsyncWriteWrapper>> {
        self.writer.clone()
    }
}

trait AsyncWriteWrapper: AsyncWrite + Unpin + Send {}
impl<T: AsyncWrite + Unpin + Send> AsyncWriteWrapper for T {}

struct AsyncWriterWrapper<'a> {
    id: String,
    path: String,
    zlib_state_file: std::fs::File,
    decoder: &'a mut Box<dyn DownloadDecoder>,
    inner: Arc<Mutex<dyn AsyncWriteWrapper>>,
}

impl<'a> AsyncWriterWrapper<'a> {
    async fn new(
        id: String,
        path: String,
        decoder: &'a mut Box<dyn DownloadDecoder>,
    ) -> Result<Self, DownloaderError> {
        let inner = decoder.get_mut();
        Ok(AsyncWriterWrapper {
            id: id.to_owned(),
            path: path.to_owned(),
            zlib_state_file: std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .open(zstate_path(&id, &path)?)?,
            decoder,
            inner,
        })
    }
}

impl<'a> AsyncWrite for AsyncWriterWrapper<'a> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> task::Poll<prelude::v1::Result<usize, io::Error>> {
        let poll_result = {
            let mut binding = self.inner.lock().unwrap();
            let inner = Pin::new(&mut *binding);
            inner.poll_write(cx, buf)
        };

        // State serialization is disabled for now.
        // let mut bytes = BytesMut::new();
        // self.decoder.save_state(&mut bytes);

        // self.zlib_state_file.seek(SeekFrom::Start(0))?;
        // self.zlib_state_file.write(&bytes)?;

        poll_result
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<prelude::v1::Result<(), io::Error>> {
        Pin::new(&mut *self.inner.lock().unwrap()).poll_flush(cx)
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<prelude::v1::Result<(), io::Error>> {
        Pin::new(&mut *self.inner.lock().unwrap()).poll_shutdown(cx)
    }
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("download failed ({0} bytes")]
    DownloadFailed(usize),
    #[error("failed to download chunk `{entry}`: {error}")]
    ChunkDownload {
        entry: String,
        error: reqwest::Error,
    },
    #[error("failed to copy chunk `{entry}`: {error}")]
    ChunkCopy {
        entry: String,
        error: std::io::Error,
    },
}

#[derive(PartialEq, Debug)]
enum EntryDownloadState {
    Fresh,
    Resumable,
    Complete,
    Borked,
}

struct DownloadContext {
    id: String,
    path: PathBuf,
}

type BytesDownloadedCallback = Box<dyn Fn(usize) + Send + Sync>;

struct EntryDownloadRequest<'a> {
    context: &'a DownloadContext,
    url: &'a str,
    entry: &'a ZipFileEntry,
    client: Client,
    decoder: Box<dyn DownloadDecoder>,
    callback: Option<BytesDownloadedCallback>,
}

impl<'a> EntryDownloadRequest<'a> {
    pub fn new(
        context: &'a DownloadContext,
        url: &'a str,
        entry: &'a ZipFileEntry,
        client: Client,
        decoder: Box<dyn DownloadDecoder>,
        callback: Option<BytesDownloadedCallback>,
    ) -> Self {
        Self {
            context,
            url,
            entry,
            client,
            decoder,
            callback,
        }
    }

    async fn state(
        context: &DownloadContext,
        entry: &ZipFileEntry,
    ) -> Result<EntryDownloadState, DownloaderError> {
        let path = context.path.join(entry.name());

        let file_size = File::open(&path).await?.metadata().await?.len() as i64;

        if file_size == 0 {
            return Ok(EntryDownloadState::Fresh);
        }

        let entry_size = *entry.uncompressed_size();
        let size_match = entry_size == file_size;

        if !size_match {
            warn!("Size mismatch: {}/{}", entry_size, file_size);
            if file_size > entry_size {
                return Ok(EntryDownloadState::Borked);
            }

            return Ok(EntryDownloadState::Borked);
        }

        // We must be calculating the hash incorrectly or something
        // let hash = match hash_file_crc32(&path) {
        //     Ok(hash) => hash,
        //     Err(_) => {
        //         warn!("Failed to retrieve hash for file {}", entry.name());
        //         0
        //     }
        // };

        // let hash_match = *entry.crc32() != hash;
        // if !hash_match {
        //     warn!("Hash mismatch");
        //     return EntryDownloadState::Borked;
        // }

        Ok(EntryDownloadState::Complete)
    }

    /// End is not inclusive
    pub async fn download_range(&mut self, start: i64, end: i64) -> Result<(), DownloaderError> {
        let offset = self.entry.data_offset();
        let range = format!("bytes={}-{}", offset + start as i64, offset + end - 1);

        let data = match self
            .client
            .get(self.url)
            .header("range", range)
            .send()
            .await
        {
            Ok(res) => res,
            Err(err) => {
                error!("Failed to download ({}): {}", self.entry.name(), err);
                return Err(DownloaderError::Download(DownloadError::ChunkDownload {
                    entry: self.entry.name().clone(),
                    error: err,
                }));
            }
        };

        let stream = data.bytes_stream();
        let counting_stream = ByteCountingStream::new(stream, self.callback.as_ref());
        let stream = counting_stream.into_async_read();
        let mut stream_reader = BufReader::new(stream.compat());

        // State deserialization is disabled for now.
        // let out_pos = self.decoder.write_out_pos();
        // self.decoder.seek(SeekFrom::Start(out_pos));

        let mut wrapper = AsyncWriterWrapper::new(
            self.context.id.to_owned(),
            self.entry.name().to_owned(),
            &mut self.decoder,
        )
        .await?;

        let result = tokio::io::copy(&mut stream_reader, &mut wrapper).await;
        if let Err(err) = result {
            return Err(DownloaderError::Download(DownloadError::ChunkCopy {
                entry: self.entry.name().clone(),
                error: err,
            }));
        }

        // Explicit `shutdown()` to flush the buffered writer chain
        // (BufWriter inside AsyncWriterWrapper → ZLibDeflateDecoder's
        // BufWriter<File>). `tokio::io::copy` doesn't flush on completion;
        // letting the wrapper drop is also not a flush (tokio's
        // AsyncWrite has no Drop-time flush guarantee). Without this,
        // the final buffered bytes — possibly the entire deflate
        // tail — never reach disk, and a "successful" file ends up
        // truncated. Gemini caught this on PR #19 review.
        use tokio::io::AsyncWriteExt;
        if let Err(err) = wrapper.shutdown().await {
            return Err(DownloaderError::Download(DownloadError::ChunkCopy {
                entry: self.entry.name().clone(),
                error: err,
            }));
        }

        Ok(())
    }
}

#[derive(Getters)]
pub struct ZipDownloader {
    id: String,
    url: String,
    path: PathBuf,
    client: Client,
    manifest: ZipFile,
}

impl ZipDownloader {
    pub async fn new<P: AsRef<Path>>(
        id: &str,
        zip_url: &str,
        path: P,
    ) -> Result<Self, DownloaderError>
    where
        PathBuf: From<P>,
    {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            return Err(DownloaderError::PathNotAbsolute(path));
        }

        let manifest = ZipFile::fetch(zip_url).await?;

        Ok(Self {
            id: id.to_owned(),
            url: zip_url.to_owned(),
            path,
            client: Client::builder().build()?,
            manifest,
        })
    }

    pub async fn read_zip_entry_bytes(
        &self,
        entry: &ZipFileEntry,
        length: u64,
    ) -> Result<Bytes, DownloaderError> {
        let offset = entry.data_offset();
        let compressed_size = *entry.compressed_size();

        let range_header = format!("bytes={}-{}", offset, offset + compressed_size - 1);

        let response = self
            .client
            .get(&self.url)
            .header("Range", range_header)
            .send()
            .await?;

        if !response.status().is_success()
            && response.status() != reqwest::StatusCode::PARTIAL_CONTENT
        {
            return Err(DownloaderError::Http(response.status()));
        }

        let compressed_data = response.bytes().await?;
        let decompressed_data = match entry.compression_type() {
            CompressionType::None => {
                let entry_size = *entry.uncompressed_size() as u64;
                let available_length = std::cmp::min(length, entry_size);

                if available_length > compressed_data.len() as u64 {
                    return Err(DownloaderError::EntrySize {
                        requested: available_length,
                        entry: compressed_data.len(),
                    });
                }

                Bytes::copy_from_slice(&compressed_data[..available_length as usize])
            }
            CompressionType::Deflate => {
                let mut decoder = BufreadDeflateDecoder::new(Cursor::new(&compressed_data));
                let mut limited_reader = decoder.take(length);
                let mut decompressed_data = Vec::with_capacity(length as usize);
                limited_reader.read_to_end(&mut decompressed_data)?;

                Bytes::from(decompressed_data)
            }
            any => {
                return Err(DownloaderError::CompressionType(any.to_owned()));
            }
        };

        Ok(decompressed_data)
    }

    pub async fn download_single_file(
        &self,
        entry: &ZipFileEntry,
        callback: Option<BytesDownloadedCallback>,
        // Live per-chunk display counter (see GameDownloader::live_bytes). Advances
        // as bytes stream, rolled back per failed non-resumable attempt so it never
        // over-counts. Separate from `callback`, which commits whole files on
        // success for the retry-safe `is_done()` total. None for one-off callers.
        live_bytes: Option<Arc<AtomicUsize>>,
    ) -> Result<usize, DownloaderError> {
        let file_path = self.path.join(entry.name());

        // Directory entry / parent-not-yet-created handling.
        // `tokio::fs::try_exists` instead of sync `Path::exists` —
        // we're hot in the `buffer_unordered(16)` install loop and
        // a sync `stat()` blocks the runtime worker thread. Gemini
        // caught this on PR #19 review.
        if !tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
            let parent = file_path.safe_parent()?;
            if !tokio::fs::try_exists(&parent).await.unwrap_or(false) {
                create_dir_all(&parent).await?;
            }

            if entry.name().ends_with("/")
                && !tokio::fs::try_exists(&file_path).await.unwrap_or(false)
            {
                debug!("{} is a directory", entry.name());
                create_dir(file_path).await?;
                return Ok(0);
            }
        }

        if *entry.uncompressed_size() == 0 {
            debug!("{} is empty", entry.name());
            return Ok(0);
        }

        let context = DownloadContext {
            id: self.id.to_owned(),
            path: self.path.clone(),
        };

        // Pre-flight: is the file already on disk at the right size?
        // (Re-running the same install over a previously-completed
        // dir should short-circuit.) `state()` opens the file read-only,
        // so this works even before the retry loop opens the writer.
        if tokio::fs::try_exists(&file_path).await.unwrap_or(false) {
            if let Ok(EntryDownloadState::Complete) =
                EntryDownloadRequest::state(&context, entry).await
            {
                if let Some(cb) = callback {
                    cb(*entry.compressed_size() as usize);
                }
                // Already-complete file (re-run over an existing dir): count it
                // toward the live display total too, else the bar under-reports.
                if let Some(lb) = &live_bytes {
                    lb.fetch_add(*entry.compressed_size() as usize, Ordering::SeqCst);
                }
                return Ok(0);
            }
        }

        // Retry loop. Each attempt opens a fresh file (truncated) and
        // a fresh decoder. Why all this ceremony per attempt:
        //
        //   1. **Decoder poisoning** — `ZLibDeflateDecoder` holds zlib
        //      stream state. When attempt N fails mid-stream, the
        //      decoder has consumed partial bytes. Reusing it for
        //      attempt N+1 feeds a fresh deflate stream into a
        //      poisoned decoder → "invalid stored block lengths"
        //      errors that look like CDN corruption but are really
        //      our own state-machine bug. Discovered while debugging
        //      `r2/sound/general_stream_patch_2.mstr` failing on
        //      attempts 2-5 with consistent deflate errors after a
        //      first-attempt `IncompleteBody`.
        //
        //   2. **File not truncated** — without `.truncate(true)`,
        //      partial bytes from attempt N stay on disk past where
        //      attempt N+1's writer stops. Even when the decode
        //      succeeded, the file would carry trailing garbage.
        //
        //   3. **Bytes over-counting** — the `BytesDownloadedCallback`
        //      fires on every chunk via `ByteCountingStream`, regardless
        //      of whether the attempt eventually succeeds. With the
        //      caller's counter being incremented from every retry's
        //      partial bytes, a single 6-retry file pushed `completed_bytes`
        //      well past `total_bytes`. Combined with `is_done() == `
        //      (also fixed in this PR), this kept installs hung forever.
        //      Fix: per-attempt local counter, committed to the caller's
        //      callback only when the attempt succeeds.
        const MAX_RETRIES: u32 = 5;
        let end = *entry.compressed_size();
        let mut last_err: Option<DownloaderError> = None;

        // Uncompressed (stored) entries can be RESUMED: the bytes already on disk
        // are a valid prefix of the final file, so a dropped connection only needs
        // the missing tail re-fetched, not the whole file. This is essential for
        // the multi-GB `.cas` archives — on a flaky link they never finish in one
        // unbroken transfer, and the old truncate+restart-from-0 loop pins the
        // install at ~0% forever (each attempt dies ~250MB into a 1GB file and
        // starts over). Deflate entries CANNOT resume mid-stream: the zlib decoder
        // carries stream state, and a fresh decoder fed a mid-stream byte offset
        // produces garbage (the decoder-poisoning failure documented above), so
        // they still restart from 0 each attempt.
        let resumable = matches!(entry.compression_type(), CompressionType::None);

        for attempt in 0..=MAX_RETRIES {
            // Resume point: for a resumable entry, keep the on-disk prefix and
            // fetch only [resume_from, end). Re-read the size every attempt so a
            // partial retry advances the floor instead of throwing it away. 0 for
            // Deflate (and for a missing/complete file) → full from-scratch fetch.
            let resume_from: i64 = if resumable {
                match tokio::fs::metadata(&file_path).await {
                    Ok(m) if (m.len() as i64) < end => m.len() as i64,
                    _ => 0,
                }
            } else {
                0
            };

            // Seed the live display counter with a pre-existing on-disk prefix on
            // the FIRST attempt only: those bytes (from a prior run/session) never
            // streamed through the per-chunk callback, so without this the bar
            // under-reports a resumed install. Later attempts' resume_from grew from
            // THIS session's streamed bytes, already counted live — don't re-add.
            if attempt == 0 && resume_from > 0 {
                if let Some(lb) = &live_bytes {
                    lb.fetch_add(resume_from as usize, Ordering::SeqCst);
                }
            }

            // From-scratch attempt truncates (clears any partial bytes); a resume
            // opens in append mode so existing bytes stay and new bytes extend the
            // file. `.append(true)` forces every write to EOF — exactly the
            // sequential tail the ranged request delivers.
            let file = if resume_from > 0 {
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .append(true)
                    .open(&file_path)
                    .await?
            } else {
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .open(&file_path)
                    .await?
            };
            let writer = tokio::io::BufWriter::new(file);

            // Fresh decoder — never reuse one that already consumed
            // partial bytes from a failed attempt.
            let decoder: Box<dyn DownloadDecoder> = match entry.compression_type() {
                CompressionType::None => Box::new(NoopDecoder::new(writer)),
                CompressionType::Deflate => Box::new(ZLibDeflateDecoder::new(writer)),
            };

            // Per-attempt byte counter. Only commits to the caller's
            // shared counter on SUCCESS — failed attempts' streamed
            // bytes don't pollute `completed_bytes`.
            let attempt_committed = Arc::new(AtomicUsize::new(0));
            let attempt_committed_cb = attempt_committed.clone();
            // Live display counter also ticks per chunk so the progress bar moves
            // in real time. Rolled back below if a non-resumable attempt fails.
            let live_for_chunk = live_bytes.clone();
            let attempt_callback: Option<BytesDownloadedCallback> = Some(Box::new(move |bytes| {
                attempt_committed_cb.fetch_add(bytes, Ordering::SeqCst);
                if let Some(lb) = &live_for_chunk {
                    lb.fetch_add(bytes, Ordering::SeqCst);
                }
            }));

            let mut request = EntryDownloadRequest::new(
                &context,
                &self.url,
                entry,
                self.client.clone(),
                decoder,
                attempt_callback,
            );

            debug!(
                "Downloading {} (compressed={}, uncompressed={}) — attempt {}/{}{}",
                entry.name(),
                entry.compressed_size(),
                entry.uncompressed_size(),
                attempt + 1,
                MAX_RETRIES + 1,
                if resume_from > 0 {
                    format!(" (resuming from {resume_from})")
                } else {
                    String::new()
                },
            );

            match request.download_range(resume_from, end).await {
                Ok(()) => {
                    // Successful attempt — commit the FULL file's bytes once to
                    // the caller's counter: the prefix carried over from earlier
                    // attempts (resume_from) plus this attempt's new bytes.
                    // Earlier failed attempts committed nothing, so this is the
                    // file's only contribution to completed_bytes.
                    if let Some(cb) = callback.as_ref() {
                        cb(attempt_committed.load(Ordering::SeqCst) + resume_from as usize);
                    }
                    return Ok(0);
                }
                Err(err) => {
                    // Roll this attempt's live-display bytes back out for a
                    // NON-resumable (deflate) entry: the next attempt truncates and
                    // restarts from 0 (or, on final failure, the file is incomplete),
                    // so those streamed bytes must not linger in the display counter
                    // or it drifts past the real total. Resumable entries KEEP their
                    // bytes — they stay on disk as the next attempt's resume point,
                    // and that attempt only streams (and counts) the remaining tail.
                    if !resumable {
                        if let Some(lb) = &live_bytes {
                            lb.fetch_sub(attempt_committed.load(Ordering::SeqCst), Ordering::SeqCst);
                        }
                    }
                    if attempt < MAX_RETRIES {
                        // Exponential backoff with jitter (500ms / 1s /
                        // 2s / 4s / 8s base, +0-250ms jitter).
                        let base_ms = 500u64.saturating_mul(1 << attempt);
                        let jitter_ms = rand::random::<u64>() % 250;
                        let delay = Duration::from_millis(base_ms + jitter_ms);
                        warn!(
                            "{} attempt {}/{} failed ({}); retrying in {:?}",
                            entry.name(),
                            attempt + 1,
                            MAX_RETRIES + 1,
                            err,
                            delay,
                        );
                        last_err = Some(err);
                        tokio::time::sleep(delay).await;
                    } else {
                        // Final attempt failed — propagate so the
                        // install flow can surface it (and so the
                        // caller's `completed_bytes` stays accurate
                        // for files we genuinely couldn't download).
                        error!(
                            "{} failed after {} attempts: {}",
                            entry.name(),
                            MAX_RETRIES + 1,
                            err,
                        );
                        last_err = Some(err);
                    }
                }
            }
        }

        Err(last_err.expect("last_err always set on failure path"))
    }
}

struct ByteCountingStream<'a, S> {
    inner: S,
    byte_count: usize,
    callback: Option<&'a BytesDownloadedCallback>,
}

impl<'a, S> ByteCountingStream<'a, S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>>,
{
    fn new(inner: S, callback: Option<&'a BytesDownloadedCallback>) -> Self {
        ByteCountingStream {
            inner,
            byte_count: 0,
            callback,
        }
    }

    fn byte_count(&self) -> usize {
        self.byte_count
    }
}

impl<'a, S> Stream for ByteCountingStream<'a, S>
where
    S: Stream<Item = Result<bytes::Bytes, reqwest::Error>> + Unpin,
{
    type Item = Result<bytes::Bytes, tokio::io::Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.inner.poll_next_unpin(cx) {
            std::task::Poll::Ready(Some(Ok(chunk))) => {
                self.byte_count += chunk.len();

                if let Some(callback) = &self.callback {
                    callback(chunk.len());
                }

                std::task::Poll::Ready(Some(Ok(chunk)))
            }
            std::task::Poll::Ready(Some(Err(err))) => {
                error!("Downloader error: {:?}", err);
                std::task::Poll::Ready(Some(Err(futures::io::Error::other(
                    DownloadError::DownloadFailed(self.byte_count),
                ))))
            }
            std::task::Poll::Ready(None) => std::task::Poll::Ready(None),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}
