use crate::{
    bridge_thread::{self, BackendError},
    views::downloads_view::QueuedDownload,
    BackendStallState, GameDetails, GameDetailsWrapper, MaximaEguiApp,
};
use log::{error, info, warn};
use std::sync::mpsc::TryRecvError;

pub fn frontend_processor(app: &mut MaximaEguiApp, ctx: &egui::Context) {
    puffin::profile_function!();

    if app.critical_error.is_some() {
        return;
    }

    'outer: loop {
        match app.backend.backend_listener.try_recv() {
            Ok(result) => {
                use bridge_thread::MaximaLibResponse::*;
                match result {
                    LoginResponse(res) => {
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
                    LoginCacheEmpty => app.backend_state = BackendStallState::UserNeedsToLogIn,
                    ServiceNeedsStarting => {
                        app.backend_state = BackendStallState::UserNeedsToInstallService
                    }
                    ServiceStarted => app.backend_state = BackendStallState::Starting,
                    GameInfoResponse(res) => {
                        app.games.insert(res.game.slug.clone(), res.game);
                    }
                    GameDetailsResponse(res) => {
                        let response = res.response;

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
                    FriendInfoResponse(res) => app.friends.push(res.friend),
                    CriticalError(err) => app.critical_error = Some(*err),
                    NonFatalError(err) => app.nonfatal_errors.push(*err),
                    ActiveGameChanged(slug) => app.playing_game = slug,
                    LocateGameResponse(res) => {
                        app.installer_state.locate_response = Some(res);
                        app.installer_state.locating = false;
                    }
                    DownloadProgressChanged(offer_id, progress) => {
                        if let Some(dl_ing) = app.installing_now.as_mut() {
                            if dl_ing.offer == offer_id {
                                dl_ing.downloaded_bytes = progress.bytes;
                                dl_ing.total_bytes = progress.bytes_total;
                            }
                        }
                    }
                    DownloadFinished(_) => {
                        // idk
                    }
                    DownloadQueueUpdate(current, queue) => {
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
                                    total_bytes: 0,
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
                                total_bytes: 0,
                            };
                            app.install_queue.insert(offer, i_fucking_hate_this);
                        }
                    }
                }
                ctx.request_repaint();
            }
            Err(variant) => {
                match variant {
                    TryRecvError::Empty => {}
                    TryRecvError::Disconnected => {
                        app.critical_error = Some(BackendError::ChannelDisconnected);
                    }
                }
                break 'outer;
            }
        }
    }
}
