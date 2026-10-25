//! Authored LevelDB tables exercise actual compressed reads, not only WAL replay.
use bleveldb::{DB, Options, ReadOptions};
use flate2::{
    Compression,
    read::{DeflateDecoder, ZlibDecoder},
    write::{DeflateEncoder, ZlibEncoder},
};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
use unastar::{
    storage::{ChunkColumn, LevelDBWorldProvider, WorldProvider},
    world::{Chunk, ChunkPos},
};

const MAGIC: u64 = 0xdb4775248b80fb57;
const KEY: &[u8] = b"authored-codec-fixture";

fn varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 128 {
        out.push(value as u8 | 128);
        value >>= 7;
    }
    out.push(value as u8);
}
fn take_varint(input: &mut &[u8]) -> u64 {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = input[0];
        *input = &input[1..];
        value |= u64::from(byte & 127) << shift;
        if byte < 128 {
            return value;
        }
    }
    panic!("overlong fixture varint")
}
fn string(out: &mut Vec<u8>, value: &[u8]) {
    varint(out, value.len() as u64);
    out.extend_from_slice(value);
}
fn masked_crc(bytes: &[u8]) -> u32 {
    crc32c::crc32c(bytes)
        .rotate_right(15)
        .wrapping_add(0xa282ead8)
}
fn entry(key: &[u8], value: &[u8]) -> Vec<u8> {
    let mut out = vec![0];
    varint(&mut out, key.len() as u64);
    varint(&mut out, value.len() as u64);
    out.extend_from_slice(key);
    out.extend_from_slice(value);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&1u32.to_le_bytes());
    out
}
fn compressed(bytes: &[u8], codec: u8) -> Vec<u8> {
    match codec {
        0 => bytes.to_vec(),
        4 => {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(bytes).unwrap();
            encoder.finish().unwrap()
        }
        2 => {
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(bytes).unwrap();
            encoder.finish().unwrap()
        }
        // A valid Snappy stream containing a single long literal. No compressor
        // dependency or native writer is involved in constructing the fixture.
        1 => {
            let mut out = Vec::new();
            varint(&mut out, bytes.len() as u64);
            out.push(62 << 2);
            let len = (bytes.len() - 1) as u32;
            out.extend_from_slice(&len.to_le_bytes()[..3]);
            out.extend_from_slice(bytes);
            out
        }
        _ => unreachable!(),
    }
}
fn block(table: &mut Vec<u8>, bytes: &[u8], codec: u8) -> Vec<u8> {
    let offset = table.len();
    let data = compressed(bytes, codec);
    let mut handle = Vec::new();
    varint(&mut handle, offset as u64);
    varint(&mut handle, data.len() as u64);
    table.extend_from_slice(&data);
    table.push(codec);
    let crc = masked_crc(&table[offset..]);
    table.extend_from_slice(&crc.to_le_bytes());
    handle
}
fn authored_database(path: &Path, codec: u8, value: &[u8]) {
    let mut internal = KEY.to_vec();
    internal.extend_from_slice(&257u64.to_le_bytes()); // sequence 1, value
    let mut table = Vec::new();
    let data = block(&mut table, &entry(&internal, value), codec);
    let meta = block(&mut table, &[0, 0, 0, 0, 1, 0, 0, 0], 0);
    let index = block(&mut table, &entry(&internal, &data), 0);
    let mut footer = meta;
    footer.extend_from_slice(&index);
    footer.resize(40, 0);
    footer.extend_from_slice(&MAGIC.to_le_bytes());
    table.extend_from_slice(&footer);
    let mut manifest = Vec::new();
    varint(&mut manifest, 1);
    string(&mut manifest, b"leveldb.BytewiseComparator");
    for (tag, number) in [(2, 0), (3, 3), (4, 1), (7, 0)] {
        varint(&mut manifest, tag);
        varint(&mut manifest, number);
    }
    varint(&mut manifest, 2);
    varint(&mut manifest, table.len() as u64);
    string(&mut manifest, &internal);
    string(&mut manifest, &internal);
    let mut checksum_input = vec![1];
    checksum_input.extend_from_slice(&manifest);
    let mut log = masked_crc(&checksum_input).to_le_bytes().to_vec();
    log.extend_from_slice(&(manifest.len() as u16).to_le_bytes());
    log.push(1);
    log.extend_from_slice(&manifest);
    fs::write(path.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
    fs::write(path.join("MANIFEST-000001"), log).unwrap();
    fs::write(path.join("000002.ldb"), table).unwrap();
}
fn assert_authored_read(codec: u8) {
    let dir = tempfile::tempdir().unwrap();
    let expected = b"native-compressed-world-data".repeat(8192);
    authored_database(dir.path(), codec, &expected);
    let db = DB::open(dir.path(), &Options::new()).unwrap();
    let options = ReadOptions::new();
    options.verify_checksums(true);
    assert_eq!(
        db.get(KEY, &options).unwrap().as_deref(),
        Some(expected.as_slice())
    );
}
#[test]
fn reads_raw_zlib_sst_not_just_uncompressed_log() {
    assert_authored_read(4);
}
#[test]
fn reads_legacy_zlib_enum_sst() {
    assert_authored_read(2);
}
#[test]
fn reads_snappy_sst() {
    assert_authored_read(1);
}

fn decode_block(table: &[u8], offset: usize, size: usize) -> Vec<u8> {
    assert_eq!(
        masked_crc(&table[offset..offset + size + 1]),
        u32::from_le_bytes(
            table[offset + size + 1..offset + size + 5]
                .try_into()
                .unwrap()
        )
    );
    let mut out = Vec::new();
    match table[offset + size] {
        0 => out.extend_from_slice(&table[offset..offset + size]),
        4 => {
            DeflateDecoder::new(&table[offset..offset + size])
                .read_to_end(&mut out)
                .unwrap();
        }
        2 => {
            ZlibDecoder::new(&table[offset..offset + size])
                .read_to_end(&mut out)
                .unwrap();
        }
        codec => panic!("non-BDS writer codec {codec}"),
    }
    out
}
fn data_handles(table: &[u8]) -> Vec<(usize, usize)> {
    let footer = &table[table.len() - 48..];
    assert_eq!(u64::from_le_bytes(footer[40..].try_into().unwrap()), MAGIC);
    let mut input = footer;
    take_varint(&mut input);
    take_varint(&mut input);
    let offset = take_varint(&mut input) as usize;
    let size = take_varint(&mut input) as usize;
    let index = decode_block(table, offset, size);
    let restarts = u32::from_le_bytes(index[index.len() - 4..].try_into().unwrap()) as usize;
    let mut entries = &index[..index.len() - 4 * (restarts + 1)];
    let mut result = Vec::new();
    while !entries.is_empty() {
        take_varint(&mut entries);
        let suffix = take_varint(&mut entries) as usize;
        let len = take_varint(&mut entries) as usize;
        entries = &entries[suffix..];
        let mut handle = &entries[..len];
        let offset = take_varint(&mut handle) as usize;
        let size = take_varint(&mut handle) as usize;
        result.push((offset, size));
        entries = &entries[len..];
    }
    result
}
#[tokio::test]
async fn provider_writes_independently_decodable_raw_zlib_tables() {
    let directory = tempfile::tempdir().unwrap();
    {
        let provider = LevelDBWorldProvider::open(directory.path(), 0).unwrap();
        let mut chunk = Chunk::new(0, 0);
        let stone = unastar::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:stone")
            .unwrap();
        for section in 0..24 {
            chunk.fill_subchunk_solid(section, stone);
        }
        provider
            .save_column(ChunkPos::new(0, 0), 0, &ChunkColumn::new(chunk))
            .await
            .unwrap();
        provider.close().await.unwrap();
    }
    let mut raw_blocks = 0;
    for file in fs::read_dir(directory.path()).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("ldb") {
            continue;
        }
        let table = fs::read(path).unwrap();
        for (offset, size) in data_handles(&table) {
            assert!(
                matches!(table[offset + size], 0 | 4),
                "BDS writes raw zlib or uncompressed blocks"
            );
            if table[offset + size] == 4 {
                raw_blocks += 1;
            }
            let _decoded = decode_block(&table, offset, size);
        }
    }
    assert!(
        raw_blocks > 0,
        "compressible chunk data must actually use raw-zlib id 4"
    );
    let provider = LevelDBWorldProvider::open(directory.path(), 0).unwrap();
    let restored = provider
        .load_column(ChunkPos::new(0, 0), 0)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        restored.chunk.get_block(8, 64, 8),
        unastar::registry::BlockRegistry::vanilla()
            .default_state_id_by_name("minecraft:stone")
            .unwrap()
    );
    provider.close().await.unwrap();
}
