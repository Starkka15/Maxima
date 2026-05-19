use egui::Context;
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

use crate::bridge_thread::BackendError;
use log::info;
use maxima::core::{
    service_layer::{
        ServiceFriends, ServiceGetMyFriendsRequestBuilder, SERVICE_REQUEST_GETMYFRIENDS,
    },
    LockedMaxima,
};
use maxima::rtm::client::RichPresence;

// TODO(headassbtw): integrate this into the enum too (out of scope for the PR i wrote this in)
pub struct EventThreadFriendStatusResponse {
    pub id: String,
    pub presence: maxima::rtm::client::RichPresence,
}

pub enum MaximaEventResponse {
    FriendStatusResponse(EventThreadFriendStatusResponse),
}

pub enum MaximaEventRequest {
    SubscribeToFriendPresence,
    ShutdownRequest,
}

pub struct EventThread {}

impl EventThread {
    pub fn new(
        ctx: &Context,
        maxima: LockedMaxima,
        rtm_cmd_listener: Receiver<MaximaEventRequest>,
        rtm_responder: Sender<MaximaEventResponse>,
    ) -> Self {
        let context = ctx.clone();

        tokio::task::spawn(async move {
            let result = EventThread::run(rtm_cmd_listener, rtm_responder, &context, maxima).await;
            if result.is_err() {
                panic!("Event thread failed! {}", result.err().unwrap());
            } else {
                info!("Event thread shut down")
            }
        });

        Self {}
    }

    async fn run(
        rtm_cmd_listener: Receiver<MaximaEventRequest>,
        rtm_responder: Sender<MaximaEventResponse>,
        ctx: &Context,
        maxima_arc: LockedMaxima,
    ) -> Result<(), BackendError> {
        let mut maxima = maxima_arc.lock().await;

        let friends: ServiceFriends = maxima
            .service_layer()
            .request(
                SERVICE_REQUEST_GETMYFRIENDS,
                ServiceGetMyFriendsRequestBuilder::default()
                    .offset(0)
                    .limit(100)
                    .is_mutual_friends_enabled(false)
                    .build()
                    .unwrap(),
            )
            .await?;

        let rtm = maxima.rtm();
        rtm.login().await?;

        let players: Vec<String> =
            friends.friends().items().iter().map(|f| f.id().to_owned()).collect();
        info!("Subscribed to {} players", players.len());

        rtm.subscribe(&players).await?;
        drop(maxima);

        // Cache last-emitted presence per friend so we only push events (and
        // repaint requests) when something actually changed. Upstream emitted
        // every friend's presence on every 500ms tick and called
        // request_repaint inside the loop — 16 friends = ~32 wasted repaints
        // per second when nothing was changing.
        let mut previous_presences: HashMap<String, RichPresence> = HashMap::new();

        'outer: loop {
            let mut maxima = maxima_arc.lock().await;
            maxima.rtm().heartbeat().await?;

            let mut any_changed = false;
            {
                let store = maxima.rtm().presence_store().lock().await;
                for entry in store.iter() {
                    let id: String = entry.0.as_ref().clone();
                    let presence: RichPresence = entry.1;
                    if previous_presences.get(&id) == Some(&presence) {
                        continue;
                    }
                    let _ = rtm_responder.send(MaximaEventResponse::FriendStatusResponse(
                        EventThreadFriendStatusResponse {
                            id: id.clone(),
                            presence: presence.clone(),
                        },
                    ));
                    previous_presences.insert(id, presence);
                    any_changed = true;
                }
            }
            if any_changed {
                egui::Context::request_repaint(&ctx);
            }

            drop(maxima);

            let request = rtm_cmd_listener.try_recv();
            if request.is_err() {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                continue;
            }

            match request? {
                MaximaEventRequest::SubscribeToFriendPresence => {}
                MaximaEventRequest::ShutdownRequest => break 'outer Ok(()),
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}
