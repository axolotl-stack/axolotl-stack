//! Crypto for LAN discovery packets.
//!
//! Uses HMAC-SHA256 for checksum and AES-ECB with PKCS7 padding for encryption.

// The aes crate v0.8 uses generic_array 0.14 which is deprecated but still functional.
// Suppressing until aes upgrades to cipher 0.5+ with generic-array 1.x support.
#![allow(deprecated)]

use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, generic_array::GenericArray};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

/// The encryption key derived from the application ID (0xdeadbeef).
fn key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(0xdeadbeefu64.to_le_bytes());
    hasher.finalize().into()
}

/// Encrypt data using AES-256-ECB with PKCS7 padding.
pub fn encrypt(data: &[u8]) -> Vec<u8> {
    let key_bytes = key();
    let cipher = Aes256::new(GenericArray::from_slice(&key_bytes));

    // PKCS7 padding
    let block_size = 16;
    let padding_len = block_size - (data.len() % block_size);
    let mut padded = data.to_vec();
    padded.resize(data.len() + padding_len, padding_len as u8);

    // Encrypt each block (ECB mode)
    let mut encrypted = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(block_size) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.encrypt_block(&mut block);
        encrypted.extend_from_slice(&block);
    }

    encrypted
}

/// Decrypt data using AES-256-ECB with PKCS7 unpadding.
pub fn decrypt(data: &[u8]) -> Result<Vec<u8>, &'static str> {
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err("invalid ciphertext length");
    }

    let key_bytes = key();
    let cipher = Aes256::new(GenericArray::from_slice(&key_bytes));

    // Decrypt each block (ECB mode)
    let mut decrypted = Vec::with_capacity(data.len());
    for chunk in data.chunks(16) {
        let mut block = GenericArray::clone_from_slice(chunk);
        cipher.decrypt_block(&mut block);
        decrypted.extend_from_slice(&block);
    }

    // PKCS7 unpadding
    let padding_len = *decrypted.last().ok_or("empty decrypted data")? as usize;
    if padding_len == 0 || padding_len > 16 {
        return Err("invalid padding");
    }
    if decrypted.len() < padding_len {
        return Err("padding length exceeds data");
    }
    for &b in &decrypted[decrypted.len() - padding_len..] {
        if b as usize != padding_len {
            return Err("invalid padding bytes");
        }
    }
    decrypted.truncate(decrypted.len() - padding_len);

    Ok(decrypted)
}

/// Compute HMAC-SHA256 checksum.
pub fn hmac_checksum(data: &[u8]) -> [u8; 32] {
    let key_bytes = key();
    let mut mac: Hmac<Sha256> =
        <Hmac<Sha256> as Mac>::new_from_slice(&key_bytes).expect("valid key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// Verify HMAC-SHA256 checksum.
pub fn verify_hmac(data: &[u8], expected: &[u8; 32]) -> bool {
    let computed = hmac_checksum(data);
    computed == *expected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let original = b"Hello, NetherNet!";
        let encrypted = encrypt(original);
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_hmac_roundtrip() {
        let data = b"test data";
        let checksum = hmac_checksum(data);
        assert!(verify_hmac(data, &checksum));
    }
}
