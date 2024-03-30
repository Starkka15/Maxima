use crate::{
    event_thread, DemoEguiApp
};

pub fn frontend_processor(app: &mut DemoEguiApp, ctx: &egui::Context) {
    puffin::profile_function!();

    while let Ok(result) = app.events.rx.try_recv() {
        match result {
            event_thread::MaximaEventResponse::FriendStatusResponse(res) => {
                for friend in &mut app.friends {
                    if friend.id != res.id {
                        continue;
                    }

                    friend.online = res.presence.basic().clone();
                }
            }
        }
    }
}