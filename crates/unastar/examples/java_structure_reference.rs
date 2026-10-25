//! Extract structure starts from a Java Edition reference world into a compact
//! fixture for the Java-style generator's structure parity test.
//!
//! Uses the decorated seed-12345 world described in `java_worldgen_reference.rs`
//! (Java 1.21.11 server, loopback only, generated outside the repo). Every saved
//! chunk at status `minecraft:structure_starts` or later stores the starts that
//! originate in it under `structures.starts`; the fixture lists those chunks and
//! their valid starts with the bounding box of all pieces.
//!
//! `cargo run -p unastar --example java_structure_reference -- <world dir> tests/fixtures/java_1_21_11_seed_12345_structures.json`

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use flate2::read::{GzDecoder, ZlibDecoder};

/// Chunk statuses before `structure_starts` (their starts are not computed yet).
const BEFORE_STARTS: [&str; 1] = ["minecraft:empty"];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: java_structure_reference <world dir> <out.json>");
        std::process::exit(2);
    }
    let chunks = load_region_chunks(&Path::new(&args[1]).join("region"));
    let mut scanned = Vec::new();
    let mut starts = Vec::new();
    for (&(cx, cz), nbt) in &chunks {
        let status = nbt.string("Status").unwrap_or_default();
        if BEFORE_STARTS.contains(&status.as_str()) {
            continue;
        }
        scanned.push((cx, cz));
        let Some(Nbt::Compound(entries)) = nbt.get("structures").and_then(|s| s.get("starts"))
        else {
            continue;
        };
        for (id, start) in entries {
            if start.string("id").as_deref() == Some("INVALID") {
                continue;
            }
            let children = start.list("Children");
            let mut bb = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
            for c in children {
                if let Some(Nbt::IntArray(b)) = c.get("BB") {
                    for i in 0..3 {
                        bb[i] = bb[i].min(b[i]);
                        bb[i + 3] = bb[i + 3].max(b[i + 3]);
                    }
                }
            }
            if children.is_empty() {
                continue;
            }
            starts.push((id.clone(), cx, cz, bb, children.len()));
        }
    }
    let mut out = String::from(
        "{\n\"edition\": \"java\",\n\"version\": \"1.21.11\",\n\"seed\": 12345,\n\"chunks\": [",
    );
    out.push_str(
        &scanned
            .iter()
            .map(|(x, z)| format!("[{x},{z}]"))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push_str("],\n\"starts\": [\n");
    out.push_str(
        &starts
            .iter()
            .map(|(id, x, z, bb, n)| {
                format!(
                    "{{\"structure\": \"{id}\", \"chunk\": [{x},{z}], \"bbox\": [{}], \"pieces\": {n}}}",
                    bb.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",\n"),
    );
    out.push_str("\n]\n}\n");
    std::fs::write(&args[2], out).expect("write fixture");
    let by: BTreeMap<&str, usize> = starts.iter().fold(BTreeMap::new(), |mut m, s| {
        *m.entry(s.0.as_str()).or_default() += 1;
        m
    });
    eprintln!(
        "{} chunks scanned, {} starts: {by:?}",
        scanned.len(),
        starts.len()
    );
}

fn gunzip(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    GzDecoder::new(data)
        .read_to_end(&mut out)
        .expect("gzip decode");
    out
}

/// Every chunk stored in the `r.X.Z.mca` files of `dir`, keyed by chunk coords.
fn load_region_chunks(dir: &Path) -> BTreeMap<(i32, i32), Nbt> {
    let mut chunks = BTreeMap::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .expect("read region dir")
        .map(|e| e.expect("region dir entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "mca"))
        .collect();
    files.sort();
    for path in files {
        let buf = std::fs::read(&path).expect("read region file");
        if buf.len() < 8192 {
            continue;
        }
        for slot in 0..1024 {
            let loc = u32::from_be_bytes(buf[slot * 4..slot * 4 + 4].try_into().unwrap());
            let sector = (loc >> 8) as usize;
            if sector == 0 {
                continue;
            }
            let off = sector * 4096;
            let len = u32::from_be_bytes(buf[off..off + 4].try_into().unwrap()) as usize;
            let compression = buf[off + 4];
            let payload = &buf[off + 5..off + 4 + len];
            let raw = match compression {
                1 => gunzip(payload),
                2 => {
                    let mut out = Vec::new();
                    ZlibDecoder::new(payload)
                        .read_to_end(&mut out)
                        .expect("zlib decode");
                    out
                }
                3 => payload.to_vec(),
                c => panic!(
                    "{}: chunk slot {slot} uses unsupported compression {c} (only gzip, zlib, none)",
                    path.display()
                ),
            };
            let nbt = Nbt::read_root(&raw);
            let pos = (
                nbt.int("xPos").expect("chunk without xPos"),
                nbt.int("zPos").expect("chunk without zPos"),
            );
            chunks.insert(pos, nbt);
        }
    }
    chunks
}

/// Minimal big-endian NBT tree: enough for level.dat and chunk data.
#[allow(dead_code)]
enum Nbt {
    Byte(i8),
    Int(i32),
    Long(i64),
    Str(String),
    List(Vec<Nbt>),
    Compound(Vec<(String, Nbt)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
    Other,
}

#[allow(dead_code)]
impl Nbt {
    fn read_root(buf: &[u8]) -> Nbt {
        let mut r = Reader { buf, off: 1 };
        let _ = r.string();
        r.payload(buf[0])
    }

    fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn list(&self, key: &str) -> &[Nbt] {
        match self.get(key) {
            Some(Nbt::List(v)) => v,
            _ => &[],
        }
    }

    fn string(&self, key: &str) -> Option<String> {
        match self.get(key)? {
            Nbt::Str(s) => Some(s.clone()),
            _ => None,
        }
    }

    fn byte(&self, key: &str) -> Option<i8> {
        match self.get(key)? {
            Nbt::Byte(v) => Some(*v),
            _ => None,
        }
    }

    fn int(&self, key: &str) -> Option<i32> {
        match self.get(key)? {
            Nbt::Int(v) => Some(*v),
            _ => None,
        }
    }

    fn long(&self, key: &str) -> Option<i64> {
        match self.get(key)? {
            Nbt::Long(v) => Some(*v),
            _ => None,
        }
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    off: usize,
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> [u8; N] {
        let v = self.buf[self.off..self.off + N].try_into().unwrap();
        self.off += N;
        v
    }

    fn count(&mut self) -> usize {
        i32::from_be_bytes(self.take()).max(0) as usize
    }

    fn string(&mut self) -> String {
        let n = u16::from_be_bytes(self.take()) as usize;
        let s = String::from_utf8_lossy(&self.buf[self.off..self.off + n]).into_owned();
        self.off += n;
        s
    }

    fn payload(&mut self, tag: u8) -> Nbt {
        match tag {
            1 => Nbt::Byte(i8::from_be_bytes(self.take())),
            2 => {
                self.off += 2;
                Nbt::Other
            }
            3 => Nbt::Int(i32::from_be_bytes(self.take())),
            4 => Nbt::Long(i64::from_be_bytes(self.take())),
            5 => {
                self.off += 4;
                Nbt::Other
            }
            6 => {
                self.off += 8;
                Nbt::Other
            }
            7 => {
                let n = self.count();
                self.off += n;
                Nbt::Other
            }
            8 => Nbt::Str(self.string()),
            9 => {
                let [et] = self.take();
                let n = self.count();
                Nbt::List((0..n).map(|_| self.payload(et)).collect())
            }
            10 => {
                let mut entries = Vec::new();
                loop {
                    let [t] = self.take();
                    if t == 0 {
                        return Nbt::Compound(entries);
                    }
                    let name = self.string();
                    let v = self.payload(t);
                    entries.push((name, v));
                }
            }
            11 => {
                let n = self.count();
                Nbt::IntArray((0..n).map(|_| i32::from_be_bytes(self.take())).collect())
            }
            12 => {
                let n = self.count();
                Nbt::LongArray((0..n).map(|_| i64::from_be_bytes(self.take())).collect())
            }
            t => panic!("unknown NBT tag {t}"),
        }
    }
}
