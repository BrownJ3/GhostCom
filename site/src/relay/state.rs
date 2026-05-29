use axum::extract::ws::WebSocket;
use rand::{Rng, distributions::Alphanumeric, rngs::OsRng};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc, oneshot};
use zeroize::Zeroize;

use crate::{
    config::SiteConfig,
    rate_limit::{RateBucket, RateLimit, allow_event, allow_global_event},
};

const ROOM_TTL: Duration = Duration::from_secs(5 * 60);
const MAX_ACTIVE_ROOMS: usize = 64;
const MAX_ACTIVE_CONNECTIONS: usize = 128;
const MAX_ACTIVE_SESSIONS: usize = 32;
const CONNECTION_RATE_LIMIT: RateLimit = RateLimit::new(30, Duration::from_secs(60));
const CREATE_RATE_LIMIT: RateLimit = RateLimit::new(6, Duration::from_secs(5 * 60));
const JOIN_RATE_LIMIT: RateLimit = RateLimit::new(30, Duration::from_secs(60));
const GLOBAL_CONNECTION_RATE_LIMIT: RateLimit = RateLimit::new(240, Duration::from_secs(60));
const GLOBAL_CREATE_RATE_LIMIT: RateLimit = RateLimit::new(40, Duration::from_secs(5 * 60));
const GLOBAL_JOIN_RATE_LIMIT: RateLimit = RateLimit::new(180, Duration::from_secs(60));

#[derive(Clone)]
pub struct RelayState {
    config: SiteConfig,
    rooms: Arc<Mutex<HashMap<String, Room>>>,
    group_rooms: Arc<Mutex<HashMap<String, GroupRoom>>>,
    active_connections: Arc<Mutex<usize>>,
    active_sessions: Arc<Mutex<usize>>,
    connection_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    create_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    join_limits: Arc<Mutex<HashMap<IpAddr, RateBucket>>>,
    global_connection_limit: Arc<Mutex<RateBucket>>,
    global_create_limit: Arc<Mutex<RateBucket>>,
    global_join_limit: Arc<Mutex<RateBucket>>,
}

pub struct Room {
    pub join_tx: oneshot::Sender<WebSocket>,
    expires_at: Instant,
}

pub struct GroupRoom {
    pub join_tx: mpsc::Sender<GroupJoin>,
    next_peer_id: u16,
    expires_at: Instant,
}

pub struct GroupJoin {
    pub peer_id: u16,
    pub socket: WebSocket,
}

impl Room {
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

impl GroupRoom {
    pub fn is_expired(&self) -> bool {
        Instant::now() > self.expires_at
    }
}

impl RelayState {
    pub fn new(config: SiteConfig) -> Self {
        let now = Instant::now();
        Self {
            config,
            rooms: Arc::default(),
            group_rooms: Arc::default(),
            active_connections: Arc::default(),
            active_sessions: Arc::default(),
            connection_limits: Arc::default(),
            create_limits: Arc::default(),
            join_limits: Arc::default(),
            global_connection_limit: Arc::new(Mutex::new(RateBucket::new(now))),
            global_create_limit: Arc::new(Mutex::new(RateBucket::new(now))),
            global_join_limit: Arc::new(Mutex::new(RateBucket::new(now))),
        }
    }

    pub fn relay_enabled(&self) -> bool {
        self.config.relay_enabled
    }

    pub fn access_token_matches(&self, supplied: Option<&str>) -> bool {
        self.config.token_matches(supplied)
    }

    pub async fn cleanup_expired(&self) {
        let now = Instant::now();
        let mut rooms = self.rooms.lock().await;
        let mut expired_codes: Vec<String> = rooms
            .iter()
            .filter(|(_, room)| room.expires_at <= now)
            .map(|(code, _)| code.clone())
            .collect();
        for code in &mut expired_codes {
            remove_room_entry(&mut rooms, &code);
            code.zeroize();
        }
        drop(rooms);

        let mut group_rooms = self.group_rooms.lock().await;
        let mut expired_codes: Vec<String> = group_rooms
            .iter()
            .filter(|(_, room)| room.expires_at <= now)
            .map(|(code, _)| code.clone())
            .collect();
        for code in &mut expired_codes {
            remove_group_room_entry(&mut group_rooms, &code);
            code.zeroize();
        }
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

    pub async fn try_acquire_session(&self) -> bool {
        let mut active_sessions = self.active_sessions.lock().await;
        if *active_sessions >= MAX_ACTIVE_SESSIONS {
            return false;
        }

        *active_sessions += 1;
        true
    }

    pub async fn release_session(&self) {
        let mut active_sessions = self.active_sessions.lock().await;
        *active_sessions = active_sessions.saturating_sub(1);
    }

    pub async fn allow_connection(&self, ip: IpAddr) -> bool {
        allow_global_event(&self.global_connection_limit, GLOBAL_CONNECTION_RATE_LIMIT).await
            && allow_event(&self.connection_limits, ip, CONNECTION_RATE_LIMIT).await
    }

    pub async fn allow_create(&self, ip: IpAddr) -> bool {
        allow_global_event(&self.global_create_limit, GLOBAL_CREATE_RATE_LIMIT).await
            && allow_event(&self.create_limits, ip, CREATE_RATE_LIMIT).await
    }

    pub async fn allow_join(&self, ip: IpAddr) -> bool {
        allow_global_event(&self.global_join_limit, GLOBAL_JOIN_RATE_LIMIT).await
            && allow_event(&self.join_limits, ip, JOIN_RATE_LIMIT).await
    }

    pub async fn create_room(&self, join_tx: oneshot::Sender<WebSocket>) -> Option<String> {
        let mut rooms = self.rooms.lock().await;
        if rooms.len() >= MAX_ACTIVE_ROOMS {
            return None;
        }

        loop {
            let mut code = generate_code();
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
            code.zeroize();
        }
    }

    pub async fn create_group_room(&self, join_tx: mpsc::Sender<GroupJoin>) -> Option<String> {
        let mut group_rooms = self.group_rooms.lock().await;
        if group_rooms.len() >= MAX_ACTIVE_ROOMS {
            return None;
        }

        loop {
            let mut code = generate_code();
            if !group_rooms.contains_key(&code) {
                group_rooms.insert(
                    code.clone(),
                    GroupRoom {
                        join_tx,
                        next_peer_id: 1,
                        expires_at: Instant::now() + ROOM_TTL,
                    },
                );
                return Some(code);
            }
            code.zeroize();
        }
    }

    pub async fn take_room(&self, code: &str) -> Option<Room> {
        let mut rooms = self.rooms.lock().await;
        remove_room_entry(&mut rooms, code)
    }

    pub async fn join_group_room(&self, code: &str, socket: WebSocket) -> Result<u16, WebSocket> {
        let mut group_rooms = self.group_rooms.lock().await;
        let Some(room) = group_rooms.get_mut(code) else {
            return Err(socket);
        };

        if room.is_expired() || room.join_tx.is_closed() {
            remove_group_room_entry(&mut group_rooms, code);
            return Err(socket);
        }
        let peer_id = room.next_peer_id;
        room.next_peer_id = room.next_peer_id.saturating_add(1);

        match room.join_tx.try_send(GroupJoin { peer_id, socket }) {
            Ok(()) => Ok(peer_id),
            Err(error) => Err(error.into_inner().socket),
        }
    }

    pub async fn remove_room(&self, code: &str) {
        let mut rooms = self.rooms.lock().await;
        remove_room_entry(&mut rooms, code);
    }

    pub async fn remove_group_room(&self, code: &str) {
        let mut group_rooms = self.group_rooms.lock().await;
        remove_group_room_entry(&mut group_rooms, code);
    }
}

fn remove_room_entry(rooms: &mut HashMap<String, Room>, code: &str) -> Option<Room> {
    rooms.remove_entry(code).map(|(mut code, room)| {
        code.zeroize();
        room
    })
}

fn remove_group_room_entry(
    group_rooms: &mut HashMap<String, GroupRoom>,
    code: &str,
) -> Option<GroupRoom> {
    group_rooms.remove_entry(code).map(|(mut code, room)| {
        code.zeroize();
        room
    })
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
        let state = RelayState::new(SiteConfig::default());

        assert!(state.try_acquire_connection().await);
        state.release_connection().await;
        assert!(state.try_acquire_connection().await);
        state.release_connection().await;
    }

    #[tokio::test]
    async fn session_cap_releases_capacity() {
        let state = RelayState::new(SiteConfig::default());

        assert!(state.try_acquire_session().await);
        state.release_session().await;
        assert!(state.try_acquire_session().await);
        state.release_session().await;
    }
}
