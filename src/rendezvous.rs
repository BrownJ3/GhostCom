use crate::terminal::line_ui::sanitize_for_terminal;
use anyhow::{Context, Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use zeroize::Zeroize;

const ACCESS_TOKEN_ENV: &str = "GHSTCOM_RELAY_ACCESS_TOKEN";
const MAX_WS_TEXT_BYTES: usize = 512;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Create {
        listen_port: u16,
        access_token: Option<String>,
    },
    Join {
        code: String,
        access_token: Option<String>,
    },
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
    send_message(
        &mut socket,
        ClientMessage::Create {
            listen_port,
            access_token: relay_access_token(),
        },
    )
    .await?;

    match read_message(&mut socket).await? {
        ServerMessage::Created { code } => {
            println!();
            println!("Invite code:");
            println!("  {code}");
            println!();
            println!("Share this code with your peer. Waiting for them to join...");
        }
        ServerMessage::Error { message } => {
            bail!("rendezvous error: {}", sanitize_for_terminal(&message))
        }
        _ => bail!("unexpected rendezvous response"),
    }

    loop {
        match read_message(&mut socket).await? {
            ServerMessage::PeerJoined => {
                println!("Peer joined. Waiting for direct encrypted connection...");
                return Ok(());
            }
            ServerMessage::Error { message } => {
                bail!("rendezvous error: {}", sanitize_for_terminal(&message))
            }
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
            access_token: relay_access_token(),
        },
    )
    .await?;

    match read_message(&mut socket).await? {
        ServerMessage::Candidate { addr } => {
            let parsed: SocketAddr = addr
                .parse()
                .with_context(|| format!("rendezvous returned invalid address: {addr}"))?;
            if is_private_ip(&parsed.ip()) {
                bail!("rendezvous returned a non-routable address; possible SSRF: {addr}");
            }
            Ok(addr)
        }
        ServerMessage::Error { message } => {
            bail!("rendezvous error: {}", sanitize_for_terminal(&message))
        }
        _ => bail!("unexpected rendezvous response"),
    }
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_v4(v4),
        IpAddr::V6(v6) => is_private_v6(v6),
    }
}

fn is_private_v4(ip: &Ipv4Addr) -> bool {
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn is_private_v6(ip: &Ipv6Addr) -> bool {
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.octets()[0] == 0xfd // ULA fd00::/8
        || matches!(ip.to_ipv4_mapped(), Some(v4) if is_private_v4(&v4))
}

fn relay_access_token() -> Option<String> {
    let mut raw = std::env::var(ACCESS_TOKEN_ENV).ok()?;
    let trimmed = raw.trim().to_string();
    raw.zeroize();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

async fn send_message<S>(socket: &mut S, message: ClientMessage) -> Result<()>
where
    S: SinkExt<Message> + Unpin,
    <S as futures_util::Sink<Message>>::Error: std::error::Error + Send + Sync + 'static,
{
    let mut text = serde_json::to_string(&message)?;
    if text.len() > MAX_WS_TEXT_BYTES {
        text.zeroize();
        bail!("rendezvous message too large");
    }
    let result = socket.send(Message::Text(std::mem::take(&mut text).into())).await;
    text.zeroize();
    result?;
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
