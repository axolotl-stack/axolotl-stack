//! Raw DEFLATE/Snappy engines of a negotiated batch compression.
//!
//! A [`Compression`] exists only once compression is negotiated and is built
//! complete for the negotiated algorithm and level: there is no lazily
//! created engine and no per-call algorithm switching.
//!
//! Levels 1..=9 retain their numeric value with libdeflate; larger values
//! are clamped to 9. Level zero sends every batch with the uncompressed
//! marker. The default remains 7; libdeflate and Go's level 6 use different
//! parsers, so numeric levels do not promise identical output sizes and must
//! be compared on the corpus.

mod snappy;

use bytes::BytesMut;
use libdeflater::{CompressionLvl, DecompressionError};

use super::batch::BatchCompression;
use crate::error::{JolyneError, ProtocolError};

/// Engines and scratch for one negotiated compression.
///
/// Outbound batches use the negotiated algorithm; inbound batches may carry
/// either algorithm's marker (as gophertunnel accepts), so both decoders
/// are ready.
pub(crate) struct Compression {
    engine: Engine,
    inflater: libdeflater::Decompressor,
    unsnapper: snap::raw::Decoder,
    scratch: BytesMut,
    decode_scratch: BytesMut,
    last_decompressed_len: usize,
}

/// The outbound engine.
enum Engine {
    /// Algorithm `None` or level 0: batches carry the uncompressed marker.
    Off,
    Deflate(libdeflater::Compressor),
    Snappy(snappy::Encoder),
}

fn failed(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::DecompressionFailed(error.to_string())
}

fn too_large(max: usize) -> ProtocolError {
    failed(format!("decompressed data exceeds max size of {max} bytes"))
}

/// Upper bound on Snappy's output/input ratio (64/3, rounded up).
const SNAPPY_MAX_EXPANSION: usize = 22;

/// Grows `scratch` to `prefix_len + bound`, copies the batch prefix and
/// returns the space the compressor writes into.
fn stage<'a>(
    scratch: &'a mut BytesMut,
    out: &BytesMut,
    prefix_len: usize,
    bound: usize,
) -> &'a mut [u8] {
    scratch.resize(prefix_len + bound, 0);
    scratch[..prefix_len].copy_from_slice(&out[..prefix_len]);
    &mut scratch[prefix_len..]
}

impl Compression {
    /// The engines for `algorithm` at `level`.
    pub(crate) fn new(algorithm: BatchCompression, level: u32) -> Result<Self, JolyneError> {
        let engine = match algorithm {
            BatchCompression::None => Engine::Off,
            _ if level == 0 => Engine::Off,
            BatchCompression::Deflate => {
                let level = CompressionLvl::new(level.min(9) as i32)
                    .map_err(|_| ProtocolError::InvalidCompressionLevel(level))?;
                Engine::Deflate(libdeflater::Compressor::new(level))
            }
            BatchCompression::Snappy => Engine::Snappy(snappy::Encoder::default()),
        };
        Ok(Self {
            engine,
            inflater: libdeflater::Decompressor::new(),
            unsnapper: snap::raw::Decoder::new(),
            scratch: BytesMut::new(),
            decode_scratch: BytesMut::new(),
            last_decompressed_len: 0,
        })
    }

    /// The marker written on batches this compresses.
    pub(crate) fn algorithm(&self) -> BatchCompression {
        match self.engine {
            Engine::Off => BatchCompression::None,
            Engine::Deflate(_) => BatchCompression::Deflate,
            Engine::Snappy(_) => BatchCompression::Snappy,
        }
    }

    #[cfg(any(test, feature = "bench"))]
    /// Synchronous callbacks cannot retain a borrow, so their output stays owned.
    pub(crate) fn reclaim(&mut self, output: BytesMut) {
        self.decode_scratch = output;
    }

    /// Compresses `out[prefix_len..]` in place with the outbound engine; a
    /// no-op when it is [`Engine::Off`].
    pub(crate) fn compress(
        &mut self,
        out: &mut BytesMut,
        prefix_len: usize,
    ) -> Result<(), JolyneError> {
        let input_len = out.len() - prefix_len;
        let len = match &mut self.engine {
            Engine::Off => return Ok(()),
            Engine::Deflate(compressor) => {
                let bound = compressor.deflate_compress_bound(input_len);
                let destination = stage(&mut self.scratch, out, prefix_len, bound);
                compressor
                    .deflate_compress(&out[prefix_len..], destination)
                    .map_err(failed)?
            }
            Engine::Snappy(encoder) => {
                let bound = snap::raw::max_compress_len(input_len);
                if bound == 0 {
                    return Err(failed("Snappy input exceeds the format size limit").into());
                }
                let destination = stage(&mut self.scratch, out, prefix_len, bound);
                encoder.compress(&out[prefix_len..], destination)
            }
        };
        self.scratch.truncate(prefix_len + len);
        std::mem::swap(out, &mut self.scratch);
        Ok(())
    }

    /// Decompresses a batch body whose marker named `algorithm`, bounded by
    /// `max` bytes of output.
    pub(crate) fn decompress(
        &mut self,
        input: &[u8],
        algorithm: BatchCompression,
        max: usize,
    ) -> Result<BytesMut, ProtocolError> {
        match algorithm {
            BatchCompression::None => {
                if input.len() > max {
                    return Err(too_large(max));
                }
                Ok(BytesMut::from(input))
            }
            BatchCompression::Snappy => self.unsnap(input, max),
            BatchCompression::Deflate => self.inflate(input, max),
        }
    }

    fn unsnap(&mut self, input: &[u8], max: usize) -> Result<BytesMut, ProtocolError> {
        let len = snap::raw::decompress_len(input).map_err(failed)?;
        if len > max {
            return Err(too_large(max));
        }
        // A Snappy element expands at most 64/3 times (a three-byte copy of
        // 64 bytes): reject impossible declared lengths before zeroing a
        // buffer of that size for a tiny batch.
        if len > input.len().saturating_mul(SNAPPY_MAX_EXPANSION) {
            return Err(failed(format!(
                "Snappy batch declares {len} bytes from {} input bytes",
                input.len()
            )));
        }
        let mut out = std::mem::take(&mut self.decode_scratch);
        out.resize(len, 0);
        let written = self.unsnapper.decompress(input, &mut out).map_err(failed)?;
        out.truncate(written);
        Ok(out)
    }

    fn inflate(&mut self, input: &[u8], max: usize) -> Result<BytesMut, ProtocolError> {
        // A geometric search avoids zeroing the full configured maximum for
        // small batches. Cap speculative allocations/retries; unknown sizes
        // and very large batches fall back to a strict streaming decoder.
        let ceiling = max.min(1024 * 1024);
        let mut size = input
            .len()
            .saturating_mul(4)
            .max(8192)
            .max(self.last_decompressed_len)
            .min(ceiling);
        let mut out = std::mem::take(&mut self.decode_scratch);
        out.resize(size, 0);
        loop {
            match self.inflater.deflate_decompress(input, &mut out) {
                Ok(len) => {
                    out.truncate(len);
                    self.last_decompressed_len = len;
                    return Ok(out);
                }
                Err(DecompressionError::BadData) => {
                    return Err(failed("invalid or truncated raw DEFLATE stream"));
                }
                Err(DecompressionError::InsufficientSpace) => {
                    if max == size {
                        return Err(too_large(size));
                    }
                    if size == ceiling {
                        break;
                    }
                    size = size.saturating_mul(2).max(1).min(ceiling);
                    out.resize(size, 0);
                }
            }
        }

        out.clear();
        let mut decoder = flate2::Decompress::new(false);
        let mut chunk = [0; 8192];
        loop {
            let before_in = decoder.total_in();
            let before_out = decoder.total_out();
            let status = decoder
                .decompress(
                    &input[before_in as usize..],
                    &mut chunk,
                    flate2::FlushDecompress::None,
                )
                .map_err(failed)?;
            let written = (decoder.total_out() - before_out) as usize;
            if written > max.saturating_sub(out.len()) {
                return Err(too_large(max));
            }
            out.extend_from_slice(&chunk[..written]);
            if status == flate2::Status::StreamEnd {
                self.last_decompressed_len = out.len();
                return Ok(out);
            }
            if decoder.total_in() == before_in && written == 0 {
                return Err(failed("invalid or truncated raw DEFLATE stream"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A file under `crates/jolyne/tests/fixtures`.
    fn fixture(path: &str) -> Vec<u8> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::read(root.join(path)).unwrap()
    }

    /// xorshift32 noise, identical to `snappyNoise` in
    /// `tools/bench-compare/go/snappy_test.go`.
    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x1234_5678u32;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    const BATCHES: [&str; 4] = ["spawn", "server_tick", "chunk_radius", "client_tick"];

    #[test]
    fn snappy_corpus_size_and_reference_roundtrip() {
        for (name, limit) in BATCHES.into_iter().zip([1155, 1599, 79052, 287]) {
            let input = fixture(&format!("batches/{name}.bin"));
            let mut output = BytesMut::from(&b"\xfe\x01"[..]);
            output.extend_from_slice(&input[1..]);
            Compression::new(BatchCompression::Snappy, 6)
                .unwrap()
                .compress(&mut output, 2)
                .unwrap();
            assert_eq!(
                snap::raw::Decoder::new()
                    .decompress_vec(&output[2..])
                    .unwrap(),
                &input[1..]
            );
            assert!(output.len() <= limit, "{name}: {} > {limit}", output.len());
        }
    }

    /// `tests/fixtures/snappy` holds this encoder's output, which the Go
    /// interop test decodes with gophertunnel; the encoder must keep
    /// producing exactly those bytes.
    #[test]
    fn snappy_output_matches_the_golden_fixtures() {
        let mut cases = vec![
            ("long-literal", noise(65537)),
            ("copy4", [noise(4096), vec![0; 70000], noise(4096)].concat()),
        ];
        for name in BATCHES {
            cases.push((name, fixture(&format!("batches/{name}.bin"))[1..].to_vec()));
        }
        for (name, plain) in cases {
            let golden = fixture(&format!("snappy/{name}.snappy"));
            let mut output = BytesMut::from(plain.as_slice());
            Compression::new(BatchCompression::Snappy, 1)
                .unwrap()
                .compress(&mut output, 0)
                .unwrap();
            assert!(output.as_ref() == golden, "{name}: encoder output changed");
            assert_eq!(
                snap::raw::Decoder::new().decompress_vec(&golden).unwrap(),
                plain
            );
        }
    }

    #[test]
    fn snappy_rejects_impossible_declared_length_without_allocating() {
        let mut codec = Compression::new(BatchCompression::Snappy, 6).unwrap();
        // Declares 16 MiB - 1 (within the batch limit) in four bytes plus a
        // one-byte literal.
        let input = [0xff, 0xff, 0xff, 0x07, 0x00, b'x'];
        let error = codec
            .decompress(&input, BatchCompression::Snappy, 16 * 1024 * 1024)
            .unwrap_err();
        assert!(error.to_string().contains("declares"), "{error}");
        assert!(codec.decode_scratch.capacity() < 4096);
        // The largest possible expansion still decodes.
        let data = vec![9u8; 64 * 1024];
        let mut compressed = BytesMut::from(data.as_slice());
        codec.compress(&mut compressed, 0).unwrap();
        assert!(compressed.len().saturating_mul(SNAPPY_MAX_EXPANSION) >= data.len());
        let decoded = codec
            .decompress(&compressed, BatchCompression::Snappy, data.len())
            .unwrap();
        assert_eq!(decoded.as_ref(), data.as_slice());
    }

    #[test]
    fn decode_reuses_unshared_storage_and_remembers_output_size() {
        let input = vec![7; 128 * 1024];
        let mut codec = Compression::new(BatchCompression::Deflate, 7).unwrap();
        let mut compressed = BytesMut::from(input.as_slice());
        codec.compress(&mut compressed, 0).unwrap();
        let first = codec
            .decompress(&compressed, BatchCompression::Deflate, input.len())
            .unwrap();
        assert_eq!(codec.last_decompressed_len, input.len());
        let pointer = first.as_ptr();
        codec.reclaim(first);
        let second = codec
            .decompress(&compressed, BatchCompression::Deflate, input.len())
            .unwrap();
        assert_eq!(second.as_ptr(), pointer);
        assert_eq!(second.as_ref(), input.as_slice());
        let held = second.freeze();
        let third = codec
            .decompress(&compressed, BatchCompression::Deflate, input.len())
            .unwrap();
        assert_ne!(third.as_ptr(), held.as_ptr());
        assert_eq!(held.as_ref(), input.as_slice());
    }

    #[test]
    fn raw_deflate_interoperates_and_enforces_exact_bound() {
        let input = b"Bedrock raw deflate compatibility".repeat(4096);
        let mut codec = Compression::new(BatchCompression::Deflate, 7).unwrap();
        let mut output = BytesMut::from(&b"\xfe\x00"[..]);
        output.extend_from_slice(&input);
        codec.compress(&mut output, 2).unwrap();
        let mut decoded = Vec::new();
        flate2::read::DeflateDecoder::new(&output[2..])
            .read_to_end(&mut decoded)
            .unwrap();
        assert_eq!(decoded, input);
        assert_eq!(
            codec
                .decompress(&output[2..], BatchCompression::Deflate, input.len())
                .unwrap(),
            input
        );
        assert!(
            codec
                .decompress(&output[2..], BatchCompression::Deflate, input.len() - 1)
                .is_err()
        );
        assert!(
            codec
                .decompress(
                    &output[2..output.len() - 1],
                    BatchCompression::Deflate,
                    usize::MAX
                )
                .is_err()
        );
    }

    #[test]
    fn accepts_flate2_raw_stream_and_unknown_size_growth() {
        let input = vec![42; 2 * 1024 * 1024];
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
        encoder.write_all(&input).unwrap();
        let compressed = encoder.finish().unwrap();
        let mut codec = Compression::new(BatchCompression::Deflate, 7).unwrap();
        assert_eq!(
            codec
                .decompress(&compressed, BatchCompression::Deflate, usize::MAX)
                .unwrap(),
            input
        );
        assert!(
            codec
                .decompress(
                    &compressed[..compressed.len() / 2],
                    BatchCompression::Deflate,
                    usize::MAX
                )
                .is_err()
        );
    }

    #[test]
    fn reusable_snappy_and_zero_bound() {
        let mut codec = Compression::new(BatchCompression::Snappy, 7).unwrap();
        for input in [
            &b""[..],
            &b"snappy repeated payload snappy repeated payload"[..],
        ] {
            let mut output = BytesMut::from(&b"\x01"[..]);
            output.extend_from_slice(input);
            codec.compress(&mut output, 1).unwrap();
            assert_eq!(
                codec
                    .decompress(&output[1..], BatchCompression::Snappy, input.len())
                    .unwrap(),
                input
            );
            if !input.is_empty() {
                assert!(
                    codec
                        .decompress(&output[1..], BatchCompression::Snappy, 0)
                        .is_err()
                );
            }
        }
        assert!(codec.decompress(&[], BatchCompression::Deflate, 0).is_err());
    }

    /// Level 0 and algorithm `None` send batches uncompressed; deflate levels
    /// above libdeflate's useful range clamp instead of failing.
    #[test]
    fn engine_follows_the_negotiated_algorithm_and_level() {
        for (algorithm, level, marker) in [
            (BatchCompression::None, 7, BatchCompression::None),
            (BatchCompression::Deflate, 0, BatchCompression::None),
            (BatchCompression::Deflate, 7, BatchCompression::Deflate),
            (BatchCompression::Deflate, 200, BatchCompression::Deflate),
            (BatchCompression::Snappy, 1, BatchCompression::Snappy),
        ] {
            let mut codec = Compression::new(algorithm, level).unwrap();
            assert_eq!(codec.algorithm(), marker, "{algorithm:?} level {level}");
            let mut output = BytesMut::from(&b"\xfe\xffsome batch body"[..]);
            let before = output.clone();
            codec.compress(&mut output, 2).unwrap();
            assert_eq!(output == before, marker == BatchCompression::None);
        }
    }
}
