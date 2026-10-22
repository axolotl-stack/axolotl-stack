use std::fmt::Write as _;
use std::sync::Arc;

use rtc_crypto::RTCCryptoProvider;
use rtc_dtls::crypto::Certificate;

use crate::error::NetherNetError;

/// Self-signed ECDSA P-256 DTLS certificate and its SDP fingerprint.
///
/// One certificate is generated per dialer/listener and reused for its
/// connections (key generation would otherwise sit on every setup); peers bind
/// it per connection through the SDP fingerprint and identity assertion.
#[derive(Clone)]
pub(crate) struct LocalCertificate {
    pub(crate) certificate: Certificate,
    /// Upper-case `AA:BB:..` SHA-256 digest, as go-nethernet's `newConn` emits.
    pub(crate) fingerprint: String,
}

impl LocalCertificate {
    pub(crate) fn generate(provider: &Arc<dyn RTCCryptoProvider>) -> Result<Self, NetherNetError> {
        let certificate =
            Certificate::generate_self_signed(vec!["nethernet".to_owned()], provider.crypto())
                .map_err(transport_error)?;
        let der = certificate
            .certificate
            .first()
            .ok_or_else(|| NetherNetError::Transport("certificate without DER".into()))?;
        let fingerprint = fingerprint(FingerprintAlgorithm::Sha256, der.as_ref());
        Ok(Self {
            certificate,
            fingerprint,
        })
    }
}

/// `a=fingerprint` hash functions accepted from peers (RFC 8122 §5). pion
/// also accepts md5/sha-1/sha-224; NetherNet peers send sha-256, and weaker
/// hashes are rejected explicitly instead of failing the handshake later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FingerprintAlgorithm {
    Sha256,
    Sha384,
    Sha512,
}

impl FingerprintAlgorithm {
    /// Hash function tokens are case-insensitive (RFC 8122 §5).
    pub(crate) fn parse(name: &str) -> Option<Self> {
        [
            ("sha-256", Self::Sha256),
            ("sha-384", Self::Sha384),
            ("sha-512", Self::Sha512),
        ]
        .into_iter()
        .find_map(|(token, algorithm)| name.eq_ignore_ascii_case(token).then_some(algorithm))
    }
}

/// Fingerprint of a DER certificate in SDP form (upper-case hex pairs).
pub(crate) fn fingerprint(algorithm: FingerprintAlgorithm, der: &[u8]) -> String {
    use sha2::Digest;
    let digest = match algorithm {
        FingerprintAlgorithm::Sha256 => sha2::Sha256::digest(der).to_vec(),
        FingerprintAlgorithm::Sha384 => sha2::Sha384::digest(der).to_vec(),
        FingerprintAlgorithm::Sha512 => sha2::Sha512::digest(der).to_vec(),
    };
    let mut out = String::with_capacity(digest.len() * 3);
    for (index, byte) in digest.iter().enumerate() {
        if index != 0 {
            out.push(':');
        }
        let _ = write!(out, "{byte:02X}");
    }
    out
}

pub(crate) fn transport_error(error: impl std::fmt::Display) -> NetherNetError {
    NetherNetError::Transport(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_upper_case_colon_separated_sha256() {
        let provider = rtc_crypto::default_provider().unwrap();
        let certificate = LocalCertificate::generate(&provider).unwrap();
        assert_eq!(certificate.fingerprint.len(), 32 * 3 - 1);
        assert!(certificate.fingerprint.split(':').all(|pair| {
            pair.len() == 2
                && pair
                    .chars()
                    .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_lowercase())
        }));
        // Known answers for "abc" per advertised hash function.
        assert!(fingerprint(FingerprintAlgorithm::Sha256, b"abc").starts_with("BA:78:16:BF"));
        assert!(fingerprint(FingerprintAlgorithm::Sha384, b"abc").starts_with("CB:00:75:3F"));
        assert!(fingerprint(FingerprintAlgorithm::Sha512, b"abc").starts_with("DD:AF:35:A1"));
    }

    #[test]
    fn fingerprint_algorithms_parse_case_insensitively_and_reject_weak_hashes() {
        assert_eq!(
            FingerprintAlgorithm::parse("SHA-384"),
            Some(FingerprintAlgorithm::Sha384)
        );
        assert_eq!(FingerprintAlgorithm::parse("sha-1"), None);
        assert_eq!(FingerprintAlgorithm::parse("md5"), None);
    }
}
