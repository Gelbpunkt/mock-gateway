use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use rand::{distributions::Alphanumeric, thread_rng, Rng};
use twilight_model::gateway::{payload::outgoing::identify::IdentifyInfo, Intents, ShardId};

type SessionId = String;

#[derive(Clone)]
pub struct Sessions(Arc<Mutex<HashMap<SessionId, Session>>>);

impl Sessions {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn create_session(&self, identify: &IdentifyInfo) -> SessionId {
        let session = Session::from(identify);

        // Session IDs are 32 bytes of ASCII
        let mut rng = thread_rng();
        let session_id: String = std::iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .map(char::from)
            .take(32)
            .collect();

        self.0
            .lock()
            .expect("Sessions mutex poisoned")
            .insert(session_id.clone(), session);

        session_id
    }

    pub fn get_session(&self, session_id: &SessionId) -> Option<Session> {
        self.0
            .lock()
            .expect("Sessions mutex poisoned")
            .get(session_id)
            .cloned()
    }

    pub fn exists(&self, session_id: &SessionId) -> bool {
        self.0
            .lock()
            .expect("Sessions mutex poisoned")
            .contains_key(session_id)
    }

    pub fn destroy_session(&self, session_id: &SessionId) {
        self.0
            .lock()
            .expect("Sessions mutex poisoned")
            .remove(session_id);
    }
}

#[derive(Clone)]
pub struct Session {
    /// Shard ID of the session.
    shard_id: Option<ShardId>,
    /// Compression as requested in IDENTIFY.
    compress: bool,
    /// Intents as requested in IDENTIFY.
    intents: Intents,
}

impl From<&IdentifyInfo> for Session {
    fn from(value: &IdentifyInfo) -> Self {
        Self {
            shard_id: value.shard,
            compress: value.compress,
            intents: value.intents,
        }
    }
}
