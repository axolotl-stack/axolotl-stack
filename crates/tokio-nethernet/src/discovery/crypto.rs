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

    // ========== Encrypt/Decrypt Tests ==========

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let original = b"Hello, NetherNet!";
        let encrypted = encrypt(original);
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, original);
    }

    #[test]
    fn test_encrypt_produces_different_output() {
        let original = b"test data";
        let encrypted = encrypt(original);

        // Encrypted data should differ from original
        assert_ne!(&encrypted[..], original);
    }

    #[test]
    fn test_encrypt_pads_to_block_boundary() {
        // Block size is 16 bytes
        for len in 1..=32 {
            let data = vec![0u8; len];
            let encrypted = encrypt(&data);

            // Should be padded to multiple of 16
            assert_eq!(encrypted.len() % 16, 0);
        }
    }

    #[test]
    fn test_encrypt_empty_data() {
        // Empty data still gets PKCS7 padding (one full block of 0x10)
        let encrypted = encrypt(&[]);
        assert_eq!(encrypted.len(), 16);

        let decrypted = decrypt(&encrypted).unwrap();
        assert!(decrypted.is_empty());
    }

    #[test]
    fn test_encrypt_exactly_one_block() {
        // 16 bytes needs one additional padding block
        let data = [0u8; 16];
        let encrypted = encrypt(&data);
        assert_eq!(encrypted.len(), 32);

        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_exactly_15_bytes() {
        // 15 bytes needs 1 byte padding
        let data = [0xAB; 15];
        let encrypted = encrypt(&data);
        assert_eq!(encrypted.len(), 16);

        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_decrypt_invalid_length_empty() {
        let result = decrypt(&[]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "invalid ciphertext length");
    }

    #[test]
    fn test_decrypt_invalid_length_not_multiple() {
        let result = decrypt(&[0u8; 15]);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "invalid ciphertext length");

        let result = decrypt(&[0u8; 17]);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_invalid_padding_zero() {
        // Create data where last byte would indicate 0 padding
        let mut data = vec![0u8; 16];
        data[15] = 0; // Invalid: padding of 0

        // This won't decrypt correctly because it's garbage
        let result = decrypt(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_invalid_padding_too_large() {
        // Create data where last byte indicates > 16 padding
        let mut data = vec![0u8; 16];
        data[15] = 17; // Invalid: padding > block size

        let result = decrypt(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_encrypt_large_data() {
        let data = vec![0x42; 1000];
        let encrypted = encrypt(&data);
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_encrypt_binary_data() {
        // Test with all byte values
        let data: Vec<u8> = (0..=255).collect();
        let encrypted = encrypt(&data);
        let decrypted = decrypt(&encrypted).unwrap();
        assert_eq!(decrypted, data);
    }

    // ========== HMAC Tests ==========

    #[test]
    fn test_hmac_roundtrip() {
        let data = b"test data";
        let checksum = hmac_checksum(data);
        assert!(verify_hmac(data, &checksum));
    }

    #[test]
    fn test_hmac_checksum_length() {
        let data = b"any data";
        let checksum = hmac_checksum(data);
        assert_eq!(checksum.len(), 32);
    }

    #[test]
    fn test_hmac_empty_data() {
        let checksum = hmac_checksum(&[]);
        assert!(verify_hmac(&[], &checksum));
    }

    #[test]
    fn test_hmac_different_data_different_checksum() {
        let checksum1 = hmac_checksum(b"data1");
        let checksum2 = hmac_checksum(b"data2");
        assert_ne!(checksum1, checksum2);
    }

    #[test]
    fn test_hmac_same_data_same_checksum() {
        let checksum1 = hmac_checksum(b"same data");
        let checksum2 = hmac_checksum(b"same data");
        assert_eq!(checksum1, checksum2);
    }

    #[test]
    fn test_verify_hmac_wrong_checksum() {
        let data = b"original data";
        let wrong_checksum = [0u8; 32];
        assert!(!verify_hmac(data, &wrong_checksum));
    }

    #[test]
    fn test_verify_hmac_modified_data() {
        let data = b"original data";
        let checksum = hmac_checksum(data);

        let modified_data = b"modified data";
        assert!(!verify_hmac(modified_data, &checksum));
    }

    #[test]
    fn test_verify_hmac_single_bit_flip() {
        let data = b"test data";
        let checksum = hmac_checksum(data);

        let mut modified_data = data.to_vec();
        modified_data[0] ^= 0x01; // Flip one bit

        assert!(!verify_hmac(&modified_data, &checksum));
    }

    #[test]
    fn test_verify_hmac_checksum_single_bit_flip() {
        let data = b"test data";
        let mut checksum = hmac_checksum(data);
        checksum[0] ^= 0x01; // Flip one bit in checksum

        assert!(!verify_hmac(data, &checksum));
    }

    // ========== Key Derivation Tests ==========

    #[test]
    fn test_key_is_deterministic() {
        let key1 = key();
        let key2 = key();
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_key_length() {
        let k = key();
        assert_eq!(k.len(), 32);
    }

    #[test]
    fn test_key_is_sha256_of_deadbeef() {
        let k = key();

        // Verify by computing manually
        let mut hasher = Sha256::new();
        hasher.update(0xdeadbeefu64.to_le_bytes());
        let expected: [u8; 32] = hasher.finalize().into();

        assert_eq!(k, expected);
    }

    // ========== ECB Mode Property Tests ==========

    #[test]
    fn test_ecb_same_block_same_ciphertext() {
        // ECB mode produces same ciphertext for same plaintext blocks
        let block = [0x42; 16];
        let repeated = [block, block].concat();

        let encrypted = encrypt(&repeated);

        // Without padding, first and second blocks should be identical
        // (But we have PKCS7 padding, so 32 bytes becomes 48)
        assert_eq!(encrypted.len(), 48);

        // First two 16-byte blocks of ciphertext should be identical
        assert_eq!(&encrypted[0..16], &encrypted[16..32]);
    }
}
