//! Crypto for LAN discovery packets.
//!
//! Uses HMAC-SHA256 for checksum and AES-ECB with PKCS7 padding for encryption.
//! The key is fixed (upstream `discovery/crypto.go:key`), so the AES key
//! schedule and the HMAC ipad/opad midstates are derived once. A checksum is
//! then exactly the SHA-256 compressions of the message and the inner digest
//! (two for a typical discovery packet), driven through `sha2::compress256`
//! (SHA-NI when available) without the generic digest/HMAC buffering layers,
//! which cost about as much as the compressions on these short inputs.

// The aes crate v0.8 uses generic_array 0.14 which is deprecated but still functional.
// Suppressing until aes upgrades to cipher 0.5+ with generic-array 1.x support.
#![allow(deprecated)]

use std::sync::LazyLock;

use aes::Aes256;
use aes::cipher::{BlockDecrypt, BlockEncrypt, KeyInit, inout::InOutBuf};
use sha2::digest::generic_array::GenericArray;
use sha2::{Digest, Sha256};

/// AES block size; PKCS7 pads to a multiple of it.
pub const BLOCK_LEN: usize = 16;
/// HMAC-SHA256 checksum prefix length.
pub const CHECKSUM_LEN: usize = 32;

/// SHA-256 block size.
const SHA_BLOCK_LEN: usize = 64;
/// SHA-256 initial hash value, FIPS 180-4 §5.3.3.
const SHA256_IV: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

struct Keys {
    cipher: Aes256,
    /// SHA-256 state after absorbing `key ^ ipad` (RFC 2104).
    inner: [u32; 8],
    /// SHA-256 state after absorbing `key ^ opad` (RFC 2104).
    outer: [u32; 8],
}

static KEYS: LazyLock<Keys> = LazyLock::new(|| {
    let key = key();
    Keys {
        cipher: Aes256::new(&key.into()),
        inner: keyed_midstate(&key, 0x36),
        outer: keyed_midstate(&key, 0x5c),
    }
});

/// The 32-byte key is shorter than a block, so RFC 2104 zero-pads it to 64 bytes.
fn keyed_midstate(key: &[u8; 32], pad: u8) -> [u32; 8] {
    let mut block = [pad; SHA_BLOCK_LEN];
    for (byte, key) in block.iter_mut().zip(key) {
        *byte ^= key;
    }
    let mut state = SHA256_IV;
    sha2::compress256(&mut state, &[block.into()]);
    state
}

/// Finishes SHA-256 from a midstate that has absorbed exactly one block.
fn finish_after_block(mut state: [u32; 8], data: &[u8]) -> [u8; 32] {
    let (blocks, rest) = data.as_chunks::<SHA_BLOCK_LEN>();
    for block in blocks {
        sha2::compress256(&mut state, &[GenericArray::from(*block)]);
    }
    // FIPS 180-4 §5.1.1 padding: 0x80, zeros, 64-bit big-endian bit length
    // (counting the keyed block already absorbed into `state`).
    let mut tail = [[0u8; SHA_BLOCK_LEN]; 2];
    let tail_blocks = if rest.len() < SHA_BLOCK_LEN - 8 { 1 } else { 2 };
    let flat = tail.as_flattened_mut();
    flat[..rest.len()].copy_from_slice(rest);
    flat[rest.len()] = 0x80;
    let bits = ((SHA_BLOCK_LEN + data.len()) as u64) * 8;
    flat[tail_blocks * SHA_BLOCK_LEN - 8..tail_blocks * SHA_BLOCK_LEN]
        .copy_from_slice(&bits.to_be_bytes());
    for block in &tail[..tail_blocks] {
        sha2::compress256(&mut state, &[GenericArray::from(*block)]);
    }
    let mut digest = [0u8; 32];
    for (out, word) in digest.chunks_exact_mut(4).zip(state) {
        out.copy_from_slice(&word.to_be_bytes());
    }
    digest
}

/// SHA256 of LE u64 application ID 0xdeadbeef: upstream `discovery/crypto.go:key`.
fn key() -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(0xdeadbeefu64.to_le_bytes());
    hasher.finalize().into()
}

/// Length of `plain_len` bytes after PKCS7 padding (always adds 1..=16 bytes).
#[inline]
pub const fn padded_len(plain_len: usize) -> usize {
    (plain_len / BLOCK_LEN + 1) * BLOCK_LEN
}

/// PKCS7-pads `buf[start..]` and AES-256-ECB encrypts it in place:
/// upstream `discovery/crypto.go:encrypt`.
pub fn encrypt_in_place(buf: &mut Vec<u8>, start: usize) {
    let plain_len = buf.len() - start;
    let padding = padded_len(plain_len) - plain_len;
    buf.resize(start + plain_len + padding, padding as u8);
    let (blocks, tail) = InOutBuf::from(&mut buf[start..]).into_chunks();
    debug_assert!(tail.is_empty());
    KEYS.cipher.encrypt_blocks_inout(blocks);
}

/// AES-ECB decrypts `buf` in place and validates PKCS7, returning the unpadded
/// length: upstream `discovery/crypto.go:decrypt`.
pub fn decrypt_in_place(buf: &mut [u8]) -> Result<usize, &'static str> {
    if buf.is_empty() || !buf.len().is_multiple_of(BLOCK_LEN) {
        return Err("invalid ciphertext length");
    }
    let (blocks, _) = InOutBuf::from(&mut *buf).into_chunks();
    KEYS.cipher.decrypt_blocks_inout(blocks);

    let padding_len = usize::from(buf[buf.len() - 1]);
    if padding_len == 0 || padding_len > BLOCK_LEN {
        return Err("invalid padding");
    }
    let plain_len = buf.len() - padding_len;
    if buf[plain_len..]
        .iter()
        .any(|&b| usize::from(b) != padding_len)
    {
        return Err("invalid padding bytes");
    }
    Ok(plain_len)
}

/// 32-byte HMAC-SHA256 over unpadded plaintext: upstream `discovery/packet.go:Marshal`.
/// `H(key ^ opad || H(key ^ ipad || data))` from the cached midstates.
pub fn hmac_checksum(data: &[u8]) -> [u8; CHECKSUM_LEN] {
    let inner = finish_after_block(KEYS.inner, data);
    finish_after_block(KEYS.outer, &inner)
}

/// Upstream `discovery/packet.go:Unmarshal` checksum, compared in constant time.
///
/// The digest is folded into one XOR accumulator with no data-dependent
/// branch; only the final accumulator passes an optimisation barrier (a
/// per-byte barrier, as in `subtle`, costs more than the compressions).
pub fn verify_hmac(data: &[u8], expected: &[u8; CHECKSUM_LEN]) -> bool {
    let actual = hmac_checksum(data);
    let diff = actual
        .chunks_exact(8)
        .zip(expected.chunks_exact(8))
        .fold(0u64, |acc, (a, b)| {
            acc | (u64::from_ne_bytes(a.try_into().expect("8-byte chunk"))
                ^ u64::from_ne_bytes(b.try_into().expect("8-byte chunk")))
        });
    std::hint::black_box(diff) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use hmac::{Hmac, Mac};

    /// The midstate HMAC must equal the RFC 2104 reference for every tail
    /// length class: empty, one tail block (< 56 bytes), the 56..64 boundary
    /// that forces a second padding block, and multi-block messages.
    #[test]
    fn midstate_hmac_matches_reference_hmac_at_every_padding_boundary() {
        let data: Vec<u8> = (0..=255u8).cycle().take(300).collect();
        for len in 0..data.len() {
            let mut reference = <Hmac<Sha256> as Mac>::new_from_slice(&key()).unwrap();
            reference.update(&data[..len]);
            let expected: [u8; 32] = reference.finalize().into_bytes().into();
            assert_eq!(hmac_checksum(&data[..len]), expected, "len {len}");
        }
    }

    fn encrypt(data: &[u8]) -> Vec<u8> {
        let mut buf = data.to_vec();
        encrypt_in_place(&mut buf, 0);
        buf
    }

    fn decrypt(data: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut buf = data.to_vec();
        let len = decrypt_in_place(&mut buf)?;
        buf.truncate(len);
        Ok(buf)
    }

    #[test]
    fn verify_hmac_rejects_a_flip_at_every_checksum_byte() {
        let data = b"CONNECTERROR 42 2";
        let checksum = hmac_checksum(data);
        assert!(verify_hmac(data, &checksum));
        for byte in 0..CHECKSUM_LEN {
            for bit in [0x01, 0x80] {
                let mut wrong = checksum;
                wrong[byte] ^= bit;
                assert!(!verify_hmac(data, &wrong), "byte {byte} bit {bit:#x}");
            }
        }
    }

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
