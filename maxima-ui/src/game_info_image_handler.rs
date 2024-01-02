use anyhow::Error;
use egui::Context;
use tokio::fs::File;
use tokio::io;
use std::fs;
use std::sync::Arc;
use std::sync::mpsc::Receiver;
use std::sync::mpsc::Sender;

use std::result::Result::Ok;
use anyhow::{bail, Result};
use egui::TextureId;
use core::slice::SlicePattern;
use log::{debug, error, info};

use crate::GameImage;
use crate::GameInfo;
use crate::ImageLoader;
use maxima::util::native::maxima_dir;

#[derive(Clone, PartialEq)]
pub enum GameImageType {
  Icon,
  Hero,
  Logo
}

#[derive(Clone)]
struct ImageRequest {
  game_slug : String,
  image_type : GameImageType,
  _fs_path : Option<String>,
  url : Option<String>,
}

pub struct ImageResponse {
  pub game_slug : String,
  pub image_type : GameImageType,
  pub image : Arc<GameImage>,
}

pub struct GameImageHandler {
  pub rx : Receiver<ImageResponse>,
  tx : Sender<ImageRequest>,
  requests : Vec<ImageRequest>,
  loader_thread : tokio::task::JoinHandle<()>
}

impl GameImageHandler {
  pub fn new(ctx: &Context) -> Self{
    let (tx0, rx1) = std::sync::mpsc::channel();
    let (tx1, rx0) = std::sync::mpsc::channel();
    let context = ctx.clone();
    Self {
      rx : rx0,
      tx : tx0,
      requests: Vec::new(),
      loader_thread : tokio::task::spawn(async move {
        //in here
        loop {
          let received = rx1.recv();
          if let Ok(received) = received {
            let filename = match received.image_type {
              GameImageType::Icon => "icon.png",
              GameImageType::Hero => "hero.jpg",
              GameImageType::Logo => "logo.png",
            };
            let slug0 = received.game_slug.clone();
            debug!("[Loader thread] received request to load {} for game \"{}\"", filename, slug0);

            let cache_folder = maxima_dir().unwrap().join("cache/ui/images").join(&slug0);
            if !fs::metadata(&cache_folder).is_ok() { // folder is missing
              let res = fs::create_dir_all(&cache_folder);
              if res.is_err() {
                error!("Failed to create directory {:?}", &cache_folder);
              }
            }
            let file_name = &cache_folder.join(&filename);
            if !fs::metadata(&file_name).is_ok() { //image hasn't been cached yet
              if let Some(img_url) = received.url {
                info!("Downloading image at {:?}", img_url);
                let result = reqwest::get(&img_url).await;
                if let Ok(response) = result {
                  if let Ok(body) = response.bytes().await {
                    if let Ok(mut file) = File::create(&file_name).await {
                      let copy_result = io::copy(&mut body.as_slice(), &mut file).await;
                      if copy_result.is_ok() {
                        debug!("Copied file!")
                      } else {
                        error!("Failed to copy file! Reason: {:?}", copy_result.err())
                      }
                    } else {
                      error!("Failed to create {:?}", &file_name);
                    }
                  }
                } else {
                  error!("Failed to download {}! Reason: {:?}", &img_url, &result);
                  
                }
              }
              // TODO: image downloading
            }
            if let Ok(img) = ImageLoader::load_from_fs(&file_name.to_str().unwrap()) {

              let tmp_size = img.size_vec2();
              let rtn = ImageResponse {
                game_slug : String::from(&slug0),
                image_type : received.image_type,
                image : GameImage {
                  retained: Some(img.into()),
                  renderable: None, //needs to be done with the egui render context
                  _fs_path: String::new(),
                  url: String::new(),
                  size: tmp_size,
                }.into()
              };
              tx1.send(rtn).expect("Failed to send from loader thread");
              context.request_repaint();
            }
          }
        }
      })
    }
  }

  pub fn process_pending(&self) {
    
  }

  pub fn get_image(&mut self, slug : String, typ : GameImageType, path : Option<String>, url : Option<String>) -> Result<TextureId>{

    if !self.requests.iter().any(|r| r.game_slug == slug && r.image_type == typ) {
      let req = ImageRequest {
        game_slug : slug.to_owned(),
        image_type : typ,
        _fs_path : path,
        url
      };
      self.requests.push(req.clone());
      self.tx.send(req).expect("FUCK");
      bail!("kys");
    } else {
      bail!("kys");
    }
  }
}

impl Drop for GameImageHandler {
  fn drop(&mut self) {
    self.loader_thread.abort();
  }
}

impl GameImageHandler {
  pub fn shutdown(&self) {
    debug!("trying to kill image handler loader thread");
    self.loader_thread.abort();
    if !self.loader_thread.is_finished() {
      error!("couldn't kill image handler loader thread");
    }
  }
}

impl GameInfo {
  pub fn icon(&self, handler : &mut GameImageHandler) -> Result<TextureId> {
    if let Some(ok) = &self.icon_renderable {
      Ok(*ok)
    } else {
      handler.get_image(self.slug.to_owned(), GameImageType::Icon, None,None)
    }
  }
  /// use this for final rendering
  pub fn hero(&self, handler : &mut GameImageHandler) -> Result<TextureId> {
    if let Some(_ret) = &self.hero.retained {
      if let Some(ren) = self.hero.renderable {
        Ok(ren)
      } else {
        bail!("not ready")
      }
    } else {
      handler.get_image(self.slug.clone(),
      GameImageType::Hero,
      Some(self.path.clone()),
      Some(self.hero.url.clone()))
    }
  }
  /// use this for final rendering
  pub fn logo(&self, handler : &mut GameImageHandler) -> Result<TextureId> {
    if let Some(logo) = &self.logo {
      if let Some(_ret) = &logo.retained {
        if let Some(ren) = logo.renderable {
          Ok(ren)
        } else {
          bail!("not ready")
        }
      } else {
        handler.get_image(self.slug.clone(),
        GameImageType::Logo,
        Some(self.path.clone()),
        Some(logo.url.clone()))
      }
    } else {
      Err(Error::msg("Game does not have a logo"))
    }
  }
}