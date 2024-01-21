use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{bail, Result};

use derive_getters::Getters;
use lazy_static::lazy_static;
use log::{debug, error, warn};
use regex::Regex;
use sysinfo::{Pid, PidExt, ProcessExt, System, SystemExt};
use tokio::sync::{MutexGuard, RwLock};

use crate::{
    core::{launch::ActiveGameContext, LockedMaxima, Maxima, MaximaEvent},
    lsx::types::LSXRequestType,
    util::simple_crypto::{simple_decrypt, simple_encrypt},
};

use super::{
    request::{
        account::handle_query_entitlements_request,
        auth::handle_auth_code_request,
        challenge::handle_challenge_response,
        config::handle_config_request,
        core::{
            handle_connectivity_request, handle_set_downloader_util_request,
            handle_settings_request,
        },
        game::{handle_all_game_info_request, handle_game_info_request},
        igo::handle_show_igo_window_request,
        license::handle_license_request,
        profile::{
            handle_presence_request, handle_profile_request, handle_query_friends_request,
            handle_query_image_request, handle_query_presence_request, handle_set_presence_request,
        },
        progressive_install::{handle_pi_availability_request, handle_pi_installed_chunks_request},
        voip::handle_voip_status_request,
    },
    types::{
        create_lsx_message, LSXChallenge, LSXEvent, LSXEventType, LSXMessageType, LSXRequest,
        LSXResponse, LSX,
    },
    winproc::get_process_id,
};

const CORE_SENDER: &str = "EALS";

const CHALLENGE_BUILD: &str = "release";
const CHALLENGE_KEY: &str = "cacf897a20b6d612ad0c05e011df52bb"; // Need to figure out how to generate this
const CHALLENGE_VERSION: &str = "10,5,30,15625";

lazy_static! {
    static ref LSX_PATTERN: Regex = Regex::new(r"<LSX>.*?</LSX>").unwrap();
}

macro_rules! lsx_message_matcher {
    (
        $connection_var:expr, $message_var:expr, $message_type:ty;
        $($name:ident $handler:ident),* $(,)?
    ) => {
        paste::paste! {
            match $message_var {
                $(
                    $message_type::$name(msg) => $handler($connection_var, msg).await,
                )*
            }?
        }
    };
}

pub enum EncryptionState {
    Disabled,
    Ready([u8; 16]),
    Enabled([u8; 16]),
}

#[derive(Getters)]
pub struct ConnectionState {
    #[getter(skip)]
    maxima: LockedMaxima,
    challenge: String,
    encryption: EncryptionState,
    pid: u32,
    /// Message responses that are waiting to be sent
    queued_messages: Vec<String>,
}

pub type LockedConnectionState = Arc<RwLock<ConnectionState>>;

impl ConnectionState {
    /// Enable encryption on the packet after next
    pub fn enable_encryption(&mut self, encryption_key: [u8; 16]) {
        self.encryption = EncryptionState::Ready(encryption_key);
    }

    pub async fn maxima(&mut self) -> MutexGuard<'_, Maxima> {
        self.maxima.lock().await
    }

    pub fn maxima_arc(&mut self) -> LockedMaxima {
        self.maxima.clone()
    }

    pub async fn access_token(&mut self) -> Result<String> {
        Ok(self.maxima().await.access_token().await?)
    }

    pub fn queue_message(&mut self, message: LSX) -> Result<()> {
        let mut str = quick_xml::se::to_string(&message)?;
        debug!("Queuing LSX Message: {}", str);

        if let EncryptionState::Enabled(key) = self.encryption {
            str = simple_encrypt(str.as_bytes(), &key)
        };

        str += "\0";
        self.queued_messages.push(str);
        Ok(())
    }
}

pub fn get_os_pid(context: &ActiveGameContext) -> Result<u32> {
    let mut pid = None;

    let sys = System::new_all();
    for e in sys.processes() {
        let (p_pid, process) = e;
        if process.cmd().is_empty() {
            continue;
        }

        let mut cmd = process.cmd()[0].to_owned();
        if !cmd.starts_with("Z:") {
            continue;
        }

        cmd = cmd.replace("Z:", "").replace('\\', "/");
        if !cmd.starts_with(context.game_path()) {
            continue;
        }

        for ele in process.environ() {
            let (key, value) = ele.split_once('=').unwrap_or((ele, ""));
            if key != "MXLaunchId" || value != context.launch_id() {
                continue;
            }

            pid = Some(p_pid.as_u32());
            break;
        }
    }

    Ok(pid.unwrap_or(0))
}

#[cfg(target_os = "windows")]
pub fn get_wine_pid(launch_id: &str, name: &str) -> Result<u32> {
    Ok(0)
}

#[cfg(target_os = "linux")]
pub fn get_wine_pid(launch_id: &str, name: &str) -> Result<u32> {
    use crate::core::background_service::wine_get_pid;

    wine_get_pid(launch_id, name)
}

pub struct Connection {
    maxima: LockedMaxima,
    stream: TcpStream,
    state: LockedConnectionState,
}

impl Connection {
    pub async fn new(maxima_arc: LockedMaxima, stream: TcpStream) -> Result<Self> {
        stream.set_nodelay(true).unwrap();
        stream.set_nonblocking(true).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let maxima: MutexGuard<'_, Maxima> = maxima_arc.lock().await;
        let playing = maxima.playing();
        if playing.is_none() {
            stream.shutdown(std::net::Shutdown::Both)?;
            bail!("There is no active game context, LSX connection cannot be established");
        }

        let context = playing.as_ref().unwrap();

        let mut pid = get_os_pid(context);
        if cfg!(unix) {
            if let Ok(os_pid) = pid {
                let sys = System::new_all();
                if let Some(process) = sys.process(Pid::from_u32(os_pid)) {
                    let filename = PathBuf::from(
                        process.cmd()[0]
                            .to_owned()
                            .replace("Z:", "")
                            .replace('\\', "/"),
                    )
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned();
    
                    pid = get_wine_pid(&context.launch_id(), &filename);
                }
            }
        }

        if pid.is_err() || pid.as_ref().unwrap() == &0 {
            warn!("Failed to find PID through launch ID, things may not work!");
        }

        let state = Arc::new(RwLock::new(ConnectionState {
            maxima: maxima_arc.clone(),
            challenge: CHALLENGE_KEY.to_string(),
            encryption: EncryptionState::Disabled,
            pid: pid.unwrap_or(0),
            queued_messages: Vec::new(),
        }));

        Ok(Self {
            maxima: maxima_arc.clone(),
            stream,
            state,
        })
    }

    // State

    pub async fn maxima(&self) -> MutexGuard<Maxima> {
        self.maxima.lock().await
    }

    // Initialization

    pub async fn send_challenge(&mut self) -> Result<()> {
        let challenge = create_lsx_message(LSXMessageType::Event(LSXEvent {
            sender: CORE_SENDER.to_string(),
            value: LSXEventType::Challenge(LSXChallenge {
                attr_build: CHALLENGE_BUILD.to_string(),
                attr_key: self.state.read().await.challenge.to_owned(),
                attr_version: CHALLENGE_VERSION.to_string(),
            }),
        }));

        self.state.write().await.queue_message(challenge)?;
        Ok(())
    }

    pub async fn listen(&mut self) -> Result<()> {
        let mut buffer = [0; 1024 * 8];

        let n = match self.stream.read(&mut buffer) {
            Ok(n) if n == 0 => {
                bail!("Connection closed");
            }
            Ok(n) => n,
            Err(err) => {
                let kind = err.kind();
                if kind == ErrorKind::WouldBlock {
                    return Ok(());
                }

                bail!("Internal error in LSX connection: {}", kind);
            }
        };

        let state = self.state.write().await;

        let trimmed_buffer = &buffer[..n];
        let message = if let EncryptionState::Enabled(key) = state.encryption {
            simple_decrypt(trimmed_buffer, &key)
        } else {
            String::from_utf8_lossy(trimmed_buffer).trim().to_owned()
        };

        drop(state);

        for mat in LSX_PATTERN.find_iter(message.as_str()) {
            if let Err(err) = self.process_message(mat.as_str()).await {
                error!("Failed to process message: {}", err);
            }
        }

        Ok(())
    }

    pub async fn process_queue(&mut self) -> Result<()> {
        let mut state = self.state.write().await;
        for message in &state.queued_messages {
            if let Err(err) = self.stream.write(message.as_bytes()) {
                error!("Failed to send LSX message: {}", err);
            }
        }

        if !state.queued_messages.is_empty() {
            self.stream.flush()?;
        }

        state.queued_messages.clear();
        Ok(())
    }

    // Message Processing

    async fn process_message(&mut self, message: &str) -> Result<()> {
        debug!("Received LSX Message: {}", message);

        let mut message = message.to_string();
        message.remove_matches("version=\"\" ");
        let lsx_message: LSX = quick_xml::de::from_str(message.as_str())?;

        let state = self.state.clone();
        tokio::spawn(async move {
            let reply = match lsx_message.value {
                LSXMessageType::Event(msg) => Connection::process_event_message(&state, msg).await,
                LSXMessageType::Request(msg) => {
                    Connection::process_request_message(&state, msg).await
                }
                LSXMessageType::Response(_) => unimplemented!(),
            };

            if let Err(err) = reply {
                error!("Failed to process LSX message: {}", err);
                return;
            }

            let reply = reply.unwrap();

            if reply.is_some() {
                let mut state = state.write().await;
                let result = state.queue_message(LSX {
                    value: reply.unwrap(),
                });

                if let Err(err) = result {
                    error!("Failed to queue LSX message: {}", err);
                    return;
                }
            }

            let mut state = state.write().await;
            if let EncryptionState::Ready(key) = state.encryption {
                state.encryption = EncryptionState::Enabled(key);
            }
        });

        Ok(())
    }

    async fn process_event_message(
        _: &LockedConnectionState,
        _: LSXEvent,
    ) -> Result<Option<LSXMessageType>> {
        Ok(None)
    }

    async fn process_request_message(
        state: &LockedConnectionState,
        message: LSXRequest,
    ) -> Result<Option<LSXMessageType>> {
        {
            let pid = *state.read().await.pid();
            state
                .write()
                .await
                .maxima()
                .await
                .call_event(MaximaEvent::ReceivedLSXRequest(pid, message.value.clone()));
        }

        let result = lsx_message_matcher!(
            state.clone(), message.value, LSXRequestType;

            ChallengeResponse handle_challenge_response,
            GetConfig handle_config_request,
            GetProfile handle_profile_request,
            GetSetting handle_settings_request,
            RequestLicense handle_license_request,
            GetGameInfo handle_game_info_request,
            GetAllGameInfo handle_all_game_info_request,
            GetInternetConnectedState handle_connectivity_request,
            IsProgressiveInstallationAvailable handle_pi_availability_request,
            AreChunksInstalled handle_pi_installed_chunks_request,
            GetAuthCode handle_auth_code_request,
            GetPresence handle_presence_request,
            SetPresence handle_set_presence_request,
            QueryPresence handle_query_presence_request,
            QueryFriends handle_query_friends_request,
            QueryEntitlements handle_query_entitlements_request,
            QueryImage handle_query_image_request,
            GetVoipStatus handle_voip_status_request,
            ShowIGOWindow handle_show_igo_window_request,
            SetDownloaderUtilization handle_set_downloader_util_request,
        );

        if result.is_none() {
            return Ok(None);
        }

        Ok(Some(LSXMessageType::Response(LSXResponse {
            sender: message.recipient,
            id: message.id,
            value: result.unwrap(),
        })))
    }
}
