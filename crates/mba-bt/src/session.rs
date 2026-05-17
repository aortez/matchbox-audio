use std::sync::{Arc, Mutex};

use mba_protocol::{ErrorCode, ProtocolError};

#[derive(Debug, Clone, Default)]
pub struct SessionGate {
    active_client: Arc<Mutex<Option<ClientId>>>,
}

pub type ClientId = u64;

impl SessionGate {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn connect(&self, client_id: ClientId) -> Result<ActiveSession, ProtocolError> {
        let mut active = self.active_client.lock().expect("session gate mutex");
        match *active {
            Some(existing) if existing != client_id => Err(ProtocolError::new(
                ErrorCode::Busy,
                format!("client {existing} is already connected"),
            )),
            Some(_) => Err(ProtocolError::new(
                ErrorCode::Busy,
                format!("client {client_id} is already connected"),
            )),
            None => {
                *active = Some(client_id);
                Ok(ActiveSession {
                    client_id,
                    active_client: Arc::clone(&self.active_client),
                })
            }
        }
    }

    pub fn active_client(&self) -> Option<ClientId> {
        *self.active_client.lock().expect("session gate mutex")
    }
}

#[derive(Debug)]
pub struct ActiveSession {
    client_id: ClientId,
    active_client: Arc<Mutex<Option<ClientId>>>,
}

impl ActiveSession {
    pub fn client_id(&self) -> ClientId {
        self.client_id
    }
}

impl Drop for ActiveSession {
    fn drop(&mut self) {
        let mut active = self.active_client.lock().expect("session gate mutex");
        if *active == Some(self.client_id) {
            *active = None;
        }
    }
}
