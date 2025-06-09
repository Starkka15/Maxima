use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use derive_builder::Builder;
use derive_getters::Getters;
use futures::StreamExt;
use log::{debug, error, info};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{fs, sync::Notify};
use tokio_util::sync::CancellationToken;

use crate::{
    content::{
        downloader::{DownloadError, ZipDownloader},
        zip::{self, CompressionType, ZipError, ZipFileEntry},
        ContentService,
    },
    core::{
        auth::storage::LockedAuthStorage,
        manifest::{self, ManifestError, MANIFEST_RELATIVE_PATH},
        service_layer::ServiceLayerError,
        MaximaEvent,
    },
    util::native::{maxima_dir, NativeError},
};

const QUEUE_FILE: &str = "download_queue.json";

#[derive(Default, Builder, Getters, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueuedGame {
    offer_id: String,
    build_id: String,
    path: PathBuf,
}

#[derive(Default, Getters, Serialize, Deserialize)]
pub struct DownloadQueue {
    current: Option<QueuedGame>,
    paused: bool,

    queued: Vec<QueuedGame>,
    completed: Vec<QueuedGame>,
}

#[derive(Error, Debug)]
pub enum ContentManagerError {
    #[error(transparent)]
    Downloader(#[from] DownloaderError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error("download in progress, you must cancel it before starting a new one")]
    DownloadInProgress,
}

#[derive(Error, Debug)]
pub enum DownloaderError {
    #[error(transparent)]
    ServiceLayer(#[from] ServiceLayerError),
    #[error(transparent)]
    Zip(#[from] ZipError),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error("path `{0}` is not absolute")]
    PathNotAbsolute(PathBuf),
    #[error("failed to download range: {0}")]
    Http(StatusCode),
    #[error("requested length ({requested}) exceeds entry size ({entry})")]
    EntrySize { requested: u64, entry: usize },
    #[error("unsupported compression type `{0:?}`")]
    CompressionType(CompressionType),
}

impl DownloadQueue {
    pub(crate) async fn load() -> Result<DownloadQueue, ContentManagerError> {
        let file = maxima_dir()?.join(QUEUE_FILE);
        if !file.exists() {
            return Ok(Self::default());
        }

        let data = fs::read_to_string(file).await?;
        let result = serde_json::from_str(&data);
        if result.is_err() {
            return Ok(Self::default());
        }

        Ok(result?)
    }

    pub(crate) async fn save(&self) -> Result<(), ContentManagerError> {
        let file = maxima_dir()?.join(QUEUE_FILE);
        fs::write(file, serde_json::to_string(&self)?).await?;
        Ok(())
    }

    pub fn push_to_current(&mut self, game: QueuedGame) {
        if let Some(current) = &self.current {
            self.queued.push(current.clone());
        }

        self.current = Some(game.clone());
    }
}

pub struct GameDownloader {
    offer_id: String,

    downloader: Arc<ZipDownloader>,
    entries: Vec<ZipFileEntry>,

    cancel_token: CancellationToken,
    completed_bytes: Arc<AtomicUsize>,
    total_count: usize,
    total_bytes: usize,
    notify: Arc<Notify>,
}

impl GameDownloader {
    pub async fn new(
        content_service: &ContentService,
        game: &QueuedGame,
    ) -> Result<Self, DownloaderError> {
        let url = content_service
            .download_url(&game.offer_id, Some(&game.build_id))
            .await?;

        debug!("URL: {}", url.url());

        let downloader = ZipDownloader::new(&game.offer_id, &url.url(), &game.path).await?;

        let mut entries = Vec::new();
        for ele in downloader.manifest().entries() {
            // TODO: Filtering
            entries.push(ele.clone());
        }

        let total_count = entries.len();
        let total_bytes = entries
            .iter()
            .map(|x| *x.compressed_size() as usize)
            .sum::<usize>()
            + 1; // Add 1 to account for running touchup at the end. Bad solution, but we're a bit rushed

        Ok(GameDownloader {
            offer_id: game.offer_id.to_owned(),

            downloader: Arc::new(downloader),
            entries,
            cancel_token: CancellationToken::new(),
            completed_bytes: Arc::new(AtomicUsize::new(0)),
            total_count,
            total_bytes,
            notify: Arc::new(Notify::new()),
        })
    }

    pub fn download(&self) {
        let (downloader_arc, entries, cancel_token, completed_bytes, notify) =
            self.prepare_download_vars();
        let total_count = self.total_count;
        tokio::spawn(async move {
            let dl = GameDownloader::start_downloads(
                total_count,
                downloader_arc,
                entries,
                cancel_token,
                completed_bytes,
                notify,
            )
            .await;
            if let Err(err) = dl {
                error!("Error when downloading!: `{:?}", err)
            }
        });
    }

    fn prepare_download_vars(
        &self,
    ) -> (
        Arc<ZipDownloader>,
        Vec<ZipFileEntry>,
        CancellationToken,
        Arc<AtomicUsize>,
        Arc<Notify>,
    ) {
        (
            self.downloader.clone(),
            self.entries.clone(),
            self.cancel_token.clone(),
            self.completed_bytes.clone(),
            self.notify.clone(),
        )
    }

    async fn start_downloads(
        total_count: usize,
        downloader_arc: Arc<ZipDownloader>,
        entries: Vec<ZipFileEntry>,
        cancel_token: CancellationToken,
        completed_bytes: Arc<AtomicUsize>,
        notify: Arc<Notify>,
    ) -> Result<(), DownloaderError> {
        let mut handles = Vec::with_capacity(total_count);

        for i in 0..total_count {
            let downloader = downloader_arc.clone();
            let ele = entries[i].clone();

            let cancel_token = cancel_token.clone();
            let completed_bytes = completed_bytes.clone();

            handles.push(async move {
                if ele.name().contains("Cleanup") {
                    info!("Ele: {:?}", ele);
                }

                tokio::select! {
                    result = downloader.download_single_file(&ele, Some(Box::new(move |bytes| {
                        completed_bytes.fetch_add(bytes, Ordering::SeqCst);
                    }))) => {
                        if let Err(err) = result {
                            error!("File download failed: {}", err);
                        }
                    },
                    _ = cancel_token.cancelled() => {
                        info!("Download of {} cancelled", ele.name());
                    },
                }
            });
        }

        let _results = futures::stream::iter(handles)
            .buffer_unordered(16)
            .collect::<Vec<_>>()
            .await;

        let path = downloader_arc.path();

        info!("Files downloaded, running touchup...");
        let manifest = manifest::read(path.join(MANIFEST_RELATIVE_PATH)).await?;

        manifest.run_touchup(path).await?;
        info!("Installation finished!");

        completed_bytes.fetch_add(1, Ordering::SeqCst);

        notify.notify_one();
        Ok(())
    }

    pub fn cancel(&self) {
        info!("Pausing installation of {}", self.offer_id);
        self.cancel_token.cancel();
    }

    pub async fn wait(&self) {
        self.notify.notified().await;
    }

    pub fn is_done(&self) -> bool {
        self.completed_bytes.load(Ordering::SeqCst) == self.total_bytes
    }

    pub fn percentage_done(&self) -> f64 {
        let completed = self.completed_bytes.load(Ordering::SeqCst);
        (completed as f64 / self.total_bytes as f64) * 100.0
    }

    pub fn bytes_downloaded(&self) -> usize {
        self.completed_bytes.load(Ordering::SeqCst)
    }

    pub fn bytes_total(&self) -> usize {
        self.total_bytes
    }

    pub fn offer_id(&self) -> &String {
        &self.offer_id
    }
}

#[derive(Getters)]
pub struct ContentManager {
    queue: DownloadQueue,
    service: ContentService,
    current: Option<GameDownloader>,
}

impl ContentManager {
    pub async fn new(auth: LockedAuthStorage, _resume: bool) -> Result<Self, ContentManagerError> {
        Ok(Self {
            queue: DownloadQueue::load().await?,
            service: ContentService::new(auth),
            current: None,
        })
    }

    pub async fn add_install(&mut self, game: QueuedGame) -> Result<(), ContentManagerError> {
        if self.queue.queued.is_empty() && self.queue.current == None && self.current.is_none() {
            self.install_now(game).await?;
        } else {
            self.queue.queued.push(game);
            self.queue.save().await?;
        }

        Ok(())
    }

    pub async fn install_now(&mut self, game: QueuedGame) -> Result<(), ContentManagerError> {
        if let Some(current) = &self.current {
            current.cancel();
            self.current = None;
        }

        if let Some(current) = &self.queue.current {
            if current == &game {
                self.install_direct(game).await?;
                return Ok(());
            }

            self.queue.queued.push(current.clone());
        }

        self.install_direct(game).await?;
        Ok(())
    }

    async fn install_direct(&mut self, game: QueuedGame) -> Result<(), ContentManagerError> {
        if self.current.is_some() {
            return Err(ContentManagerError::DownloadInProgress);
        }

        self.queue.current = Some(game.clone());
        self.queue.save().await?;

        let downloader = GameDownloader::new(&self.service, &game).await?;
        downloader.download();
        self.current = Some(downloader);
        Ok(())
    }

    pub(crate) async fn update(&mut self) -> Result<Option<MaximaEvent>, ContentManagerError> {
        let mut event = None;

        if let Some(current) = &self.current {
            if current.is_done() {
                event = Some(MaximaEvent::InstallFinished(current.offer_id.to_owned()));
                self.current = None;
                self.queue.current = None;

                if let Some(game) = self.queue.queued.pop() {
                    self.install_now(game).await?;
                }

                self.queue.save().await?;
            }
        }

        Ok(event)
    }
}
