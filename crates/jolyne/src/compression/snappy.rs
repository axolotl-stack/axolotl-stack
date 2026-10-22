// Copyright (c) 2012 The Go Authors. All rights reserved.
// Copyright (c) 2019 Klaus Post. All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
// * Redistributions of source code must retain the above copyright notice,
//   this list of conditions and the following disclaimer.
// * Redistributions in binary form must reproduce the above copyright notice,
//   this list of conditions and the following disclaimer in the documentation
//   and/or other materials provided with the distribution.
// * Neither the name of Google Inc. nor the names of its contributors may be
//   used to endorse or promote products derived from this software without
//   specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE
// ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT OWNER OR CONTRIBUTORS BE
// LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR
// CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF
// SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS
// INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN
// CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE)
// ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE
// POSSIBILITY OF SUCH DAMAGE.

//! Whole-block Snappy matcher. Search parameters follow the BSD-licensed
//! klauspost/compress S2 Snappy encoder; no S2 repeat tags are emitted.
//!
//! Hash widths grow with the input, not with connection lifetime. Three
//! adjacent probes share one input load; failed searches skip progressively
//! farther through literals. Backward and forward extension retain short
//! matches, including the last seven bytes. Copies use the full 64-byte tag
//! capacity when possible, instead of splitting every 60 bytes.
//!
//! Tables are cleared per block and retained per connection. Output never
//! exceeds snap's maximum encoded size: an expanding parse is replaced by one
//! literal, the standard Snappy representation for incompressible data.

#[derive(Default)]
pub(super) struct Encoder {
    table: Option<Box<[u32; 16384]>>,
}

/// # Safety
/// `at + 8 <= src.len()` must hold without overflowing `usize`.
#[inline]
unsafe fn load(src: &[u8], at: usize) -> u64 {
    debug_assert!(at.checked_add(8).is_some_and(|end| end <= src.len()));
    // SAFETY: the caller guarantees eight initialized bytes at this offset;
    // read_unaligned does not require the slice pointer to be u64-aligned.
    unsafe { src.as_ptr().add(at).cast::<u64>().read_unaligned().to_le() }
}

/// # Safety
/// `at + 4 <= src.len()` must hold without overflowing `usize`.
#[inline]
unsafe fn word(src: &[u8], at: usize) -> u32 {
    debug_assert!(at.checked_add(4).is_some_and(|end| end <= src.len()));
    // SAFETY: the caller guarantees four initialized bytes at this offset;
    // read_unaligned does not require the slice pointer to be u32-aligned.
    unsafe { src.as_ptr().add(at).cast::<u32>().read_unaligned().to_le() }
}

#[inline]
fn hash<const B: u32, const N: u32>(v: u64) -> usize {
    let prime = match N {
        4 => 2654435761,
        5 => 889523592379,
        _ => 227718039650203,
    };
    ((v << (64 - N * 8)).wrapping_mul(prime) >> (64 - B)) as usize
}

#[inline]
fn literal(out: &mut [u8], d: &mut usize, src: &[u8]) {
    if src.is_empty() {
        return;
    }
    let n = src.len() - 1;
    if n < 60 {
        out[*d] = (n << 2) as u8;
        *d += 1;
    } else {
        let count = (usize::BITS - n.leading_zeros()).div_ceil(8) as usize;
        out[*d] = ((59 + count) << 2) as u8;
        *d += 1;
        out[*d..*d + count].copy_from_slice(&n.to_le_bytes()[..count]);
        *d += count;
    }
    out[*d..*d + src.len()].copy_from_slice(src);
    *d += src.len();
}

#[inline]
fn copy(out: &mut [u8], d: &mut usize, offset: usize, mut len: usize) {
    while len > 0 {
        let n = if len >= 68 {
            64
        } else if len > 64 {
            60
        } else {
            len
        };
        if (4..12).contains(&n) && offset < 2048 {
            out[*d] = (1 | ((n - 4) << 2) | ((offset >> 8) << 5)) as u8;
            out[*d + 1] = offset as u8;
            *d += 2;
        } else if offset < 65536 {
            out[*d] = (2 | ((n - 1) << 2)) as u8;
            out[*d + 1..*d + 3].copy_from_slice(&(offset as u16).to_le_bytes());
            *d += 3;
        } else {
            out[*d] = (3 | ((n - 1) << 2)) as u8;
            out[*d + 1..*d + 5].copy_from_slice(&(offset as u32).to_le_bytes());
            *d += 5;
        }
        len -= n;
    }
}

impl Encoder {
    pub(super) fn compress(&mut self, src: &[u8], out: &mut [u8]) -> usize {
        let mut len = src.len();
        let mut d = 0;
        while len >= 128 {
            out[d] = len as u8 | 128;
            len >>= 7;
            d += 1;
        }
        out[d] = len as u8;
        d += 1;
        if src.len() < 32 {
            literal(out, &mut d, src);
            return d;
        }
        // block parameters: table bits, hashed bytes, literal skip shift.
        // Short blocks need four-byte hashes to retain short matches; larger
        // tables use wider hashes to reduce collisions. The large-block skip
        // shift is 5 rather than S2's 6: measured faster at the +0.5% size gate.
        let encoded = match src.len() {
            0..512 => self.block::<8, 4, 4>(src, out, d),
            512..4096 => self.block::<10, 4, 5>(src, out, d),
            4096..16384 => self.block::<12, 5, 5>(src, out, d),
            _ => self.block::<14, 6, 5>(src, out, d),
        };
        if encoded != 0 {
            return encoded;
        }
        literal(out, &mut d, src);
        d
    }

    // A zero result asks compress() to emit the original input as a literal.
    // Candidates are local offsets: clearing the table prevents cross-batch
    // references, while u32 entries permit matches beyond Snappy's old 64 KiB
    // encoder block boundary.
    fn block<const B: u32, const N: u32, const SKIP: usize>(
        &mut self,
        src: &[u8],
        out: &mut [u8],
        mut d: usize,
    ) -> usize {
        // DEFLATE-only connections and tiny literal-only blocks need no table.
        let table = self.table.get_or_insert_with(|| Box::new([0; 16384]));
        let table = &mut table[..1 << B];
        table.fill(0);
        // A barely compressible parse is not worth its tag/decoder overhead.
        // Keep S2's minimum saving; the caller emits one literal on failure.
        let out_limit = d + src.len() - (src.len() >> 5) - 5;
        let limit = src.len() - 8;
        let mut s = 1;
        let mut next_emit = 0;
        let mut repeat = 1;
        'search: while s + 8 <= src.len() {
            let mut candidate;
            loop {
                let next_s = s + ((s - next_emit) >> SKIP) + 4;
                if next_s >= limit {
                    break 'search;
                }
                // SAFETY: s < next_s < limit = src.len() - 8.
                let cv = unsafe { load(src, s) };
                let h0 = hash::<B, N>(cv);
                let h1 = hash::<B, N>(cv >> 8);
                let h2 = hash::<B, N>(cv >> 16);
                candidate = table[h0] as usize;
                let candidate2 = table[h1] as usize;
                table[h0] = s as u32;
                table[h1] = (s + 1) as u32;
                // SAFETY: 1 <= repeat <= s, hence s - repeat + 1 <= s;
                // s < limit leaves at least eight bytes at s.
                if (cv >> 8) as u32 == unsafe { word(src, s - repeat + 1) } {
                    let mut base = s + 1;
                    while base > next_emit
                        && base > repeat
                        && src[base - repeat - 1] == src[base - 1]
                    {
                        base -= 1;
                    }
                    if d + (base - next_emit) + 5 > out_limit {
                        return 0;
                    }
                    literal(out, &mut d, &src[next_emit..base]);
                    s += 5;
                    while s <= limit {
                        // SAFETY: s <= limit, and 1 <= repeat <= base <= s;
                        // both s and s - repeat therefore have eight bytes.
                        let diff = unsafe { load(src, s) ^ load(src, s - repeat) };
                        if diff != 0 {
                            s += (diff.trailing_zeros() / 8) as usize;
                            break;
                        }
                        s += 8;
                    }
                    while s < src.len() && src[s] == src[s - repeat] {
                        s += 1;
                    }
                    if d + (s - base).div_ceil(60) * 5 > out_limit {
                        return 0;
                    }
                    copy(out, &mut d, repeat, s - base);
                    next_emit = s;
                    if s >= limit {
                        break 'search;
                    }
                    continue;
                }
                // SAFETY: table offsets are zero or were inserted with at
                // least six input bytes remaining; src.len() >= 32.
                if cv as u32 == unsafe { word(src, candidate) } {
                    break;
                }
                candidate = table[h2] as usize;
                table[h2] = (s + 2) as u32;
                // SAFETY: candidate2 has the same initialized-table bound:
                // at + 4 <= src.len(), including the initial offset zero.
                if (cv >> 8) as u32 == unsafe { word(src, candidate2) } {
                    candidate = candidate2;
                    s += 1;
                    break;
                }
                // SAFETY: every table insertion leaves at least six bytes;
                // the four-byte read also fits if candidate was just updated.
                if (cv >> 16) as u32 == unsafe { word(src, candidate) } {
                    s += 2;
                    break;
                }
                s = next_s;
            }
            while candidate > 0 && s > next_emit && src[candidate - 1] == src[s - 1] {
                candidate -= 1;
                s -= 1;
            }
            if d + (s - next_emit) + 5 > out_limit {
                return 0;
            }
            literal(out, &mut d, &src[next_emit..s]);
            loop {
                let base = s;
                repeat = s - candidate;
                s += 4;
                while s <= limit {
                    // SAFETY: s <= limit, and 1 <= repeat <= base <= s;
                    // both s and s - repeat therefore have eight bytes.
                    let diff = unsafe { load(src, s) ^ load(src, s - repeat) };
                    if diff != 0 {
                        s += (diff.trailing_zeros() / 8) as usize;
                        break;
                    }
                    s += 8;
                }
                while s < src.len() && src[s] == src[s - repeat] {
                    s += 1;
                }
                if d + (s - base).div_ceil(60) * 5 > out_limit {
                    return 0;
                }
                copy(out, &mut d, repeat, s - base);
                next_emit = s;
                if s >= limit {
                    break 'search;
                }
                // SAFETY: a match consumes >=4 bytes so s >= 4, and the
                // preceding guard establishes s < src.len() - 8.
                let x = unsafe { load(src, s - 2) };
                let h = hash::<B, N>(x >> 16);
                candidate = table[h] as usize;
                table[hash::<B, N>(x)] = (s - 2) as u32;
                table[h] = s as u32;
                // SAFETY: candidate is zero or an offset inserted with at
                // least six bytes remaining, so candidate + 4 <= src.len().
                if (x >> 16) as u32 != unsafe { word(src, candidate) } {
                    s += 1;
                    break;
                }
            }
        }
        if d + (src.len() - next_emit) + 5 > out_limit {
            return 0;
        }
        literal(out, &mut d, &src[next_emit..]);
        d
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn random_bytes(len: usize) -> Vec<u8> {
        let mut seed = 0x1234_5678_u32;
        (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect()
    }

    // (tag kind, decoded length, offset); literal offsets are zero.
    fn tags(encoded: &[u8]) -> Vec<(u8, usize, usize)> {
        let mut at = 0;
        while encoded[at] & 128 != 0 {
            at += 1;
        }
        at += 1;
        let mut result = Vec::new();
        while at < encoded.len() {
            let tag = encoded[at];
            at += 1;
            let (len, offset) = match tag & 3 {
                0 => {
                    let mut len = (tag >> 2) as usize;
                    if len >= 60 {
                        let count = len - 59;
                        len = 0;
                        for shift in 0..count {
                            len |= (encoded[at] as usize) << (shift * 8);
                            at += 1;
                        }
                    }
                    at += len + 1;
                    (len + 1, 0)
                }
                1 => {
                    let offset = ((tag as usize & 0xe0) << 3) | encoded[at] as usize;
                    at += 1;
                    ((((tag >> 2) & 7) + 4) as usize, offset)
                }
                2 => {
                    let offset =
                        u16::from_le_bytes(encoded[at..at + 2].try_into().unwrap()) as usize;
                    at += 2;
                    ((tag >> 2) as usize + 1, offset)
                }
                _ => {
                    let offset =
                        u32::from_le_bytes(encoded[at..at + 4].try_into().unwrap()) as usize;
                    at += 4;
                    ((tag >> 2) as usize + 1, offset)
                }
            };
            result.push((tag & 3, len, offset));
        }
        assert_eq!(at, encoded.len());
        result
    }

    #[test]
    fn barely_compressible_input_falls_back_to_one_literal() {
        let mut input = random_bytes(2000);
        input.copy_within(0..32, 128);
        let mut encoder = Encoder::default();
        let mut out = vec![0; snap::raw::max_compress_len(input.len())];
        let n = encoder.compress(&input, &mut out);
        assert_eq!(tags(&out[..n]), [(0, input.len(), 0)]);
        assert_eq!(
            snap::raw::Decoder::new().decompress_vec(&out[..n]).unwrap(),
            input
        );
    }

    fn roundtrip(encoder: &mut Encoder, input: &[u8]) {
        let bound = snap::raw::max_compress_len(input.len());
        let mut output = vec![0; bound];
        let n = encoder.compress(input, &mut output);
        assert_eq!(
            snap::raw::Decoder::new()
                .decompress_vec(&output[..n])
                .unwrap(),
            input
        );
    }

    #[test]
    fn tails_table_transitions_and_reused_state_roundtrip() {
        let mut encoder = Encoder::default();
        let mut state = 0x1234_5678_u32;
        for len in (0..256).chain([
            511, 512, 513, 4095, 4096, 4097, 16383, 16384, 16385, 65535, 65536, 65537, 131072,
        ]) {
            let input: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
            roundtrip(&mut encoder, &input);
            roundtrip(&mut encoder, &vec![0; len]);
            let patterned: Vec<_> = (0..len).map(|i| (i % 251) as u8).collect();
            roundtrip(&mut encoder, &patterned);
        }
    }

    #[test]
    fn long_offsets_and_short_matches_roundtrip() {
        let mut encoder = Encoder::default();
        let source = random_bytes(4096);
        let mut input = source.clone();
        input.resize(input.len() + 70000, 0);
        input.extend_from_slice(&source);
        let mut output = vec![0; snap::raw::max_compress_len(input.len())];
        let n = encoder.compress(&input, &mut output);
        assert!(
            tags(&output[..n])
                .iter()
                .any(|&(kind, _, offset)| kind == 3 && offset > 65535),
            "the fixture must exercise a real four-byte copy offset"
        );
        assert_eq!(
            snap::raw::Decoder::new()
                .decompress_vec(&output[..n])
                .unwrap(),
            input
        );
        for len in 4..=132 {
            let repeated = input[..len].to_vec();
            input.extend_from_slice(&[0, 1, 2]);
            input.extend_from_slice(&repeated);
        }
        roundtrip(&mut encoder, &input);
    }

    #[test]
    fn literal_larger_than_64_kib_roundtrips() {
        let input = random_bytes(65537);
        let mut output = vec![0; snap::raw::max_compress_len(input.len())];
        let n = Encoder::default().compress(&input, &mut output);
        assert_eq!(tags(&output[..n]), [(0, input.len(), 0)]);
        assert_eq!(
            snap::raw::Decoder::new()
                .decompress_vec(&output[..n])
                .unwrap(),
            input
        );
    }

    #[test]
    fn generated_literals_overlapping_copies_and_near_misses_roundtrip() {
        use rand::{RngCore, SeedableRng};
        let mut rng = rand::rngs::StdRng::seed_from_u64(0x5a17_2193);
        let mut encoder = Encoder::default();
        let mut decoder = snap::raw::Decoder::new();
        for case in 0..256 {
            let mut input = vec![0; 70032];
            rng.fill_bytes(&mut input);
            for step in 0..128 {
                let offset = match step % 16 {
                    0 => 1,
                    1 => 2,
                    2 => 65535,
                    3 => 65536,
                    4 => 70000,
                    _ => 1 + rng.next_u32() as usize % input.len(),
                };
                let len = 4 + rng.next_u32() as usize % 197;
                let start = input.len();
                for _ in 0..len {
                    input.push(input[input.len() - offset]);
                }
                if step % 5 == 0 {
                    input[start + len / 2] ^= 0x80;
                }
                if step % 3 == 0 {
                    for _ in 0..rng.next_u32() % 64 {
                        input.push(rng.next_u32() as u8);
                    }
                }
            }
            input.truncate(input.len() - case % 8);
            let bound = snap::raw::max_compress_len(input.len());
            let mut output = vec![0; bound];
            let n = encoder.compress(&input, &mut output);
            assert!(n <= bound && n <= input.len() + 10, "case {case} expanded");
            assert_eq!(
                decoder.decompress_vec(&output[..n]).unwrap(),
                input,
                "case {case}"
            );
        }
    }
}
