mod protocol;
mod state;

pub use state::RendezvousState;

use crate::client_ip::resolve_client_ip;
use axum::extract::{
    ConnectInfo, State,
    ws::{Message, WebSocket, WebSocketUpgrade},
};
use axum::http::HeaderMap;
use futures_util::{SinkExt, StreamExt};
use protocol::{ClientMessage, ServerMessage};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};

const FIRST_MESSAGE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_WS_TEXT_BYTES: usize = 512;

pub async fn rendezvous_ws(
    State(state): State<RendezvousState>,
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> impl axum::response::IntoResponse {
    let client_ip = resolve_client_ip(&headers, remote_addr);
    ws.max_message_size(MAX_WS_TEXT_BYTES)
        .max_frame_size(MAX_WS_TEXT_BYTES)
        .on_upgrade(move |socket| handle_socket(socket, state, client_ip))
}

async fn handle_socket(mut socket: WebSocket, state: RendezvousState, remote_ip: IpAddr) {
    if !state.rendezvous_enabled() {
        send_error(&mut socket, "rendezvous is unavailable").await;
        return;
    }

    if !state.allow_ws(remote_ip).await {
        send_error(&mut socket, "too many rendezvous connection attempts").await;
        return;
    }

    if !state.try_acquire_connection().await {
        send_error(&mut socket, "rendezvous server is busy").await;
        return;
    }

    handle_socket_inner(socket, state.clone(), remote_ip).await;
    state.release_connection().await;
}

async fn handle_socket_inner(mut socket: WebSocket, state: RendezvousState, remote_ip: IpAddr) {
    let first_message = match tokio::time::timeout(FIRST_MESSAGE_TIMEOUT, socket.next()).await {
        Ok(Some(Ok(Message::Text(text)))) if text.len() <= MAX_WS_TEXT_BYTES => text,
        _ => {
            send_error(&mut socket, "expected small text setup message").await;
            return;
        }
    };

    let client_message = match serde_json::from_str::<ClientMessage>(&first_message) {
        Ok(message) => message,
        Err(_) => {
            send_error(&mut socket, "invalid rendezvous message").await;
            return;
        }
    };

    state.cleanup_expired().await;

    match client_message {
        ClientMessage::Create {
            listen_port,
            access_token,
        } => {
            if !state.access_token_matches(access_token.as_deref()) {
                send_error(&mut socket, "rendezvous access denied").await;
                return;
            }
            if !state.allow_create(remote_ip).await {
                send_error(&mut socket, "too many invite creation attempts").await;
                return;
            }
            handle_create(socket, state, remote_ip, listen_port).await;
        }
        ClientMessage::Join { code, access_token } => {
            if !state.access_token_matches(access_token.as_deref()) {
                send_error(&mut socket, "rendezvous access denied").await;
                return;
            }
            if !state.allow_join(remote_ip).await {
                send_error(&mut socket, "too many invite join attempts").await;
                return;
            }
            handle_join(socket, state, code).await;
        }
    }
}

async fn handle_create(
    socket: WebSocket,
    state: RendezvousState,
    caller_ip: IpAddr,
    listen_port: u16,
) {
    let candidate = SocketAddr::new(caller_ip, listen_port).to_string();
    let (mut sender, mut receiver) = socket.split();
    let (caller_tx, mut caller_rx) = tokio::sync::mpsc::unbounded_channel();

    let Some(code) = state.create_room(candidate, caller_tx).await else {
        let _ = send_split_message(
            &mut sender,
            ServerMessage::Error {
                message: "rendezvous server has too many active invites".to_string(),
            },
        )
        .await;
        return;
    };

    if send_split_message(&mut sender, ServerMessage::Created { code: code.clone() })
        .await
        .is_err()
    {
        state.remove_room(&code).await;
        return;
    }

    let writer = tokio::spawn(async move {
        while let Some(message) = caller_rx.recv().await {
            if send_split_message(&mut sender, message).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(message)) = receiver.next().await {
        if matches!(message, Message::Close(_)) {
            break;
        }
    }

    state.remove_room(&code).await;
    writer.abort();
}

async fn handle_join(mut socket: WebSocket, state: RendezvousState, code: String) {
    if !state::valid_code(&code) {
        send_error(&mut socket, "invalid invite code").await;
        return;
    }

    let Some(room) = state.take_room(&code).await else {
        send_error(&mut socket, "invite code not found or expired").await;
        return;
    };

    if room.is_expired() {
        send_error(&mut socket, "invite code expired").await;
        return;
    }

    let _ = room.caller_tx.send(ServerMessage::PeerJoined);
    let _ = send_server_message(
        &mut socket,
        ServerMessage::Candidate {
            addr: room.candidate,
        },
    )
    .await;
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

async fn send_split_message<S>(sender: &mut S, message: ServerMessage) -> anyhow::Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let text = serde_json::to_string(&message)?;
    sender.send(Message::Text(text.into())).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        app::{app, app_with_config},
        config::SiteConfig,
    };
    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use std::net::SocketAddr;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    #[tokio::test]
    async fn pairs_invite_with_candidate() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app().into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let url = format!("ws://{addr}/rv");
        let (mut caller, _) = connect_async(&url).await.unwrap();
        caller
            .send(Message::Text(
                json!({ "type": "create", "listen_port": 7777 })
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let created = read_test_message(&mut caller).await;
        let code = created["code"].as_str().unwrap();

        let (mut joiner, _) = connect_async(&url).await.unwrap();
        joiner
            .send(Message::Text(
                json!({ "type": "join", "code": code }).to_string().into(),
            ))
            .await
            .unwrap();

        let caller_message = read_test_message(&mut caller).await;
        let joiner_message = read_test_message(&mut joiner).await;

        assert_eq!(caller_message["type"], "peer_joined");
        assert_eq!(joiner_message["type"], "candidate");
        assert_eq!(joiner_message["addr"], format!("127.0.0.1:{}", 7777));

        server.abort();
    }

    #[tokio::test]
    async fn rejects_missing_access_token_when_configured() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app_with_config(SiteConfig::for_tests(true, true, Some("expected-token")))
                    .into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .unwrap();
        });

        let url = format!("ws://{addr}/rv");
        let (mut caller, _) = connect_async(&url).await.unwrap();
        caller
            .send(Message::Text(
                json!({ "type": "create", "listen_port": 7777 })
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let response = read_test_message(&mut caller).await;

        assert_eq!(response["type"], "error");
        assert_eq!(response["message"], "rendezvous access denied");

        server.abort();
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
}
