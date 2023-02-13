use std::{
    borrow::Cow,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, OnceLock,
    },
};

use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use serde::{Deserialize, Serialize};
use simd_json::OwnedValue;
use tokio::{net::TcpStream, sync::mpsc};
use tokio_tungstenite::{
    tungstenite::{
        protocol::{frame::coding::CloseCode, CloseFrame},
        Error as TungsteniteError, Message,
    },
    WebSocketStream,
};
use tracing::{debug, enabled, error, info, trace, Level};
use twilight_model::gateway::{
    payload::{
        incoming::{Hello, Ready},
        outgoing::{identify::IdentifyInfo, resume::ResumeInfo},
    },
    OpCode, ShardId,
};

use crate::{
    config::CONFIG,
    script,
    session::{Session, Sessions},
};

const HEARTBEAT_INTERVAL: u64 = 41250;
const PAYLOAD_DECODE_ERROR_MSG: &str = "Error while decoding payload.";
const DISALLOWED_INTENTS_ERROR_MSG: &str = "Disallowed intent(s).";
const AUTHENTICATION_FAILED_ERROR_MSG: &str = "Authentication failed.";
const READY_VERSION: u64 = 6;

#[derive(Debug)]
pub enum Error {
    Websocket(TungsteniteError),
    Sending(mpsc::error::SendError<Message>),
}

impl From<TungsteniteError> for Error {
    fn from(value: TungsteniteError) -> Self {
        Self::Websocket(value)
    }
}

impl From<mpsc::error::SendError<Message>> for Error {
    fn from(value: mpsc::error::SendError<Message>) -> Self {
        Self::Sending(value)
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GatewayEvent {
    t: Option<String>, // None if op is not 0
    s: Option<u64>,    // None if op is not 0
    op: OpCode,
    d: Option<GatewayEventData>,
}

impl GatewayEvent {
    pub fn into_identify(self) -> Result<IdentifyInfo, PayloadError> {
        if let Some(data) = self.d {
            data.into_identify()
        } else {
            Err(PayloadError::InvalidData)
        }
    }

    pub fn into_resume(self) -> Result<ResumeInfo, PayloadError> {
        if let Some(data) = self.d {
            data.into_resume()
        } else {
            Err(PayloadError::InvalidData)
        }
    }

    pub fn heartbeat_ack() -> Self {
        Self {
            t: None,
            s: None,
            op: OpCode::HeartbeatAck,
            d: None,
        }
    }

    pub fn heartbeat() -> Self {
        Self {
            t: None,
            s: None,
            op: OpCode::HeartbeatAck,
            d: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum GatewayEventData {
    Hello(Hello),
    Identify(IdentifyInfo),
    Resume(ResumeInfo),
    InvalidSession(bool),
    Ready(Ready),
    Heartbeat(u64),
    RawDispatch {
        #[serde(skip)]
        event_type: String,
        data: OwnedValue,
    },
    Resumed,
}

pub enum PayloadError {
    InvalidData,
}

impl GatewayEventData {
    pub fn hello() -> Self {
        Self::Hello(Hello {
            heartbeat_interval: HEARTBEAT_INTERVAL,
        })
    }

    pub fn ready(session_id: String, shard: Option<ShardId>) -> Self {
        Self::Ready(Ready {
            application: (&CONFIG.bot).into(),
            guilds: Vec::new(), // TODO
            resume_gateway_url: CONFIG.externally_accessible_url.clone(),
            session_id,
            shard,
            user: (&CONFIG.bot).into(),
            version: READY_VERSION,
        })
    }

    pub fn raw_dispatch(event_type: String, data: OwnedValue) -> Self {
        Self::RawDispatch { event_type, data }
    }

    pub fn into_identify(self) -> Result<IdentifyInfo, PayloadError> {
        if let Self::Identify(identify) = self {
            Ok(identify)
        } else {
            Err(PayloadError::InvalidData)
        }
    }

    pub fn into_resume(self) -> Result<ResumeInfo, PayloadError> {
        if let Self::Resume(resume) = self {
            Ok(resume)
        } else {
            Err(PayloadError::InvalidData)
        }
    }

    pub fn dispatch_event_name(&self) -> Option<&str> {
        match self {
            Self::Ready(_) => Some("READY"),
            Self::Resumed => Some("RESUMED"),
            Self::RawDispatch { event_type, .. } => Some(event_type),
            _ => None,
        }
    }
}

impl From<(u64, GatewayEventData)> for GatewayEvent {
    fn from((sequence, event): (u64, GatewayEventData)) -> Self {
        match &event {
            GatewayEventData::Hello(_) => Self {
                t: None,
                s: None,
                op: OpCode::Hello,
                d: Some(event),
            },
            GatewayEventData::Heartbeat(_) => Self {
                t: None,
                s: None,
                op: OpCode::Heartbeat,
                d: Some(event),
            },
            GatewayEventData::Identify(_) => Self {
                t: None,
                s: None,
                op: OpCode::Identify,
                d: Some(event),
            },
            GatewayEventData::Resume(_) => Self {
                t: None,
                s: None,
                op: OpCode::Resume,
                d: Some(event),
            },
            GatewayEventData::InvalidSession(_) => Self {
                t: None,
                s: None,
                op: OpCode::InvalidSession,
                d: Some(event),
            },
            _ => {
                if let Some(dispatch_event_name) = event.dispatch_event_name() {
                    Self {
                        t: Some(dispatch_event_name.to_string()),
                        s: Some(sequence),
                        op: OpCode::Dispatch,
                        d: Some(event),
                    }
                } else {
                    unreachable!()
                }
            }
        }
    }
}

async fn write_forward_task(
    mut sink: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut rx: mpsc::UnboundedReceiver<Message>,
) {
    while let Some(msg) = rx.recv().await {
        if sink.send(msg).await.is_err() {
            break;
        };
    }
}

#[derive(Clone)]
pub struct WriteHandle {
    sender: mpsc::UnboundedSender<Message>,
    sequence: Arc<AtomicU64>,
}

impl WriteHandle {
    pub fn send(&self, event: GatewayEvent) -> Result<(), Error> {
        match simd_json::to_string(&event) {
            Ok(json) => {
                debug!("Sending {json} to client");
                self.sender.send(Message::Text(json))?;
            }
            Err(e) => {
                error!("Failed to serialize {event:?} to JSON due to {e}");
            }
        };

        Ok(())
    }

    pub fn send_data(&self, event: GatewayEventData) -> Result<(), Error> {
        let sequence = if event.dispatch_event_name().is_some() {
            self.sequence.fetch_add(1, Ordering::Relaxed)
        } else {
            0 // Won't be used
        };

        let event = GatewayEvent::from((sequence, event));

        self.send(event)?;

        Ok(())
    }

    pub fn send_raw(&self, msg: Message) -> Result<(), Error> {
        self.sender.send(msg)?;
        Ok(())
    }

    fn close(&self, close_code: CloseCode, reason: &'static str) -> Result<(), Error> {
        self.send_raw(Message::Close(Some(CloseFrame {
            code: close_code,
            reason: Cow::Borrowed(reason),
        })))?;

        Ok(())
    }
}

#[derive(Clone)]
pub struct ConnectionState {
    pub writer: WriteHandle,
    sessions: Sessions,
    session_id: Arc<OnceLock<String>>,
}

impl ConnectionState {
    pub fn session(&self) -> Option<Session> {
        self.session_id
            .get()
            .and_then(|id| self.sessions.get_session(id))
    }

    pub fn set_session_id(&self, session_id: String) {
        let _ = self.session_id.set(session_id);
    }

    pub fn invalidate_session(&self, resumable: bool) -> Result<(), Error> {
        if let Some(session_id) = self.session_id.get() {
            self.sessions.destroy_session(session_id);
            self.writer
                .send_data(GatewayEventData::InvalidSession(resumable))?;
            self.writer.close(CloseCode::Normal, "")?;
        }

        Ok(())
    }

    fn set_ready(&self) {
        tokio::spawn(script::run(self.clone()));
    }

    fn process(&self, event: GatewayEvent) -> Result<(), Error> {
        match event.op {
            OpCode::Identify => {
                if let Ok(data) = event.into_identify() {
                    let allowed_intents = CONFIG.bot.allowed_intents();

                    if !allowed_intents.contains(data.intents) {
                        self.writer
                            .close(CloseCode::Library(4014), DISALLOWED_INTENTS_ERROR_MSG)?;
                        return Ok(());
                    }

                    if !data.token.strip_prefix("Bot ").contains(&CONFIG.bot.token) {
                        self.writer
                            .close(CloseCode::Library(4004), AUTHENTICATION_FAILED_ERROR_MSG)?;
                        return Ok(());
                    }

                    let session_id = self.sessions.create_session(&data);
                    self.set_session_id(session_id.clone());
                    self.writer
                        .send_data(GatewayEventData::ready(session_id, data.shard))?;

                    // TODO: Startup GUILD_CREATE payloads

                    self.set_ready();

                    info!("Client has identified");
                } else {
                    self.invalidate_session(false)?;
                }
            }
            OpCode::Resume => {
                if let Ok(data) = event.into_resume() {
                    if !data.token.strip_prefix("Bot ").contains(&CONFIG.bot.token) {
                        self.writer
                            .close(CloseCode::Library(4004), AUTHENTICATION_FAILED_ERROR_MSG)?;
                        return Ok(());
                    }

                    if self.sessions.exists(&data.session_id) && !CONFIG.scenarios.expired_sessions
                    {
                        self.set_session_id(data.session_id);
                        self.writer.send_data(GatewayEventData::Resumed)?;
                        self.set_ready();

                        info!("Client has resumed");
                    } else {
                        self.invalidate_session(false)?;
                    };
                } else {
                    self.invalidate_session(false)?;
                }
            }
            OpCode::Heartbeat => {
                if !CONFIG.scenarios.unanswered_heartbeats {
                    // Note: Discord does not validate the heartbeat sequence sent in the data part
                    // of the payload.
                    self.writer.send(GatewayEvent::heartbeat_ack())?;
                }
            }
            _ => debug!("Ignoring event {event:?}"),
        }

        Ok(())
    }
}

pub struct Connection {
    stream: SplitStream<WebSocketStream<TcpStream>>,
    state: ConnectionState,
}

impl Connection {
    pub fn new(stream: WebSocketStream<TcpStream>, sessions: Sessions) -> Self {
        let (sink, stream) = stream.split();
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(write_forward_task(sink, rx));

        let write_handle = WriteHandle {
            sender: tx,
            sequence: Arc::new(AtomicU64::new(0)),
        };

        let state = ConnectionState {
            writer: write_handle,
            sessions,
            session_id: Arc::new(OnceLock::new()),
        };

        Self { stream, state }
    }

    pub fn send(&self, event: GatewayEvent) -> Result<(), Error> {
        self.state.writer.send(event)
    }

    pub fn send_data(&self, event: GatewayEventData) -> Result<(), Error> {
        self.state.writer.send_data(event)
    }

    pub fn send_raw(&self, msg: Message) -> Result<(), Error> {
        self.state.writer.send_raw(msg)
    }

    pub fn close(&self, close_code: CloseCode, reason: &'static str) -> Result<(), Error> {
        self.state.writer.close(close_code, reason)
    }

    pub async fn handle(&mut self) -> Result<(), Error> {
        self.send_data(GatewayEventData::hello())?;

        while let Some(Ok(msg)) = self.stream.next().await {
            if msg.is_text() || msg.is_binary() {
                let mut data = msg.into_data();

                if enabled!(Level::TRACE) {
                    trace!("Got data: {}", String::from_utf8_lossy(&data));
                }

                match simd_json::from_slice::<GatewayEvent>(&mut data) {
                    Ok(event) => {
                        debug!("Got {event:?}");
                        self.state.process(event)?;
                    }
                    Err(e) => {
                        error!("Failed to deserialize client event with {e}");
                        self.close(CloseCode::Library(4002), PAYLOAD_DECODE_ERROR_MSG)?;
                    }
                }
            }
        }

        Ok(())
    }
}
