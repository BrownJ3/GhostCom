use rustls_pki_types::CertificateDer;
use sha2::{Digest, Sha256};

pub fn session_verification_code(
    local_cert: &CertificateDer<'_>,
    peer_cert: &CertificateDer<'_>,
) -> String {
    let mut certs = [local_cert.as_ref(), peer_cert.as_ref()];
    certs.sort();

    let mut hasher = Sha256::new();
    hasher.update(b"GhostCom session verification v1");
    hasher.update((certs[0].len() as u32).to_be_bytes());
    hasher.update(certs[0]);
    hasher.update((certs[1].len() as u32).to_be_bytes());
    hasher.update(certs[1]);

    format_digest(&hasher.finalize()[..16])
}

pub fn generated_session_name(local_cert: &CertificateDer<'_>) -> String {
    const ADJECTIVES: &[&str] = &[
        "Amber", "Brisk", "Cobalt", "Dawn", "Ember", "Frost", "Ivory", "Jade", "Lunar", "Nova",
        "Onyx", "Quiet", "Solar", "Vivid", "Wild", "Zinc",
    ];
    const NOUNS: &[&str] = &[
        "Anchor", "Beacon", "Cipher", "Drift", "Echo", "Flare", "Harbor", "Key", "Lantern",
        "Mirror", "Pulse", "Relay", "Signal", "Trace", "Vector", "Wave",
    ];

    let digest = Sha256::digest(local_cert.as_ref());
    let adjective = ADJECTIVES[digest[0] as usize % ADJECTIVES.len()];
    let noun = NOUNS[digest[1] as usize % NOUNS.len()];
    let suffix = u16::from_be_bytes([digest[2], digest[3]]) % 10_000;

    format!("{adjective}{noun}{suffix:04}")
}

fn format_digest(digest: &[u8]) -> String {
    digest
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_code_is_order_independent() {
        let cert_a = CertificateDer::from(vec![1, 2, 3]);
        let cert_b = CertificateDer::from(vec![4, 5, 6]);

        assert_eq!(
            session_verification_code(&cert_a, &cert_b),
            session_verification_code(&cert_b, &cert_a)
        );
    }

    #[test]
    fn generated_name_is_stable_for_cert() {
        let cert = CertificateDer::from(vec![1, 2, 3]);

        assert_eq!(generated_session_name(&cert), generated_session_name(&cert));
    }
}
