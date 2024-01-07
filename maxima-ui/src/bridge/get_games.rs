use anyhow::{Error, Ok, Result, bail};
use egui::Context;
use log::{debug, info};
use maxima::core::Maxima;
use std::sync::{mpsc::Sender, Arc};
use tokio::sync::Mutex;

use crate::{
    interact_thread::{InteractThreadGameListResponse, MaximaLibResponse},
    GameDetailsWrapper, GameInfo, GameUIImagesWrapper,
};

pub async fn get_games_request(
    maxima_arc: Arc<Mutex<Maxima>>,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    debug!("recieved request to load games");
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        bail!("Ignoring request to load games, not logged in.");
    }

    let owned_games = maxima.owned_games(1).await.unwrap();
    let owned_game_products = owned_games.owned_game_products();
    if owned_game_products.is_none() {
        return Ok(());
    }

    for game in owned_game_products.as_ref().unwrap().items() {
        let game_info = GameInfo {
            slug: game.product().game_slug().clone(),
            offer: game.origin_offer_id().clone(),
            name: game.product().name().clone(),
            images: GameUIImagesWrapper::Unloaded,
            details: GameDetailsWrapper::Unloaded,
        };
        let res = MaximaLibResponse::GameInfoResponse(InteractThreadGameListResponse {
            game: game_info,
        });
        channel.send(res)?;

        egui::Context::request_repaint(&ctx);
    }
    Ok(())
}
