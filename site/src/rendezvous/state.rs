use super::protocol::ServerMessage;
use crate::rate_limit::{RateBucket, RateLimit, allow_event};
use rand::{Rng, distributions::Alphanumeric, rngs::OsRng};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc};

const ROOM_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_ROOMS: usize = 512;
const MAX_ACTIVE_CONNECTIONS: usize = 1024;
const WS_RATE_LIMIT: RateLimit = RateLimit::new(30, Duration::from_secs(60));
const CREATE_RATE_LIMIT: RateLimit = RateLimit::new(10, Duration::from_secs(5 * 60));
const JOIN_RATE_LIMIT: RateLimit = RateLimit::new(60, Duration::from_secs(60));

#[derive(Clone, Default)]
pub struct RendezvousState {
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    active_connections: Arc<Mutex<usize>>,
    ws_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    create_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    join_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
}

pub struct Room {
    pub candidate: String,
    pub caller_tx: mpsc::UnboundedSender<ServerMessage>,
    expires_at: Instant,
}

impl Room {
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

impl RendezvousState {
    pub async fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut rooms = self.rooms.lock().await;
        rooms.retain(|_, room| room.expires_at > now);
    }

    pub async fn try_acquire_connection(&self) -> bool {
        let mut active_connections = self.active_connections.lock().await;
        if *active_connections >= MAX_ACTIVE_CONNECTIONS {
            return false;
        }

        *active_connections += 1;
        true
    }

    pub async fn release_connection(&self) {
        let mut active_connections = self.active_connections.lock().await;
        *active_connections = active_connections.saturating_sub(1);
    }

    pub async fn allow_ws(&self, ip: IpAddr) -> bool {
        allow_event(&self.ws_limits, ip, WS_RATE_LIMIT).await
    }

    pub async fn allow_create(&self, ip: IpAddr) -> bool {
        allow_event(&self.create_limits, ip, CREATE_RATE_LIMIT).await
    }

    pub async fn allow_join(&self, ip: IpAddr) -> bool {
        allow_event(&self.join_limits, ip, JOIN_RATE_LIMIT).await
    }

    pub async fn create_room(
        &self,
        candidate: String,
        caller_tx: mpsc::UnboundedSender<ServerMessage>,
    ) -> Option<String> {
        let mut rooms = self.rooms.lock().await;
        if rooms.len() >= MAX_ACTIVE_ROOMS {
            return None;
        }

        loop {
            let code = generate_code();
            if !rooms.contains_key(&code) {
                rooms.insert(
                    code.clone(),
                    Room {
                        candidate,
                        caller_tx,
                        expires_at: Instant::now() + ROOM_TTL,
                    },
                );
                return Some(code);
            }
        }
    }

    pub async fn take_room(&self, code: &str) -> Option<Room> {
        let mut rooms = self.rooms.lock().await;
        rooms.remove(code)
    }

    pub async fn remove_room(&self, code: &str) {
        let mut rooms = self.rooms.lock().await;
        rooms.remove(code);
    }
}

fn generate_code() -> String {
    OsRng
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(char::from)
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

pub fn valid_code(code: &str) -> bool {
    code.len() == 16 && code.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connection_cap_releases_capacity() {
        let state = RendezvousState::default();

        assert!(state.try_acquire_connection().await);
        state.release_connection().await;
        assert!(state.try_acquire_connection().await);
        state.release_connection().await;
    }
}
