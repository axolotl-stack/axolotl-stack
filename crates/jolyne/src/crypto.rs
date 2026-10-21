//! Owned, in-place Bedrock batch encryption and checksum verification.

use aes::Aes256Enc;
use bytes::BytesMut;
use ctr::cipher::{
    BlockSizeUser, KeyIvInit, StreamCipher, StreamCipherBackend, StreamCipherClosure,
    StreamCipherCore, consts::U16, typenum::Unsigned,
};
use sha2::{Digest, Sha256};

use crate::error::{JolyneError, ProtocolError};

type Aes256Ctr = ctr::Ctr32BE<Aes256Enc>;
const CHECKSUM_LEN: usize = 8;

struct BackendWidth<'a>(&'a mut usize);

impl BlockSizeUser for BackendWidth<'_> {
    type BlockSize = U16;
}

impl StreamCipherClosure for BackendWidth<'_> {
    fn call<B: StreamCipherBackend<BlockSize = U16>>(self, _: &mut B) {
        *self.0 = B::ParBlocksSize::USIZE * 16;
    }
}

/// One direction of Bedrock's continuous AES-256-CTR batch stream.
///
/// Create separate instances for sending and receiving with the same key and IV.
/// Each nonempty encrypted batch appends the first eight bytes of
/// SHA256(little-endian batch counter || payload || key). The counter starts at
/// zero; neither it nor the AES stream resets between batches.
pub struct BatchCipher {
    cipher: Aes256Ctr,
    key: [u8; 32],
    counter: u64,
    keystream: Box<[u8]>,
    used_keystream: usize,
}

impl BatchCipher {
    /// Creates a stream using the Bedrock IV expansion `[iv, 0, 0, 0, 2]`.
    pub fn new(key: [u8; 32], iv: [u8; 12]) -> Self {
        let mut iv16 = [0u8; 16];
        iv16[..12].copy_from_slice(&iv);
        iv16[15] = 2;
        let mut core =
            ctr::CtrCore::<Aes256Enc, ctr::flavors::Ctr32BE>::new_from_slices(&key, &iv16)
                .expect("AES-256-CTR key/iv lengths are fixed (32/16 bytes)");
        let mut group_len = 16;
        core.process_with_backend(BackendWidth(&mut group_len));
        Self {
            cipher: Aes256Ctr::from_core(core),
            key,
            counter: 0,
            keystream: vec![0; group_len].into_boxed_slice(),
            used_keystream: group_len,
        }
    }

    // Prefetch one complete group for the selected AES/VAES backend, retaining
    // unused tail bytes across batches rather than encrypting scalar blocks.
    fn apply_keystream(&mut self, data: &mut [u8]) {
        let cached = (self.keystream.len() - self.used_keystream).min(data.len());
        let (head, rest) = data.split_at_mut(cached);
        for (byte, key) in head.iter_mut().zip(&self.keystream[self.used_keystream..]) {
            *byte ^= key;
        }
        self.used_keystream += cached;
        if rest.is_empty() {
            return;
        }
        let group_len = rest.len() / self.keystream.len() * self.keystream.len();
        let (groups, tail) = rest.split_at_mut(group_len);
        self.cipher.apply_keystream(groups);
        if !tail.is_empty() {
            // Do not prefetch beyond the underlying cipher's stream limit.
            self.keystream.fill(0);
            if self
                .cipher
                .try_apply_keystream(&mut self.keystream)
                .is_err()
            {
                self.cipher.apply_keystream(tail);
                return;
            }
            for (byte, key) in tail.iter_mut().zip(self.keystream.iter()) {
                *byte ^= key;
            }
            self.used_keystream = tail.len();
        }
    }

    /// Appends a checksum and encrypts an owned batch in place.
    ///
    /// `header_len` leading bytes remain cleartext and are not checksummed:
    /// normally one for RakNet's `0xFE` prefix, or zero for prefix-free frames.
    /// An empty buffer is a no-op. Invalid header lengths leave state unchanged.
    /// Reserve eight spare bytes in `buf` to avoid allocation.
    pub fn encrypt(&mut self, buf: &mut BytesMut, header_len: usize) -> Result<(), JolyneError> {
        if buf.is_empty() {
            return Ok(());
        }
        if header_len > buf.len() {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "encryption header length {} exceeds packet length {}",
                header_len,
                buf.len()
            ))
            .into());
        }

        let counter_value = self.counter;
        self.counter = self.counter.wrapping_add(1);

        let mut digest = Sha256::new();
        digest.update(counter_value.to_le_bytes());
        digest.update(&buf[header_len..]);
        digest.update(self.key);
        let checksum = digest.finalize();

        buf.extend_from_slice(&checksum[..CHECKSUM_LEN]);
        self.apply_keystream(&mut buf[header_len..]);
        Ok(())
    }

    /// Decrypts, verifies, and removes a batch checksum in place without allocation.
    ///
    /// `header_len` has the same meaning as in [`Self::encrypt`]. Short frames
    /// leave state unchanged. A checksum failure consumes stream and counter
    /// state and leaves unauthenticated plaintext in `buf`; discard it and close
    /// the connection rather than retrying it or using its contents.
    pub fn decrypt(&mut self, buf: &mut BytesMut, header_len: usize) -> Result<(), JolyneError> {
        if buf
            .len()
            .checked_sub(header_len)
            .is_none_or(|len| len < CHECKSUM_LEN)
        {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "encrypted packet must be at least {} bytes long, got {}",
                header_len.saturating_add(CHECKSUM_LEN),
                buf.len()
            ))
            .into());
        }

        self.apply_keystream(&mut buf[header_len..]);
        let checksum_start = buf.len() - CHECKSUM_LEN;
        let their_checksum = &buf[checksum_start..];
        let counter_value = self.counter;
        self.counter = self.counter.wrapping_add(1);

        let mut digest = Sha256::new();
        digest.update(counter_value.to_le_bytes());
        digest.update(&buf[header_len..checksum_start]);
        digest.update(self.key);
        let our_checksum_full = digest.finalize();
        let our_checksum = &our_checksum_full[..CHECKSUM_LEN];

        if their_checksum != our_checksum {
            return Err(ProtocolError::UnexpectedHandshake(format!(
                "invalid checksum of packet {}: expected {:02x?}, got {:02x?}",
                counter_value, our_checksum, their_checksum
            ))
            .into());
        }

        buf.truncate(checksum_start);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [0x42; 32];
    const IV: [u8; 12] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12];

    #[test]
    fn independent_vectors_cross_parallel_cache_boundaries() {
        // Node/OpenSSL continuous AES-256-CTR; digests keep long vectors compact.
        let mut send = BatchCipher::new(KEY, IV);
        for (len, expected) in [
            (
                127,
                "7db18ae72567b7c6c6ff5ab2fd4b05bd6e897eca092fa957f4923f3ecddb6756",
            ),
            (
                1,
                "43dbfe30e3cf3daa173b0a03ba7330caf0fe486f77c0702b2c36c4619948367c",
            ),
            (
                129,
                "fa351c3a48276a107a64eab31a003555263413c865a65b83f21784c55bb6afbb",
            ),
            (
                479,
                "e986be4964c93395834decce6d0367b80b01e8cf12590c776a4b6d4f396a1646",
            ),
            (
                1,
                "d245097ad5f409da6370a5733228d13b6974778dc22a22b0ff2a3bb9a9b0b958",
            ),
            (
                481,
                "89e6ddbc5bfe37bedaa983997a4382951261fa3cf8e82a3551df9f8e90602a7b",
            ),
        ] {
            let mut input = BytesMut::with_capacity(len + 9);
            input.extend_from_slice(&[0xfe]);
            input.extend((0..len).map(|i| i as u8));
            send.encrypt(&mut input, 1).unwrap();
            for (i, byte) in Sha256::digest(&input).iter().enumerate() {
                assert_eq!(
                    *byte,
                    u8::from_str_radix(&expected[2 * i..2 * i + 2], 16).unwrap()
                );
            }
        }
    }

    #[test]
    fn cache_prefetch_preserves_short_reads_at_stream_limit() {
        use ctr::cipher::StreamCipherSeek;

        let mut send = BatchCipher::new(KEY, IV);
        let mut recv = BatchCipher::new(KEY, IV);
        let position = (u64::from(u32::MAX) - 1) * 16;
        send.cipher.seek(position);
        recv.cipher.seek(position);
        let mut input = BytesMut::from(&b"\xfeone"[..]);
        send.encrypt(&mut input, 1).unwrap();
        recv.decrypt(&mut input, 1).unwrap();
        assert_eq!(&input[..], b"\xfeone");
    }

    #[test]
    fn known_vectors_preserve_continuous_stream_and_batch_counter() {
        // Independently generated with Node/OpenSSL AES-256-CTR and SHA-256.
        // Both frames share one cipher stream, including the partial AES block.
        let plaintexts: [&[u8]; 2] = [&[0xfe, 1, 2, 3, 4], b"\xfesecond batch"];
        let ciphertexts: [&[u8]; 2] = [
            &[254, 180, 248, 37, 72, 4, 118, 67, 143, 112, 1, 185, 134],
            &[
                254, 57, 236, 13, 163, 254, 219, 227, 164, 102, 235, 84, 26, 43, 168, 64, 39, 140,
                4, 245, 252,
            ],
        ];
        let mut send = BatchCipher::new(KEY, IV);
        let mut recv = BatchCipher::new(KEY, IV);
        for (plaintext, ciphertext) in plaintexts.into_iter().zip(ciphertexts) {
            let mut buf = BytesMut::from(plaintext);
            send.encrypt(&mut buf, 1).unwrap();
            assert_eq!(&buf[..], ciphertext);
            recv.decrypt(&mut buf, 1).unwrap();
            assert_eq!(&buf[..], plaintext);
        }
    }

    #[test]
    fn roundtrip_header_and_block_boundaries_without_reallocation() {
        for header_len in [0, 1, 3] {
            let mut send = BatchCipher::new(KEY, IV);
            let mut recv = BatchCipher::new(KEY, IV);
            for payload_len in [0, 1, 7, 8, 15, 16, 17, 63, 64, 65, 1024] {
                if header_len + payload_len == 0 {
                    continue; // Empty outgoing frames are deliberately a no-op.
                }
                let original = vec![0xfe; header_len + payload_len];
                let mut buf = BytesMut::with_capacity(original.len() + CHECKSUM_LEN);
                buf.extend_from_slice(&original);
                let ptr = buf.as_ptr();
                send.encrypt(&mut buf, header_len).unwrap();
                assert_eq!(buf.as_ptr(), ptr);
                assert_eq!(buf.len(), original.len() + CHECKSUM_LEN);
                assert_eq!(&buf[..header_len], &original[..header_len]);
                recv.decrypt(&mut buf, header_len).unwrap();
                assert_eq!(buf.as_ptr(), ptr);
                assert_eq!(&buf[..], &original);
            }
        }
    }

    #[test]
    fn rejects_payload_and_checksum_tampering() {
        for offset in [2, 12] {
            let mut send = BatchCipher::new(KEY, IV);
            let mut recv = BatchCipher::new(KEY, IV);
            let mut buf = BytesMut::from(&[0xfe, 1, 2, 3, 4][..]);
            send.encrypt(&mut buf, 1).unwrap();
            buf[offset] ^= 0xff;
            assert!(matches!(
                recv.decrypt(&mut buf, 1),
                Err(JolyneError::Protocol(ProtocolError::UnexpectedHandshake(_)))
            ));
            assert_eq!(recv.counter, 1);
        }
    }

    #[test]
    fn rejects_wrong_counter_and_key() {
        for wrong_key in [false, true] {
            let mut send = BatchCipher::new(KEY, IV);
            let mut recv = BatchCipher::new(if wrong_key { [0x43; 32] } else { KEY }, IV);
            if !wrong_key {
                recv.counter = 1;
            }
            let mut buf = BytesMut::from(&b"\xfepayload"[..]);
            send.encrypt(&mut buf, 1).unwrap();
            assert!(recv.decrypt(&mut buf, 1).is_err());
        }
    }

    #[test]
    fn empty_encrypt_and_invalid_headers_do_not_advance_state() {
        let mut cipher = BatchCipher::new(KEY, IV);
        let mut empty = BytesMut::new();
        cipher.encrypt(&mut empty, 1).unwrap();
        assert!(empty.is_empty());
        assert_eq!(cipher.counter, 0);
        for header_len in [2, usize::MAX] {
            let mut buf = BytesMut::from(&b"x"[..]);
            assert!(cipher.encrypt(&mut buf, header_len).is_err());
            assert_eq!(&buf[..], b"x");
            assert_eq!(cipher.counter, 0);
        }
        for (header_len, len) in [(0, 0), (0, 7), (1, 8), (usize::MAX, 8)] {
            let mut buf = BytesMut::from(vec![0; len].as_slice());
            assert!(cipher.decrypt(&mut buf, header_len).is_err());
            assert_eq!(cipher.counter, 0);
        }
        let mut expected = BytesMut::from(&b"\xfevalid"[..]);
        let mut actual = expected.clone();
        BatchCipher::new(KEY, IV).encrypt(&mut expected, 1).unwrap();
        cipher.encrypt(&mut actual, 1).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn batch_counter_wraps_without_resetting_cipher() {
        let mut send = BatchCipher::new(KEY, IV);
        let mut recv = BatchCipher::new(KEY, IV);
        send.counter = u64::MAX;
        recv.counter = u64::MAX;
        for expected_counter in [0, 1] {
            let mut buf = BytesMut::from(&b"\xfepayload"[..]);
            send.encrypt(&mut buf, 1).unwrap();
            recv.decrypt(&mut buf, 1).unwrap();
            assert_eq!(&buf[..], b"\xfepayload");
            assert_eq!(send.counter, expected_counter);
            assert_eq!(recv.counter, expected_counter);
        }
    }
}
