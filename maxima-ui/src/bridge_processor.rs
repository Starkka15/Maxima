use log::{debug, error, info, warn};
use std::sync::mpsc::TryRecvError;
use crate::{
    bridge_thread, views::{downloads_view::QueuedDownload, friends_view::UIFriendImageWrapper}, BackendStallState, GameDetails, GameDetailsWrapper, GameUIImages, GameUIImagesWrapper, MaximaEguiApp
};

pub fn frontend_processor(app: &mut MaximaEguiApp, ctx: &egui::Context) {
    puffin::profile_function!();
    
    if app.critical_bg_thread_crashed { return; }

    'outer: loop {
        match app.backend.backend_listener.try_recv() {
            Ok(result) => {
                match result {
                    bridge_thread::MaximaLibResponse::LoginResponse(res) => {
                        if let Err(error) = &res {
                            warn!("Login failed. {}", error);
                            continue;
                        }
                        let res = res.unwrap();
        
                        info!("Logged in as {}!", &res.you.display_name());
                        app.user_name = res.you.display_name().clone();
                        app.user_id = res.you.id().clone();
                        app.backend_state = BackendStallState::BingChilling;
                        app.backend
                            .backend_commander
                            .send(bridge_thread::MaximaLibRequest::GetGamesRequest)
                            .unwrap();
                        app.backend
                            .backend_commander
                            .send(bridge_thread::MaximaLibRequest::GetFriendsRequest)
                            .unwrap();
                    }
                    bridge_thread::MaximaLibResponse::LoginCacheEmpty => {
                        app.backend_state = BackendStallState::UserNeedsToLogIn;
                    }
                    bridge_thread::MaximaLibResponse::ServiceNeedsStarting => {
                        app.backend_state = BackendStallState::UserNeedsToInstallService;
                    }
                    bridge_thread::MaximaLibResponse::ServiceStarted => {
                        app.backend_state = BackendStallState::Starting
                    }
                    bridge_thread::MaximaLibResponse::GameInfoResponse(res) => {
                        app.games.insert(res.game.slug.clone(), res.game);
                    }
                    bridge_thread::MaximaLibResponse::GameDetailsResponse(res) => {
                        if res.response.is_err() {
                            continue;
                        }
        
                        let response = res.response.unwrap();
        
                        for (slug, game) in &mut app.games {
                            if !slug.eq(&res.slug) {
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
                    }
                    bridge_thread::MaximaLibResponse::FriendInfoResponse(res) => {
                        app.friends.push(res.friend);
                    }
                    bridge_thread::MaximaLibResponse::GameUIImagesResponse(res) => {
                        debug!("Got UIImages back from the interact thread");
                        if res.response.is_err() {
                            continue;
                        }
        
                        let response = res.response.unwrap();
        
                        for (slug, game) in &mut app.games {
                            if !slug.eq(&res.slug) {
                                continue;
                            }
        
                            debug!("setting images for {:?}", game.slug);
                            game.images = GameUIImagesWrapper::Available(GameUIImages {
                                hero: response.hero.to_owned(),
                                logo: response.logo.to_owned(),
                            });
                        }
                    }
                    bridge_thread::MaximaLibResponse::UserAvatarResponse(res) => {
                        
                        if res.response.is_err() {
                            error!("{}", res.response.err().expect("").to_string());
                            continue;
                        }
        
                        let response = res.response.unwrap();
        
                        if app.user_id.eq(&res.id) {
                            app.local_user_pfp = UIFriendImageWrapper::Available(response.clone());
                            debug!("your own pfp");
                            continue;
                        }
        
                        for user in &mut app.friends {
                            if !user.id.eq(&res.id) {
                                continue;
                            }
                            debug!("Got {}'s Avatar back from the interact thread", &user.name);
                            user.avatar = UIFriendImageWrapper::Available(response.clone());
                        }
                    }
                    bridge_thread::MaximaLibResponse::InteractionThreadDiedResponse => {
                        error!("interact thread died");
                        app.critical_bg_thread_crashed = true;
                    }
                    bridge_thread::MaximaLibResponse::ActiveGameChanged(slug) => {
                        app.playing_game = slug;
                    },
                    bridge_thread::MaximaLibResponse::LocateGameResponse(res) => {
                        app.installer_state.locate_response = Some(res);
                        app.installer_state.locating = false;
                    },
                    bridge_thread::MaximaLibResponse::DownloadProgressChanged(offer_id, progress) => {
                        if let Some(dl_ing) = app.installing_now.as_mut() {
                            if dl_ing.offer == offer_id {
                                dl_ing.downloaded_bytes = progress.bytes;
                                dl_ing.total_bytes = progress.bytes_total;
                            }
                        }
                    },
                    bridge_thread::MaximaLibResponse::DownloadFinished(_) => {
                        // idk
                    },
                    bridge_thread::MaximaLibResponse::DownloadQueueUpdate(current, queue) => {
                        if let Some(current) = current {
                            if !app.installing_now.as_ref().is_some_and(|n| n.offer == current) {
                                app.installing_now = Some(QueuedDownload {
                                    slug: {
                                        // This sucks!
                                        let mut rtn: String = String::new();
                                        for (slug, game) in &app.games {
                                            if game.offer.eq(&current) {
                                                // "but it's less code in the nest"
                                                // "WHO CARES"
                                                // (it was the same amount overall)
                                                rtn = slug.to_string();
                                                break;
                                            }
                                        }
                                        rtn
                                    },
                                    offer: current,
                                    downloaded_bytes: 0,
                                    total_bytes: 0
                                })
                            }
                        } else {
                            app.installing_now = None;
                        }
        
                        app.install_queue.clear();
                        for offer in queue {
                            let i_fucking_hate_this = QueuedDownload {
                                slug: {
                                    let mut rtn: String = String::new();
                                    for (slug, game) in &app.games {
                                        if game.offer.eq(&offer) {
                                            rtn = slug.to_string();
                                            break;
                                        }
                                    }
                                    rtn
                                },
                                offer: offer.clone(),
                                downloaded_bytes: 0,
                                total_bytes: 0
                            };
                            app.install_queue.insert(offer, i_fucking_hate_this);
                        }
                        
                    },
                }
                ctx.request_repaint();
            },
            Err(variant) => {
                match variant {
                    TryRecvError::Empty => {},
                    TryRecvError::Disconnected => { app.critical_bg_thread_crashed = true; },
                }
                break 'outer;
            },
        }
    }
}
