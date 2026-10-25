"""Readers for the CC0 protocolgen native 1.26.51 data the sweep jobs are
built from (block state palette and mining-tool classifications), in
`crates/valentine_gen/protocolgen/generated/data` (git submodule)."""

from __future__ import annotations

import re
import struct
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
DATA = REPO / 'crates' / 'valentine_gen' / 'protocolgen' / 'generated' / 'data'
TOOL_FAMILIES = ('pickaxe', 'axe', 'shovel', 'hoe', 'sword', 'shears')


class _Reader:
    """Network little-endian NBT (varint lengths, zigzag varint ints)."""

    def __init__(self, data: bytes) -> None:
        self.data = data
        self.pos = 0

    def byte(self) -> int:
        value = self.data[self.pos]
        self.pos += 1
        return value

    def uvarint(self) -> int:
        value = shift = 0
        while True:
            byte = self.byte()
            value |= (byte & 0x7F) << shift
            if not byte & 0x80:
                return value
            shift += 7

    def varint(self) -> int:
        value = self.uvarint()
        return (value >> 1) ^ -(value & 1)

    def string(self) -> str:
        length = self.uvarint()
        value = self.data[self.pos:self.pos + length].decode('utf-8')
        self.pos += length
        return value

    def payload(self, tag: int):
        if tag == 1:
            return struct.unpack('<b', bytes([self.byte()]))[0]
        if tag == 2:
            value = struct.unpack_from('<h', self.data, self.pos)[0]
            self.pos += 2
            return value
        if tag in (3, 4):
            return self.varint()
        if tag == 5:
            value = struct.unpack_from('<f', self.data, self.pos)[0]
            self.pos += 4
            return value
        if tag == 6:
            value = struct.unpack_from('<d', self.data, self.pos)[0]
            self.pos += 8
            return value
        if tag == 8:
            return self.string()
        if tag == 9:
            element = self.byte()
            return [self.payload(element) for _ in range(self.varint())]
        if tag == 10:
            compound = {}
            while (child := self.byte()) != 0:
                # The name precedes the payload (Python evaluates the value
                # of `d[k] = v` before the key).
                key = self.string()
                compound[key] = self.payload(child)
            return compound
        if tag == 7:
            n = self.varint()
            value = list(self.data[self.pos:self.pos + n])
            self.pos += n
            return value
        if tag in (11, 12):
            return [self.varint() for _ in range(self.varint())]
        raise ValueError(f'unknown NBT tag {tag}')


def block_states() -> list[dict]:
    """Every canonical block state, in palette order: `{name, states}`."""
    reader = _Reader((DATA / 'registry' / 'block_states.nbt').read_bytes())
    out = []
    while reader.pos < len(reader.data):
        if reader.byte() != 10:
            raise ValueError('expected a root compound')
        reader.string()
        entry = reader.payload(10)
        out.append({'name': entry['name'], 'states': entry['states']})
    return out


def default_states() -> dict[str, dict]:
    """First palette state per block, used as the block's default probe."""
    first: dict[str, dict] = {}
    for entry in block_states():
        first.setdefault(entry['name'], entry['states'])
    return first


_ROW = re.compile(r'\{Name: "([^"]+)", Facts: MiningFacts\{EffectiveTools: ([^,]+), '
                  r'HarvestTools: ([^,]+), HarvestLevel: (-?\d+)\}\}')


def _families(expr: str) -> list[str]:
    return [name.removeprefix('Tool').lower() for name in re.findall(r'Tool\w+', expr)]


def mining() -> dict[str, dict]:
    """Native mining classification per block: effective and harvest tool
    families and harvest level (-1: no correct tool needed)."""
    text = (DATA / 'block' / 'mining_generated.go').read_text(encoding='utf-8')
    return {
        name: {'effective': _families(effective), 'harvest': _families(harvest), 'level': int(level)}
        for name, effective, harvest, level in _ROW.findall(text)
    }


def items() -> list[str]:
    """Every item identifier of the protocolgen item registry, sorted."""
    reader = _Reader((DATA / 'registry' / 'items.nbt').read_bytes())
    tag = reader.byte()
    reader.string()
    return sorted(reader.payload(tag))


_TAG_ROW = re.compile(r'^\t"([^"]+)":\s+\[\]string\{([^}]*)\},$', re.M)


def item_tags() -> dict[str, list[str]]:
    """Native item tags per item (protocolgen `item/runtime_generated.go`)."""
    text = (DATA / 'item' / 'runtime_generated.go').read_text(encoding='utf-8')
    body = text[text.index('var runtimeTags = map[string][]string{'):]
    body = body[:body.index('\n}\n')]
    return {name: re.findall(r'"([^"]+)"', tags) for name, tags in _TAG_ROW.findall(body)}
