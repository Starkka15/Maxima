use egui::{ColorImage, TextureHandle, TextureOptions};
use image::{DynamicImage, ImageError, ImageResult};
use std::{
    collections::HashMap,
    fmt::Display,
    fs,
    path::PathBuf,
    sync::{
        mpsc::{Receiver, Sender},
        Arc, Mutex,
    },
};
use tokio::{fs::File, io};

use core::slice::SlicePattern;
use log::{debug, error, info};

use image::io::Reader as ImageReader;
use maxima::util::native::{maxima_dir, NativeError, SafeStr};

#[derive(Clone, PartialEq, Eq, Hash, std::fmt::Debug)]
pub enum UIImageType {
    Hero(String),
    Logo(String),
    Background(String),
    Avatar(String),
}

impl Display for UIImageType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                UIImageType::Hero(slug) => format!("`{}`'s hero", slug),
                UIImageType::Logo(slug) => format!("`{}`'s logo", slug),
                UIImageType::Background(slug) => format!("`{}`'s background", slug),
                UIImageType::Avatar(id) => format!("`{}`'s avatar", id),
            }
        )
    }
}

pub struct UIImageCache {
    cache: Arc<Mutex<HashMap<UIImageType, Option<TextureHandle>>>>, // none represents loading, lack of represents untouched
    commander: Sender<UIImageCacheLoaderCommand>,
    pub placeholder_avatar: TextureHandle,
}

pub enum UIImageCacheLoaderCommand {
    ProvideRemote(UIImageType, String),
    Load(UIImageType),
    /// Force an image to never load (like for games that don't have logos)
    Stub(UIImageType),
}

pub fn load_image_bytes(image_bytes: &[u8]) -> ImageResult<ColorImage> {
    let image = image::load_from_memory(image_bytes)?;
    let size = [image.width() as _, image.height() as _];
    let image_buffer = image.to_rgba8();
    let pixels = image_buffer.as_flat_samples();
    Ok(ColorImage::from_rgba_unmultiplied(size, pixels.as_slice()))
}

#[derive(thiserror::Error, Debug)]
pub enum ImageLoadError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Image(#[from] ImageError),

    #[error("{path} had an unsupported amount of channels ({channels})")]
    UnsupportedChannelCount { path: PathBuf, channels: u8 },
}

impl UIImageCache {
    pub fn new(ctx: egui::Context) -> (Self, Sender<UIImageCacheLoaderCommand>) {
        let cache = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = std::sync::mpsc::channel::<UIImageCacheLoaderCommand>();
        let tx1 = tx.clone();

        let yeah = load_image_bytes(include_bytes!("../res/usericon_tmp.png")).unwrap();
        let avatar = ctx.load_texture("Placeholder Avatar", yeah, TextureOptions::LINEAR);
        let send_cache = cache.clone();
        tokio::task::spawn(async move {
            UIImageCache::run(ctx, rx, send_cache).await;
        });

        (
            Self {
                cache,
                commander: tx,
                placeholder_avatar: avatar,
            },
            tx1,
        )
    }

    fn get_path_for_image(variant: &UIImageType) -> Result<PathBuf, NativeError> {
        let image_cache_root = maxima_dir()?.join("cache/ui/images");
        // lib should probably create the pfp path but we'll check both just in case we're first
        let avatar_cache_root = maxima_dir()?.join("cache/avatars");
        match variant {
            UIImageType::Hero(slug) => Ok(image_cache_root.join(&slug).join("hero.jpg")),
            UIImageType::Logo(slug) => Ok(image_cache_root.join(&slug).join("logo.png")),
            UIImageType::Background(slug) => {
                Ok(image_cache_root.join(&slug).join("background.jpg"))
            }
            UIImageType::Avatar(uid) => {
                // avatars can be both jpeg and png
                let png_cache = avatar_cache_root.join(uid.clone() + "_208x208.png");
                let jpeg_cache = avatar_cache_root.join(uid.clone() + "_208x208.jpg");
                if fs::metadata(&jpeg_cache).is_ok() {
                    Ok(jpeg_cache)
                } else {
                    Ok(png_cache)
                }
            }
        }
    }

    async fn load(
        needle: UIImageType,
        cache: Arc<Mutex<HashMap<UIImageType, Option<TextureHandle>>>>,
        remotes: HashMap<UIImageType, String>,
        context: egui::Context,
    ) -> Result<(), ImageLoadError> {
        let path = UIImageCache::get_path_for_image(&needle)?;

        if !path.exists() {
            debug!("{:?} ({:?}) doesn't exist, downloading", &needle, &path);
            if let Some(parent) = &path.parent() {
                // not sure why it *wouldn't* have a parent but i'm just being safe
                // i don't have a way to catch-all the slugs atm, so this is the better solution, it's infrequent and a non-ui thread anyway
                if !fs::metadata(&parent).is_ok() {
                    let res = fs::create_dir_all(&parent)?;
                }
            }
            if let Some(remote) = remotes.get(&needle) {
                let result = reqwest::get(remote).await?;

                let mut file = File::create(&path).await?;

                let body = result.bytes().await?;
                io::copy(&mut body.as_slice(), &mut file).await?;

                let ci = load_image_bytes(&body)?;
                cache.lock().unwrap().insert(
                    needle,
                    Some(context.load_texture(
                        path.safe_str()?.to_string(),
                        ci,
                        TextureOptions::LINEAR,
                    )),
                );
            } else {
                debug!("{:?} has no remote or local file", needle);
                return Ok(());
            }
        } else {
            let img = ImageReader::open(&path)?.with_guessed_format()?;
            let img_decoded = img.decode()?;

            let color_image = match img_decoded.color().channel_count() {
                2 => {
                    let img_a = DynamicImage::ImageRgba8(img_decoded.into_rgba8());
                    ColorImage::from_rgba_unmultiplied(
                        [img_a.width() as usize, img_a.height() as usize],
                        img_a.as_bytes(),
                    )
                }
                3 => ColorImage::from_rgb(
                    [img_decoded.width() as usize, img_decoded.height() as usize],
                    img_decoded.as_bytes(),
                ),
                4 => ColorImage::from_rgba_unmultiplied(
                    [img_decoded.width() as usize, img_decoded.height() as usize],
                    img_decoded.as_bytes(),
                ),
                channels => {
                    return Err(ImageLoadError::UnsupportedChannelCount { path, channels });
                }
            };

            cache.lock().unwrap().insert(
                needle,
                Some(context.load_texture(
                    path.safe_str()?.to_string(),
                    color_image,
                    TextureOptions::LINEAR,
                )),
            );
        }
        context.request_repaint();
        Ok(())
    }

    async fn run(
        context: egui::Context,
        commander: Receiver<UIImageCacheLoaderCommand>,
        cache: Arc<Mutex<HashMap<UIImageType, Option<TextureHandle>>>>,
    ) {
        let mut remotes: HashMap<UIImageType, String> = HashMap::new();

        'outer: loop {
            match commander.try_recv() {
                Err(error) => {
                    if error == std::sync::mpsc::TryRecvError::Disconnected {
                        break 'outer;
                    }
                }
                Ok(request) => match request {
                    UIImageCacheLoaderCommand::ProvideRemote(needle, target) => {
                        debug!("remote provided for {:?}", &needle);
                        // undoes a race condition, ui's fast so it can get to the images before the backend can
                        let mut cache = cache.lock().unwrap();
                        if let Some(test_none) = cache.get(&needle) {
                            if test_none.is_none() {
                                cache.remove(&needle);
                            }
                        }
                        remotes.insert(needle, target);
                    }
                    UIImageCacheLoaderCommand::Load(needle) => {
                        // this might cause some slowdown, whoops!
                        let ctx_send = context.clone();
                        let remotes_send = remotes.clone();
                        let cache_send = cache.clone();

                        tokio::task::spawn(async move {
                            match UIImageCache::load(
                                needle.clone(),
                                cache_send,
                                remotes_send,
                                ctx_send,
                            )
                            .await
                            {
                                Ok(_) => {
                                    debug!("finished async load of {:?}", &needle);
                                }
                                Err(err) => {
                                    error!("async load of {} failed: {:?}", &needle, err);
                                }
                            }
                        });
                        tokio::task::yield_now().await;
                    }
                    UIImageCacheLoaderCommand::Stub(needle) => {
                        cache.lock().unwrap().insert(needle, None);
                    }
                },
            }
        }
        info!("Shutting down image loader thread");
    }

    pub fn get(&self, needle: UIImageType) -> Option<TextureHandle> {
        // i'm hardly building this in a performant way but it's robust and solid unlike the previous mess
        let mut cache = self.cache.lock().unwrap();
        if let Some(loading_or_loaded) = cache.get(&needle) {
            if let Some(loaded) = loading_or_loaded {
                Some(loaded.clone())
            } else {
                None
            }
        } else {
            cache.insert(needle.clone(), None);
            let _ = self.commander.send(UIImageCacheLoaderCommand::Load(needle));
            None
        }
    }
}
