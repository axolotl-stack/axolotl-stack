//! Dump every saved actor record (`actorprefix` keys) of a BDS world as JSON
//! lines `{"key_id": <unique id>, "hex": <little-endian NBT>}`, for the
//! sweep distillers (`tools/bds-sweep/spawn_bonus_fixture.py`):
//!
//! ```sh
//! cargo run -p unastar --example bds_actor_records -- <world>/db <out.jsonl>
//! ```
//!
//! Reads only; the NBT is left undecoded (BDS writes a binary `StorageKey`
//! string that is not UTF-8).

use bleveldb::options::Compression;
use bleveldb::{DB, Options, ReadOptions};

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(db_path), Some(out)) = (args.next(), args.next()) else {
        eprintln!("usage: bds_actor_records <world>/db <out.jsonl>");
        std::process::exit(2);
    };
    let options = Options::new();
    // BDS stores compressible blocks as raw deflate.
    options.compression(Compression::ZlibRaw);
    let db = DB::open(&db_path, &options).expect("open the world database");
    let read = ReadOptions::new();
    let mut iter = db.iter(&read);
    iter.seek(b"actorprefix");
    let mut lines = String::new();
    let mut count = 0usize;
    while iter.valid() {
        let Some(key) = iter.key() else { break };
        if !key.starts_with(b"actorprefix") {
            break;
        }
        let value = iter.value().unwrap_or_default();
        let id = i64::from_le_bytes(
            key[11..19]
                .try_into()
                .expect("actor keys end in an 8-byte id"),
        );
        let hex: String = value.iter().map(|byte| format!("{byte:02x}")).collect();
        lines.push_str(&format!("{{\"key_id\":{id},\"hex\":\"{hex}\"}}\n"));
        count += 1;
        iter.next_native();
    }
    std::fs::write(&out, lines).expect("write the dump");
    println!("{count} actor records -> {out}");
}
