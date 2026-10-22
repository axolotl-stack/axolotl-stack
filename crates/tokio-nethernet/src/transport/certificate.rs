use std::fmt::Write as _;
use std::sync::Arc;

use rtc_crypto::{HashAlgorithm, RTCCryptoProvider};
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
        let fingerprint = fingerprint(provider, der.as_ref())?;
        Ok(Self {
            certificate,
            fingerprint,
        })
    }
}

/// SHA-256 fingerprint of a DER certificate in SDP form (upper-case hex pairs).
pub(crate) fn fingerprint(
    provider: &Arc<dyn RTCCryptoProvider>,
    der: &[u8],
) -> Result<String, NetherNetError> {
    let digest = provider
        .crypto()
        .hash(HashAlgorithm::Sha256, der)
        .map_err(|error| NetherNetError::Transport(error.to_string()))?;
    let mut out = String::with_capacity(digest.len() * 3);
    for (index, byte) in digest.iter().enumerate() {
        if index != 0 {
            out.push(':');
        }
        let _ = write!(out, "{byte:02X}");
    }
    Ok(out)
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
        // SHA-256("abc") known answer.
        assert!(
            fingerprint(&provider, b"abc")
                .unwrap()
                .starts_with("BA:78:16:BF")
        );
    }
}
