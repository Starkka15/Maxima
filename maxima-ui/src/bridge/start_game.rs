use log::{debug, error, info};
use maxima::core::{launch::{self, LaunchMode}, LockedMaxima};

use crate::{GameInfo, GameSettings};

pub async fn start_game_request(maxima_arc: LockedMaxima, game_info: GameInfo, game_settings: Option<GameSettings>) {
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        info!("Ignoring request to start game, not logged in.");
        return;
    }

    debug!("got request to start game {:?}", game_info.offer);

    // This is kind of gross, but it kind of makes sense to have?
    let (exe_override, args) = if let Some(settings) = game_settings {
        (
            if settings.exe_override.is_empty() {
                None
            } else {
                Some(settings.exe_override)
            }
        ,
        launch::parse_arguments(&settings.launch_args))

    } else {
        (None, Vec::new())
    };

    drop(maxima);
    let result = launch::start_game(maxima_arc.clone(), LaunchMode::Online(game_info.offer), exe_override, args).await;
    if result.is_err() {
        error!("Failed to start game! Reason: {}", result.err().unwrap());
    }
    

}
