use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier as _, VerifyingKey};

use crate::VerificationError;

pub trait SignatureVerifier: Send + Sync {
    fn verify(&self, message: &[u8], detached_signature: &[u8]) -> Result<(), VerificationError>;
}

#[derive(Debug, Clone)]
pub struct Ed25519Verifier {
    public_key: VerifyingKey,
}

impl Ed25519Verifier {
    pub fn from_public_key_bytes(bytes: &[u8; 32]) -> Result<Self, VerificationError> {
        let public_key =
            VerifyingKey::from_bytes(bytes).map_err(VerificationError::InvalidPublicKey)?;
        Ok(Self { public_key })
    }
}

impl SignatureVerifier for Ed25519Verifier {
    fn verify(&self, message: &[u8], detached_signature: &[u8]) -> Result<(), VerificationError> {
        let encoded = std::str::from_utf8(detached_signature)
            .map_err(|_| VerificationError::InvalidSignatureEncoding)?;
        let decoded = STANDARD.decode(encoded.trim())?;
        let signature = Signature::from_slice(&decoded)?;
        self.public_key
            .verify(message, &signature)
            .map_err(|_| VerificationError::InvalidSignature)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    #[test]
    fn verifies_a_valid_detached_signature() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let verifier =
            Ed25519Verifier::from_public_key_bytes(signing_key.verifying_key().as_bytes()).unwrap();
        let message = b"signed catalog bytes";
        let signature = STANDARD.encode(signing_key.sign(message).to_bytes());

        assert!(verifier.verify(message, signature.as_bytes()).is_ok());
    }

    #[test]
    fn rejects_tampered_catalog_bytes() {
        let signing_key = SigningKey::from_bytes(&[9_u8; 32]);
        let verifier =
            Ed25519Verifier::from_public_key_bytes(signing_key.verifying_key().as_bytes()).unwrap();
        let signature = STANDARD.encode(signing_key.sign(b"original").to_bytes());

        assert!(matches!(
            verifier.verify(b"tampered", signature.as_bytes()),
            Err(VerificationError::InvalidSignature)
        ));
    }

    #[test]
    fn rejects_malformed_signature_encoding() {
        let signing_key = SigningKey::from_bytes(&[11_u8; 32]);
        let verifier =
            Ed25519Verifier::from_public_key_bytes(signing_key.verifying_key().as_bytes()).unwrap();

        assert!(matches!(
            verifier.verify(b"catalog", b"not base64!"),
            Err(VerificationError::InvalidEncoding(_))
        ));
    }
}
