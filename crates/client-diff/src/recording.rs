//! Client-side wire recording shared by the `jolyne` capture client and
//! `bds-extractor`.
//!
//! [`RecordingTransport`] wraps a RakNet transport and hands every frame, in
//! both directions, to a [`Recorder`]. The recorder follows the session the
//! way the peer does (compression after `NetworkSettings`, AES-256-CTR after
//! `ServerToClientHandshake`, derived from the client identity key), splits
//! batches and writes one [`crate::Record`] JSONL line per packet: the
//! decrypted, decompressed, unbatched packet including its varuint header,
//! without the batch-entry length.

use std::{
    fs::File,
    io::{self, BufWriter, Read, Write},
    net::SocketAddr,
    path::Path,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context as TaskContext, Poll},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE_NO_PAD},
};
use bytes::Bytes;
use ctr::cipher::{KeyIvInit, StreamCipher};
use jolyne::{
    proto::raw::decode_packet_raw,
    stream::transport::{Datagram, Framing, RakNetTransport, Transmit, Transport},
    valentine::{McpePacketArgs, McpePacketData},
};
use p384::{PublicKey, SecretKey, pkcs8::DecodePublicKey};
use sha2::{Digest, Sha256};

/// Removes the batch-entry length prefix of one batched packet.
pub(crate) fn packet_bytes(frame: &[u8]) -> Result<&[u8]> {
    for (i, byte) in frame.iter().take(5).enumerate() {
        if byte & 0x80 == 0 {
            return Ok(&frame[i + 1..]);
        }
    }
    bail!("invalid packet length prefix")
}

fn numeric_varint(data: &[u8]) -> Result<(u32, usize)> {
    let mut value = 0u32;
    for (index, byte) in data.iter().take(5).enumerate() {
        ensure!(index != 4 || *byte <= 15, "capture varuint overflow");
        value |= u32::from(byte & 127) << (index * 7);
        if byte & 128 == 0 {
            return Ok((value, index + 1));
        }
    }
    bail!("truncated capture varuint")
}

// Split numeric packet frames before typed dispatch so future/unknown packets
// are retained verbatim, even when Jolyne itself cannot interpret them.
fn capture_entries(frame: &Bytes, compression: bool) -> Result<Vec<Bytes>> {
    const MAX: usize = 4 * 1024 * 1024;
    let payload = if frame.first() == Some(&0xfe) {
        frame.slice(1..)
    } else {
        frame.clone()
    };
    let payload = if compression {
        let (&algorithm, compressed) = payload.split_first().context("empty compressed capture")?;
        let decompressed = match algorithm {
            0 => {
                let mut output = Vec::new();
                flate2::read::DeflateDecoder::new(compressed)
                    .take((MAX + 1) as u64)
                    .read_to_end(&mut output)?;
                output
            }
            1 => {
                ensure!(
                    snap::raw::decompress_len(compressed)? <= MAX,
                    "capture decompression limit"
                );
                snap::raw::Decoder::new().decompress_vec(compressed)?
            }
            0xff => compressed.to_vec(),
            _ => bail!("unknown capture compression algorithm {algorithm}"),
        };
        Bytes::from(decompressed)
    } else {
        payload
    };
    ensure!(payload.len() <= MAX, "capture decompression limit");
    let mut packets = Vec::new();
    let mut offset = 0;
    while offset < payload.len() {
        let (length, prefix) = numeric_varint(&payload[offset..])?;
        ensure!(length != 0, "empty capture packet");
        let end = offset
            .checked_add(prefix)
            .and_then(|n| n.checked_add(length as usize))
            .context("capture frame length overflow")?;
        ensure!(end <= payload.len(), "truncated capture packet");
        packets.push(payload.slice(offset..end));
        offset = end;
    }
    Ok(packets)
}

type Cipher = ctr::Ctr32BE<aes::Aes256>;

/// Writes the packets of one client session as capture JSONL.
pub struct Recorder {
    writer: Box<dyn Write + Send>,
    started: Instant,
    seq: u64,
    compression: bool,
    identity_key: SecretKey,
    key: Option<[u8; 32]>,
    ciphers: Option<[Cipher; 2]>,
    counters: [u64; 2],
}

impl Recorder {
    /// A recorder for a session whose Login is signed by `identity_key`
    /// (needed to derive the encryption key).
    pub fn new(writer: Box<dyn Write + Send>, identity_key: SecretKey) -> Self {
        Self {
            writer,
            started: Instant::now(),
            seq: 0,
            compression: false,
            identity_key,
            key: None,
            ciphers: None,
            counters: [0, 0],
        }
    }

    /// Creates (truncates) `path` and records into it.
    pub fn create(path: &Path, identity_key: SecretKey) -> Result<Self> {
        let file = File::create(path).with_context(|| format!("creating {}", path.display()))?;
        Ok(Self::new(Box::new(BufWriter::new(file)), identity_key))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }

    /// Records one transport frame; `direction` 0 is client to server.
    fn frame(&mut self, direction: usize, frame: &Bytes) -> Result<()> {
        if frame.is_empty() {
            return Ok(());
        }
        let data = if let Some(ciphers) = &mut self.ciphers {
            ensure!(frame.len() >= 9, "truncated encrypted frame");
            let mut plain = frame.to_vec();
            ciphers[direction].apply_keystream(&mut plain[1..]);
            let end = plain.len() - 8;
            let mut digest = Sha256::new();
            digest.update(self.counters[direction].to_le_bytes());
            digest.update(&plain[1..end]);
            digest.update(self.key.context("capture cipher has no key")?);
            ensure!(
                digest.finalize()[..8] == plain[end..],
                "capture encryption checksum mismatch"
            );
            self.counters[direction] += 1;
            plain.truncate(end);
            Bytes::from(plain)
        } else {
            frame.clone()
        };
        let packets = capture_entries(&data, self.compression)?;
        for packet in packets {
            let payload = packet_bytes(&packet)?;
            let id = numeric_varint(payload)?.0 & 1023;
            let name = decode_packet_raw(&mut packet.clone())
                .ok()
                .and_then(|known| known.name())
                .map_or_else(|| format!("Unknown_{id}"), |name| format!("{name:?}"));
            let record = serde_json::json!({
                "seq": self.seq,
                "t_ms": self.started.elapsed().as_millis(),
                "dir": if direction == 0 { "c2s" } else { "s2c" },
                "id": id,
                "name": name,
                "hex": hex::encode(payload),
            });
            serde_json::to_writer(&mut self.writer, &record)?;
            self.writer.write_all(b"\n")?;
            // Surface storage errors at the packet that caused them, not on drop.
            self.writer.flush()?;
            self.seq += 1;
            if direction == 1 {
                match id {
                    143 => self.compression = true,
                    3 => {
                        if let (_, McpePacketData::ServerToClientHandshakePacket(handshake)) =
                            McpePacketData::decode_inner_bytes(&mut packet.clone(), McpePacketArgs)?
                        {
                            self.enable_encryption(&handshake.handshake_web_token)?;
                        }
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn enable_encryption(&mut self, token: &str) -> Result<()> {
        // The real client independently verifies the signature before accepting it.
        let parts: Vec<_> = token.split('.').collect();
        ensure!(parts.len() == 3, "invalid server handshake JWT");
        let header: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0])?)?;
        let claims: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1])?)?;
        let der = STANDARD.decode(header["x5u"].as_str().context("handshake missing x5u")?)?;
        let public = PublicKey::from_public_key_der(&der)?;
        let salt = claims["salt"].as_str().context("handshake missing salt")?;
        let salt = STANDARD
            .decode(salt)
            .or_else(|_| STANDARD_NO_PAD.decode(salt))?;
        let secret =
            p384::ecdh::diffie_hellman(self.identity_key.to_nonzero_scalar(), public.as_affine());
        let mut digest = Sha256::new();
        digest.update(salt);
        digest.update(secret.raw_secret_bytes());
        let key: [u8; 32] = digest.finalize().into();
        let mut iv = [0; 16];
        iv[..12].copy_from_slice(&key[..12]);
        iv[15] = 2;
        let cipher = Cipher::new_from_slices(&key, &iv)
            .map_err(|_| anyhow::anyhow!("invalid capture cipher key"))?;
        self.ciphers = Some([cipher.clone(), cipher]);
        self.key = Some(key);
        Ok(())
    }
}

/// A RakNet transport that records every frame it sends and receives.
pub struct RecordingTransport {
    inner: RakNetTransport,
    recorder: Arc<Mutex<Recorder>>,
}

impl RecordingTransport {
    pub fn new(inner: RakNetTransport, recorder: Arc<Mutex<Recorder>>) -> Self {
        Self { inner, recorder }
    }

    fn observe(&self, direction: usize, bytes: &Bytes) -> io::Result<()> {
        self.recorder
            .lock()
            .map_err(|_| io::Error::other("capture lock poisoned"))?
            .frame(direction, bytes)
            .map_err(|error| io::Error::other(format!("packet capture: {error:#}")))
    }
}

impl Transport for RecordingTransport {
    type Error = io::Error;
    const FRAMING: Framing = Framing::RakNet;

    fn poll_send_ready(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_send_ready(cx)
            .map_err(io::Error::other)
    }

    fn start_send(mut self: Pin<&mut Self>, message: Transmit) -> io::Result<()> {
        let bytes = message.payload.clone();
        Pin::new(&mut self.inner)
            .start_send(message)
            .map_err(io::Error::other)?;
        self.observe(0, &bytes)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_flush(cx)
            .map_err(io::Error::other)
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner)
            .poll_close(cx)
            .map_err(io::Error::other)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
    ) -> Poll<Option<io::Result<Datagram>>> {
        match Pin::new(&mut self.inner).poll_recv(cx) {
            Poll::Ready(Some(Ok(datagram))) => {
                let bytes = datagram.into_bytes();
                Poll::Ready(Some(
                    self.observe(1, &bytes)
                        .map(|()| Datagram::Contiguous(bytes)),
                ))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(io::Error::other(error)))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer_addr()
    }

    fn latency(&self) -> Option<Duration> {
        self.inner.latency()
    }
}

/// Opens a RakNet connection to `address` (30 s timeout) wrapped in a
/// recording transport.
pub async fn connect(
    address: SocketAddr,
    recorder: Arc<Mutex<Recorder>>,
) -> Result<RecordingTransport> {
    let socket = tokio::time::timeout(
        Duration::from_secs(30),
        tokio_raknet::RaknetStream::connect(address),
    )
    .await
    .context("RakNet connection timeout")??;
    Ok(RecordingTransport::new(
        RakNetTransport::new(socket),
        recorder,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_capture_removes_only_length_prefix() {
        let frame = Bytes::from_static(&[4, 0x81, 0x02, 0xaa, 0xbb]);
        assert_eq!(packet_bytes(&frame).unwrap(), &[0x81, 0x02, 0xaa, 0xbb]);
        assert!(packet_bytes(&Bytes::from_static(&[0x80])).is_err());
    }

    #[test]
    fn capture_preserves_wire_headers_and_sequences_both_directions() {
        let path = std::env::temp_dir().join(format!("client-diff-{}.jsonl", uuid::Uuid::new_v4()));
        let mut capture =
            Recorder::create(&path, SecretKey::from_slice(&[1; 48]).unwrap()).unwrap();
        // RequestNetworkSettings with a two-byte varuint packet header, and
        // PlayStatus, each in an uncompressed pre-negotiation RakNet batch.
        capture
            .frame(0, &Bytes::from_static(&[0xfe, 6, 0xc1, 1, 0, 0, 8, 0x91]))
            .unwrap();
        capture
            .frame(1, &Bytes::from_static(&[0xfe, 5, 2, 0, 0, 0, 0]))
            .unwrap();
        // An unknown numeric ID must be retained without losing neighboring packets.
        capture
            .frame(
                1,
                &Bytes::from_static(&[0xfe, 3, 0xff, 0x07, 0xaa, 5, 2, 0, 0, 0, 0]),
            )
            .unwrap();
        drop(capture);
        let records: Vec<serde_json::Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        std::fs::remove_file(path).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[0]["seq"], 0);
        assert_eq!(records[1]["seq"], 1);
        assert_eq!(records[0]["dir"], "c2s");
        assert_eq!(records[1]["dir"], "s2c");
        assert_eq!(records[0]["id"], 193);
        assert_eq!(records[0]["hex"], "c10100000891");
        assert_eq!(records[1]["hex"], "0200000000");
        assert_eq!(records[2]["id"], 1023);
        assert_eq!(records[2]["hex"], "ff07aa");
        assert_eq!(records[3]["hex"], "0200000000");
    }
}
