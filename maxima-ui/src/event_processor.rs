use crate::{event_thread, MaximaEguiApp};

pub fn frontend_processor(app: &mut MaximaEguiApp, ctx: &egui::Context) {
    puffin::profile_function!();

    while let Ok(result) = app.events.rx.try_recv() {
        match result {
            event_thread::MaximaEventResponse::FriendStatusResponse(res) => {
                for friend in &mut app.friends {
                    if friend.id != res.id {
                        continue;
                    }

                    friend.online = res.presence.basic().clone();
                    if res.presence.game().is_some() {
                        friend.game = Some(res.presence.status().clone());
                    }
                }
            }
        }
    }
}