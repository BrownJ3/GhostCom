use crate::protocol::frame::{Frame, read_frame, write_frame};
use crate::rendezvous;
use crate::security::certificate::generate_ephemeral_identity;
use crate::security::fingerprint::{generated_session_name, session_verification_code};
use crate::security::verifier::ManualPeerVerifier;
use crate::terminal::line_ui::{confirm_peer, prompt_display_name, run_chat};
use anyhow::{Context, Result, bail};
use rustls::{ClientConfig, ServerConfig};
use rustls_pki_types::ServerName;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::{TlsAcceptor, TlsConnector};

pub async fn call(bind: SocketAddr, rendezvous_url: String) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    let local_addr = listener.local_addr()?;

    println!("GhostCom listening on {local_addr}");
    println!("Registering temporary invite with rendezvous server...");
    rendezvous::create_invite(&rendezvous_url, local_addr.port()).await?;

    accept_secure_connection(listener).await
}

pub async fn join(code: String, rendezvous_url: String) -> Result<()> {
    println!("Resolving invite code with rendezvous server...");
    let target = rendezvous::join_invite(&rendezvous_url, &code).await?;
    println!("Rendezvous returned candidate {target}");

    connect(target).await
}

pub async fn listen(bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;
    println!("GhostCom listening on {bind}");
    println!("Waiting for one peer...");

    accept_secure_connection(listener).await
}

async fn accept_secure_connection(listener: TcpListener) -> Result<()> {
    let (socket, peer_addr) = listener.accept().await?;
    println!("Incoming connection from {peer_addr}");

    let mut identity = generate_ephemeral_identity()?;
    let local_cert = identity
        .first_cert()
        .context("local identity did not include a certificate")?;
    let config = ServerConfig::builder()
        .with_client_cert_verifier(ManualPeerVerifier::new())
        .with_single_cert(identity.take_cert_chain(), identity.take_private_key()?)?;

    let acceptor = TlsAcceptor::from(Arc::new(config));
    let mut stream = acceptor.accept(socket).await?;

    let peer_cert = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .cloned()
        .context("peer did not present a certificate")?;
    let verification_code = session_verification_code(&local_cert, &peer_cert);

    if !confirm_peer(&verification_code).await? {
        bail!("session verification was not confirmed");
    }

    let local_name = prompt_display_name(&generated_session_name(&local_cert))?;

    write_frame(&mut stream, Frame::Hello(local_name)).await?;
    match read_frame(&mut stream).await? {
        Frame::Hello(peer_name) => run_chat(stream, peer_name).await,
        _ => bail!("expected hello frame"),
    }
}

pub async fn connect(target: String) -> Result<()> {
    println!("Connecting to {target}...");
    let socket = TcpStream::connect(&target).await?;

    let mut identity = generate_ephemeral_identity()?;
    let local_cert = identity
        .first_cert()
        .context("local identity did not include a certificate")?;
    let config = ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(ManualPeerVerifier::new())
        .with_client_auth_cert(identity.take_cert_chain(), identity.take_private_key()?)?;

    let connector = TlsConnector::from(Arc::new(config));
    let server_name = ServerName::try_from("ghostcom.local")?.to_owned();
    let mut stream = connector.connect(server_name, socket).await?;

    let peer_cert = stream
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|certs| certs.first())
        .cloned()
        .context("peer did not present a certificate")?;
    let verification_code = session_verification_code(&local_cert, &peer_cert);

    if !confirm_peer(&verification_code).await? {
        bail!("session verification was not confirmed");
    }

    let local_name = prompt_display_name(&generated_session_name(&local_cert))?;

    write_frame(&mut stream, Frame::Hello(local_name)).await?;
    match read_frame(&mut stream).await? {
        Frame::Hello(peer_name) => run_chat(stream, peer_name).await,
        _ => bail!("expected hello frame"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[tokio::test]
    async fn tls_peers_exchange_certificates() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut identity = generate_ephemeral_identity().unwrap();
            let config = ServerConfig::builder()
                .with_client_cert_verifier(ManualPeerVerifier::new())
                .with_single_cert(identity.take_cert_chain(), identity.take_private_key().unwrap())
                .unwrap();

            let acceptor = TlsAcceptor::from(Arc::new(config));
            let mut stream = acceptor.accept(socket).await.unwrap();
            let peer_cert_count = stream.get_ref().1.peer_certificates().unwrap().len();
            let mut byte = [0_u8; 1];
            stream.read_exact(&mut byte).await.unwrap();
            stream.write_all(&byte).await.unwrap();
            peer_cert_count
        });

        let mut identity = generate_ephemeral_identity().unwrap();
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(ManualPeerVerifier::new())
            .with_client_auth_cert(identity.take_cert_chain(), identity.take_private_key().unwrap())
            .unwrap();

        let socket = TcpStream::connect(addr).await.unwrap();
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = ServerName::try_from("ghostcom.local").unwrap().to_owned();
        let mut stream = connector.connect(server_name, socket).await.unwrap();

        let client_seen_cert_count = stream.get_ref().1.peer_certificates().unwrap().len();
        stream.write_all(&[42]).await.unwrap();
        let mut byte = [0_u8; 1];
        stream.read_exact(&mut byte).await.unwrap();

        assert_eq!(byte[0], 42);
        assert_eq!(client_seen_cert_count, 1);
        assert_eq!(server.await.unwrap(), 1);
    }
}
