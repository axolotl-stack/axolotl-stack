//! Minimal ZIP reader for the Java server jar (build time only).
//!
//! Handles what Mojang's jars use: a single-disk archive without ZIP64, with
//! stored (0) or deflated (8) entries. The official server jar is a "bundler"
//! whose game code and data live in the nested jar
//! `META-INF/versions/<version>/server-<version>.jar`.

use flate2::read::DeflateDecoder;
use std::collections::BTreeMap;
use std::io::Read;

pub type Result<T, E = Box<dyn std::error::Error>> = std::result::Result<T, E>;

/// One archive member.
#[derive(Debug, Clone)]
struct Entry {
    method: u16,
    compressed_size: usize,
    uncompressed_size: usize,
    local_header: usize,
}

/// An in-memory ZIP archive.
pub struct Zip {
    data: Vec<u8>,
    entries: BTreeMap<String, Entry>,
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl Zip {
    /// Parse the central directory of `data`.
    pub fn new(data: Vec<u8>) -> Result<Self> {
        let min = data.len().saturating_sub(22 + 65_535);
        let eocd = (min..=data.len().saturating_sub(22))
            .rev()
            .find(|&o| u32_at(&data, o) == 0x0605_4b50)
            .ok_or("zip: end of central directory not found")?;
        let count = u16_at(&data, eocd + 10) as usize;
        let cd_offset = u32_at(&data, eocd + 16) as usize;
        if cd_offset == 0xFFFF_FFFF {
            return Err("zip: ZIP64 archives are not supported".into());
        }
        let mut entries = BTreeMap::new();
        let mut o = cd_offset;
        for _ in 0..count {
            if u32_at(&data, o) != 0x0201_4b50 {
                return Err("zip: bad central directory entry".into());
            }
            let method = u16_at(&data, o + 10);
            let compressed_size = u32_at(&data, o + 20) as usize;
            let uncompressed_size = u32_at(&data, o + 24) as usize;
            let name_len = u16_at(&data, o + 28) as usize;
            let extra_len = u16_at(&data, o + 30) as usize;
            let comment_len = u16_at(&data, o + 32) as usize;
            let local_header = u32_at(&data, o + 42) as usize;
            let name = String::from_utf8_lossy(&data[o + 46..o + 46 + name_len]).into_owned();
            entries.insert(
                name,
                Entry {
                    method,
                    compressed_size,
                    uncompressed_size,
                    local_header,
                },
            );
            o += 46 + name_len + extra_len + comment_len;
        }
        Ok(Self { data, entries })
    }

    /// Member names.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// Read and decompress a member.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let e = self
            .entries
            .get(name)
            .ok_or_else(|| format!("zip: no member {name}"))?;
        let o = e.local_header;
        if u32_at(&self.data, o) != 0x0403_4b50 {
            return Err(format!("zip: bad local header for {name}").into());
        }
        let start = o + 30 + u16_at(&self.data, o + 26) as usize + u16_at(&self.data, o + 28) as usize;
        let raw = &self.data[start..start + e.compressed_size];
        match e.method {
            0 => Ok(raw.to_vec()),
            8 => {
                let mut out = Vec::with_capacity(e.uncompressed_size);
                DeflateDecoder::new(raw).read_to_end(&mut out)?;
                Ok(out)
            }
            m => Err(format!("zip: {name} uses unsupported method {m}").into()),
        }
    }
}

/// Open the server jar and return the archive holding `data/` (the nested
/// versioned jar of a bundler, or the jar itself).
pub fn open_server_data(path: &std::path::Path) -> Result<Zip> {
    let outer = Zip::new(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?)?;
    let nested: Vec<String> = outer
        .names()
        .filter(|n| n.starts_with("META-INF/versions/") && n.ends_with(".jar"))
        .map(str::to_string)
        .collect();
    match nested.as_slice() {
        [] => Ok(outer),
        [one] => Zip::new(outer.read(one)?),
        many => Err(format!("{}: several nested server jars: {many:?}", path.display()).into()),
    }
}
