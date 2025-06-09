use std::{
    collections::HashMap,
    error::Error,
    io::{self, ErrorKind},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::proto::{communication_v1, Communication, CommunicationV1};
use super::RtmError;
use log::{error, warn};
use prost::{
    bytes::{Buf, BufMut, BytesMut},
    Message,
};
use rustls::{ClientConfig, OwnedTrustAnchor};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{mpsc, oneshot},
    time,
};
use tokio_rustls::TlsConnector;
use webpki_roots::TLS_SERVER_ROOTS;

// TnT as far as I've heard means "Tools and Technology"
pub const RTM_DOMAIN: &str = "rtm.tnt-ea.com";
pub const RTM_TCP_HOST: &str = "rtm.tnt-ea.com:9000";

// We don't use this, but it exists. EA Desktop natively connects to the TCP host,
// and connects to the WS host from the javascript frontend
pub const RTM_WS_HOST: &str = "wss://rtm.tnt-ea.com:8095/websocket";

pub struct RtmRequest {
    id: String,
    payload: communication_v1::Body,
    response_tx: Option<oneshot::Sender<Communication>>,
}

pub struct RtmConnectionManager {
    request_tx: mpsc::Sender<RtmRequest>,
    request_index: u32,
}

impl RtmConnectionManager {
    pub fn new(
        reconnect_delay: Duration,
        update_presence_tx: mpsc::Sender<communication_v1::Body>,
    ) -> RtmConnectionManager {
        let (request_tx, request_rx) = mpsc::channel(32);

        tokio::spawn(async move {
            RtmConnectionManager::run(reconnect_delay, request_rx, update_presence_tx).await;
        });

        Self {
            request_tx: request_tx.clone(),
            request_index: 0,
        }
    }

    async fn run(
        reconnect_delay: Duration,
        mut request_rx: mpsc::Receiver<RtmRequest>,
        mut update_presence_tx: mpsc::Sender<communication_v1::Body>,
    ) {
        loop {
            match TcpStream::connect(RTM_TCP_HOST).await {
                Ok(stream) => {
                    if let Err(e) = RtmConnectionManager::handle_stream(
                        stream,
                        &mut request_rx,
                        &mut update_presence_tx,
                    )
                    .await
                    {
                        println!("Stream error: {}", e);
                        // Reconnection will be attempted after the delay
                    }
                }
                Err(e) => {
                    println!("Failed to connect: {}", e);
                }
            }

            time::sleep(reconnect_delay).await;
        }
    }

    async fn handle_stream(
        stream: TcpStream,
        request_rx: &mut mpsc::Receiver<RtmRequest>,
        update_presence_tx: &mut mpsc::Sender<communication_v1::Body>,
    ) -> Result<(), Box<dyn Error>> {
        let anchors = TLS_SERVER_ROOTS.0.iter().map(|ta| {
            OwnedTrustAnchor::from_subject_spki_name_constraints(
                ta.subject,
                ta.spki,
                ta.name_constraints,
            )
        });

        let mut store = rustls::RootCertStore::empty();
        store.add_server_trust_anchors(anchors);

        let config = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(store)
            .with_no_client_auth();

        let connector = TlsConnector::from(Arc::new(config));

        let domain = rustls::ServerName::try_from(RTM_DOMAIN)?;
        let mut tls_stream = connector.connect(domain, stream).await?;

        let mut pending_responses: HashMap<String, oneshot::Sender<Communication>> = HashMap::new();

        let mut expected_size: i32 = -1;
        let mut bytes = BytesMut::with_capacity(1024 * 4);

        loop {
            tokio::select! {
                size = tls_stream.read_buf(&mut bytes) => {
                    match size {
                        Ok(0) => {
                            warn!("RTM connection closed");
                            break;
                        },
                        Ok(_) => {
                            loop {
                                if bytes.len() < 4 {
                                    break;
                                }

                                if expected_size == -1 {
                                    expected_size = bytes.get_i32();
                                }

                                if bytes.len() < expected_size as usize {
                                    break;
                                }

                                let buf = bytes.clone().freeze().slice(0..expected_size as usize);
                                let msg = Communication::decode(buf)?;
                                let id = &msg.v1.as_ref().ok_or(RtmError::NoBody)?.request_id;

                                if let Some(tx) =
                                    pending_responses.remove(id)
                                {
                                    tx.send(msg).unwrap();
                                } else if id.is_empty() {
                                    if let Some(body) = &msg.v1.as_ref().ok_or(RtmError::NoBody)?.body {
                                        update_presence_tx.send(body.clone()).await?;
                                    }
                                }

                                bytes.advance(expected_size as usize);
                                expected_size = -1;
                            }
                        },
                        Err(e) => {
                            error!("Failed to read from RTM socket: {}", e);
                            break;
                        },
                    }
                },
                request = request_rx.recv(), if expected_size == -1 => {
                    if let Some(request) = request {
                        let communication = Communication {
                            v1: Some(CommunicationV1 {
                                request_id: request.id.to_owned(),
                                body: Some(request.payload),
                            }),
                        };

                        let mut buf = BytesMut::new();
                        buf.put_i32(communication.encoded_len() as i32);
                        communication.encode(&mut buf)?;

                        let frozen = buf.freeze();
                        tls_stream.write_all(frozen.chunk()).await?;

                        if let Some(response_tx) = request.response_tx {
                            pending_responses.insert(request.id, response_tx);
                        }
                    }
                },
            }
        }

        Ok(())
    }

    pub async fn send_request(
        &mut self,
        message: communication_v1::Body,
    ) -> Result<communication_v1::Body, RtmError> {
        let (response_tx, response_rx) = oneshot::channel();
        let request_id = self.get_new_request_id();

        self.request_tx
            .send(RtmRequest {
                id: request_id,
                payload: message,
                response_tx: Some(response_tx),
            })
            .await?;

        match response_rx.await {
            Ok(response) => Ok(response
                .v1
                .ok_or(RtmError::NoBody)?
                .body
                .ok_or(RtmError::NoBody)?),
            Err(_) => Err(RtmError::Io(io::Error::new(
                ErrorKind::Other,
                "Failed to receive response",
            ))),
        }
    }

    pub async fn send_and_forget_request(
        &mut self,
        message: communication_v1::Body,
    ) -> Result<(), RtmError> {
        let request_id = self.get_new_request_id();

        self.request_tx
            .send(RtmRequest {
                id: request_id,
                payload: message,
                response_tx: None,
            })
            .await?;

        Ok(())
    }

    fn get_new_request_id(&mut self) -> String {
        let secs_since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards..?")
            .as_secs();
        let request_id = format!(
            "c-{}-{}-{}",
            self.request_index, secs_since_epoch, secs_since_epoch
        );

        self.request_index += 1;

        request_id
    }
}
