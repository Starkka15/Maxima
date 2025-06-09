use egui::Context;
use std::sync::mpsc::{Receiver, Sender};

use crate::bridge_thread::BackendError;
use log::info;
use maxima::core::{
    service_layer::{
        ServiceFriends, ServiceGetMyFriendsRequestBuilder, SERVICE_REQUEST_GETMYFRIENDS,
    },
    LockedMaxima,
};

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

        'outer: loop {
            let mut maxima = maxima_arc.lock().await;
            maxima.rtm().heartbeat().await?;

            {
                let store = maxima.rtm().presence_store().lock().await;
                for entry in store.iter() {
                    let _ = rtm_responder.send(MaximaEventResponse::FriendStatusResponse(
                        EventThreadFriendStatusResponse {
                            id: entry.0.to_string(),
                            presence: entry.1,
                        },
                    ));
                    // This can cause excessive repainting if it keeps updating friends we know about
                    egui::Context::request_repaint(&ctx);
                }
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
