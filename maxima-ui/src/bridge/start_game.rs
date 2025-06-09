use crate::{bridge_thread::BackendError, GameInfo, GameSettings};
use log::{debug, error, info};
use maxima::core::launch::LaunchError;
use maxima::core::{
    launch::{self, LaunchMode},
    LockedMaxima,
};

pub async fn start_game_request(
    maxima_arc: LockedMaxima,
    game_info: GameInfo,
    game_settings: Option<GameSettings>,
) -> Result<(), LaunchError> {
    let maxima = maxima_arc.lock().await;
    let logged_in = maxima.auth_storage().lock().await.current().is_some();
    if !logged_in {
        info!("Ignoring request to start game, not logged in.");
        return Ok(()); // TODO(headassbtw): look into if it's worth properly reporting this
    }

    debug!("got request to start game {:?}", game_info.offer);

    // This is kind of gross, but it kind of makes sense to have?
    let (exe_override, args) = if let Some(settings) = game_settings {
        (
            if settings.exe_override.is_empty() {
                None
            } else {
                Some(settings.exe_override)
            },
            launch::parse_arguments(&settings.launch_args),
        )
    } else {
        (None, Vec::new())
    };

    drop(maxima);
    launch::start_game(
        maxima_arc.clone(),
        LaunchMode::Online(game_info.offer),
        exe_override,
        args,
    )
    .await
}
