use anyhow::Result;
use rcgen::generate_simple_self_signed;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use zeroize::Zeroizing;

pub struct EphemeralIdentity {
    pub cert_chain: Vec<CertificateDer<'static>>,
    pub private_key: PrivateKeyDer<'static>,
}

pub fn generate_ephemeral_identity() -> Result<EphemeralIdentity> {
    let cert = generate_simple_self_signed(vec!["ghostcom.local".to_string()])?;
    let cert_der = cert.serialize_der()?;
    let key_der = Zeroizing::new(cert.serialize_private_key_der());

    Ok(EphemeralIdentity {
        cert_chain: vec![CertificateDer::from(cert_der)],
        private_key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(key_der.to_vec())),
    })
}
