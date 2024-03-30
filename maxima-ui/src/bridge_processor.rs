use log::{debug, error, info, warn};

use crate::{
    bridge_thread, DemoEguiApp, GameDetails, GameDetailsWrapper, GameUIImages,
    GameUIImagesWrapper, views::friends_view::UIFriendImageWrapper,
};

pub fn frontend_processor(app: &mut DemoEguiApp, ctx: &egui::Context) {
    puffin::profile_function!();
 
    while let Ok(result) = app.backend.rx.try_recv() {
        match result{
            bridge_thread::MaximaLibResponse::LoginResponse(res) => {
                info!("Got something");
                if !res.success {
                    warn!("Login failed.");
                    app.in_progress_credential_status = res.description;
                    continue;
                }
    
                app.logged_in = true;
                info!("Logged in as {}!", &res.description);
                app.user_name = res.description.clone();
                app.login_cache_waiting = false;
                app.backend
                    .tx
                    .send(bridge_thread::MaximaLibRequest::GetGamesRequest)
                    .unwrap();
                app.backend
                    .tx
                    .send(bridge_thread::MaximaLibRequest::GetFriendsRequest)
                    .unwrap();
            }
            bridge_thread::MaximaLibResponse::LoginCacheEmpty => {
                app.login_cache_waiting = false;
            }
            bridge_thread::MaximaLibResponse::GameInfoResponse(res) => {
                app.games.push(res.game);
                ctx.request_repaint(); // Run this loop once more, just to see if any games got lost
            }
            bridge_thread::MaximaLibResponse::GameDetailsResponse(res) => {
                if res.response.is_err() {
                    continue;
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
            bridge_thread::MaximaLibResponse::FriendInfoResponse(res) => {
                app.friends.push(res.friend);
                ctx.request_repaint();
            }
            bridge_thread::MaximaLibResponse::GameUIImagesResponse(res) => {
                debug!("Got UIImages back from the interact thread");
                if res.response.is_err() {
                    continue;
                }
    
                let response = res.response.unwrap();
    
                for game in &mut app.games {
                    if game.slug != res.slug {
                        continue;
                    }
    
                    debug!("setting images for {:?}", game.slug);
                    game.images = GameUIImagesWrapper::Available(GameUIImages {
                        hero: response.hero.to_owned(),
                        logo: response.logo.to_owned(),
                    });
                }
                ctx.request_repaint(); // Run this loop once more, just to see if any games got lost
            }
            bridge_thread::MaximaLibResponse::UserAvatarResponse(res) => {
                debug!("Got Avatar back from the interact thread");
                if res.response.is_err() {
                    error!("{}", res.response.err().expect("").to_string());
                    continue;
                }
    
                let response = res.response.unwrap();
    
                for user in &mut app.friends {
                    if !user.id.eq(&res.id) {
                        continue;
                    }

                    user.avatar = UIFriendImageWrapper::Available(response.clone());
                }
                ctx.request_repaint(); // Run this loop once more, just to see if any games got lost
            }
            bridge_thread::MaximaLibResponse::InteractionThreadDiedResponse => {
                error!("interact thread died");
                app.critical_bg_thread_crashed = true;
            }
        }
    }
}
