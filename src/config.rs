use std::{
    fmt::{Display, Formatter, Result as FmtResult},
    fs::read_to_string,
    process::exit,
    sync::LazyLock,
};

use serde::Deserialize;
use simd_json::Error as JsonError;
use twilight_model::{
    gateway::Intents,
    id::{
        marker::{ApplicationMarker, UserMarker},
        Id,
    },
    oauth::{ApplicationFlags, PartialApplication},
    user::{CurrentUser, UserFlags},
    util::ImageHash,
};

use crate::script::{self, Action};

#[derive(Deserialize)]
pub struct Config {
    pub log_level: String,
    pub port: u16,
    pub externally_accessible_url: String,
    pub scenarios: Scenarios,
    pub bot: Bot,
    pub mock_data: MockData,
}

#[derive(Deserialize)]
pub struct Bot {
    pub token: String,
    pub application_flags: ApplicationFlags,
    pub application_id: Id<ApplicationMarker>,
    pub user_flags: Option<UserFlags>,
    pub user_id: Id<UserMarker>,
    pub avatar: Option<String>,
    pub discriminator: u16,
    pub name: String,
    pub public_flags: Option<UserFlags>,
}

impl Into<CurrentUser> for &Bot {
    fn into(self) -> CurrentUser {
        CurrentUser {
            accent_color: None, // Bots don't have this
            avatar: self
                .avatar
                .as_ref()
                .and_then(|avatar| ImageHash::parse(avatar.as_bytes()).ok()),
            banner: None, // Bots don't have this
            bot: true,
            discriminator: self.discriminator,
            email: None, // Bots don't have this
            flags: self.user_flags,
            id: self.user_id,
            locale: None,      // Bots don't have this
            mfa_enabled: true, // Always true for bots
            name: self.name.clone(),
            premium_type: None, // Bots don't have this
            public_flags: self.public_flags,
            verified: Some(true), // Always true for bots
        }
    }
}

impl Into<PartialApplication> for &Bot {
    fn into(self) -> PartialApplication {
        PartialApplication {
            flags: self.application_flags,
            id: self.application_id,
        }
    }
}

impl Bot {
    pub fn allowed_intents(&self) -> Intents {
        let mut intents = Intents::all();

        if !self.application_flags.intersects(
            ApplicationFlags::GATEWAY_PRESENCE | ApplicationFlags::GATEWAY_PRESENCE_LIMITED,
        ) {
            intents.remove(Intents::GUILD_PRESENCES);
        }

        if !self.application_flags.intersects(
            ApplicationFlags::GATEWAY_GUILD_MEMBERS
                | ApplicationFlags::GATEWAY_GUILD_MEMBERS_LIMITED,
        ) {
            intents.remove(Intents::GUILD_MEMBERS);
        }

        if !self.application_flags.intersects(
            ApplicationFlags::GATEWAY_MESSAGE_CONTENT
                | ApplicationFlags::GATEWAY_MESSAGE_CONTENT_LIMITED,
        ) {
            intents.remove(Intents::MESSAGE_CONTENT);
        }

        intents
    }
}

#[derive(Deserialize)]
pub struct Scenarios {
    /// All heartbeats sent by the client will go unanswered and not be
    /// acknowledged.
    pub unanswered_heartbeats: bool,
    /// All resumes will fail and result in invalid sessions.
    pub expired_sessions: bool,
}

#[derive(Deserialize)]
pub struct MockData {
    guilds: u32,
    users: u32,
    channels: u32,
    voice_states: u32,
}

pub enum Error {
    InvalidConfig(JsonError),
    NotFound(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidConfig(s) => s.fmt(f),
            Self::NotFound(s) => f.write_fmt(format_args!("File {s} not found or access denied")),
        }
    }
}

pub fn load(path: &str) -> Result<Config, Error> {
    let mut content = read_to_string(path).map_err(|_| Error::NotFound(path.to_string()))?;
    let config = unsafe { simd_json::from_str(&mut content) }.map_err(Error::InvalidConfig)?;

    Ok(config)
}

pub static CONFIG: LazyLock<Config> = LazyLock::new(|| {
    match load("config.json") {
        Ok(config) => config,
        Err(err) => {
            // Avoid panicking
            eprintln!("Config Error: {err}");
            exit(1);
        }
    }
});

pub static SCRIPT: LazyLock<Vec<Action>> = LazyLock::new(|| {
    let script_content = read_to_string("script.txt").unwrap_or_default();

    match script::parse(&script_content) {
        Ok(script) => script,
        Err(err) => {
            eprintln!("Failed to parse script: {err}");
            exit(1);
        }
    }
});
