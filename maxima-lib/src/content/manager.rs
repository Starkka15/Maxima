use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use anyhow::{bail, Result};
use derive_builder::Builder;
use derive_getters::Getters;
use futures::StreamExt;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use tokio::{fs, sync::Notify};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use crate::core::launch::mx_linux_setup;

use crate::{
    core::{
        auth::storage::LockedAuthStorage,
        dip::{DiPManifest, DIP_RELATIVE_PATH},
        MaximaEvent,
    },
    util::native::maxima_dir,
};

use super::{downloader::ZipDownloader, ContentService};

const QUEUE_FILE: &str = "download_queue.json";

#[derive(Default, Builder, Clone, Serialize, Deserialize, PartialEq)]
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

impl DownloadQueue {
    pub(crate) async fn load() -> Result<DownloadQueue> {
        let file = maxima_dir()?.join(QUEUE_FILE);
        if !file.exists() {
            return Ok(Self::default());
        }

        let data = fs::read_to_string(file).await?;
        let result = serde_json::from_str(&data);
        if result.is_err() {
            return Ok(Self::default());
        }

        Ok(result.unwrap())
    }

    pub(crate) async fn save(&self) -> Result<()> {
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
    cancel_token: CancellationToken,
    completed_bytes: Arc<AtomicUsize>,
    total_count: usize,
    total_bytes: usize,
    notify: Arc<Notify>,
}

impl GameDownloader {
    pub async fn new(content_service: &ContentService, game: &QueuedGame) -> Result<Self> {
        let url = content_service
            .download_url(&game.offer_id, Some(&game.build_id))
            .await?;

        debug!("URL: {}", url.url());

        let downloader = ZipDownloader::new(&game.offer_id, &url.url(), &game.path).await?;

        let total_count = downloader.manifest().entries().len();
        let total_bytes = downloader
            .manifest()
            .entries()
            .iter()
            .map(|x| *x.uncompressed_size() as usize)
            .sum();

        Ok(GameDownloader {
            offer_id: game.offer_id.to_owned(),

            downloader: Arc::new(downloader),
            cancel_token: CancellationToken::new(),
            completed_bytes: Arc::new(AtomicUsize::new(0)),
            total_count,
            total_bytes,
            notify: Arc::new(Notify::new()),
        })
    }

    pub fn download(&self) {
        let (downloader_arc, cancel_token, completed_bytes, notify) = self.prepare_download_vars();
        let total_count = self.total_count + 1; // Add 1 to account for running touchup at the end. Bad solution, but we're a bit rushed
        tokio::spawn(async move {
            GameDownloader::start_downloads(
                total_count,
                downloader_arc,
                cancel_token,
                completed_bytes,
                notify,
            )
            .await;
        });
    }

    fn prepare_download_vars(
        &self,
    ) -> (
        Arc<ZipDownloader>,
        CancellationToken,
        Arc<AtomicUsize>,
        Arc<Notify>,
    ) {
        (
            self.downloader.clone(),
            self.cancel_token.clone(),
            self.completed_bytes.clone(),
            self.notify.clone(),
        )
    }

    async fn start_downloads(
        total_count: usize,
        downloader_arc: Arc<ZipDownloader>,
        cancel_token: CancellationToken,
        completed_bytes: Arc<AtomicUsize>,
        notify: Arc<Notify>,
    ) {
        let mut handles = Vec::with_capacity(total_count);

        for i in 0..total_count - 1 {
            let downloader = downloader_arc.clone();
            let cancel_token = cancel_token.clone();
            let completed_bytes = completed_bytes.clone();
            let notify = notify.clone();

            handles.push(async move {
                let ele = &downloader.manifest().entries()[i];
                let size = *ele.uncompressed_size() as usize;

                tokio::select! {
                    result = downloader.download_single_file(ele) => {
                        result.unwrap();
                        completed_bytes.fetch_add(size, Ordering::SeqCst);
                        notify.notify_one();
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

        #[cfg(unix)]
        mx_linux_setup().await.unwrap();

        info!("Files downloaded, running touchup...");
        let manifest = DiPManifest::read(&path.join(DIP_RELATIVE_PATH))
            .await
            .unwrap();

        manifest.run_touchup(path).await.unwrap();
        info!("Installation finished!");

        completed_bytes.fetch_add(1, Ordering::SeqCst);
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
    pub async fn new(auth: LockedAuthStorage, _resume: bool) -> Result<Self> {
        Ok(Self {
            queue: DownloadQueue::load().await?,
            service: ContentService::new(auth),
            current: None,
        })
    }

    pub async fn add_install(&mut self, game: QueuedGame) -> Result<()> {
        if self.queue.queued.is_empty() && self.queue.current == None && self.current.is_none() {
            self.install_now(game).await?;
        } else {
            self.queue.queued.push(game);
            self.queue.save().await.unwrap();
        }

        Ok(())
    }

    pub async fn install_now(&mut self, game: QueuedGame) -> Result<()> {
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

    async fn install_direct(&mut self, game: QueuedGame) -> Result<()> {
        if let Some(current) = &self.current {
            bail!(
                "Download in progress - {}. You must cancel it before starting a new one",
                current.offer_id
            );
        }

        self.queue.current = Some(game.clone());
        self.queue.save().await.unwrap();

        let downloader = GameDownloader::new(&self.service, &game).await?;
        downloader.download();
        self.current = Some(downloader);
        Ok(())
    }

    pub(crate) async fn update(&mut self) -> Result<Option<MaximaEvent>> {
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
