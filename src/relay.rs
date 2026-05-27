use crate::protocol::frame::validate_display_name;
use crate::terminal::line_ui::{
    ChatInput, ChatInputReader, chat_println, chat_prompt, chat_status, chat_success, confirm_peer,
    print_invite_box, prompt_display_name, sanitize_for_terminal, typing_enabled,
};
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use snow::{Builder, TransportState, params::NoiseParams};
use subtle::ConstantTimeEq;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use zeroize::Zeroize;

type RelaySocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const ACCESS_TOKEN_ENV: &str = "GHSTCOM_RELAY_ACCESS_TOKEN";
const MAX_RELAY_SETUP_BYTES: usize = 512;
const MAX_NOISE_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_CHAT_MESSAGE_BYTES: usize = 8 * 1024;
const NOISE_PATTERN: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
const INVITE_SECRET_BYTES: usize = 32;
const INVITE_AUTH_PROOF_BYTES: usize = 32;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Create {
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
    chat_status("Creating secure invite...")?;
    let secret = InviteSecret::generate();
    let socket = create_relay(&relay_url, &secret).await?;
    run_noise_chat(socket, RelayRole::Caller, Some(secret)).await
}

pub async fn join(mut code: String, relay_url: String) -> Result<()> {
    chat_status("Joining secure invite...")?;
    let invite = match RelayInvite::parse(&code) {
        Ok(invite) => invite,
        Err(error) => {
            code.zeroize();
            return Err(error);
        }
    };
    code.zeroize();
    let socket = join_relay(&relay_url, invite.room_code()).await?;
    run_noise_chat(socket, RelayRole::Joiner, invite.into_secret()).await
}

async fn create_relay(relay_url: &str, secret: &InviteSecret) -> Result<RelaySocket> {
    let (mut socket, _) = connect_async(relay_url).await?;
    send_setup(
        &mut socket,
        ClientMessage::Create {
            access_token: relay_access_token(),
        },
    )
    .await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Created { code } => {
            validate_relay_code(&code)?;
            print_invite_box(
                "Share this invite code with your peer:",
                &RelayInvite::format(&code, secret),
            )?;
            chat_status("Waiting for peer to join...")?;
        }
        ServerMessage::Error { message } => {
            bail!("relay error: {}", sanitize_for_terminal(&message))
        }
        _ => bail!("unexpected relay response"),
    }

    loop {
        match read_setup(&mut socket).await? {
            ServerMessage::PeerJoined => {
                chat_status("Peer joined. Establishing end-to-end encryption...")?;
                return Ok(socket);
            }
            ServerMessage::Error { message } => {
                bail!("relay error: {}", sanitize_for_terminal(&message))
            }
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
            access_token: relay_access_token(),
        },
    )
    .await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Joined => {
            chat_status("Joined relay. Establishing end-to-end encryption...")?;
            Ok(socket)
        }
        ServerMessage::Error { message } => {
            bail!("relay error: {}", sanitize_for_terminal(&message))
        }
        _ => bail!("unexpected relay response"),
    }
}

fn relay_access_token() -> Option<String> {
    std::env::var(ACCESS_TOKEN_ENV)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

async fn run_noise_chat(
    mut socket: RelaySocket,
    role: RelayRole,
    invite_secret: Option<InviteSecret>,
) -> Result<()> {
    let (mut transport, mut handshake_hash, verification_code) =
        noise_handshake(&mut socket, role).await?;

    if let Some(secret) = invite_secret {
        verify_invite_secret(&mut socket, &mut transport, role, &secret, &handshake_hash).await?;
        handshake_hash.zeroize();
        chat_success("Invite verified end-to-end.")?;
    } else if !confirm_peer(&verification_code).await? {
        handshake_hash.zeroize();
        bail!("session verification was not confirmed");
    }
    handshake_hash.zeroize();

    let local_name = prompt_display_name(default_relay_name(&verification_code))?;
    send_encrypted(&mut socket, &mut transport, RelayFrame::Hello(local_name)).await?;

    let peer_name = loop {
        match read_encrypted(&mut socket, &mut transport).await? {
            RelayFrame::Hello(name) => break name,
            RelayFrame::Close => bail!("peer closed before sending display name"),
            RelayFrame::InviteProof(_) => bail!("unexpected invite proof"),
            RelayFrame::Chat(_) => {}
            RelayFrame::TypingStart | RelayFrame::TypingStop => {}
        }
    };

    run_chat_loop(socket, transport, peer_name).await
}

async fn noise_handshake(
    socket: &mut RelaySocket,
    role: RelayRole,
) -> Result<(TransportState, Vec<u8>, String)> {
    let params: NoiseParams = NOISE_PATTERN.parse()?;
    let builder = Builder::new(params);
    let mut static_key = builder.generate_keypair()?.private;
    let mut noise = match role {
        RelayRole::Caller => builder.local_private_key(&static_key)?.build_responder()?,
        RelayRole::Joiner => builder.local_private_key(&static_key)?.build_initiator()?,
    };
    static_key.zeroize();

    let mut buf = vec![0_u8; MAX_NOISE_MESSAGE_BYTES];

    match role {
        RelayRole::Joiner => {
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
            buf[..len].zeroize();
            let mut msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
            msg.zeroize();
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
            buf[..len].zeroize();
        }
        RelayRole::Caller => {
            let mut msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
            msg.zeroize();
            let len = noise.write_message(&[], &mut buf)?;
            send_binary(socket, &buf[..len]).await?;
            buf[..len].zeroize();
            let mut msg = read_binary(socket).await?;
            noise.read_message(&msg, &mut buf)?;
            msg.zeroize();
        }
    }

    let handshake_hash = noise.get_handshake_hash().to_vec();
    let verification_code = format_verification_code(&handshake_hash);
    buf.zeroize();
    Ok((
        noise.into_transport_mode()?,
        handshake_hash,
        verification_code,
    ))
}

async fn verify_invite_secret(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
    role: RelayRole,
    secret: &InviteSecret,
    handshake_hash: &[u8],
) -> Result<()> {
    let mut local_proof = invite_auth_proof(secret, handshake_hash, role.local_auth_label());
    send_encrypted(socket, transport, RelayFrame::InviteProof(local_proof)).await?;
    local_proof.zeroize();

    let mut expected_peer_proof = invite_auth_proof(secret, handshake_hash, role.peer_auth_label());
    match read_encrypted(socket, transport).await? {
        RelayFrame::InviteProof(mut peer_proof) => {
            if peer_proof.ct_eq(&expected_peer_proof).into() {
                peer_proof.zeroize();
                expected_peer_proof.zeroize();
                return Ok(());
            }
            peer_proof.zeroize();
            expected_peer_proof.zeroize();
            bail!("invite authentication failed");
        }
        RelayFrame::Close => bail!("peer closed before invite authentication"),
        RelayFrame::Hello(_)
        | RelayFrame::Chat(_)
        | RelayFrame::TypingStart
        | RelayFrame::TypingStop => {
            bail!("peer sent chat data before invite authentication");
        }
    }
}

async fn run_chat_loop(
    mut socket: RelaySocket,
    mut transport: TransportState,
    peer_name: String,
) -> Result<()> {
    let mut input_events = ChatInputReader::spawn();
    let mut typing_indicator = crate::terminal::line_ui::TypingIndicator::new(peer_name.clone());
    let typing_enabled = typing_enabled();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(350));

    chat_println("")?;
    chat_println("--------------------------------------------------")?;
    chat_success(&format!("Connected to {peer_name}. Type /quit to close."))?;
    chat_println("--------------------------------------------------")?;
    chat_prompt()?;

    loop {
        tokio::select! {
            input = input_events.recv() => {
                let Some(input) = input else {
                    let _ = send_encrypted(&mut socket, &mut transport, RelayFrame::Close).await;
                    break;
                };

                match input {
                    ChatInput::Line(mut line) => {
                        if line.trim() == "/quit" {
                            line.zeroize();
                            let _ = send_encrypted(&mut socket, &mut transport, RelayFrame::Close).await;
                            break;
                        }

                        send_encrypted(&mut socket, &mut transport, RelayFrame::Chat(line)).await?;
                    }
                    ChatInput::TypingStart => {
                        if typing_enabled {
                            send_encrypted(&mut socket, &mut transport, RelayFrame::TypingStart).await?;
                        }
                    }
                    ChatInput::TypingStop => {
                        if typing_enabled {
                            send_encrypted(&mut socket, &mut transport, RelayFrame::TypingStop).await?;
                        }
                    }
                    ChatInput::Closed => {
                        let _ = send_encrypted(&mut socket, &mut transport, RelayFrame::Close).await;
                        break;
                    }
                }
            }
            frame = read_encrypted(&mut socket, &mut transport) => {
                match frame? {
                    RelayFrame::Hello(_) => {}
                    RelayFrame::InviteProof(_) => bail!("unexpected invite proof"),
                    RelayFrame::Chat(mut message) => {
                        typing_indicator.stop()?;
                        chat_println(&format!("{peer_name}> {}", sanitize_for_terminal(&message)))?;
                        message.zeroize();
                        chat_prompt()?;
                    }
                    RelayFrame::TypingStart => typing_indicator.start()?,
                    RelayFrame::TypingStop => typing_indicator.stop()?,
                    RelayFrame::Close => {
                        typing_indicator.stop()?;
                        chat_println("Peer closed the session.")?;
                        break;
                    }
                }
            }
            _ = tick.tick() => {
                typing_indicator.tick()?;
            }
            _ = tokio::signal::ctrl_c() => {
                let _ = send_encrypted(&mut socket, &mut transport, RelayFrame::Close).await;
                break;
            }
        }
    }

    Ok(())
}

enum RelayFrame {
    InviteProof([u8; INVITE_AUTH_PROOF_BYTES]),
    Hello(String),
    Chat(String),
    TypingStart,
    TypingStop,
    Close,
}

impl RelayFrame {
    fn encode(self) -> Result<Vec<u8>> {
        match self {
            Self::InviteProof(mut proof) => {
                let mut out = vec![6];
                out.extend_from_slice(&proof);
                proof.zeroize();
                Ok(out)
            }
            Self::Hello(mut name) => {
                validate_display_name(&name)?;
                let mut out = vec![1];
                out.extend_from_slice(name.as_bytes());
                name.zeroize();
                Ok(out)
            }
            Self::Chat(mut message) => {
                if message.len() > MAX_CHAT_MESSAGE_BYTES {
                    message.zeroize();
                    bail!("message too large");
                }
                let mut out = vec![2];
                out.extend_from_slice(message.as_bytes());
                message.zeroize();
                Ok(out)
            }
            Self::TypingStart => Ok(vec![4]),
            Self::TypingStop => Ok(vec![5]),
            Self::Close => Ok(vec![3]),
        }
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        let Some((&frame_type, payload)) = bytes.split_first() else {
            bail!("empty relay frame");
        };

        match frame_type {
            6 => {
                if payload.len() != INVITE_AUTH_PROOF_BYTES {
                    bail!("invalid invite proof");
                }
                Ok(Self::InviteProof(payload.try_into()?))
            }
            1 => {
                let mut payload = payload.to_vec();
                let name = String::from_utf8(payload.clone());
                payload.zeroize();
                let name = name?;
                validate_display_name(&name)?;
                Ok(Self::Hello(name))
            }
            2 => {
                if payload.len() > MAX_CHAT_MESSAGE_BYTES {
                    bail!("message too large");
                }
                let mut payload = payload.to_vec();
                let message = String::from_utf8(payload.clone());
                payload.zeroize();
                Ok(Self::Chat(message?))
            }
            4 if payload.is_empty() => Ok(Self::TypingStart),
            5 if payload.is_empty() => Ok(Self::TypingStop),
            3 if payload.is_empty() => Ok(Self::Close),
            _ => bail!("unknown relay frame"),
        }
    }
}

async fn send_encrypted(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
    frame: RelayFrame,
) -> Result<()> {
    let mut plaintext = frame.encode()?;
    let mut encrypted = vec![0_u8; plaintext.len() + 16];
    let len = transport.write_message(&plaintext, &mut encrypted)?;
    let result = send_binary(socket, &encrypted[..len]).await;
    plaintext.zeroize();
    encrypted.zeroize();
    result
}

async fn read_encrypted(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
) -> Result<RelayFrame> {
    let mut encrypted = read_binary(socket).await?;
    let mut plaintext = vec![0_u8; encrypted.len()];
    let len = transport.read_message(&encrypted, &mut plaintext)?;
    encrypted.zeroize();
    let frame = RelayFrame::decode(&plaintext[..len]);
    plaintext.zeroize();
    frame
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

impl RelayRole {
    fn local_auth_label(self) -> &'static [u8] {
        match self {
            Self::Caller => b"caller",
            Self::Joiner => b"joiner",
        }
    }

    fn peer_auth_label(self) -> &'static [u8] {
        match self {
            Self::Caller => b"joiner",
            Self::Joiner => b"caller",
        }
    }
}

#[derive(Clone)]
struct InviteSecret([u8; INVITE_SECRET_BYTES]);

impl InviteSecret {
    fn generate() -> Self {
        Self(rand::random())
    }

    fn parse(encoded: &str) -> Result<Self> {
        let mut bytes = URL_SAFE_NO_PAD.decode(encoded)?;
        if bytes.len() != INVITE_SECRET_BYTES {
            bytes.zeroize();
            bail!("relay invite secret has invalid length");
        }
        let mut secret = [0_u8; INVITE_SECRET_BYTES];
        secret.copy_from_slice(&bytes);
        bytes.zeroize();
        Ok(Self(secret))
    }

    fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }
}

impl Drop for InviteSecret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

struct RelayInvite {
    room_code: String,
    secret: Option<InviteSecret>,
}

impl RelayInvite {
    fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        let (room_code, secret) = match trimmed.split_once('.') {
            Some((room_code, secret)) => (room_code, Some(InviteSecret::parse(secret)?)),
            None => (trimmed, None),
        };

        validate_relay_code(room_code)?;
        Ok(Self {
            room_code: room_code.to_string(),
            secret,
        })
    }

    fn format(room_code: &str, secret: &InviteSecret) -> String {
        format!("{room_code}.{}", secret.encode())
    }

    fn room_code(&self) -> &str {
        &self.room_code
    }

    fn into_secret(self) -> Option<InviteSecret> {
        self.secret
    }
}

fn invite_auth_proof(
    secret: &InviteSecret,
    handshake_hash: &[u8],
    role_label: &[u8],
) -> [u8; INVITE_AUTH_PROOF_BYTES] {
    let mut hasher = Sha256::new();
    hasher.update(b"GhostCom relay invite authentication v1");
    hasher.update(role_label);
    hasher.update([0]);
    hasher.update(secret.0);
    hasher.update(handshake_hash);
    let digest: [u8; INVITE_AUTH_PROOF_BYTES] = hasher.finalize().into();
    digest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_invite_keeps_room_code_separate_from_secret() {
        let secret = InviteSecret([7; INVITE_SECRET_BYTES]);
        let invite = RelayInvite::format("abcd1234efgh5678", &secret);
        let parsed = RelayInvite::parse(&invite).unwrap();

        assert_eq!(parsed.room_code(), "abcd1234efgh5678");
        assert!(parsed.secret.is_some());
    }

    #[test]
    fn relay_invite_still_accepts_legacy_room_codes() {
        let parsed = RelayInvite::parse("abcd1234efgh5678").unwrap();

        assert_eq!(parsed.room_code(), "abcd1234efgh5678");
        assert!(parsed.secret.is_none());
    }

    #[test]
    fn invite_proofs_are_directional() {
        let secret = InviteSecret([9; INVITE_SECRET_BYTES]);
        let handshake_hash = [3; 32];

        let caller = invite_auth_proof(&secret, &handshake_hash, b"caller");
        let joiner = invite_auth_proof(&secret, &handshake_hash, b"joiner");

        assert_ne!(caller, joiner);
    }
}
