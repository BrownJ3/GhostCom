use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use zeroize::{Zeroize, Zeroizing};

pub struct EphemeralIdentity {
    cert_chain: Vec<CertificateDer<'static>>,
    // Stored in an Option so it can be taken without moving the struct,
    // allowing Drop to reliably zeroize whatever is left behind.
    private_key: Option<PrivateKeyDer<'static>>,
}

impl EphemeralIdentity {
    pub fn take_cert_chain(&mut self) -> Vec<CertificateDer<'static>> {
        std::mem::take(&mut self.cert_chain)
    }

    pub fn take_private_key(&mut self) -> Result<PrivateKeyDer<'static>> {
        self.private_key.take().context("private key already consumed")
    }

    pub fn first_cert(&self) -> Option<CertificateDer<'static>> {
        self.cert_chain.first().cloned()
    }
}

impl Drop for EphemeralIdentity {
    fn drop(&mut self) {
        if let Some(mut key) = self.private_key.take() {
            key.zeroize();
        }
    }
}

pub fn generate_ephemeral_identity() -> Result<EphemeralIdentity> {
    let cert = generate_simple_self_signed(vec!["ghostcom.local".to_string()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = Zeroizing::new(cert.serialize_private_key_der());

    Ok(EphemeralIdentity {
        cert_chain: vec![CertificateDer::from(cert_der)],
        private_key: Some(PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_der.to_vec()))),
    })
}
