use crate::client_ip::resolve_client_ip;
use axum::extract::{
    ConnectInfo, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::http::HeaderMap;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use protocol::{ClientMessage, DeviceAuth, HostControlMessage, ServerMessage};
use sha2::{Digest, Sha256};
pub use state::RelayState;
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroize;

mod protocol;
mod state;

const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);
const ROOM_WAIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_WS_TEXT_BYTES: usize = 512;
const MAX_RELAY_BINARY_BYTES: usize = 32 * 1024;
const MAX_RELAY_BYTES_PER_DIRECTION: u64 = 8 * 1024 * 1024;
const RELAY_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_RELAY_SESSION_DURATION: Duration = Duration::from_secs(60 * 60);
const GROUP_JOIN_QUEUE: usize = 16;
const GROUP_MAX_PEERS: usize = 8;
const GROUP_PEER_ID_BYTES: usize = 2;
const DEVICE_PUBLIC_KEY_BYTES: usize = 32;
const DEVICE_SIGNATURE_BYTES: usize = 64;
const DEVICE_APPROVAL_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

enum GroupPeerEvent {
    Binary {
        peer_id: u16,
        bytes: axum::body::Bytes,
    },
    Closed {
        peer_id: u16,
    },
}

pub async fn relay_ws(
    State(state): State<RelayState>,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl axum::response::IntoResponse {
    let client_ip = resolve_client_ip(&headers, remote_addr);
    ws.max_message_size(MAX_RELAY_BINARY_BYTES)
        .max_frame_size(MAX_RELAY_BINARY_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state, client_ip))
}

async fn handle_socket(mut socket: WebSocket, state: RelayState, remote_ip: IpAddr) {
    if !state.relay_enabled() {
        send_error(&mut socket, "relay is unavailable").await;
        return;
    }

    if !state.try_acquire_connection().await {
        send_error(&mut socket, "relay server has too many active connections").await;
        return;
    }

    handle_socket_inner(socket, state.clone(), remote_ip).await;
    state.release_connection().await;
}

async fn handle_socket_inner(mut socket: WebSocket, state: RelayState, remote_ip: IpAddr) {
    if !state.allow_connection(remote_ip).await {
        send_error(&mut socket, "too many relay connection attempts").await;
        return;
    }

    let mut first_message = match tokio::time::timeout(FIRST_MESSAGE_TIMEOUT, socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= MAX_WS_TEXT_BYTES => text.to_string(),
        _ => {
            send_error(&mut socket, "expected small relay setup message").await;
            return;
        }
    };

    let mut client_message = match serde_json::from_str::<ClientMessage>(&first_message) {
        Ok(message) => message,
        Err(_) => {
            first_message.zeroize();
            send_error(&mut socket, "invalid relay message").await;
            return;
        }
    };
    first_message.zeroize();

    state.cleanup_expired().await;

    match &mut client_message {
        ClientMessage::Create {
            access_token,
            device_auth,
        } => {
            if !relay_access_allowed(&state, access_token.as_deref(), device_auth.as_ref(), "create", None).await {
                scrub_access_token(access_token);
                send_relay_access_denied(&mut socket, &state, device_auth.as_ref()).await;
                return;
            }
            scrub_access_token(access_token);
            handle_create(socket, state, remote_ip).await;
        }
        ClientMessage::GroupCreate {
            access_token,
            device_auth,
        } => {
            if !relay_access_allowed(&state, access_token.as_deref(), device_auth.as_ref(), "group_create", None).await {
                scrub_access_token(access_token);
                send_relay_access_denied(&mut socket, &state, device_auth.as_ref()).await;
                return;
            }
            scrub_access_token(access_token);
            handle_group_create(socket, state, remote_ip).await;
        }
        ClientMessage::Join {
            code,
            access_token,
            device_auth,
        } => {
            if !relay_access_allowed(&state, access_token.as_deref(), device_auth.as_ref(), "join", Some(code)).await {
                scrub_access_token(access_token);
                code.zeroize();
                send_relay_access_denied(&mut socket, &state, device_auth.as_ref()).await;
                return;
            }
            scrub_access_token(access_token);
            handle_join(socket, state, remote_ip, code).await;
            code.zeroize();
        }
        ClientMessage::GroupJoin {
            code,
            access_token,
            device_auth,
        } => {
            if !relay_access_allowed(&state, access_token.as_deref(), device_auth.as_ref(), "group_join", Some(code)).await {
                scrub_access_token(access_token);
                code.zeroize();
                send_relay_access_denied(&mut socket, &state, device_auth.as_ref()).await;
                return;
            }
            scrub_access_token(access_token);
            handle_group_join(socket, state, remote_ip, code).await;
            code.zeroize();
        }
    }
}

fn scrub_access_token(access_token: &mut Option<String>) {
    if let Some(token) = access_token {
        token.zeroize();
    }
    *access_token = None;
}

async fn relay_access_allowed(
    state: &RelayState,
    access_token: Option<&str>,
    device_auth: Option<&DeviceAuth>,
    action: &str,
    code: Option<&str>,
) -> bool {
    if !state.access_token_matches(access_token) {
        return false;
    }

    if !state.requires_device_key() {
        return true;
    }

    let Some(device_auth) = device_auth else {
        return false;
    };

    // Key is in the list but its expiry has passed — operator explicitly revoked it.
    if state.device_key_is_revoked(&device_auth.public_key) {
        return false;
    }

    // Key is in the static allowlist and not expired → verify signature and pass.
    if state.device_key_allowed(&device_auth.public_key) {
        return verify_device_signature(device_auth, action, code);
    }

    // Key is in the runtime-approved set (already auto-registered this session).
    if state.device_key_allowed_runtime(&device_auth.public_key).await {
        return verify_device_signature(device_auth, action, code);
    }

    // Key is unknown. Access token already passed and signature is valid →
    // auto-register so this device can connect without any operator step.
    if verify_device_signature(device_auth, action, code) {
        state.approve_device(device_auth.public_key.clone()).await;
        return true;
    }

    false
}

fn verify_device_signature(device_auth: &DeviceAuth, action: &str, code: Option<&str>) -> bool {
    let Ok(public_key_bytes) = URL_SAFE_NO_PAD.decode(&device_auth.public_key) else {
        return false;
    };
    let Ok(signature_bytes) = URL_SAFE_NO_PAD.decode(&device_auth.signature) else {
        return false;
    };
    if public_key_bytes.len() != DEVICE_PUBLIC_KEY_BYTES
        || signature_bytes.len() != DEVICE_SIGNATURE_BYTES
    {
        return false;
    }

    let Ok(public_key) = VerifyingKey::from_bytes(&public_key_bytes.try_into().unwrap()) else {
        return false;
    };
    let signature = Signature::from_bytes(&signature_bytes.try_into().unwrap());
    let payload = protocol::device_auth_payload(action, code, &device_auth.nonce);

    public_key.verify(payload.as_bytes(), &signature).is_ok()
}

fn device_fingerprint(public_key: &str) -> String {
    let digest = Sha256::digest(public_key.as_bytes());
    digest[..8]
        .chunks(2)
        .map(|chunk| {
            chunk
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("-")
}

async fn handle_create(mut caller: WebSocket, state: RelayState, remote_ip: IpAddr) {
    if !state.allow_create(remote_ip).await {
        send_error(&mut caller, "too many relay creation attempts").await;
        return;
    }

    let (join_tx, join_rx) = oneshot::channel();
    let Some(mut code) = state.create_room(join_tx).await else {
        send_error(&mut caller, "relay server has too many active rooms").await;
        return;
    };

    if send_server_message(&mut caller, ServerMessage::Created { code: code.clone() })
        .await
        .is_err()
    {
        state.remove_room(&code).await;
        code.zeroize();
        return;
    }

    let mut joiner = match tokio::time::timeout(ROOM_WAIT_TIMEOUT, join_rx).await {
        Ok(Ok(joiner)) => joiner,
        _ => {
            state.remove_room(&code).await;
            let _ = send_server_message(
                &mut caller,
                ServerMessage::Error {
                    message: "relay invite expired".to_string(),
                },
            )
            .await;
            code.zeroize();
            return;
        }
    };

    if send_server_message(&mut caller, ServerMessage::PeerJoined)
        .await
        .is_err()
    {
        code.zeroize();
        return;
    }

    if !state.try_acquire_session().await {
        send_error(&mut caller, "relay server has too many active sessions").await;
        send_error(&mut joiner, "relay server has too many active sessions").await;
        code.zeroize();
        return;
    }

    bridge(caller, joiner).await;
    code.zeroize();
    state.release_session().await;
}

async fn handle_join(mut joiner: WebSocket, state: RelayState, remote_ip: IpAddr, code: &str) {
    if !state.allow_join(remote_ip).await {
        send_error(&mut joiner, "too many relay join attempts").await;
        return;
    }

    if !state::valid_code(&code) {
        send_error(&mut joiner, "invalid relay invite code").await;
        return;
    }

    let Some(room) = state.take_room(&code).await else {
        send_error(&mut joiner, "relay invite not found or expired").await;
        return;
    };

    if room.is_expired() {
        send_error(&mut joiner, "relay invite expired").await;
        return;
    }

    if send_server_message(&mut joiner, ServerMessage::Joined)
        .await
        .is_err()
    {
        return;
    }

    let _ = room.join_tx.send(joiner);
}

async fn handle_group_create(mut host: WebSocket, state: RelayState, remote_ip: IpAddr) {
    if !state.allow_create(remote_ip).await {
        send_error(&mut host, "too many relay creation attempts").await;
        return;
    }

    let (join_tx, join_rx) = mpsc::channel(GROUP_JOIN_QUEUE);
    let Some(mut code) = state.create_group_room(join_tx).await else {
        send_error(&mut host, "relay server has too many active rooms").await;
        return;
    };

    if send_server_message(&mut host, ServerMessage::GroupCreated { code: code.clone() })
        .await
        .is_err()
    {
        state.remove_group_room(&code).await;
        code.zeroize();
        return;
    }

    if !state.try_acquire_session().await {
        state.remove_group_room(&code).await;
        send_error(&mut host, "relay server has too many active sessions").await;
        code.zeroize();
        return;
    }

    run_group_room(host, join_rx, state.clone(), code.clone()).await;
    state.remove_group_room(&code).await;
    code.zeroize();
    state.release_session().await;
}

async fn handle_group_join(
    mut joiner: WebSocket,
    state: RelayState,
    remote_ip: IpAddr,
    code: &str,
) {
    if !state.allow_join(remote_ip).await {
        send_error(&mut joiner, "too many relay join attempts").await;
        return;
    }

    if !state::valid_code(&code) {
        send_error(&mut joiner, "invalid relay invite code").await;
        return;
    }

    match state.join_group_room(&code, joiner).await {
        Ok(_) => {}
        Err(mut joiner) => send_error(&mut joiner, "relay invite not found or expired").await,
    }
}

async fn run_group_room(
    mut host: WebSocket,
    mut join_rx: mpsc::Receiver<state::GroupJoin>,
    state: RelayState,
    mut code: String,
) {
    let mut peers = HashMap::new();
    let (peer_event_tx, mut peer_event_rx) = mpsc::channel::<GroupPeerEvent>(64);
    let session_deadline = tokio::time::sleep(MAX_RELAY_SESSION_DURATION);
    tokio::pin!(session_deadline);

    loop {
        tokio::select! {
            join = join_rx.recv(), if peers.len() < GROUP_MAX_PEERS => {
                let Some(mut join) = join else {
                    break;
                };

                if send_server_message(&mut join.socket, ServerMessage::GroupJoined).await.is_err() {
                    continue;
                }
                if send_server_message(&mut host, ServerMessage::GroupPeerJoined { peer_id: join.peer_id }).await.is_err() {
                    break;
                }
                let (peer_tx, peer_rx) = join.socket.split();
                peers.insert(join.peer_id, peer_tx);
                tokio::spawn(forward_group_peer(join.peer_id, peer_rx, peer_event_tx.clone()));
            }
            message = host.next() => {
                let Some(Ok(message)) = message else {
                    break;
                };

                match message {
                    Message::Text(text) if text.len() <= MAX_WS_TEXT_BYTES => {
                        let mut text = text.to_string();
                        match serde_json::from_str::<HostControlMessage>(&text) {
                            Ok(HostControlMessage::CloseGroupInvite) => {
                                state.remove_group_room(&code).await;
                                text.zeroize();
                            }
                            Err(_) => {
                                text.zeroize();
                                break;
                            }
                        }
                    }
                    Message::Binary(bytes) if bytes.len() > GROUP_PEER_ID_BYTES && bytes.len() <= MAX_RELAY_BINARY_BYTES + GROUP_PEER_ID_BYTES => {
                        let peer_id = u16::from_be_bytes([bytes[0], bytes[1]]);
                        let payload = bytes.slice(GROUP_PEER_ID_BYTES..);
                        let Some(peer) = peers.get_mut(&peer_id) else {
                            continue;
                        };
                        if peer.send(Message::Binary(payload)).await.is_err() {
                            peers.remove(&peer_id);
                            let _ = send_server_message(&mut host, ServerMessage::GroupPeerLeft { peer_id }).await;
                        }
                    }
                    Message::Close(_) => break,
                    _ => break,
                }
            }
            peer_event = peer_event_rx.recv() => {
                let Some(peer_event) = peer_event else {
                    break;
                };
                match peer_event {
                    GroupPeerEvent::Binary { peer_id, bytes } => {
                        if !peers.contains_key(&peer_id) {
                            continue;
                        }
                        let mut envelope = Vec::with_capacity(GROUP_PEER_ID_BYTES + bytes.len());
                        envelope.extend_from_slice(&peer_id.to_be_bytes());
                        envelope.extend_from_slice(&bytes);
                        if host.send(Message::Binary(envelope.into())).await.is_err() {
                            return;
                        }
                    }
                    GroupPeerEvent::Closed { peer_id } => {
                        if peers.remove(&peer_id).is_some() {
                            let _ = send_server_message(&mut host, ServerMessage::GroupPeerLeft { peer_id }).await;
                        }
                    }
                }
            }
            _ = &mut session_deadline => break,
            _ = tokio::time::sleep(RELAY_IDLE_TIMEOUT), if peers.is_empty() => break,
            else => break,
        }
    }

    for (_, mut peer) in peers {
        let _ = peer.send(Message::Close(None)).await;
    }
    let _ = host.send(Message::Close(None)).await;
    code.zeroize();
}

async fn forward_group_peer(
    peer_id: u16,
    mut peer_rx: SplitStream<WebSocket>,
    peer_event_tx: mpsc::Sender<GroupPeerEvent>,
) {
    let mut forwarded = 0_u64;

    while let Ok(Some(Ok(message))) = tokio::time::timeout(RELAY_IDLE_TIMEOUT, peer_rx.next()).await
    {
        match message {
            Message::Binary(bytes) if bytes.len() <= MAX_RELAY_BINARY_BYTES => {
                forwarded = forwarded.saturating_add(bytes.len() as u64);
                if forwarded > MAX_RELAY_BYTES_PER_DIRECTION {
                    break;
                }
                if peer_event_tx
                    .send(GroupPeerEvent::Binary { peer_id, bytes })
                    .await
                    .is_err()
                {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => break,
        }
    }

    let _ = peer_event_tx.send(GroupPeerEvent::Closed { peer_id }).await;
}

async fn bridge(caller: WebSocket, joiner: WebSocket) {
    let (caller_tx, caller_rx) = caller.split();
    let (joiner_tx, joiner_rx) = joiner.split();

    let caller_to_joiner = forward_binary(caller_rx, joiner_tx);
    let joiner_to_caller = forward_binary(joiner_rx, caller_tx);

    let session_deadline = tokio::time::sleep(MAX_RELAY_SESSION_DURATION);
    tokio::pin!(session_deadline);

    tokio::select! {
        _ = caller_to_joiner => {}
        _ = joiner_to_caller => {}
        _ = &mut session_deadline => {}
    }
}

async fn forward_binary<S>(mut reader: SplitStream<WebSocket>, mut writer: S)
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let mut forwarded = 0_u64;

    while let Ok(Some(Ok(message))) = tokio::time::timeout(RELAY_IDLE_TIMEOUT, reader.next()).await
    {
        match message {
            Message::Binary(bytes) if bytes.len() <= MAX_RELAY_BINARY_BYTES => {
                forwarded = forwarded.saturating_add(bytes.len() as u64);
                if forwarded > MAX_RELAY_BYTES_PER_DIRECTION {
                    break;
                }
                if writer.send(Message::Binary(bytes)).await.is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => break,
        }
    }

    let _ = writer.send(Message::Close(None)).await;
}

async fn send_error(socket: &mut WebSocket, message: &str) {
    let _ = send_server_message(
        socket,
        ServerMessage::Error {
            message: message.to_string(),
        },
    )
    .await;
}

async fn send_relay_access_denied(
    socket: &mut WebSocket,
    state: &RelayState,
    device_auth: Option<&DeviceAuth>,
) {
    if state.requires_device_key() {
        if let Some(device_auth) = device_auth {
            let expires_at = default_device_approval_expiry();
            let _ = send_server_message(
                socket,
                ServerMessage::DeviceApprovalRequired {
                    public_key: device_auth.public_key.clone(),
                    fingerprint: device_fingerprint(&device_auth.public_key),
                    suggested_approval: suggested_device_approval(&device_auth.public_key, expires_at),
                    expires_at,
                },
            )
            .await;
            return;
        }
    }

    send_error(socket, "relay access denied").await;
}

fn default_device_approval_expiry() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration + DEVICE_APPROVAL_TTL)
        .map(|duration| duration.as_secs())
        .unwrap_or(DEVICE_APPROVAL_TTL.as_secs())
}

fn suggested_device_approval(public_key: &str, expires_at: u64) -> String {
    format!("{public_key}@{expires_at}")
}

async fn send_server_message(
    socket: &mut WebSocket,
    message: ServerMessage,
) -> anyhow::Result<()> {
    let mut text = serde_json::to_string(&message)?;
    let result = socket.send(Message::Text(std::mem::take(&mut text).into())).await;
    text.zeroize();
    result?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::app_with_config, config::SiteConfig};
    use ed25519_dalek::{Signer, SigningKey};
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use std::net::SocketAddr;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    #[tokio::test]
    async fn relay_auto_registers_unknown_device_with_valid_signature() {
        let new_device = SigningKey::from_bytes(&[7; DEVICE_PUBLIC_KEY_BYTES]);
        let other = SigningKey::from_bytes(&[9; DEVICE_PUBLIC_KEY_BYTES]);
        let other_key = URL_SAFE_NO_PAD.encode(other.verifying_key().as_bytes());

        let (url, server) = spawn_relay_with_devices(&[&other_key]).await;
        let (mut socket, _) = connect_async(&url).await.unwrap();
        socket
            .send(Message::Text(
                json!({
                    "type": "create",
                    "access_token": null,
                    "device_auth": test_device_auth(&new_device, "create", None)
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let response = read_test_message(&mut socket).await;
        assert_eq!(response["type"], "created", "unknown device with valid signature should auto-register");

        server.abort();
    }

    #[tokio::test]
    async fn relay_accepts_allowlisted_signed_device() {
        let approved = SigningKey::from_bytes(&[11; DEVICE_PUBLIC_KEY_BYTES]);
        let approved_public_key = URL_SAFE_NO_PAD.encode(approved.verifying_key().as_bytes());

        let (url, server) = spawn_relay_with_devices(&[&approved_public_key]).await;
        let (mut socket, _) = connect_async(&url).await.unwrap();
        socket
            .send(Message::Text(
                json!({
                    "type": "create",
                    "access_token": null,
                    "device_auth": test_device_auth(&approved, "create", None)
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let response = read_test_message(&mut socket).await;

        assert_eq!(response["type"], "created");
        assert!(response["code"].as_str().is_some());

        server.abort();
    }

    async fn spawn_relay_with_devices(
        allowed_device_keys: &[&str],
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let allowed_device_keys = allowed_device_keys
            .iter()
            .map(|key| key.to_string())
            .collect::<Vec<_>>();
        let server = tokio::spawn(async move {
            let allowed_device_keys = allowed_device_keys
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            axum::serve(
                listener,
                app_with_config(SiteConfig::for_tests_with_devices(
                    true,
                    false,
                    None,
                    &allowed_device_keys,
                ))
                .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        (format!("ws://{addr}/relay"), server)
    }

    fn test_device_auth(
        signing_key: &SigningKey,
        action: &str,
        code: Option<&str>,
    ) -> serde_json::Value {
        let public_key = URL_SAFE_NO_PAD.encode(signing_key.verifying_key().as_bytes());
        let nonce = "test-nonce";
        let payload = protocol::device_auth_payload(action, code, nonce);
        let signature = URL_SAFE_NO_PAD.encode(signing_key.sign(payload.as_bytes()).to_bytes());

        json!({
            "public_key": public_key,
            "nonce": nonce,
            "signature": signature
        })
    }

    async fn read_test_message<S>(socket: &mut S) -> serde_json::Value
    where
        S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
    {
        match socket.next().await.unwrap().unwrap() {
            Message::Text(text) => serde_json::from_str(&text).unwrap(),
            other => panic!("unexpected message: {other:?}"),
        }
    }

    #[tokio::test]
    async fn device_auto_registers_on_first_connect() {
        let new_device = SigningKey::from_bytes(&[13; DEVICE_PUBLIC_KEY_BYTES]);
        // Start relay with a device key restriction in place (non-empty list) but
        // this device is not pre-approved.
        let other_device = SigningKey::from_bytes(&[15; DEVICE_PUBLIC_KEY_BYTES]);
        let other_key = URL_SAFE_NO_PAD.encode(other_device.verifying_key().as_bytes());
        let (url, server) = spawn_relay_with_devices(&[&other_key]).await;

        // First connect: device is unknown but signature is valid — should auto-register.
        let (mut socket, _) = connect_async(&url).await.unwrap();
        socket
            .send(Message::Text(
                json!({
                    "type": "create",
                    "access_token": null,
                    "device_auth": test_device_auth(&new_device, "create", None)
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let response = read_test_message(&mut socket).await;
        assert_eq!(response["type"], "created", "auto-registration should allow first connect");

        server.abort();
    }

    #[tokio::test]
    async fn revoked_device_is_blocked() {
        let revoked = SigningKey::from_bytes(&[16; DEVICE_PUBLIC_KEY_BYTES]);
        let revoked_key = URL_SAFE_NO_PAD.encode(revoked.verifying_key().as_bytes());
        // Add key with Unix timestamp 1 (already expired) to signal revocation.
        let revoked_entry = format!("{revoked_key}@1");
        let (url, server) = spawn_relay_with_devices(&[&revoked_entry]).await;

        let (mut socket, _) = connect_async(&url).await.unwrap();
        socket
            .send(Message::Text(
                json!({
                    "type": "create",
                    "access_token": null,
                    "device_auth": test_device_auth(&revoked, "create", None)
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap();

        let response = read_test_message(&mut socket).await;
        assert_eq!(response["type"], "device_approval_required", "revoked device must be blocked");

        server.abort();
    }
}
