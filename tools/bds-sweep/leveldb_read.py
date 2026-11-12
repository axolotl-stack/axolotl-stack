"""Read-only LevelDB reader for BDS worlds, independent of unastar.

Reads every table (`.ldb`/`.sst`; block compression 0, raw deflate 4 or zlib
2, as BDS writes them) and every `.log` file of a `db` directory and returns
the newest value of each key, deletions applied. Never writes: BDS worlds are
read from a copy anyway (opening one with LevelDB compacts its log).
"""

from __future__ import annotations

import struct
import zlib
from pathlib import Path

LOG_BLOCK = 32768
TABLE_MAGIC = 0xDB4775248B80FB57
FULL, FIRST, MIDDLE, LAST = 1, 2, 3, 4


def _varint(buf: bytes, off: int) -> tuple[int, int]:
    value = shift = 0
    while True:
        byte = buf[off]
        off += 1
        value |= (byte & 0x7F) << shift
        if not byte & 0x80:
            return value, off
        shift += 7


def _log_entries(path: Path):
    """(key, sequence, value or None) of every write batch in a log file."""
    data = path.read_bytes()
    records, pending, off = [], b'', 0
    while off < len(data):
        block_end = min((off // LOG_BLOCK + 1) * LOG_BLOCK, len(data))
        while off + 7 <= block_end:
            _crc, length, kind = struct.unpack_from('<IHB', data, off)
            if kind == 0 and length == 0:
                break  # zero padding up to the block end
            payload = data[off + 7:off + 7 + length]
            off += 7 + length
            if kind == FULL:
                records.append(payload)
            elif kind == FIRST:
                pending = payload
            elif kind == MIDDLE:
                pending += payload
            elif kind == LAST:
                records.append(pending + payload)
        off = block_end
    for record in records:
        sequence, count = struct.unpack_from('<QI', record, 0)
        p = 12
        for i in range(count):
            kind = record[p]
            klen, p = _varint(record, p + 1)
            key = record[p:p + klen]
            p += klen
            value = None
            if kind == 1:
                vlen, p = _varint(record, p)
                value = record[p:p + vlen]
                p += vlen
            yield key, sequence + i, value


def _block(data: bytes, offset: int, size: int) -> bytes:
    raw = data[offset:offset + size]
    codec = data[offset + size]
    if codec == 0:
        return raw
    if codec == 4:
        return zlib.decompress(raw, -15)
    if codec == 2:
        return zlib.decompress(raw)
    raise ValueError(f'unsupported table block compression {codec}')


def _block_entries(block: bytes):
    restarts = struct.unpack_from('<I', block, len(block) - 4)[0]
    end = len(block) - 4 - 4 * restarts
    off, key = 0, b''
    while off < end:
        shared, off = _varint(block, off)
        unshared, off = _varint(block, off)
        vlen, off = _varint(block, off)
        key = key[:shared] + block[off:off + unshared]
        off += unshared
        yield key, block[off:off + vlen]
        off += vlen


def _table_entries(path: Path):
    data = path.read_bytes()
    footer = data[-48:]
    if struct.unpack_from('<Q', footer, 40)[0] != TABLE_MAGIC:
        raise ValueError(f'{path}: not a LevelDB table')
    p = 0
    for _ in range(2):  # skip the metaindex handle
        _, p = _varint(footer, p)
    index_off, p = _varint(footer, p)
    index_size, p = _varint(footer, p)
    for _key, handle in _block_entries(_block(data, index_off, index_size)):
        block_off, q = _varint(handle, 0)
        block_size, _ = _varint(handle, q)
        for internal, value in _block_entries(_block(data, block_off, block_size)):
            tag = struct.unpack_from('<Q', internal, len(internal) - 8)[0]
            yield internal[:-8], tag >> 8, value if tag & 0xFF == 1 else None


def read_db(db_dir: str | Path) -> dict[bytes, bytes]:
    """Newest live value of every key in a LevelDB directory."""
    newest: dict[bytes, tuple[int, bytes | None]] = {}
    for path in sorted(Path(db_dir).iterdir()):
        if path.suffix in ('.ldb', '.sst'):
            entries = _table_entries(path)
        elif path.suffix == '.log':
            entries = _log_entries(path)
        else:
            continue
        for key, sequence, value in entries:
            if key not in newest or newest[key][0] < sequence:
                newest[key] = (sequence, value)
    return {key: value for key, (_, value) in newest.items() if value is not None}
