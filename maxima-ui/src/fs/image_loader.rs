use anyhow::{bail, Result};
use egui::ColorImage;
use egui_extras::RetainedImage;
use image::{io::Reader as ImageReader, DynamicImage};
use log::{debug, error};
use std::io::Read;

pub struct ImageLoader {}

impl ImageLoader {
    pub fn load_from_fs(path: &str) -> Result<egui_extras::RetainedImage> {
        debug!("Loading image {:?}", path);
        let img = ImageReader::open(path);
        if img.is_err() {
            error!("Failed to open \"{}\"!", path);
            // TODO: fix this
            return Self::load_from_fs("./res/placeholder.png"); // probably a really shitty idea but i don't want to embed the png, or make a system to return pointers to the texture, suffer.
        }

        let img_decoded = img?.decode();
        if img_decoded.is_err() {
            error!("Failed to decode \"{}\"! Trying as SVG...", path);
            // this is incredibly fucking stupid
            // i should've never done this, i should've found a proper method to detect things
            // but here we are. if it works, it works, and i sure as hell don't want to fix it.
            let mut f = std::fs::File::open(path)?;
            let mut buffer = String::new();
            f.read_to_string(&mut buffer)?;
            let yeah = RetainedImage::from_svg_str(format!("{:?}_Retained_Decoded", path), &buffer);
            if yeah.is_err() {
                bail!("Failed to read SVG from \"{}\"!", path);
            }

            return Ok(yeah.unwrap());
        }

        let img_decoded = img_decoded?;

        match img_decoded.color().channel_count() {
            2 => {
                let img_a = DynamicImage::ImageRgba8(img_decoded.into_rgba8());
                let ci = ColorImage::from_rgba_unmultiplied(
                    [img_a.width() as usize, img_a.height() as usize],
                    img_a.as_bytes(),
                );

                Ok(RetainedImage::from_color_image(
                    format!("{:?}_Retained_Decoded", path),
                    ci,
                ))
            }
            3 => {
                let ci = ColorImage::from_rgb(
                    [img_decoded.width() as usize, img_decoded.height() as usize],
                    img_decoded.as_bytes(),
                );
                
                Ok(RetainedImage::from_color_image(
                    format!("{:?}_Retained_Decoded", path),
                    ci,
                ))
            }
            4 => {
                let ci = ColorImage::from_rgba_unmultiplied(
                    [img_decoded.width() as usize, img_decoded.height() as usize],
                    img_decoded.as_bytes(),
                );

                Ok(RetainedImage::from_color_image(
                    format!("{:?}_Retained_Decoded", path),
                    ci,
                ))
            }
            _ => bail!("unsupported amount of channels"),
        }
    }
}
