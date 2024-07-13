use anyhow::{Ok, Result, bail};
use egui::Context;
use log::debug;
use maxima::core::LockedMaxima;
use std::sync::mpsc::Sender;

use crate::{
    bridge_thread::{InteractThreadGameListResponse, MaximaLibResponse},
    GameDetailsWrapper, GameInfo, GameUIImagesWrapper,
};

pub async fn get_games_request(
    maxima_arc: LockedMaxima,
    channel: Sender<MaximaLibResponse>,
    ctx: &Context,
) -> Result<()> {
    debug!("recieved request to load games");
    let mut maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        bail!("Ignoring request to load games, not logged in.");
    }

    let owned_games = maxima.mut_library().games().await;
    
    if owned_games.len() <= 0 {
        return Ok(());
    }

    for game in owned_games {
        let game_info = GameInfo {
            slug: game.base_offer().slug().to_string(),
            offer: game.base_offer().offer().offer_id().to_string(),
            name: game.name(),
            images: GameUIImagesWrapper::Unloaded,
            details: GameDetailsWrapper::Unloaded,
            dlc: game.extra_offers().clone(),
            installed: game.base_offer().installed().await,
        };
        let res = MaximaLibResponse::GameInfoResponse(InteractThreadGameListResponse {
            game: game_info,
        });
        channel.send(res)?;

        egui::Context::request_repaint(&ctx);
    }
    Ok(())
}
