use std::{io, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use core::future::Future;
use derive_getters::Getters;
use log::{debug, info, warn};
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, Mutex};

use crate::{
    core::auth::storage::LockedAuthStorage,
    rtm::proto::{LoginRequestV3, PlatformV1, PresenceSubscribeV1, PresenceV1, UserType},
};

use super::{
    connection::RtmConnectionManager,
    proto::{
        communication_v1, success_v1, BasicPresenceType, HeartbeatV1, LoginV3Response, Player,
        PresenceUpdateV1, RichPresenceType, RichPresenceV1, SessionCleanupV1,
    },
};

macro_rules! send_and_forget_rtm_request {
    ($connection_manager: expr, $request_body_name: ident, $comm_name: ident, $comm_initializer:tt) => {
        $connection_manager.send_and_forget_request(communication_v1::Body::$request_body_name($comm_name $comm_initializer))
    }
}

macro_rules! send_rtm_request {
    ($connection_manager: expr, $request_body_name: ident, $comm_name: ident, $response_body_name: ident, $response_comm_name: ident, $comm_initializer:tt) => {
        {
            fn _rtm_transform(
                fut: impl Future<Output = Result<communication_v1::Body, std::io::Error>> + Send,
            ) -> impl Future<Output = Option<$response_comm_name>> + Send {
                async move {
                    match fut.await {
                        Ok(communication_v1::Body::Success(success)) => match success.body.unwrap() {
                            success_v1::Body::$response_body_name(data) => Some(data),
                        },
                        _ => None,
                    }
                }
            }

            _rtm_transform($connection_manager.send_request(communication_v1::Body::$request_body_name($comm_name $comm_initializer)))
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClientVersion {
    client_type: String,
    version: String,
    integrations: String,
}

#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct CustomRichPresenceData {
    game_product_id: String,
    version: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum BasicPresence {
    Unknown,
    Offline,
    /// Doesn't work
    Dnd,
    Away,
    Online,
}

#[derive(Clone, Getters, Debug)]
pub struct RichPresence {
    basic: BasicPresence,
    status: String,
    game: Option<String>,
}

impl RichPresence {
    pub fn from(presence: &PresenceV1) -> Result<Self> {
        let basic = match presence.basic_presence_type() {
            BasicPresenceType::Offline => BasicPresence::Offline,
            BasicPresenceType::Dnd => BasicPresence::Dnd,
            BasicPresenceType::Away => BasicPresence::Away,
            BasicPresenceType::Online => BasicPresence::Online,
            _ => BasicPresence::Unknown,
        };

        let rich = presence.rich_presence.clone().unwrap_or_default();
        let custom_data: CustomRichPresenceData =
            serde_json::from_str(&rich.custom_rich_presence_data).unwrap_or_default();

        Ok(Self {
            basic,
            status: rich.game,
            game: if !custom_data.game_product_id.is_empty() {
                Some(custom_data.game_product_id)
            } else {
                None
            },
        })
    }
}

pub enum RtmEvent {
    PresenceUpdate(RichPresence),
}

type LockedRtmPresenceStore = Arc<Mutex<Cache<String, RichPresence>>>;

#[derive(Getters)]
pub struct RtmClient {
    #[getter(skip)]
    auth: LockedAuthStorage,

    conn_man: RtmConnectionManager,
    presence_store: LockedRtmPresenceStore,
}

impl RtmClient {
    pub fn new(auth: LockedAuthStorage) -> RtmClient {
        let (sender_tx, mut receiver_tx) = mpsc::channel(32);

        let client = Self {
            conn_man: RtmConnectionManager::new(Duration::from_millis(50), sender_tx),
            auth,
            presence_store: Arc::new(Mutex::new(
                Cache::builder()
                    .max_capacity(256)
                    .time_to_idle(Duration::from_secs_f64(3.154e+7f64)) // 1 year
                    .time_to_live(Duration::from_secs(60 * 5)) // 5 minutes
                    .build(),
            )),
        };

        let cloned_presence_store = client.presence_store.clone();
        tokio::spawn(async move {
            loop {
                match receiver_tx.recv().await {
                    Some(body) => {
                        RtmClient::process_update(body, cloned_presence_store.clone()).await
                    }
                    None => break,
                };
            }
        });

        client
    }

    async fn process_update(body: communication_v1::Body, presence_store: LockedRtmPresenceStore) {
        match body {
            communication_v1::Body::Presence(presence) => {
                if presence.client_version.is_none() {
                    return;
                }

                let res = serde_json::from_str(presence.client_version.as_ref().unwrap());
                if res.is_err() {
                    return;
                }

                let res: ClientVersion = res.unwrap();
                if res.client_type != "Client" && res.client_type != "LegacyClient" {
                    return;
                }

                let rich = RichPresence::from(&presence);
                if rich.is_err() {
                    return;
                }

                let id = presence.player.as_ref().unwrap().player_id.to_owned();
                presence_store
                    .lock()
                    .await
                    .insert(id.to_owned(), rich.unwrap());

                debug!("Updated {}'s presence", id);
            }
            _ => {
                warn!("Got unhandled RPC update {:?}", body);
            }
        }
    }

    pub async fn login(&mut self) -> Result<()> {
        let token = self
            .auth
            .lock()
            .await
            .access_token()
            .await
            .unwrap()
            .unwrap()
            .to_owned();

        let version = format!(
            "{}-{}-mxa",
            env!("CARGO_CRATE_NAME"),
            env!("CARGO_PKG_VERSION")
        );
        info!("Connecting to RTM with version {}", version);

        let client_version = ClientVersion {
            client_type: "Client".to_owned(),
            version,
            integrations: "".to_owned(),
        };

        let res = send_rtm_request!(self.conn_man, LoginRequestV3, LoginRequestV3, LoginV3Response, LoginV3Response, {
            token: token.to_owned(),
            reconnect: false,
            heartbeat: false,
            user_type: UserType::Nucleus as i32,
            product_id: "origin".to_owned(),
            platform: PlatformV1::Pc as i32,
            client_version: serde_json::to_string(&client_version)?,
            session_key: None,
            force_disconnect_session_key: None,
        }).await.ok_or(anyhow!("RTM login failed"))?;

        for ele in res.connected_sessions {
            let platform = PlatformV1::try_from(ele.platform)?;
            if platform != PlatformV1::Pc {
                continue;
            }

            self.session_cleanup(&ele.session_key).await?;
        }

        info!("Succesfully logged into RTM");
        Ok(())
    }

    pub async fn set_presence(
        &mut self,
        basic_presence: BasicPresence,
        status: &str,
        offer_id: &str,
    ) -> Result<(), std::io::Error> {
        info!("Updating RTM presence to '{}'", status);

        let rpc_data = CustomRichPresenceData {
            game_product_id: offer_id.to_owned(),
            version: 1,
        };

        let basic_presence_type = match basic_presence {
            BasicPresence::Unknown => BasicPresenceType::UnknownPresence,
            BasicPresence::Offline => BasicPresenceType::Offline,
            BasicPresence::Dnd => BasicPresenceType::Dnd,
            BasicPresence::Away => BasicPresenceType::Away,
            BasicPresence::Online => BasicPresenceType::Online,
        };

        send_and_forget_rtm_request!(self.conn_man, PresenceUpdate, PresenceUpdateV1, {
            status: "".to_owned(),
            basic_presence_type: basic_presence_type as i32,
            user_defined_presence: "".to_owned(),
            rich_presence: Some(RichPresenceV1 {
                game: status.to_owned(),
                platform: PlatformV1::Pc as i32,
                game_mode_type: "".to_owned(),
                game_mode: "".to_owned(),
                game_session_data: "".to_owned(),
                rich_presence_type: RichPresenceType::UnknownRichPresence as i32,
                start_timestamp: "".to_owned(),
                end_timestamp: "".to_owned(),
                custom_rich_presence_data: serde_json::to_string(&rpc_data)?,
            })
        })
        .await
    }

    /// Subscribe to a list of user IDs' presences
    pub async fn subscribe(&mut self, players: &Vec<String>) -> Result<(), io::Error> {
        send_and_forget_rtm_request!(self.conn_man, PresenceSubscribe, PresenceSubscribeV1, {
            players: players.iter().map(|id| Player{ player_id: id.to_owned(), product_id: String::from("origin"), }).collect()
        })
        .await
    }

    pub async fn session_cleanup(&mut self, session_key: &str) -> Result<(), io::Error> {
        send_and_forget_rtm_request!(self.conn_man, SessionCleanupV1, SessionCleanupV1, {
            session_key: session_key.to_owned()
        })
        .await
    }

    pub async fn heartbeat(&mut self) -> Result<(), io::Error> {
        send_and_forget_rtm_request!(self.conn_man, Heartbeat, HeartbeatV1, {}).await
    }
}
