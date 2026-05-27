mod protocol;
mod state;

use crate::client_ip::resolve_client_ip;
use axum::extract::{
    ConnectInfo, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::http::HeaderMap;
use futures_util::{SinkExt, StreamExt, stream::SplitStream};
use protocol::{ClientMessage, ServerMessage};
pub use state::RelayState;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::sync::oneshot;

const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);
const ROOM_WAIT_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const MAX_WS_TEXT_BYTES: usize = 512;
const MAX_RELAY_BINARY_BYTES: usize = 32 * 1024;
const MAX_RELAY_BYTES_PER_DIRECTION: u64 = 8 * 1024 * 1024;
const RELAY_IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const MAX_RELAY_SESSION_DURATION: Duration = Duration::from_secs(60 * 60);

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

    let first_message = match tokio::time::timeout(FIRST_MESSAGE_TIMEOUT, socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= MAX_WS_TEXT_BYTES => text,
        _ => {
            send_error(&mut socket, "expected small relay setup message").await;
            return;
        }
    };

    let client_message = match serde_json::from_str::<ClientMessage>(&first_message) {
        Ok(message) => message,
        Err(_) => {
            send_error(&mut socket, "invalid relay message").await;
            return;
        }
    };

    state.cleanup_expired().await;

    match client_message {
        ClientMessage::Create { access_token } => {
            if !state.access_token_matches(access_token.as_deref()) {
                send_error(&mut socket, "relay access denied").await;
                return;
            }
            handle_create(socket, state, remote_ip).await;
        }
        ClientMessage::Join { code, access_token } => {
            if !state.access_token_matches(access_token.as_deref()) {
                send_error(&mut socket, "relay access denied").await;
                return;
            }
            handle_join(socket, state, remote_ip, code).await;
        }
    }
}

async fn handle_create(mut caller: WebSocket, state: RelayState, remote_ip: IpAddr) {
    if !state.allow_create(remote_ip).await {
        send_error(&mut caller, "too many relay creation attempts").await;
        return;
    }

    let (join_tx, join_rx) = oneshot::channel();
    let Some(code) = state.create_room(join_tx).await else {
        send_error(&mut caller, "relay server has too many active rooms").await;
        return;
    };

    if send_server_message(&mut caller, ServerMessage::Created { code: code.clone() })
        .await
        .is_err()
    {
        state.remove_room(&code).await;
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
            return;
        }
    };

    if send_server_message(&mut caller, ServerMessage::PeerJoined)
        .await
        .is_err()
    {
        return;
    }

    if !state.try_acquire_session().await {
        send_error(&mut caller, "relay server has too many active sessions").await;
        send_error(&mut joiner, "relay server has too many active sessions").await;
        return;
    }

    bridge(caller, joiner).await;
    state.release_session().await;
}

async fn handle_join(mut joiner: WebSocket, state: RelayState, remote_ip: IpAddr, code: String) {
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

async fn send_server_message(socket: &mut WebSocket, message: ServerMessage) -> anyhow::Result<()> {
    let text = serde_json::to_string(&message)?;
    socket.send(Message::Text(text.into())).await?;
    Ok(())
}
