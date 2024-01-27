use egui::{Context, TextureId, Vec2};
use egui_extras::RetainedImage;
use std::{fs, path::PathBuf, sync::Arc};
use tokio::fs::File;
use tokio::io;

use anyhow::{bail, Result};
use core::slice::SlicePattern;
use log::{debug, error, info};
use std::result::Result::Ok;

use crate::ImageLoader;
use maxima::util::native::maxima_dir;

#[derive(Clone)]
pub struct UIImage {
    /// Holds the actual texture data
    _retained: Arc<RetainedImage>,
    /// Pass to egui to render
    pub renderable: TextureId,
    /// width and height of the image, in pixels
    pub size: Vec2,
}

#[derive(Clone, PartialEq)]
pub enum GameImageType {
    Hero,
    Logo,
}

pub async fn download_image(url: String, file_name: &PathBuf) -> Result<()> {
    info!("Downloading image at {:?}", &url);
    let result = reqwest::get(&url).await;
    if result.is_err() {
        bail!("Failed to download {}! Reason: {:?}", &url, &result);
    }

    let body = result?.bytes().await?;
    let file = File::create(&file_name).await;
    if file.is_err() {
        bail!("Failed to create {:?}! Reason: {:?}", &file_name, &file);
    }

    if let Err(err) = io::copy(&mut body.as_slice(), &mut file?).await {
        error!("Failed to copy file! Reason: {:?}", err)
    }

    debug!("Copied file!");
    Ok(())
}

impl UIImage {
    pub async fn load(
        slug: String,
        diff: GameImageType,
        url: Option<String>,
        ctx: Context,
    ) -> Result<UIImage> {
        let cache_folder = maxima_dir().unwrap().join("cache/ui/images").join(&slug);
        let file_name = match diff {
            GameImageType::Hero => cache_folder.join("hero.jpg"),
            GameImageType::Logo => cache_folder.join("logo.png"),
        };

        if !fs::metadata(&cache_folder).is_ok() {
            // folder is missing
            let res = fs::create_dir_all(&cache_folder);
            if res.is_err() {
                error!("Failed to create directory {:?}", &cache_folder);
            }
        }

        if fs::metadata(&file_name).is_err() {
            //image hasn't been cached yet
            if url.is_none() {
                bail!(
                    "file does not exist on disk, and a URL was not provided to retrieve it from!"
                );
            }

            download_image(url.unwrap(), &file_name).await?;
        }

        let fs_load = ImageLoader::load_from_fs(&file_name.to_str().unwrap());
        if fs_load.is_ok() {
            let img = fs_load?;
            Ok(UIImage {
                renderable: img.texture_id(&ctx),
                size: img.size_vec2(),
                _retained: img.into(),
            })
        } else {
            bail!("could not load from FS")
        }
    }
    pub async fn load_friend(id: String, url: String, ctx: Context) -> Result<UIImage> {
        let avatar_cache_folder = maxima_dir().unwrap().join("cache/avatars");

        if !fs::metadata(&avatar_cache_folder).is_ok() {
            // ideally lib should always make this but we're playing it safe if he doesn't
            let res = fs::create_dir_all(&avatar_cache_folder);
            if res.is_err() {
                error!("Failed to create directory {:?}", &avatar_cache_folder);
            }
        }

        let png_cache = avatar_cache_folder.join(id.clone() + "_208x208.png");
        let jpeg_cache = avatar_cache_folder.join(id.clone() + "_208x208.jpeg");

        if fs::metadata(&png_cache).is_err() && fs::metadata(&jpeg_cache).is_err() {
            download_image(
                url.clone(),
                if url.ends_with("JPEG") {
                    &jpeg_cache
                } else {
                    &png_cache
                },
            )
            .await?
        }

        let file_name = if fs::metadata(&jpeg_cache).is_ok() {
            jpeg_cache
        } else {
            png_cache
        };

        let fs_load = ImageLoader::load_from_fs(&file_name.to_str().unwrap());
        if fs_load.is_ok() {
            let img = fs_load?;
            return Ok(UIImage {
                renderable: img.texture_id(&ctx),
                size: img.size_vec2(),
                _retained: img.into(),
            });
        } else {
            bail!("could not load from FS")
        }
    }
}
