use log::{debug, error, info, warn};

use crate::{
    interact_thread, DemoEguiApp, GameDetails, GameDetailsWrapper, GameUIImages,
    GameUIImagesWrapper,
};

pub fn frontend_processor(app: &mut DemoEguiApp, ctx: &egui::Context) {
    let result = app.backend.rx.try_recv();
    if !result.is_ok() {
        return;
    }
    match result.unwrap() {
        interact_thread::MaximaLibResponse::LoginResponse(res) => {
            info!("Got something");
            if !res.success {
                warn!("Login failed.");
                app.in_progress_credential_status = res.description;
                return;
            }

            app.logged_in = true;
            info!("Logged in as {}!", &res.description);
            app.user_name = res.description.clone();
            app.backend
                .tx
                .send(interact_thread::MaximaLibRequest::GetGamesRequest)
                .unwrap();
        }
        interact_thread::MaximaLibResponse::GameInfoResponse(res) => {
            app.games.push(res.game);
            ctx.request_repaint(); // Run this loop once more, just to see if any games got lost
        }
        interact_thread::MaximaLibResponse::GameDetailsResponse(res) => {
            if res.response.is_err() {
                return;
            }

            let response = res.response.unwrap();

            for game in &mut app.games {
                if game.slug != res.slug {
                    continue;
                }
                
                game.details = GameDetailsWrapper::Available(GameDetails {
                    time: response.time,
                    achievements_unlocked: response.achievements_unlocked,
                    achievements_total: response.achievements_total,
                    path: response.path.clone(),
                    system_requirements_min: response.system_requirements_min.clone(),
                    system_requirements_rec: response.system_requirements_rec.clone(),
                });
            }

            ctx.request_repaint();
        }
        interact_thread::MaximaLibResponse::GameUIImagesResponse(res) => {
            debug!("Got UIImages back from the interact thread");
            if res.response.is_err() {
                return;
            }

            let response = res.response.unwrap();

            for game in &mut app.games {
                if game.slug != res.slug {
                    continue;
                }

                info!("setting images for {:?}", game.slug);
                game.images = GameUIImagesWrapper::Available(GameUIImages {
                    hero: response.hero.to_owned(),
                    logo: response.logo.to_owned(),
                });
            }
            ctx.request_repaint(); // Run this loop once more, just to see if any games got lost
        }
        interact_thread::MaximaLibResponse::InteractionThreadDiedResponse => {
            error!("interact thread died");
            app.critical_bg_thread_crashed = true;
        }
    }
}
