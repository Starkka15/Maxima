use std::{
    io::{ErrorKind, Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use anyhow::{Result, bail};

use log::{debug, error, info, warn};
use regex::Regex;
use sysinfo::{System, SystemExt, ProcessExt, PidExt};
use tokio::sync::{Mutex, MutexGuard};

use crate::{
    core::{ecommerce::CommerceOffer, Maxima, MaximaEvent},
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

pub struct Connection {
    maxima: Arc<Mutex<Maxima>>,
    stream: TcpStream,
    challenge: String,
    encryption: EncryptionState,
    pid: u32,
}

impl Connection {
    pub fn new(maxima: Arc<Mutex<Maxima>>, stream: TcpStream) -> Self {
        stream.set_nodelay(true).unwrap();
        stream.set_nonblocking(true).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .unwrap();

        let mut pid = None;

        // First attempt to look up the PID through the TCP table
        let mut i = 0;
        while pid.is_none() && i < 10 {
            pid = get_process_id(stream.peer_addr().unwrap().port());
            std::thread::sleep(Duration::from_secs(1));
            i += 1;
        }

        // If that didn't work, fall back to the exe name we know. We try the TCP table
        // first to handle games with wrapping launchers, so if we need to do this for one
        // of those games, we'll probably run into issues
        if pid.is_none() {
            warn!("Failed to find PID through TCP table, falling back to known executable name");
            let sys = System::new_all();
            for p in sys.processes_by_exact_name("starwarsbattlefrontii.exe") {
                pid = Some(p.pid().as_u32())
            }
        }

        info!("PID: {:?}", pid.unwrap());

        Self {
            maxima,
            stream,
            challenge: CHALLENGE_KEY.to_string(),
            encryption: EncryptionState::Disabled,
            pid: pid.expect("Failed to get process ID"),
        }
    }

    // State

    pub fn process_id(&self) -> u32 {
        self.pid
    }

    pub async fn maxima(&self) -> MutexGuard<Maxima> {
        self.maxima.lock().await
    }

    pub fn challenge(&self) -> String {
        self.challenge.to_owned()
    }

    // IPC shorthands

    pub async fn access_token(&self) -> String {
        self.maxima().await.access_token().to_owned()
    }

    pub async fn current_offer(&self) -> CommerceOffer {
        self.maxima()
            .await
            .playing()
            .as_ref()
            .unwrap()
            .offer
            .to_owned()
    }

    // Enable encryption on the packet after next
    pub fn enable_encryption(&mut self, encryption_key: [u8; 16]) {
        self.encryption = EncryptionState::Ready(encryption_key);
    }

    // Initialization

    pub fn send_challenge(&mut self) -> Result<()> {
        let challenge = create_lsx_message(LSXMessageType::Event(LSXEvent {
            sender: CORE_SENDER.to_string(),
            value: LSXEventType::Challenge(LSXChallenge {
                attr_build: CHALLENGE_BUILD.to_string(),
                attr_key: self.challenge.to_owned(),
                attr_version: CHALLENGE_VERSION.to_string(),
            }),
        }));

        self.send_lsx(challenge)?;
        Ok(())
    }

    pub async fn listen(&mut self) -> Result<()> {
        let re = Regex::new(r"(<LSX>.*?</LSX>)").unwrap();
        let mut buffer = [0; 1024 * 8];

        let n = match self.stream.read(&mut buffer) {
            Ok(n) if n == 0 => return Ok(()),
            Ok(n) => n,
            Err(err) => {
                let kind = err.kind();
                if kind == ErrorKind::WouldBlock {
                    return Ok(());
                }

                bail!("Internal error in LSX connection: {}", kind);
            }
        };

        let trimmed_buffer = &buffer[..n];
        let message = if let EncryptionState::Enabled(key) = self.encryption {
            simple_decrypt(trimmed_buffer, &key)
        } else {
            String::from_utf8_lossy(trimmed_buffer).trim().to_owned()
        };

        let captures = re.captures(message.as_str()).unwrap();
        for group in captures.iter().skip(1) {
            if let Err(err) = self.process_message(group.unwrap().as_str()).await {
                error!("Failed to process message: {}", err);
            }

            if let EncryptionState::Ready(key) = self.encryption {
                self.encryption = EncryptionState::Enabled(key);
            }
        }

        Ok(())
    }

    // Message Sending

    pub fn send_lsx(&mut self, message: LSX) -> Result<()> {
        let mut str = quick_xml::se::to_string(&message)?;
        debug!("Sending LSX Message: {}", str);

        if let EncryptionState::Enabled(key) = self.encryption {
            str = simple_encrypt(str.as_bytes(), &key)
        };

        str += "\0";
        self.send_message(str.as_bytes())
    }

    fn send_message(&mut self, message: &[u8]) -> Result<()> {
        self.stream.write(message)?;
        self.stream.flush()?;
        Ok(())
    }

    // Message Processing

    async fn process_message(&mut self, message: &str) -> Result<()> {
        debug!("Received LSX Message: {}", message);

        let mut message = message.to_string();
        message.remove_matches("version=\"\" ");
        let lsx_message: LSX = quick_xml::de::from_str(message.as_str())?;

        let reply = match lsx_message.value {
            LSXMessageType::Event(msg) => self.process_event_message(msg).await,
            LSXMessageType::Request(msg) => self.process_request_message(msg).await,
            LSXMessageType::Response(_) => unimplemented!(),
        }?;

        if reply.is_some() {
            self.send_lsx(LSX {
                value: reply.unwrap(),
            })?;
        }

        Ok(())
    }

    async fn process_event_message(&mut self, _: LSXEvent) -> Result<Option<LSXMessageType>> {
        Ok(None)
    }

    async fn process_request_message(
        &mut self,
        message: LSXRequest,
    ) -> Result<Option<LSXMessageType>> {
        {
            let mut maxima = self.maxima.lock().await;
            maxima.call_event(MaximaEvent::ReceivedLSXRequest(self.pid, message.value.clone()));
        }

        let result = lsx_message_matcher!(
            self, message.value, LSXRequestType;

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
