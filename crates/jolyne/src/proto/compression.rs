//! Reusable whole-batch raw DEFLATE/Snappy engines.
//!
//! Levels 1..=9 retain their numeric value with libdeflate; larger values
//! are clamped to 9. Level zero is
//! handled by batch framing as an uncompressed marker. The default remains
//! 7; libdeflate and Go's level 6 use different parsers, so numeric levels
//! do not promise identical output sizes and must be compared on the corpus.

mod snappy;

use bytes::BytesMut;
use libdeflater::{CompressionLvl, Compressor, DecompressionError, Decompressor};

use super::batch::BatchCompression;
use crate::error::{JolyneError, ProtocolError};

pub(crate) struct CompressionContext {
    compressor: Option<(u32, Compressor)>,
    decompressor: Option<Decompressor>,
    snappy_encoder: snappy::Encoder,
    snappy_decoder: snap::raw::Decoder,
    scratch: BytesMut,
    decode_scratch: BytesMut,
    last_decompressed_len: usize,
}

impl Default for CompressionContext {
    fn default() -> Self {
        Self {
            compressor: None,
            decompressor: None,
            snappy_encoder: snappy::Encoder::default(),
            snappy_decoder: snap::raw::Decoder::new(),
            scratch: BytesMut::new(),
            decode_scratch: BytesMut::new(),
            last_decompressed_len: 0,
        }
    }
}

fn failed(error: impl std::fmt::Display) -> ProtocolError {
    ProtocolError::DecompressionFailed(error.to_string())
}

fn too_large(max: usize) -> ProtocolError {
    failed(format!("decompressed data exceeds max size of {max} bytes"))
}

impl CompressionContext {
    /// Synchronous callbacks cannot retain a borrow, so their output stays owned.
    pub(crate) fn reclaim(&mut self, output: BytesMut) {
        self.decode_scratch = output;
    }

    pub(crate) fn compress(
        &mut self,
        out: &mut BytesMut,
        prefix_len: usize,
        algorithm: BatchCompression,
        level: u32,
    ) -> Result<(), JolyneError> {
        let input = &out[prefix_len..];
        let bound = match algorithm {
            BatchCompression::Deflate => {
                let level = level.clamp(1, 9);
                if self.compressor.as_ref().map(|(old, _)| *old) != Some(level) {
                    self.compressor = Some((
                        level,
                        Compressor::new(
                            CompressionLvl::new(level as i32).expect("clamped compression level"),
                        ),
                    ));
                }
                self.compressor
                    .as_mut()
                    .unwrap()
                    .1
                    .deflate_compress_bound(input.len())
            }
            BatchCompression::Snappy => {
                let bound = snap::raw::max_compress_len(input.len());
                if bound == 0 {
                    return Err(failed("Snappy input exceeds the format size limit").into());
                }
                bound
            }
            BatchCompression::None => return Ok(()),
        };
        self.scratch.resize(prefix_len + bound, 0);
        self.scratch[..prefix_len].copy_from_slice(&out[..prefix_len]);
        let destination = &mut self.scratch[prefix_len..];
        let len = match algorithm {
            BatchCompression::Deflate => self
                .compressor
                .as_mut()
                .unwrap()
                .1
                .deflate_compress(input, destination)
                .map_err(failed)?,
            BatchCompression::Snappy => self.snappy_encoder.compress(input, destination),
            BatchCompression::None => unreachable!(),
        };
        self.scratch.truncate(prefix_len + len);
        std::mem::swap(out, &mut self.scratch);
        Ok(())
    }

    pub(crate) fn decompress(
        &mut self,
        input: &[u8],
        algorithm: BatchCompression,
        max: usize,
    ) -> Result<BytesMut, ProtocolError> {
        if algorithm == BatchCompression::Snappy {
            let len = snap::raw::decompress_len(input).map_err(failed)?;
            if len > max {
                return Err(too_large(max));
            }
            let mut out = std::mem::take(&mut self.decode_scratch);
            out.resize(len, 0);
            let written = self
                .snappy_decoder
                .decompress(input, &mut out)
                .map_err(failed)?;
            out.truncate(written);
            return Ok(out);
        }

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
        let decompressor = self.decompressor.get_or_insert_with(Decompressor::new);
        loop {
            match decompressor.deflate_decompress(input, &mut out) {
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

    #[test]
    fn snappy_corpus_size_and_reference_roundtrip() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../valentine_gen/protocolgen/testdata/bench");
        for (name, limit) in [
            ("spawn", 1155),
            ("server_tick", 1599),
            ("chunk_radius", 79052),
            ("client_tick", 287),
        ] {
            let input = std::fs::read(root.join(format!("synthetic_batch_{name}.bin"))).unwrap();
            let mut output = BytesMut::from(&b"\xfe\x01"[..]);
            output.extend_from_slice(&input[1..]);
            CompressionContext::default()
                .compress(&mut output, 2, BatchCompression::Snappy, 6)
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

    #[test]
    fn decode_reuses_unshared_storage_and_remembers_output_size() {
        let input = vec![7; 128 * 1024];
        let mut codec = CompressionContext::default();
        let mut compressed = BytesMut::from(input.as_slice());
        codec
            .compress(&mut compressed, 0, BatchCompression::Deflate, 7)
            .unwrap();
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
        let mut codec = CompressionContext::default();
        let mut output = BytesMut::from(&b"\xfe\x00"[..]);
        output.extend_from_slice(&input);
        codec
            .compress(&mut output, 2, BatchCompression::Deflate, 7)
            .unwrap();
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
        let mut codec = CompressionContext::default();
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
        let mut codec = CompressionContext::default();
        for input in [
            &b""[..],
            &b"snappy repeated payload snappy repeated payload"[..],
        ] {
            let mut output = BytesMut::from(&b"\x01"[..]);
            output.extend_from_slice(input);
            codec
                .compress(&mut output, 1, BatchCompression::Snappy, 7)
                .unwrap();
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
}
