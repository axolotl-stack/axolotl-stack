"""Little-endian NBT as BDS saves it (`level.dat`, world LevelDB records).

Independent of unastar's reader. Values keep their tag type and compounds keep
their order, so a decoded value encodes back to the same bytes:

    tag     (type, value)
    1-6     int or float
    7/11/12 list of ints (byte, int and long arrays)
    8       str
    9       (element type, [element values])
    10      [(name, tag), ...] in stored order

`flat()` lists every leaf as `path: type = value` lines for diffs.
"""

from __future__ import annotations

import struct

END, BYTE, SHORT, INT, LONG, FLOAT, DOUBLE, BYTE_ARRAY, STRING, LIST, COMPOUND, INT_ARRAY, LONG_ARRAY = range(13)
NAMES = ['end', 'byte', 'short', 'int', 'long', 'float', 'double', 'byte_array', 'string', 'list',
         'compound', 'int_array', 'long_array']
_SCALAR = {BYTE: '<b', SHORT: '<h', INT: '<i', LONG: '<q', FLOAT: '<f', DOUBLE: '<d'}
_ARRAY = {BYTE_ARRAY: '<b', INT_ARRAY: '<i', LONG_ARRAY: '<q'}
MAX_DEPTH = 512


class _Reader:
    def __init__(self, data: bytes, off: int = 0):
        self.data, self.off = data, off

    def take(self, n: int) -> bytes:
        if n < 0 or self.off + n > len(self.data):
            raise ValueError(f'truncated NBT at byte {self.off}')
        out = self.data[self.off:self.off + n]
        self.off += n
        return out

    def unpack(self, fmt: str):
        return struct.unpack(fmt, self.take(struct.calcsize(fmt)))[0]

    def string(self) -> str:
        return self.take(self.unpack('<H')).decode('utf-8')

    def payload(self, tag: int, depth: int):
        if depth > MAX_DEPTH:
            raise ValueError('NBT nested too deeply')
        if tag in _SCALAR:
            return self.unpack(_SCALAR[tag])
        if tag in _ARRAY:
            n = self.unpack('<i')
            size = struct.calcsize(_ARRAY[tag])
            return list(struct.unpack(f'<{n}{_ARRAY[tag][1]}', self.take(n * size)))
        if tag == STRING:
            return self.string()
        if tag == LIST:
            element = self.take(1)[0]
            n = self.unpack('<i')
            return element, [self.payload(element, depth + 1) for _ in range(max(n, 0))]
        if tag == COMPOUND:
            out = []
            while (child := self.take(1)[0]) != END:
                name = self.string()
                out.append((name, (child, self.payload(child, depth + 1))))
            return out
        raise ValueError(f'unknown NBT tag {tag} at byte {self.off - 1}')


def read(data: bytes, off: int = 0) -> tuple[str, tuple, int]:
    """(root name, root tag, end offset) of one rooted NBT value at `off`."""
    reader = _Reader(data, off)
    tag = reader.take(1)[0]
    if tag == END:
        raise ValueError('root tag is TAG_End')
    name = reader.string()
    return name, (tag, reader.payload(tag, 0)), reader.off


def read_all(data: bytes) -> list[tuple[str, tuple]]:
    """Every rooted value of a concatenation (block entity records)."""
    out, off = [], 0
    while off < len(data):
        name, tag, off = read(data, off)
        out.append((name, tag))
    return out


def _write_string(out: bytearray, text: str) -> None:
    raw = text.encode('utf-8')
    out += struct.pack('<H', len(raw)) + raw


def _write_payload(out: bytearray, tag: int, value) -> None:
    if tag in _SCALAR:
        out += struct.pack(_SCALAR[tag], value)
    elif tag in _ARRAY:
        out += struct.pack('<i', len(value)) + struct.pack(f'<{len(value)}{_ARRAY[tag][1]}', *value)
    elif tag == STRING:
        _write_string(out, value)
    elif tag == LIST:
        element, items = value
        out += bytes([element]) + struct.pack('<i', len(items))
        for item in items:
            _write_payload(out, element, item)
    elif tag == COMPOUND:
        for name, (child, child_value) in value:
            out.append(child)
            _write_string(out, name)
            _write_payload(out, child, child_value)
        out.append(END)
    else:
        raise ValueError(f'unknown NBT tag {tag}')


def write(name: str, tag: tuple) -> bytes:
    """One rooted value."""
    out = bytearray([tag[0]])
    _write_string(out, name)
    _write_payload(out, *tag)
    return bytes(out)


def get(compound: tuple, *path: str):
    """The tag at `path` in a compound tag, or None."""
    node = compound
    for key in path:
        if node is None or node[0] != COMPOUND:
            return None
        node = next((child for name, child in node[1] if name == key), None)
    return node


def plain(tag: tuple):
    """The value without tag types (compounds become dicts, lists lists)."""
    kind, value = tag
    if kind == COMPOUND:
        return {name: plain(child) for name, child in value}
    if kind == LIST:
        element, items = value
        return [plain((element, item)) for item in items]
    return value


def flat(tag: tuple, prefix: str = '') -> list[str]:
    """`path: type = value` for every leaf, in stored order."""
    kind, value = tag
    if kind == COMPOUND:
        if not value:
            return [f'{prefix or "."}: compound = {{}}']
        lines = []
        for name, child in value:
            lines += flat(child, f'{prefix}.{name}' if prefix else name)
        return lines
    if kind == LIST:
        element, items = value
        if not items:
            return [f'{prefix}: list<{NAMES[element]}> = []']
        lines = []
        for i, item in enumerate(items):
            lines += flat((element, item), f'{prefix}[{i}]')
        return lines
    if kind == FLOAT:
        # The f32 as stored, shortest round-trip form.
        value = float(struct.unpack('<f', struct.pack('<f', value))[0])
    return [f'{prefix}: {NAMES[kind]} = {value!r}']
