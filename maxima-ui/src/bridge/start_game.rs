use std::sync::Arc;
use tokio::sync::Mutex;

use log::{debug, error, info};
use maxima::core::{launch, Maxima};

pub async fn start_game_request(maxima_arc: Arc<Mutex<Maxima>>, offer_id: String) {
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        info!("Ignoring request to start game, not logged in.");
        return;
    }

    debug!("got request to start game {:?}", offer_id);
    let maybe_path: Option<String> = if offer_id.eq("Origin.OFR.50.0001456") {
        Some(
            "/home/headass/.local/share/Steam/steamapps/common/Titanfall2/Titanfall2.exe"
                .to_owned(),
        )
    } else if offer_id.eq("Origin.OFR.50.0000739") {
        Some("H:\\SteamLibrary\\steamapps\\common\\Titanfall\\Titanfall.exe".to_owned())
    } else if offer_id.eq("Origin.OFR.50.0004976") {
        Some("/kronos/Games/Steam/steamapps/common/Excalibur/NeedForSpeedUnbound.exe".to_owned())
    } else if offer_id.eq("Origin.OFR.50.0002688") {
        Some("/kronos/Games/Oregon/Anthem/Anthem.exe".to_owned())
    } else if offer_id.eq("Origin.OFR.50.0002148") {
        Some("/home/battledash/games/battlefront/starwarsbattlefrontii.exe".to_owned())
    } else if offer_id.eq("OFB-EAST:109552314") {
        Some("/kronos/Games/Steam/steamapps/common/Battlefield 4/bf4.exe".to_owned())
    } else {
        None
    };
    let maybe_args: Vec<String> = if offer_id.eq("Origin.OFR.50.0001456") {
        vec!["-windowed".to_string(), "-novid".to_string()]
    } else if offer_id.eq("Origin.OFR.50.0000739") {
        vec!["-windowed".to_string(), "-novid".to_string()]
    } else {
        vec![]
    };

    drop(maxima);
    let result = launch::start_game(&offer_id, maybe_path, maybe_args, maxima_arc.clone()).await;
    if result.is_err() {
        error!("Failed to start game! Reason: {}", result.err().unwrap());
    }
}
