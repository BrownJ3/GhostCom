use crate::protocol::frame::validate_display_name;
use crate::terminal::line_ui::{
    ChatInput, ChatInputReader, chat_println, chat_prompt, chat_status, chat_success, confirm_peer,
    print_invite_box, prompt_display_name, sanitize_for_terminal, typing_enabled,
};
use anyhow::{Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::{Deserialize, Serialize};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use snow::{Builder, TransportState, params::NoiseParams};
use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};
use subtle::ConstantTimeEq;
use tokio::sync::{Mutex, mpsc};
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
const GROUP_PEER_ID_BYTES: usize = 2;

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMessage {
    Create { access_token: Option<String> },
    GroupCreate { access_token: Option<String> },
    Join { code: String, access_token: Option<String> },
    GroupJoin { code: String, access_token: Option<String> },
}

impl Drop for ClientMessage {
    fn drop(&mut self) {
        match self {
            Self::Create { access_token } | Self::GroupCreate { access_token } => {
                if let Some(token) = access_token { token.zeroize(); }
            }
            Self::Join { code, access_token } | Self::GroupJoin { code, access_token } => {
                code.zeroize();
                if let Some(token) = access_token { token.zeroize(); }
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Created { code: String },
    GroupCreated { code: String },
    Joined,
    GroupJoined,
    PeerJoined,
    GroupPeerJoined { peer_id: u16 },
    GroupPeerLeft { peer_id: u16 },
    Error { message: String },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HostControlMessage {
    CloseGroupInvite,
}

#[derive(Clone, Copy)]
pub enum RelayRole {
    Caller,
    Joiner,
}

pub async fn call(relay_url: String, relay_pin: Option<String>) -> Result<()> {
    chat_status("Creating secure invite...")?;
    let secret = InviteSecret::generate();
    let socket = create_relay(&relay_url, relay_pin.as_deref(), &secret).await?;
    run_noise_chat(socket, RelayRole::Caller, Some(secret)).await
}

pub async fn group(relay_url: String, relay_pin: Option<String>) -> Result<()> {
    chat_status("Creating secure group invite...")?;
    let secret = InviteSecret::generate();
    let socket = create_group_relay(&relay_url, relay_pin.as_deref(), &secret).await?;
    run_group_host(socket, secret).await
}

pub async fn join(mut code: String, relay_url: String, relay_pin: Option<String>) -> Result<()> {
    chat_status("Joining secure invite...")?;
    let invite = match RelayInvite::parse(&code) {
        Ok(invite) => invite,
        Err(error) => {
            code.zeroize();
            return Err(error);
        }
    };
    code.zeroize();
    let socket = join_relay(&relay_url, relay_pin.as_deref(), invite.room_code(), invite.is_group()).await?;
    if invite.is_group() {
        let Some(secret) = invite.into_secret() else {
            bail!("group invite is missing its authentication secret");
        };
        run_group_joiner(socket, secret).await
    } else {
        run_noise_chat(socket, RelayRole::Joiner, invite.into_secret()).await
    }
}

fn relay_access_token() -> Option<String> {
    std::env::var(ACCESS_TOKEN_ENV)
        .ok()
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
}

fn verify_relay_pin(socket: &RelaySocket, pin: &str) -> Result<()> {
    let pin = pin.trim().to_ascii_lowercase();
    if pin.len() != 64 || !pin.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("--relay-pin must be a 64-character SHA-256 hex fingerprint");
    }
    let tls = match socket.get_ref() {
        tokio_tungstenite::MaybeTlsStream::Rustls(tls) => tls,
        _ => bail!("relay connection is not TLS — --relay-pin requires a wss:// URL"),
    };
    let (_, conn) = tls.get_ref();
    let leaf = conn
        .peer_certificates()
        .and_then(|c| c.first())
        .ok_or_else(|| anyhow::anyhow!("relay did not present a TLS certificate"))?;
    let digest = Sha256::digest(leaf.as_ref());
    let actual: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if actual != pin {
        bail!(
            "relay TLS certificate fingerprint mismatch\n  expected: {pin}\n  actual:   {actual}\nUpdate --relay-pin or verify the relay is legitimate."
        );
    }
    Ok(())
}

async fn create_relay(relay_url: &str, relay_pin: Option<&str>, secret: &InviteSecret) -> Result<RelaySocket> {
    let (mut socket, _) = connect_async(relay_url).await?;
    if let Some(pin) = relay_pin { verify_relay_pin(&socket, pin)?; }
    send_setup(&mut socket, ClientMessage::Create { access_token: relay_access_token() }).await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Created { code } => {
            validate_relay_code(&code)?;
            let mut invite = RelayInvite::format(&code, secret);
            let invite_result = print_invite_box("Share this invite code with your peer:", &invite);
            invite.zeroize();
            invite_result?;
            chat_status("Waiting for peer to join...")?;
        }
        ServerMessage::Error { message } => bail!("relay error: {}", sanitize_for_terminal(&message)),
        _ => bail!("unexpected relay response"),
    }

    loop {
        match read_setup(&mut socket).await? {
            ServerMessage::PeerJoined => {
                chat_status("Peer joined. Establishing end-to-end encryption...")?;
                return Ok(socket);
            }
            ServerMessage::Error { message } => bail!("relay error: {}", sanitize_for_terminal(&message)),
            _ => {}
        }
    }
}

async fn create_group_relay(relay_url: &str, relay_pin: Option<&str>, secret: &InviteSecret) -> Result<RelaySocket> {
    let (mut socket, _) = connect_async(relay_url).await?;
    if let Some(pin) = relay_pin { verify_relay_pin(&socket, pin)?; }
    send_setup(&mut socket, ClientMessage::GroupCreate { access_token: relay_access_token() }).await?;

    match read_setup(&mut socket).await? {
        ServerMessage::GroupCreated { code } => {
            validate_relay_code(&code)?;
            let mut invite = RelayInvite::format_group(&code, secret);
            let invite_result = print_invite_box("Share this group invite code with trusted participants:", &invite);
            invite.zeroize();
            invite_result?;
            chat_status("Group room is open. Participants can join until you close it.")?;
            Ok(socket)
        }
        ServerMessage::Error { message } => bail!("relay error: {}", sanitize_for_terminal(&message)),
        _ => bail!("unexpected relay response"),
    }
}

async fn join_relay(relay_url: &str, relay_pin: Option<&str>, code: &str, group: bool) -> Result<RelaySocket> {
    validate_relay_code(code)?;
    let (mut socket, _) = connect_async(relay_url).await?;
    if let Some(pin) = relay_pin { verify_relay_pin(&socket, pin)?; }
    let setup = if group {
        ClientMessage::GroupJoin { code: code.to_string(), access_token: relay_access_token() }
    } else {
        ClientMessage::Join { code: code.to_string(), access_token: relay_access_token() }
    };
    send_setup(&mut socket, setup).await?;

    match read_setup(&mut socket).await? {
        ServerMessage::Joined => { chat_status("Joined relay. Establishing end-to-end encryption...")?; Ok(socket) }
        ServerMessage::GroupJoined => { chat_status("Joined group relay. Establishing end-to-end encryption...")?; Ok(socket) }
        ServerMessage::Error { message } => bail!("relay error: {}", sanitize_for_terminal(&message)),
        _ => bail!("unexpected relay response"),
    }
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
            RelayFrame::GroupChat { .. } => {}
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
        | RelayFrame::GroupChat { .. }
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

                        let result = send_encrypted(&mut socket, &mut transport, RelayFrame::Chat(std::mem::take(&mut line))).await;
                        line.zeroize();
                        result?;
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
                    RelayFrame::GroupChat {
                        sender_id,
                        sender,
                        mut message,
                    } => {
                        typing_indicator.stop()?;
                        chat_println(&format!(
                            "{}> {}",
                            format_group_sender(sender_id, &sender),
                            sanitize_for_terminal(&message)
                        ))?;
                        message.zeroize();
                        chat_prompt()?;
                    }
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

struct GroupOutboundPeer {
    name: String,
    tx: mpsc::Sender<RelayFrame>,
}

impl Drop for GroupOutboundPeer {
    fn drop(&mut self) {
        self.name.zeroize();
    }
}

enum GroupHostEvent {
    Ready {
        peer_id: u16,
        name: String,
        tx: mpsc::Sender<RelayFrame>,
    },
    Chat {
        peer_id: u16,
        sender: String,
        message: String,
    },
    Closed {
        peer_id: u16,
    },
}

async fn run_group_host(socket: RelaySocket, secret: InviteSecret) -> Result<()> {
    let secret = Arc::new(secret);
    let local_name = prompt_display_name("GroupHost")?;
    let (writer, mut reader) = socket.split();
    let writer = Arc::new(Mutex::new(writer));
    let (events_tx, mut events_rx) = mpsc::channel(64);
    let mut peer_inputs: HashMap<u16, mpsc::Sender<Vec<u8>>> = HashMap::new();
    let mut pending_peers: HashMap<u16, GroupOutboundPeer> = HashMap::new();
    let mut peers: HashMap<u16, GroupOutboundPeer> = HashMap::new();
    let mut input_events = ChatInputReader::spawn();

    chat_println("")?;
    chat_println("--------------------------------------------------")?;
    chat_success("Group room ready. Type /quit to close.")?;
    chat_println("--------------------------------------------------")?;
    chat_prompt()?;

    loop {
        tokio::select! {
            input = input_events.recv() => {
                let Some(input) = input else {
                    break;
                };
                match input {
                    ChatInput::Line(mut line) => {
                        if line.trim() == "/quit" {
                            line.zeroize();
                            break;
                        }
                        if line.trim() == "/who" {
                            print_group_roster(&pending_peers, &peers)?;
                            chat_prompt()?;
                            line.zeroize();
                            continue;
                        }
                        if line.trim() == "/close-invite" {
                            send_host_control(writer.clone(), HostControlMessage::CloseGroupInvite).await?;
                            chat_println("Group invite closed. Current participants stay connected.")?;
                            chat_prompt()?;
                            line.zeroize();
                            continue;
                        }
                        if let Some(peer_id) = parse_group_command(line.trim(), "/allow") {
                            if let Some(peer) = pending_peers.remove(&peer_id) {
                                chat_println(&format!(
                                    "{} admitted to the group.",
                                    format_group_sender(peer_id, &peer.name)
                                ))?;
                                peers.insert(peer_id, peer);
                                chat_prompt()?;
                            } else {
                                chat_println("No pending participant with that id.")?;
                                chat_prompt()?;
                            }
                            line.zeroize();
                            continue;
                        }
                        if let Some(peer_id) = parse_group_command(line.trim(), "/deny") {
                            if let Some(peer) = pending_peers.remove(&peer_id) {
                                let _ = peer.tx.send(RelayFrame::Close).await;
                                peer_inputs.remove(&peer_id);
                                chat_println(&format!(
                                    "{} denied.",
                                    format_group_sender(peer_id, &peer.name)
                                ))?;
                                chat_prompt()?;
                            } else {
                                chat_println("No pending participant with that id.")?;
                                chat_prompt()?;
                            }
                            line.zeroize();
                            continue;
                        }
                        for peer in peers.values() {
                            let _ = peer.tx.send(RelayFrame::GroupChat {
                                sender_id: 0,
                                sender: local_name.clone(),
                                message: line.clone(),
                            }).await;
                        }
                        line.zeroize();
                    }
                    ChatInput::Closed => break,
                    ChatInput::TypingStart | ChatInput::TypingStop => {}
                }
            }
            message = reader.next() => {
                let Some(message) = message else {
                    break;
                };
                match message? {
                    Message::Text(text) if text.len() <= MAX_RELAY_SETUP_BYTES => {
                        match serde_json::from_str::<ServerMessage>(&text)? {
                            ServerMessage::GroupPeerJoined { peer_id } => {
                                let (raw_tx, raw_rx) = mpsc::channel(64);
                                peer_inputs.insert(peer_id, raw_tx);
                                tokio::spawn(run_group_host_peer(
                                    peer_id,
                                    raw_rx,
                                    writer.clone(),
                                    events_tx.clone(),
                                    Arc::clone(&secret),
                                    local_name.clone(),
                                ));
                            }
                            ServerMessage::GroupPeerLeft { peer_id } => {
                                peer_inputs.remove(&peer_id);
                                if let Some(peer) = pending_peers.remove(&peer_id) {
                                    chat_println(&format!(
                                        "{} left before admission.",
                                        format_group_sender(peer_id, &peer.name)
                                    ))?;
                                    chat_prompt()?;
                                } else if let Some(peer) = peers.remove(&peer_id) {
                                    chat_println(&format!(
                                        "{} left the group.",
                                        format_group_sender(peer_id, &peer.name)
                                    ))?;
                                    chat_prompt()?;
                                }
                            }
                            ServerMessage::Error { message } => {
                                bail!("relay error: {}", sanitize_for_terminal(&message));
                            }
                            _ => {}
                        }
                    }
                    Message::Binary(bytes) if bytes.len() > GROUP_PEER_ID_BYTES => {
                        let peer_id = u16::from_be_bytes([bytes[0], bytes[1]]);
                        if let Some(tx) = peer_inputs.get(&peer_id) {
                            let _ = tx.send(bytes[GROUP_PEER_ID_BYTES..].to_vec()).await;
                        }
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            event = events_rx.recv() => {
                let Some(event) = event else {
                    break;
                };
                match event {
                    GroupHostEvent::Ready { peer_id, name, tx } => {
                        chat_println(&format!(
                            "{} wants to join. Type /allow {} to admit or /deny {} to reject.",
                            format_group_sender(peer_id, &name),
                            peer_id,
                            peer_id
                        ))?;
                        pending_peers.insert(peer_id, GroupOutboundPeer { name, tx });
                        chat_prompt()?;
                    }
                    GroupHostEvent::Chat { peer_id, mut sender, mut message } => {
                        if !peers.contains_key(&peer_id) {
                            sender.zeroize();
                            message.zeroize();
                            continue;
                        }
                        chat_println(&format!(
                            "{}> {}",
                            format_group_sender(peer_id, &sender),
                            sanitize_for_terminal(&message)
                        ))?;
                        for (other_id, peer) in peers.iter() {
                            if *other_id != peer_id {
                                let _ = peer.tx.send(RelayFrame::GroupChat {
                                    sender_id: peer_id,
                                    sender: sender.clone(),
                                    message: message.clone(),
                                }).await;
                            }
                        }
                        sender.zeroize();
                        message.zeroize();
                        chat_prompt()?;
                    }
                    GroupHostEvent::Closed { peer_id } => {
                        peer_inputs.remove(&peer_id);
                        if let Some(peer) = pending_peers.remove(&peer_id) {
                            chat_println(&format!(
                                "{} left before admission.",
                                format_group_sender(peer_id, &peer.name)
                            ))?;
                            chat_prompt()?;
                        } else if let Some(peer) = peers.remove(&peer_id) {
                            chat_println(&format!(
                                "{} left the group.",
                                format_group_sender(peer_id, &peer.name)
                            ))?;
                            chat_prompt()?;
                        }
                    }
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }

    for peer in peers.values() {
        let _ = peer.tx.send(RelayFrame::Close).await;
    }
    for peer in pending_peers.values() {
        let _ = peer.tx.send(RelayFrame::Close).await;
    }

    Ok(())
}

async fn run_group_host_peer(
    peer_id: u16,
    mut raw_rx: mpsc::Receiver<Vec<u8>>,
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
    events_tx: mpsc::Sender<GroupHostEvent>,
    secret: Arc<InviteSecret>,
    local_name: String,
) -> Result<()> {
    let (mut transport, mut handshake_hash) =
        group_host_handshake(peer_id, &mut raw_rx, writer.clone()).await?;
    verify_group_invite(
        peer_id,
        &mut raw_rx,
        writer.clone(),
        &mut transport,
        &secret,
        &handshake_hash,
    )
    .await?;
    handshake_hash.zeroize();

    send_group_encrypted_to_peer(
        peer_id,
        writer.clone(),
        &mut transport,
        RelayFrame::Hello(local_name),
    )
    .await?;
    let peer_name = loop {
        match read_group_peer_frame(&mut raw_rx, &mut transport).await? {
            RelayFrame::Hello(name) => break name,
            RelayFrame::Close => bail!("peer closed before sending display name"),
            RelayFrame::InviteProof(_) => bail!("unexpected invite proof"),
            RelayFrame::Chat(_)
            | RelayFrame::GroupChat { .. }
            | RelayFrame::TypingStart
            | RelayFrame::TypingStop => {}
        }
    };

    let (out_tx, mut out_rx) = mpsc::channel(64);
    let _ = events_tx
        .send(GroupHostEvent::Ready {
            peer_id,
            name: peer_name.clone(),
            tx: out_tx,
        })
        .await;

    loop {
        tokio::select! {
            outbound = out_rx.recv() => {
                let Some(frame) = outbound else {
                    break;
                };
                send_group_encrypted_to_peer(peer_id, writer.clone(), &mut transport, frame).await?;
            }
            inbound = read_group_peer_frame(&mut raw_rx, &mut transport) => {
                match inbound? {
                    RelayFrame::Chat(message) | RelayFrame::GroupChat { message, .. } => {
                        let _ = events_tx.send(GroupHostEvent::Chat {
                            peer_id,
                            sender: peer_name.clone(),
                            message,
                        }).await;
                    }
                    RelayFrame::Close => break,
                    RelayFrame::Hello(_) | RelayFrame::InviteProof(_) | RelayFrame::TypingStart | RelayFrame::TypingStop => {}
                }
            }
        }
    }

    let _ = events_tx.send(GroupHostEvent::Closed { peer_id }).await;
    Ok(())
}

async fn run_group_joiner(mut socket: RelaySocket, secret: InviteSecret) -> Result<()> {
    let (mut transport, mut handshake_hash, _) =
        noise_handshake(&mut socket, RelayRole::Joiner).await?;
    verify_invite_secret(
        &mut socket,
        &mut transport,
        RelayRole::Joiner,
        &secret,
        &handshake_hash,
    )
    .await?;
    handshake_hash.zeroize();
    chat_success("Group invite verified end-to-end.")?;

    let local_name = prompt_display_name("GroupPeer")?;
    send_encrypted(&mut socket, &mut transport, RelayFrame::Hello(local_name)).await?;

    let host_name = loop {
        match read_encrypted(&mut socket, &mut transport).await? {
            RelayFrame::Hello(name) => break name,
            RelayFrame::Close => bail!("host closed before sending display name"),
            RelayFrame::InviteProof(_) => bail!("unexpected invite proof"),
            RelayFrame::Chat(_)
            | RelayFrame::GroupChat { .. }
            | RelayFrame::TypingStart
            | RelayFrame::TypingStop => {}
        }
    };

    run_group_joiner_loop(socket, transport, host_name).await
}

async fn run_group_joiner_loop(
    mut socket: RelaySocket,
    mut transport: TransportState,
    host_name: String,
) -> Result<()> {
    let mut input_events = ChatInputReader::spawn();

    chat_println("")?;
    chat_println("--------------------------------------------------")?;
    chat_success(&format!(
        "Connected to group host {host_name}. Type /quit to close."
    ))?;
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
                        let result = send_encrypted(&mut socket, &mut transport, RelayFrame::Chat(std::mem::take(&mut line))).await;
                        line.zeroize();
                        result?;
                    }
                    ChatInput::Closed => {
                        let _ = send_encrypted(&mut socket, &mut transport, RelayFrame::Close).await;
                        break;
                    }
                    ChatInput::TypingStart | ChatInput::TypingStop => {}
                }
            }
            frame = read_encrypted(&mut socket, &mut transport) => {
                match frame? {
                    RelayFrame::GroupChat {
                        sender_id,
                        sender,
                        mut message,
                    } => {
                        chat_println(&format!(
                            "{}> {}",
                            format_group_sender(sender_id, &sender),
                            sanitize_for_terminal(&message)
                        ))?;
                        message.zeroize();
                        chat_prompt()?;
                    }
                    RelayFrame::Chat(mut message) => {
                        chat_println(&format!("{host_name}> {}", sanitize_for_terminal(&message)))?;
                        message.zeroize();
                        chat_prompt()?;
                    }
                    RelayFrame::Close => {
                        chat_println("Group host closed the session.")?;
                        break;
                    }
                    RelayFrame::Hello(_) | RelayFrame::InviteProof(_) | RelayFrame::TypingStart | RelayFrame::TypingStop => {}
                }
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
    GroupChat {
        sender_id: u16,
        sender: String,
        message: String,
    },
    TypingStart,
    TypingStop,
    Close,
}


impl RelayFrame {
    fn encode(mut self) -> Result<Vec<u8>> {
        // Match on &mut self so we borrow fields rather than move them
        // (moving out of a type with Drop is forbidden by E0509).
        // Drop still runs after this method returns, giving a second zeroize pass.
        match &mut self {
            Self::InviteProof(proof) => {
                let mut out = vec![6];
                out.extend_from_slice(proof.as_ref());
                proof.zeroize();
                Ok(out)
            }
            Self::Hello(name) => {
                validate_display_name(name)?;
                let mut out = vec![1];
                out.extend_from_slice(name.as_bytes());
                name.zeroize();
                Ok(out)
            }
            Self::Chat(message) => {
                if message.len() > MAX_CHAT_MESSAGE_BYTES {
                    message.zeroize();
                    bail!("message too large");
                }
                let mut out = vec![2];
                out.extend_from_slice(message.as_bytes());
                message.zeroize();
                Ok(out)
            }
            Self::GroupChat { sender_id, sender, message } => {
                validate_display_name(sender)?;
                if message.len() > MAX_CHAT_MESSAGE_BYTES {
                    sender.zeroize();
                    message.zeroize();
                    bail!("message too large");
                }
                if sender.len() > u8::MAX as usize {
                    sender.zeroize();
                    message.zeroize();
                    bail!("sender name too large");
                }
                let sender_len = sender.len();
                let mut out = Vec::with_capacity(4 + sender_len + message.len());
                out.push(7);
                out.extend_from_slice(&sender_id.to_be_bytes());
                out.push(sender_len as u8);
                out.extend_from_slice(sender.as_bytes());
                out.extend_from_slice(message.as_bytes());
                sender.zeroize();
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
                let name = String::from_utf8(std::mem::take(&mut payload));
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
                let message = String::from_utf8(std::mem::take(&mut payload));
                payload.zeroize();
                Ok(Self::Chat(message?))
            }
            7 => {
                if payload.len() < 3 {
                    bail!("invalid group chat frame");
                }
                let sender_id = u16::from_be_bytes([payload[0], payload[1]]);
                let sender_len = payload[2] as usize;
                let payload = &payload[3..];
                if payload.len() < sender_len {
                    bail!("invalid group chat frame");
                };
                let (sender, message) = payload.split_at(sender_len);
                if message.len() > MAX_CHAT_MESSAGE_BYTES {
                    bail!("message too large");
                }
                let mut sender = sender.to_vec();
                let mut message = message.to_vec();
                let sender_text = String::from_utf8(std::mem::take(&mut sender));
                let message_text = String::from_utf8(std::mem::take(&mut message));
                sender.zeroize();
                message.zeroize();
                let sender_text = sender_text?;
                validate_display_name(&sender_text)?;
                Ok(Self::GroupChat {
                    sender_id,
                    sender: sender_text,
                    message: message_text?,
                })
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
    let mut encrypted = encrypt_frame(transport, frame)?;
    let result = send_binary(socket, &encrypted).await;
    encrypted.zeroize();
    result
}

async fn read_encrypted(
    socket: &mut RelaySocket,
    transport: &mut TransportState,
) -> Result<RelayFrame> {
    let mut encrypted = read_binary(socket).await?;
    let frame = decrypt_frame(transport, &encrypted);
    encrypted.zeroize();
    frame
}

fn encrypt_frame(transport: &mut TransportState, frame: RelayFrame) -> Result<Vec<u8>> {
    let plaintext = zeroize::Zeroizing::new(frame.encode()?);
    let mut encrypted = vec![0_u8; plaintext.len() + 16];
    let len = transport.write_message(&plaintext, &mut encrypted)?;
    encrypted.truncate(len);
    Ok(encrypted)
}

fn decrypt_frame(transport: &mut TransportState, encrypted: &[u8]) -> Result<RelayFrame> {
    let mut plaintext = vec![0_u8; encrypted.len()];
    let len = transport.read_message(encrypted, &mut plaintext)?;
    let frame = RelayFrame::decode(&plaintext[..len]);
    plaintext.zeroize();
    frame
}

async fn group_host_handshake(
    peer_id: u16,
    raw_rx: &mut mpsc::Receiver<Vec<u8>>,
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
) -> Result<(TransportState, Vec<u8>)> {
    let params: NoiseParams = NOISE_PATTERN.parse()?;
    let builder = Builder::new(params);
    let mut static_key = builder.generate_keypair()?.private;
    let mut noise = builder.local_private_key(&static_key)?.build_responder()?;
    static_key.zeroize();

    let mut buf = vec![0_u8; MAX_NOISE_MESSAGE_BYTES];
    let mut msg = read_group_peer_binary(raw_rx).await?;
    noise.read_message(&msg, &mut buf)?;
    msg.zeroize();
    let len = noise.write_message(&[], &mut buf)?;
    send_group_binary_to_peer(peer_id, writer.clone(), &buf[..len]).await?;
    buf[..len].zeroize();
    let mut msg = read_group_peer_binary(raw_rx).await?;
    noise.read_message(&msg, &mut buf)?;
    msg.zeroize();

    let handshake_hash = noise.get_handshake_hash().to_vec();
    buf.zeroize();
    Ok((noise.into_transport_mode()?, handshake_hash))
}

async fn verify_group_invite(
    peer_id: u16,
    raw_rx: &mut mpsc::Receiver<Vec<u8>>,
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
    transport: &mut TransportState,
    secret: &InviteSecret,
    handshake_hash: &[u8],
) -> Result<()> {
    let mut local_proof =
        invite_auth_proof(secret, handshake_hash, RelayRole::Caller.local_auth_label());
    send_group_encrypted_to_peer(
        peer_id,
        writer.clone(),
        transport,
        RelayFrame::InviteProof(local_proof),
    )
    .await?;
    local_proof.zeroize();

    let mut expected_peer_proof =
        invite_auth_proof(secret, handshake_hash, RelayRole::Caller.peer_auth_label());
    match read_group_peer_frame(raw_rx, transport).await? {
        RelayFrame::InviteProof(mut peer_proof) => {
            if peer_proof.ct_eq(&expected_peer_proof).into() {
                peer_proof.zeroize();
                expected_peer_proof.zeroize();
                return Ok(());
            }
            peer_proof.zeroize();
            expected_peer_proof.zeroize();
            bail!("group invite authentication failed");
        }
        RelayFrame::Close => bail!("peer closed before invite authentication"),
        RelayFrame::Hello(_)
        | RelayFrame::Chat(_)
        | RelayFrame::GroupChat { .. }
        | RelayFrame::TypingStart
        | RelayFrame::TypingStop => {
            bail!("peer sent chat data before invite authentication");
        }
    }
}

async fn read_group_peer_binary(raw_rx: &mut mpsc::Receiver<Vec<u8>>) -> Result<Vec<u8>> {
    match raw_rx.recv().await {
        Some(bytes) if bytes.len() <= MAX_NOISE_MESSAGE_BYTES => Ok(bytes),
        Some(_) => bail!("noise message too large"),
        None => bail!("relay closed"),
    }
}

async fn read_group_peer_frame(
    raw_rx: &mut mpsc::Receiver<Vec<u8>>,
    transport: &mut TransportState,
) -> Result<RelayFrame> {
    let mut encrypted = read_group_peer_binary(raw_rx).await?;
    let frame = decrypt_frame(transport, &encrypted);
    encrypted.zeroize();
    frame
}

async fn send_group_encrypted_to_peer(
    peer_id: u16,
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
    transport: &mut TransportState,
    frame: RelayFrame,
) -> Result<()> {
    let mut encrypted = encrypt_frame(transport, frame)?;
    let result = send_group_binary_to_peer(peer_id, writer, &encrypted).await;
    encrypted.zeroize();
    result
}

async fn send_group_binary_to_peer(
    peer_id: u16,
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
    bytes: &[u8],
) -> Result<()> {
    if bytes.len() > MAX_NOISE_MESSAGE_BYTES {
        bail!("noise message too large");
    }
    let mut envelope = Vec::with_capacity(GROUP_PEER_ID_BYTES + bytes.len());
    envelope.extend_from_slice(&peer_id.to_be_bytes());
    envelope.extend_from_slice(bytes);
    writer
        .lock()
        .await
        .send(Message::Binary(envelope.into()))
        .await?;
    Ok(())
}

async fn send_host_control(
    writer: Arc<Mutex<SplitSink<RelaySocket, Message>>>,
    message: HostControlMessage,
) -> Result<()> {
    let mut text = serde_json::to_string(&message)?;
    if text.len() > MAX_RELAY_SETUP_BYTES {
        text.zeroize();
        bail!("relay control message too large");
    }
    let result = writer
        .lock()
        .await
        .send(Message::Text(std::mem::take(&mut text).into()))
        .await;
    text.zeroize();
    result?;
    Ok(())
}

async fn send_setup(socket: &mut RelaySocket, message: ClientMessage) -> Result<()> {
    let mut text = serde_json::to_string(&message)?;
    if text.len() > MAX_RELAY_SETUP_BYTES {
        text.zeroize();
        bail!("relay setup message too large");
    }
    // Move `text` into the message rather than cloning, so the original memory
    // is owned by the sink and not duplicated in a clone that outlives zeroize.
    let result = socket.send(Message::Text(std::mem::take(&mut text).into())).await;
    text.zeroize();
    result?;
    Ok(())
}

async fn read_setup(socket: &mut RelaySocket) -> Result<ServerMessage> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) if text.len() <= MAX_RELAY_SETUP_BYTES => {
                let mut text = text.to_string();
                let result = serde_json::from_str(&text);
                text.zeroize();
                return Ok(result?);
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

fn default_relay_name(_verification_code: &str) -> &str {
    "RelayPeer"
}

fn format_group_sender(sender_id: u16, sender: &str) -> String {
    if sender_id == 0 {
        sanitize_for_terminal(sender)
    } else {
        format!("{}#{}", sanitize_for_terminal(sender), sender_id)
    }
}

fn parse_group_command(line: &str, command: &str) -> Option<u16> {
    let id = line.strip_prefix(command)?.trim();
    if id.is_empty() || id.contains(char::is_whitespace) {
        return None;
    }
    id.parse().ok()
}

fn print_group_roster(
    pending_peers: &HashMap<u16, GroupOutboundPeer>,
    peers: &HashMap<u16, GroupOutboundPeer>,
) -> Result<()> {
    chat_println("Participants")?;
    chat_println("--------------------------------------------------")?;
    chat_println("0  GroupHost  admitted")?;

    for (peer_id, peer) in peers {
        chat_println(&format!(
            "{}  {}  admitted",
            peer_id,
            sanitize_for_terminal(&peer.name)
        ))?;
    }

    for (peer_id, peer) in pending_peers {
        chat_println(&format!(
            "{}  {}  pending",
            peer_id,
            sanitize_for_terminal(&peer.name)
        ))?;
    }

    Ok(())
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
    group: bool,
}

impl Drop for RelayInvite {
    fn drop(&mut self) {
        self.room_code.zeroize();
    }
}

impl RelayInvite {
    fn parse(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        let (group, trimmed) = match trimmed.strip_prefix("g:") {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        let (room_code, secret) = match trimmed.split_once('.') {
            Some((room_code, secret)) => (room_code, Some(InviteSecret::parse(secret)?)),
            None => (trimmed, None),
        };

        validate_relay_code(room_code)?;
        Ok(Self {
            room_code: room_code.to_string(),
            secret,
            group,
        })
    }

    fn format(room_code: &str, secret: &InviteSecret) -> String {
        let mut encoded = secret.encode();
        let invite = format!("{room_code}.{encoded}");
        encoded.zeroize();
        invite
    }

    fn format_group(room_code: &str, secret: &InviteSecret) -> String {
        let mut encoded = secret.encode();
        let invite = format!("g:{room_code}.{encoded}");
        encoded.zeroize();
        invite
    }

    fn room_code(&self) -> &str {
        &self.room_code
    }

    fn is_group(&self) -> bool {
        self.group
    }

    fn into_secret(mut self) -> Option<InviteSecret> {
        self.secret.take()
    }
}

fn invite_auth_proof(
    secret: &InviteSecret,
    handshake_hash: &[u8],
    role_label: &[u8],
) -> [u8; INVITE_AUTH_PROOF_BYTES] {
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(&secret.0)
        .expect("HMAC accepts any key length");
    mac.update(b"GhostCom relay invite authentication v1\0");
    mac.update(role_label);
    mac.update(b"\0");
    mac.update(handshake_hash);
    mac.finalize().into_bytes().into()
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
    fn relay_invite_marks_group_codes() {
        let secret = InviteSecret([7; INVITE_SECRET_BYTES]);
        let invite = RelayInvite::format_group("abcd1234efgh5678", &secret);
        let parsed = RelayInvite::parse(&invite).unwrap();

        assert!(parsed.is_group());
        assert_eq!(parsed.room_code(), "abcd1234efgh5678");
        assert!(parsed.secret.is_some());
    }

    #[test]
    fn parses_group_admission_commands() {
        assert_eq!(parse_group_command("/allow 12", "/allow"), Some(12));
        assert_eq!(parse_group_command("/deny 7", "/deny"), Some(7));
        assert_eq!(parse_group_command("/allow", "/allow"), None);
        assert_eq!(parse_group_command("/allow 1 2", "/allow"), None);
        assert_eq!(parse_group_command("/who", "/allow"), None);
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
