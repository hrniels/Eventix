// Copyright (C) 2026 Nils Asmussen
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::io::Read;
use std::os::unix::net::UnixStream;

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, Generate, KeyInit, Nonce as AeadNonce},
};
use ashpd::desktop::secret::Secret;
use base64::{Engine as _, engine::general_purpose};
use hkdf::Hkdf;
use sha2::Sha256;
use thiserror::Error;
use tokio::sync::OnceCell;

use crate::settings::EncryptedPassword;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("Portal error: {0}")]
    Portal(#[from] ashpd::Error),
    #[error("I/O error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Encryption error")]
    Encryption,
    #[error("Decryption error")]
    Decryption,
    #[error("Invalid nonce")]
    InvalidNonce,
    #[error("Invalid ciphertext")]
    InvalidCiphertext,
}

pub type Result<T> = std::result::Result<T, CryptoError>;

const TEST_PORTAL_SECRET_ENV: &str = "EVENTIX_TESTS";

static PORTAL_SECRET: OnceCell<Vec<u8>> = OnceCell::const_new();

/// Retrieves the secret from the secret portal
///
/// If this has already been done, the cached secret will be returned. Otherwise, the secret is
/// retrieved from the secret portal.
pub async fn retrieve_portal_secret() -> Result<Vec<u8>> {
    // this environment variable is set for "./b test" in which case we use the same static secret
    // in both the tests itself and the spawned child processes like eventix-getpw to avoid calling
    // the secret portal.
    if std::env::var(TEST_PORTAL_SECRET_ENV).is_ok() {
        return Ok("eventix test secret".to_string().into());
    }

    PORTAL_SECRET
        .get_or_try_init(|| async {
            let secret = Secret::new().await?;

            let (mut rd, wr) = UnixStream::pair()?;
            secret.retrieve(&wr, Default::default()).await?;
            drop(wr);

            let mut buf = Vec::new();
            rd.read_to_end(&mut buf)?;

            Ok(buf)
        })
        .await
        .cloned()
}

/// Derives a 32-byte AES key from the portal secret using Hkdf.
fn derive_key(portal_secret: &[u8]) -> [u8; 32] {
    let hkdf = Hkdf::<Sha256>::new(None, portal_secret);
    let mut key_bytes = [0u8; 32];
    hkdf.expand(b"eventix state aes-gcm key v1", &mut key_bytes)
        .expect("32 bytes is a valid HKDF output length");
    key_bytes
}

/// Encrypts a plaintext password using the portal secret.
pub fn encrypt_password(portal_secret: &[u8], plaintext: &str) -> Result<EncryptedPassword> {
    let key_bytes = derive_key(portal_secret);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|_| CryptoError::Encryption)?;
    let nonce = AeadNonce::<Aes256Gcm>::generate(); // 96-bits; unique per message

    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_bytes())
        .map_err(|_| CryptoError::Encryption)?;

    Ok(EncryptedPassword {
        nonce: general_purpose::STANDARD.encode(nonce),
        ciphertext: general_purpose::STANDARD.encode(ciphertext),
    })
}

/// Decrypts an `EncryptedPassword` using the portal secret.
pub fn decrypt_password(portal_secret: &[u8], encrypted: &EncryptedPassword) -> Result<String> {
    let key_bytes = derive_key(portal_secret);
    let cipher = Aes256Gcm::new_from_slice(&key_bytes).map_err(|_| CryptoError::Decryption)?;

    let nonce_bytes = general_purpose::STANDARD
        .decode(&encrypted.nonce)
        .map_err(|_| CryptoError::InvalidNonce)?;
    if nonce_bytes.len() != 12 {
        return Err(CryptoError::InvalidNonce);
    }
    let nonce = Nonce::try_from(nonce_bytes.as_slice()).map_err(|_| CryptoError::InvalidNonce)?;

    let ciphertext = general_purpose::STANDARD
        .decode(&encrypted.ciphertext)
        .map_err(|_| CryptoError::InvalidCiphertext)?;

    let plaintext_bytes = cipher
        .decrypt(&nonce, ciphertext.as_ref())
        .map_err(|_| CryptoError::Decryption)?;

    String::from_utf8(plaintext_bytes).map_err(|_| CryptoError::Decryption)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let secret = b"super-secret-portal-key";
        let password = "my-password-123";

        let encrypted = encrypt_password(secret, password).unwrap();
        assert_ne!(encrypted.ciphertext, password);

        let decrypted = decrypt_password(secret, &encrypted).unwrap();
        assert_eq!(decrypted, password);
    }

    #[test]
    fn test_decrypt_fails_with_wrong_secret() {
        let secret = b"super-secret-portal-key";
        let wrong_secret = b"wrong-secret";
        let password = "my-password-123";

        let encrypted = encrypt_password(secret, password).unwrap();
        let result = decrypt_password(wrong_secret, &encrypted);
        assert!(matches!(result, Err(CryptoError::Decryption)));
    }

    #[test]
    fn test_decrypt_fails_with_tampered_ciphertext() {
        let secret = b"super-secret-portal-key";
        let password = "my-password-123";

        let mut encrypted = encrypt_password(secret, password).unwrap();
        // Modify one character in ciphertext
        let mut bytes = general_purpose::STANDARD
            .decode(&encrypted.ciphertext)
            .unwrap();
        bytes[0] ^= 1;
        encrypted.ciphertext = general_purpose::STANDARD.encode(bytes);

        let result = decrypt_password(secret, &encrypted);
        assert!(matches!(result, Err(CryptoError::Decryption)));
    }

    #[test]
    fn test_decrypt_fails_with_tampered_nonce() {
        let secret = b"super-secret-portal-key";
        let password = "my-password-123";

        let mut encrypted = encrypt_password(secret, password).unwrap();
        // Modify one character in nonce
        let mut bytes = general_purpose::STANDARD.decode(&encrypted.nonce).unwrap();
        bytes[0] ^= 1;
        encrypted.nonce = general_purpose::STANDARD.encode(bytes);

        let result = decrypt_password(secret, &encrypted);
        assert!(matches!(result, Err(CryptoError::Decryption)));
    }

    #[test]
    fn test_decrypt_fails_with_invalid_base64() {
        let secret = b"super-secret-portal-key";
        let encrypted = EncryptedPassword {
            nonce: "not-base64-!".to_string(),
            ciphertext: "also-not-base64-!".to_string(),
        };

        let result = decrypt_password(secret, &encrypted);
        assert!(matches!(result, Err(CryptoError::InvalidNonce)));
    }
}
