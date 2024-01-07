use anyhow::{Ok, Result};
use egui::Context;
use log::{debug, error};
use maxima::{
    core::{
        service_layer::{ServiceGame, ServiceGameImagesRequestBuilder, SERVICE_REQUEST_GAMEIMAGES},
        Maxima,
    },
    util::native::maxima_dir,
};
use std::{
    fs,
    sync::{mpsc::Sender, Arc},
};
use tokio::sync::Mutex;

use crate::{
    interact_thread::{InteractThreadGameUIImagesResponse, MaximaLibResponse},
    ui_image::{GameImageType, UIImage},
    GameUIImages,
};

async fn get_preferred_hero_image(slug: &String, images: &Option<ServiceGame>) -> Option<String> {
    if images.is_none() {
        return None;
    }

    let key_art = images.as_ref().unwrap().key_art();
    if key_art.is_none() {
        return None;
    }

    let key_art = key_art.as_ref().unwrap();

    if let Some(img) = key_art.aspect_2x1_image() {
        return Some(img.path().clone());
    }

    if let Some(img) = key_art.aspect_16x9_image() {
        return Some(img.path().clone());
    }

    None
}

async fn get_logo_image(images: &Option<ServiceGame>) -> Option<String> {
    if images.is_none() {
        return None;
    }

    let logo_set = images.as_ref().unwrap().primary_logo();
    if logo_set.is_none() {
        return None;
    }

    let largest_logo = logo_set.as_ref().unwrap().largest_image();
    if largest_logo.is_none() {
        return None;
    }

    Some(largest_logo.as_ref().unwrap().path().to_string())
}

pub async fn game_images_request(
    maxima_arc: Arc<Mutex<Maxima>>,
    slug: String,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    debug!("got request to load game images for {:?}", slug);
    let game_hero = maxima_dir()
        .unwrap()
        .join("cache/ui/images/")
        .join(&slug)
        .join("hero.jpg");
    let game_logo = maxima_dir()
        .unwrap()
        .join("cache/ui/images/")
        .join(&slug)
        .join("logo.png");
    let has_hero = fs::metadata(&game_hero).is_ok();
    let has_logo = fs::metadata(&game_logo).is_ok();
    let images: Option<ServiceGame> = // TODO: make it a result
        if !has_hero || !has_logo { //game hasn't been cached yet
            let maxima = maxima_arc.lock().await;
            maxima.service_layer()
            .request(SERVICE_REQUEST_GAMEIMAGES, ServiceGameImagesRequestBuilder::default()
            .should_fetch_context_image(!has_logo)
            .should_fetch_backdrop_images(!has_hero)
            .game_slug(slug.clone())
            .locale(maxima.locale().short_str().to_owned())
            .build()?).await?
        } else { None };
    let hero_url: Option<String> = if has_hero {
        debug!("Using cached hero image for {:?}", slug);
        None
    } else {
        get_preferred_hero_image(&slug, &images).await
    };
    let logo_url: Option<String> = if has_logo {
        debug!("Using cached logo for {:?}", slug);
        None
    } else {
        get_logo_image(&images).await
    };

    let ctx = ctx.clone();
    let is_logo = logo_url.is_some() || has_logo;
    tokio::task::spawn(async move {
        // this looks weird but i was going to flip them anyway, heroes are always bigger, and putting the await inside of the optional logo makes sense
        let hero = UIImage::load(
            slug.clone(),
            GameImageType::Hero,
            if has_hero { None } else { hero_url },
            ctx.clone(),
        );

        let logo = UIImage::load(
            slug.clone(),
            GameImageType::Logo,
            if has_logo { None } else { logo_url },
            ctx.clone(),
        );
        
        let hero = hero.await;
        let logo = logo.await;

        if hero.is_ok() {
            let res = MaximaLibResponse::GameUIImagesResponse(InteractThreadGameUIImagesResponse {
                slug: slug.clone(),
                response: Ok(GameUIImages {
                    logo: if is_logo {
                        Some(logo.expect("no logo").into())
                    } else {
                        None
                    },
                    hero: hero.expect("no hero").into(),
                }),
            });
            debug!("sending {}'s GameUIImages back to UI", &slug);
            let _ = channel.send(res);
            egui::Context::request_repaint(&ctx);
        } else {
            if !hero.is_ok() {
                error!("hero image not ok");
            }
        }
    });
    tokio::task::yield_now().await; // LMAO
    Ok(())
}
