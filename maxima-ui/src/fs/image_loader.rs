use anyhow::{bail, Result};
use egui::{ColorImage, Image};
use egui_extras::{RetainedImage};
use std::{io::Cursor, rc::Rc};
use image::{io::Reader as ImageReader, DynamicImage};

pub struct ImageLoader {
  
}



impl ImageLoader {
  pub fn load_from_fs(path : &str) -> Result<egui_extras::RetainedImage> {
    println!("Loading image {:?}",path);
    if let Ok(img) = ImageReader::open(path) {
      println!("Image loaded, trying decode");
      if let Ok(img_decoded) = img.decode() {
        println!("Image decoded, trying generate");
        println!("{} bytes per pixel", img_decoded.color().bytes_per_pixel());
        println!("{} channels", img_decoded.color().channel_count());
        match img_decoded.color().channel_count() {
          2 => {
              let img_a = DynamicImage::ImageRgba8(img_decoded.into_rgba8());
              let ci = ColorImage::from_rgba_unmultiplied([img_a.width() as usize,img_a.height() as usize], img_a.as_bytes());
              Ok(RetainedImage::from_color_image(format!("{:?}_Retained_Decoded",path), ci))
          },
          4 => {
            let ci = ColorImage::from_rgba_unmultiplied([img_decoded.width() as usize,img_decoded.height() as usize], img_decoded.as_bytes());
            Ok(RetainedImage::from_color_image(format!("{:?}_Retained_Decoded",path), ci))
          },
          3 => {
            let ci = ColorImage::from_rgb([img_decoded.width() as usize,img_decoded.height() as usize], img_decoded.as_bytes());
            Ok(RetainedImage::from_color_image(format!("{:?}_Retained_Decoded",path), ci))
          },
          _ => bail!("unsupported amount of channels")
        }
      } else {
        println!("Failed to decode \"{}\"!", path);
        bail!("yeah")
      }
    } else {
      println!("Failed to open \"{}\"!", path);
      bail!("yeah")
    }
  }
}

pub async fn save_image_from_url() {
  
}