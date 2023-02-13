use std::{
    fmt::{self, Display},
    time::Duration,
};

use simd_json::OwnedValue;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::{
    config::SCRIPT,
    handler::{ConnectionState, GatewayEvent, GatewayEventData},
};

#[derive(Debug)]
pub enum Action {
    Sleep(Duration),
    InvalidateSession(bool),
    Dispatch {
        event_type: String,
        data: OwnedValue,
    },
    Heartbeat,
    RandomMessageCreate,
    RandomGuildCreate,
    GracefulClose,
    AbruptClose,
    // add more as needed
}

impl TryFrom<(&str, Option<&str>)> for Action {
    type Error = ParseError;

    fn try_from((action_name, arguments): (&str, Option<&str>)) -> Result<Self, Self::Error> {
        match action_name {
            "sleep_ms" => Ok(Self::Sleep(Duration::from_millis(
                arguments
                    .ok_or(ParseError::MissingRequiredArgument)?
                    .parse()
                    .map_err(|_| ParseError::ExpectedInteger)?,
            ))),
            "sleep_s" => Ok(Self::Sleep(Duration::from_secs(
                arguments
                    .ok_or(ParseError::MissingRequiredArgument)?
                    .parse()
                    .map_err(|_| ParseError::ExpectedInteger)?,
            ))),
            "invalidate_session" => Ok(Self::InvalidateSession(
                arguments
                    .ok_or(ParseError::MissingRequiredArgument)?
                    .parse()
                    .map_err(|_| ParseError::ExpectedBoolean)?,
            )),
            "dispatch" => {
                let arguments = arguments.ok_or(ParseError::MissingRequiredArgument)?;
                let (event_type, data) = arguments
                    .split_once(" ")
                    .ok_or(ParseError::MissingRequiredArgument)?;
                let mut data = data.to_string();
                Ok(Self::Dispatch {
                    event_type: event_type.to_string(),
                    data: unsafe { simd_json::from_str(&mut data) }
                        .map_err(|_| ParseError::InvalidJson)?,
                })
            }
            "heartbeat" => Ok(Self::Heartbeat),
            "random_message_create" => Ok(Self::RandomMessageCreate),
            "random_guild_create" => Ok(Self::RandomGuildCreate),
            "graceful_close" => Ok(Self::GracefulClose),
            "abrupt_close" => Ok(Self::AbruptClose),
            _ => Err(ParseError::InvalidAction),
        }
    }
}

pub enum ParseError {
    InvalidAction,
    ExpectedBoolean,
    ExpectedInteger,
    MissingRequiredArgument,
    InvalidJson,
}

impl Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAction => f.write_str("Invalid action"),
            Self::ExpectedBoolean => f.write_str("Expected a boolean"),
            Self::ExpectedInteger => f.write_str("Expected an integer"),
            Self::MissingRequiredArgument => f.write_str("Missing required argument"),
            Self::InvalidJson => f.write_str("Invalid JSON"),
        }
    }
}

pub fn parse(input: &str) -> Result<Vec<Action>, ParseError> {
    let mut actions = Vec::new();

    for line in input.lines() {
        if line.is_empty() {
            continue;
        }

        let (action_name, arguments) = match line.split_once(" ") {
            Some((action_name, arguments)) => (action_name, Some(arguments)),
            None => (line.trim(), None),
        };

        let action = Action::try_from((action_name, arguments))?;
        actions.push(action);
    }

    Ok(actions)
}

pub async fn run(state: ConnectionState) {
    for action in SCRIPT.iter() {
        info!("Running {action:?}");

        match action {
            Action::Sleep(duration) => sleep(*duration).await,
            Action::InvalidateSession(resumable) => {
                let _ = state.invalidate_session(*resumable);
            }
            Action::Dispatch { event_type, data } => {
                let event = GatewayEventData::raw_dispatch(event_type.clone(), data.clone());
                let _ = state.writer.send_data(event);
            }
            Action::Heartbeat => {
                let _ = state.writer.send(GatewayEvent::heartbeat());
            }
            _ => warn!("Skipping action {action:?} because it is currently unimplemented"),
        }
    }
}
