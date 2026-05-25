use axum::extract::ws::WebSocket;
use rand::{Rng, distributions::Alphanumeric, rngs::OsRng};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, oneshot};

use crate::rate_limit::{RateBucket, RateLimit, allow_event};

const ROOM_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_ROOMS: usize = 256;
const CONNECTION_RATE_LIMIT: RateLimit = RateLimit::new(60, Duration::from_secs(60));
const CREATE_RATE_LIMIT: RateLimit = RateLimit::new(10, Duration::from_secs(5 * 60));
const JOIN_RATE_LIMIT: RateLimit = RateLimit::new(60, Duration::from_secs(60));

#[derive(Clone, Default)]
pub struct RelayState {
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    connection_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    create_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    join_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
}

pub struct Room {
    pub join_tx: oneshot::Sender<WebSocket>,
    expires_at: Instant,
}

impl Room {
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

impl RelayState {
    pub async fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut rooms = self.rooms.lock().await;
        rooms.retain(|_, room| room.expires_at > now);
    }

    pub async fn allow_connection(&self, ip: IpAddr) -> bool {
        allow_event(&self.connection_limits, ip, CONNECTION_RATE_LIMIT).await
    }

    pub async fn allow_create(&self, ip: IpAddr) -> bool {
        allow_event(&self.create_limits, ip, CREATE_RATE_LIMIT).await
    }

    pub async fn allow_join(&self, ip: IpAddr) -> bool {
        allow_event(&self.join_limits, ip, JOIN_RATE_LIMIT).await
    }

    pub async fn create_room(&self, join_tx: oneshot::Sender<WebSocket>) -> Option<String> {
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
                        join_tx,
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
