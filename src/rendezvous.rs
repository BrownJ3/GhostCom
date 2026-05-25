use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::{connect_async, tungstenite::Message};

const MAX_WS_TEXT_BYTES: usize = 512;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Create { listen_port: u16 },
    Join { code: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Created { code: String },
    Candidate { addr: String },
    PeerJoined,
    Error { message: String },
}

pub async fn create_invite(rendezvous_url: &str, listen_port: u16) -> Result<()> {
    let (mut socket, _) = connect_async(rendezvous_url)
        .await
        .with_context(|| format!("failed to connect to rendezvous server at {rendezvous_url}"))?;
    send_message(&mut socket, ClientMessage::Create { listen_port }).await?;

    match read_message(&mut socket).await? {
        ServerMessage::Created { code } => {
            println!();
            println!("Invite code:");
            println!("  {code}");
            println!();
            println!("Share this code with your peer. Waiting for them to join...");
        }
        ServerMessage::Error { message } => bail!("rendezvous error: {message}"),
        _ => bail!("unexpected rendezvous response"),
    }

    loop {
        match read_message(&mut socket).await? {
            ServerMessage::PeerJoined => {
                println!("Peer joined. Waiting for direct encrypted connection...");
                return Ok(());
            }
            ServerMessage::Error { message } => bail!("rendezvous error: {message}"),
            _ => {}
        }
    }
}

pub async fn join_invite(rendezvous_url: &str, code: &str) -> Result<String> {
    validate_invite_code(code)?;

    let (mut socket, _) = connect_async(rendezvous_url)
        .await
        .with_context(|| format!("failed to connect to rendezvous server at {rendezvous_url}"))?;
    send_message(
        &mut socket,
        ClientMessage::Join {
            code: code.to_string(),
        },
    )
    .await?;

    match read_message(&mut socket).await? {
        ServerMessage::Candidate { addr } => Ok(addr),
        ServerMessage::Error { message } => bail!("rendezvous error: {message}"),
        _ => bail!("unexpected rendezvous response"),
    }
}

async fn send_message<S>(socket: &mut S, message: ClientMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let text = serde_json::to_string(&message)?;
    if text.len() > MAX_WS_TEXT_BYTES {
        bail!("rendezvous message too large");
    }
    socket.send(Message::Text(text.into())).await?;
    Ok(())
}

async fn read_message<S>(socket: &mut S) -> Result<ServerMessage>
where
    S: StreamExt<Item = std::result::Result<Message, tokio_tungstenite::tungstenite::Error>>
        + Unpin,
{
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) if text.len() <= MAX_WS_TEXT_BYTES => {
                return Ok(serde_json::from_str(&text)?);
            }
            Message::Text(_) => bail!("rendezvous message too large"),
            Message::Close(_) => bail!("rendezvous server closed the connection"),
            _ => {}
        }
    }

    bail!("rendezvous server closed the connection")
}

fn validate_invite_code(code: &str) -> Result<()> {
    if code.len() != 16 || !code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        bail!("invite code must be 16 alphanumeric characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_invite_code;

    #[test]
    fn validates_invite_codes() {
        assert!(validate_invite_code("abcd1234efgh5678").is_ok());
        assert!(validate_invite_code("short").is_err());
        assert!(validate_invite_code("abcd-234-efgh567").is_err());
    }
}
