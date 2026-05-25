use crate::protocol::frame::validate_display_name;
use crate::terminal::line_ui::{confirm_peer, prompt_display_name, spawn_stdin_reader};
use anyhow::{Result, bail};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::{Builder, TransportState, params::NoiseParams};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};

type RelaySocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const MAX_RELAY_SETUP_BYTES: usize = 512;
const MAX_NOISE_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_CHAT_MESSAGE_BYTES: usize = 8 * 1024;
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Create,
    Join { code: String },
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Created { code: String },
    Joined,
    PeerJoined,
    Error { message: String },
}

#[derive(Clone, Copy)]
pub enum RelayRole {
    Caller,
    Joiner,
}

pub async fn call(relay_url: String) -> Result<()> {
    println!("Creating relay invite...");
    let socket = create_relay(&relay_url).await?;
    run_noise_chat(socket, RelayRole::Caller).await
}

pub async fn join(code: String, relay_url: String) -> Result<()> {
    println!("Joining relay invite...");
    let socket = join_relay(&relay_url, &code).await?;
    run_noise_chat(socket, RelayRole::Joiner).await
}

async fn create_relay(relay_url: &str) -> Result<RelaySocket> {
    let (mut socket, _) = connect_async(relay_url).await?;
    send_setup(&mut socket, ClientMessage::Create).await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Created { code } => {
            println!();
            println!("Relay invite code:");
            println!("  {code}");
            println!();
            println!("Share this code with your peer. Waiting for them to join...");
        }
        ServerMessage::Error { message } => bail!("relay error: {message}"),
        _ => bail!("unexpected relay response"),
    }

    loop {
        match read_setup(&mut socket).await? {
            ServerMessage::PeerJoined => {
                println!("Peer joined relay. Starting end-to-end Noise handshake...");
                return Ok(socket);
            }
            ServerMessage::Error { message } => bail!("relay error: {message}"),
            _ => {}
        }
    }
}

async fn join_relay(relay_url: &str, code: &str) -> Result<RelaySocket> {
    validate_relay_code(code)?;

    let (mut socket, _) = connect_async(relay_url).await?;
    send_setup(
        &mut socket,
        ClientMessage::Join {
            code: code.to_string(),
        },
    )
    .await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Joined => {
            println!("Relay joined. Starting end-to-end Noise handshake...");
            Ok(socket)
        }
        ServerMessage::Error { message } => bail!("relay error: {message}"),
        _ => bail!("unexpected relay response"),
    }
}

async fn run_noise_chat(mut socket: RelaySocket, role: RelayRole) -> Result<()> {
    let (mut transport, verification_code) = noise_handshake(&mut socket, role).await?;

    if !confirm_peer(&verification_code).await? {
        bail!("session verification was not confirmed");
    }

    let local_name = prompt_display_name(default_relay_name(&verification_code))?;
    send_encrypted(&mut socket, &mut transport, &RelayFrame::Hello(local_name)).await?;

    let peer_name = loop {
        match read_encrypted(&mut socket, &mut transport).await? {
            RelayFrame::Hello(name) => break name,
            RelayFrame::Close => bail!("peer closed before sending display name"),
            RelayFrame::Chat(_) => {}
        }
    };

    run_chat_loop(socket, transport, peer_name).await
}

async fn noise_handshake(
    socket: &mut RelaySocket,
    role: RelayRole,
) -> Result<(TransportState, String)> {
    let params: NoiseParams = NOISE_PATTERN.parse()?;
    let builder = Builder::new(params);
    let static_key = builder.generate_keypair()?.private;
    let mut noise = match role {
        RelayRole::Caller => builder.local_private_key(&static_key)?.build_responder()?,
        RelayRole::Joiner => builder.local_private_key(&static_key)?.build_initiator()?,
    };

    let mut buf = vec![0_u8; MAX_NOISE_MESSAGE_BYTES];

    match role {
        RelayRole::Joiner => {
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
            let msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
        }
        RelayRole::Caller => {
            let msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
            let msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
        }
    }

    let verification_code = format_verification_code(noise.get_handshake_hash());
    Ok((noise.into_transport_mode()?, verification_code))
}

async fn run_chat_loop(
    mut socket: RelaySocket,
    mut transport: TransportState,
    peer_name: String,
) -> Result<()> {
    let mut stdin_lines = spawn_stdin_reader();

    println!("Relay chat started with {peer_name}. Type /quit to close the session.");

    loop {
        tokio::select! {
            line = stdin_lines.recv() => {
                let Some(line) = line else {
                    let _ = send_encrypted(&mut socket, &mut transport, &RelayFrame::Close).await;
                    break;
                };

                if line.trim() == "/quit" {
                    let _ = send_encrypted(&mut socket, &mut transport, &RelayFrame::Close).await;
                    break;
                }

                send_encrypted(&mut socket, &mut transport, &RelayFrame::Chat(line)).await?;
            }
            frame = read_encrypted(&mut socket, &mut transport) => {
                match frame? {
                    RelayFrame::Hello(_) => {}
                    RelayFrame::Chat(message) => println!("{peer_name}> {message}"),
                    RelayFrame::Close => {
                        println!("Peer closed the session.");
                        break;
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = send_encrypted(&mut socket, &mut transport, &RelayFrame::Close).await;
                break;
            }
        }
    }

    Ok(())
}

enum RelayFrame {
    Hello(String),
    Chat(String),
    Close,
}

impl RelayFrame {
    fn encode(&self) -> Result<Vec<u8>> {
        match self {
            Self::Hello(name) => {
                validate_display_name(name)?;
                let mut out = vec![1];
                out.extend_from_slice(name.as_bytes());
                Ok(out)
            }
            Self::Chat(message) => {
                if message.len() > MAX_CHAT_MESSAGE_BYTES {
                    bail!("message too large");
                }
                let mut out = vec![2];
                out.extend_from_slice(message.as_bytes());
                Ok(out)
            }
            Self::Close => Ok(vec![3]),
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let Some((&frame_type, payload)) = bytes.split_first() else {
            bail!("empty relay frame");
        };

        match frame_type {
            1 => {
                let name = String::from_utf8(payload.to_vec())?;
                validate_display_name(&name)?;
                Ok(Self::Hello(name))
            }
            2 => {
                if payload.len() > MAX_CHAT_MESSAGE_BYTES {
                    bail!("message too large");
                }
                Ok(Self::Chat(String::from_utf8(payload.to_vec())?))
            }
            3 if payload.is_empty() => Ok(Self::Close),
            _ => bail!("unknown relay frame"),
        }
    }
}

async fn send_encrypted(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
    frame: &RelayFrame,
) -> Result<()> {
    let plaintext = frame.encode()?;
    let mut encrypted = vec![0_u8; plaintext.len() + 16];
    let len = transport.write_message(&plaintext, &mut encrypted)?;
    send_binary(socket, &encrypted[..len]).await
}

async fn read_encrypted(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
) -> Result<RelayFrame> {
    let encrypted = read_binary(socket).await?;
    let mut plaintext = vec![0_u8; encrypted.len()];
    let len = transport.read_message(&encrypted, &mut plaintext)?;
    RelayFrame::decode(&plaintext[..len])
}

async fn send_setup(socket: &mut RelaySocket, message: ClientMessage) -> Result<()> {
    let text = serde_json::to_string(&message)?;
    if text.len() > MAX_RELAY_SETUP_BYTES {
        bail!("relay setup message too large");
    }
    socket.send(Message::Text(text.into())).await?;
    Ok(())
}

async fn read_setup(socket: &mut RelaySocket) -> Result<ServerMessage> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) if text.len() <= MAX_RELAY_SETUP_BYTES => {
                return Ok(serde_json::from_str(&text)?);
            }
            Message::Text(_) => bail!("relay setup message too large"),
            Message::Close(_) => bail!("relay closed"),
            _ => {}
        }
    }
    bail!("relay closed")
}

async fn send_binary(socket: &mut RelaySocket, bytes: &[u8]) -> Result<()> {
    if bytes.len() > MAX_NOISE_MESSAGE_BYTES {
        bail!("noise message too large");
    }
    socket.send(Message::Binary(bytes.to_vec().into())).await?;
    Ok(())
}

async fn read_binary(socket: &mut RelaySocket) -> Result<Vec<u8>> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Binary(bytes) if bytes.len() <= MAX_NOISE_MESSAGE_BYTES => {
                return Ok(bytes.to_vec());
            }
            Message::Binary(_) => bail!("noise message too large"),
            Message::Close(_) => bail!("relay closed"),
            _ => {}
        }
    }
    bail!("relay closed")
}

fn format_verification_code(handshake_hash: &[u8]) -> String {
    let digest = Sha256::digest(handshake_hash);
    digest[..16]
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

fn default_relay_name(verification_code: &str) -> &str {
    if verification_code.as_bytes()[0].is_ascii_hexdigit() {
        "RelayPeer"
    } else {
        "Peer"
    }
}

fn validate_relay_code(code: &str) -> Result<()> {
    if code.len() != 16 || !code.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        bail!("relay invite code must be 16 alphanumeric characters");
    }
    Ok(())
}
