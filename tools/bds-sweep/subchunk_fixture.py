"""Sub-chunk records saved by BDS 1.26.51, and what BDS reports they hold.

Writes `crates/unastar/tests/fixtures/bds_1_26_51_subchunks.json` for
unastar's world-loading tests (`crates/unastar/tests/bds_subchunks.rs`).

BDS saves a sub-chunk under LevelDB key `<chunk><dimension?> 0x2f <y>` as:
version 9, storage count, y index, then one persistent block storage per
layer: header `bits << 1`, the index words, the palette length as i32 LE
(absent when bits is 0) and one `{name, states, version}` NBT compound per
palette entry. Steps, each reading the previous one's output in OUT:

    build OUT          flat world: /fill and /setblock one shape per sub-chunk (every
                       bit width, unused palette entries, a waterlogged second layer,
                       nether and End); BDS saves it on stop; snapshot
    probe OUT          reload that world; /testforblock sample cells of every shape
    natural OUT        default world, seed 12345: ticking areas at spawn, in the
                       nether and on the End island; snapshot
    natural-probe OUT  reload it; /testforblock cells of the natural records kept
    fixture OUT        write the fixture

BDS_RUNTIME and BDS_PORT pick the runtime (README, "Parallel slots"); OUT keeps
console logs and world snapshots. /testforblock reads only the first layer, so
the second layer's cells are checked against the placement plan (water kept
under blocks set into water) and an independent decode of the record.
"""

from __future__ import annotations

import collections
import hashlib
import json
import math
import queue
import re
import shutil
import struct
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import leveldb_read  # noqa: E402

REPO = Path(__file__).resolve().parents[2]
FIXTURE = REPO / 'crates' / 'unastar' / 'tests' / 'fixtures' / 'bds_1_26_51_subchunks.json'
FLAT_LEVEL = 'subchunk-palettes'
NATURAL_LEVEL = 'subchunk-natural'
AIR = ('minecraft:air', ())
DIM_PREFIX = {0: '', 1: 'execute in nether run ', 2: 'execute in the_end run '}
# Overworld chunk row the flat shapes use, and untouched air cells (in loaded
# overworld chunks) for the sync markers of each world.
ROW_Z = 4
FLAT_SYNC = (240, 160, 64)
NATURAL_SYNC = (0, 300, 0)

# --- persistent sub-chunk storages (independent of unastar's decoder) --------

WORDS = {1: 128, 2: 256, 3: 410, 4: 512, 5: 683, 6: 820, 8: 1024, 16: 2048}
NBT_TYPES = {1: 'byte', 3: 'int', 8: 'string'}


class _Nbt:
    """Little-endian NBT, just the tags palette entries use."""

    def __init__(self, data: bytes, off: int):
        self.data, self.off = data, off

    def take(self, n: int) -> bytes:
        if self.off + n > len(self.data):
            raise ValueError('truncated NBT')
        out = self.data[self.off:self.off + n]
        self.off += n
        return out

    def string(self) -> str:
        return self.take(struct.unpack('<H', self.take(2))[0]).decode()

    def payload(self, tag: int):
        if tag == 1:
            return self.take(1)[0]
        if tag == 3:
            return struct.unpack('<i', self.take(4))[0]
        if tag == 8:
            return self.string()
        if tag == 10:
            out = {}
            while (child := self.take(1)[0]) != 0:
                key = self.string()
                out[key] = (child, self.payload(child))
            return out
        raise ValueError(f'unexpected NBT tag {tag} in a palette entry')


def palette_entry(data: bytes, off: int) -> tuple[tuple, int]:
    """(state, end offset); state is (name, ((key, type, value), ...)) sorted by key."""
    nbt = _Nbt(data, off)
    if nbt.take(1)[0] != 10:
        raise ValueError('palette entry is not a compound')
    nbt.string()
    root = nbt.payload(10)
    states = tuple(sorted((key, NBT_TYPES[tag], value) for key, (tag, value) in root['states'][1].items()))
    return (root['name'][1], states), nbt.off


def storage(data: bytes, off: int) -> tuple[dict, int]:
    """One persistent block storage: bits, palette, the 4096 indices (XZY order)."""
    header = data[off]
    if header & 1:
        raise ValueError('runtime-form storage in a BDS save')
    bits = header >> 1
    off += 1
    if bits == 0:
        indices = [0] * 4096
        length = 1
    else:
        per_word = 32 // bits
        words = struct.unpack_from(f'<{WORDS[bits]}I', data, off)
        off += 4 * WORDS[bits]
        indices = [(word >> (i * bits)) & ((1 << bits) - 1) for word in words for i in range(per_word)][:4096]
        length = struct.unpack_from('<i', data, off)[0]
        off += 4
    palette = []
    for _ in range(length):
        state, off = palette_entry(data, off)
        palette.append(state)
    if max(indices) >= len(palette):
        raise ValueError('index outside the palette')
    return {'bits': bits, 'palette': palette, 'indices': indices}, off


def subchunk(value: bytes) -> list[dict]:
    """Storages of a version 9 record, checked to consume every byte."""
    if value[0] != 9:
        raise ValueError(f'sub-chunk version {value[0]}')
    off, layers = 3, []
    for _ in range(value[1]):
        layer, off = storage(value, off)
        layers.append(layer)
    if off != len(value):
        raise ValueError(f'{len(value) - off} bytes after the storages')
    return layers


def cell(state: tuple) -> str:
    """The fixture's cell form: `name|key=<b|i|s>value,...`, keys sorted."""
    name, states = state
    return f"{name}|{','.join(f'{k}={t[0]}{v}' for k, t, v in states)}"


def cells_sha256(states: list[tuple]) -> str:
    return hashlib.sha256('\n'.join(cell(s) for s in states).encode()).hexdigest()


def layer_states(layer: dict) -> list[tuple]:
    return [layer['palette'][i] for i in layer['indices']]


def chunk_key(dim: int, x: int, z: int, y: int) -> bytes:
    key = struct.pack('<ii', x, z) + (struct.pack('<i', dim) if dim else b'')
    return key + bytes([0x2F, y & 0xFF])


def parse_key(key: bytes):
    """(dim, x, z, y) of a sub-chunk key, else None."""
    if len(key) == 10 and key[8] == 0x2F:
        return (0, *struct.unpack_from('<ii', key), struct.unpack('<b', key[9:])[0])
    if len(key) == 14 and key[12] == 0x2F:
        x, z, dim = struct.unpack_from('<iii', key)
        return dim, x, z, struct.unpack('<b', key[13:])[0]
    return None


# --- shapes ------------------------------------------------------------------

def st(name: str, *states) -> tuple:
    return ('minecraft:' + name, tuple(sorted(states)))


def pool() -> list[tuple]:
    """Distinct states with no physics and no state driven by neighbours."""
    colors = ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray',
              'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black']
    woods = ['oak', 'spruce', 'birch', 'jungle', 'acacia', 'dark_oak', 'mangrove', 'cherry', 'pale_oak']
    planks = woods + ['bamboo', 'crimson', 'warped']
    stones = ['stone', 'granite', 'polished_granite', 'diorite', 'polished_diorite', 'andesite',
              'polished_andesite', 'cobblestone', 'mossy_cobblestone', 'stone_bricks', 'smooth_stone',
              'brick_block', 'tuff', 'calcite']
    slabs = [w + '_slab' for w in planks] + [
        'cobblestone_slab', 'stone_brick_slab', 'brick_slab', 'nether_brick_slab', 'quartz_slab',
        'andesite_slab', 'diorite_slab', 'granite_slab', 'mossy_cobblestone_slab', 'blackstone_slab',
        'smooth_stone_slab', 'polished_andesite_slab', 'deepslate_brick_slab', 'deepslate_tile_slab',
        'tuff_slab', 'end_stone_brick_slab', 'purpur_slab', 'prismarine_slab']
    out = [st(c + suffix) for suffix in ('_wool', '_concrete') for c in colors]
    out += [st(s) for s in stones] + [st(p + '_planks') for p in planks]
    out += [st(c + suffix) for suffix in ('_terracotta', '_stained_glass') for c in colors]
    for axis in ('y', 'x', 'z'):
        for w in woods:
            out.append(st(w + '_log', ('pillar_axis', 'string', axis)))
            out.append(st('stripped_' + w + '_log', ('pillar_axis', 'string', axis)))
        for b in ('deepslate', 'basalt', 'purpur_block', 'quartz_block'):
            out.append(st(b, ('pillar_axis', 'string', axis)))
    glazed = [c if c != 'light_gray' else 'silver' for c in colors]
    out += [st(g + '_glazed_terracotta', ('facing_direction', 'int', face)) for face in range(6) for g in glazed]
    out += [st(s, ('minecraft:vertical_half', 'string', half)) for half in ('bottom', 'top') for s in slabs]
    assert len(set(out)) == len(out)
    return out


def state_arg(state: tuple) -> str:
    name, states = state
    parts = []
    for key, kind, value in states:
        if kind == 'string':
            parts.append(f'"{key}"="{value}"')
        elif kind == 'byte':
            parts.append(f'"{key}"={"true" if value else "false"}')
        else:
            parts.append(f'"{key}"={value}')
    short = name.removeprefix('minecraft:')
    return f'{short} [{",".join(parts)}]' if parts else short


class Shape:
    """One sub-chunk: the commands that build it and the expected cells."""

    def __init__(self, sid: str, dim: int, cx: int, cz: int, sy: int, note: str):
        self.id, self.dim, self.cx, self.cz, self.sy, self.note = sid, dim, cx, cz, sy, note
        self.commands: list[str] = []
        self.cells: dict[tuple, tuple] = {}  # (x, y, z) -> first-layer state
        self.liquid: set[tuple] = set()  # cells where a block was set into water

    def base(self) -> tuple[int, int, int]:
        return self.cx * 16, self.sy * 16, self.cz * 16

    def fill(self, a, b, state, mode=''):
        bx, by, bz = self.base()
        self.commands.append(f'{DIM_PREFIX[self.dim]}fill {bx + a[0]} {by + a[1]} {bz + a[2]} '
                             f'{bx + b[0]} {by + b[1]} {bz + b[2]} {state_arg(state)}'
                             + (f' {mode}' if mode else ''))
        for x in range(a[0], b[0] + 1):
            for y in range(a[1], b[1] + 1):
                for z in range(a[2], b[2] + 1):
                    inside = a[0] < x < b[0] and a[1] < y < b[1] and a[2] < z < b[2]
                    if mode == 'hollow' and inside:
                        self.cells[(x, y, z)] = AIR
                    else:
                        self.set(x, y, z, state)

    def setblock(self, p, state):
        bx, by, bz = self.base()
        self.commands.append(f'{DIM_PREFIX[self.dim]}setblock {bx + p[0]} {by + p[1]} {bz + p[2]} '
                             f'{state_arg(state)}')
        self.set(*p, state)

    def set(self, x, y, z, state):
        # Blocks set into water keep it as the second layer (measured).
        if self.cells.get((x, y, z), AIR)[0] == 'minecraft:water' and state[0] != 'minecraft:water':
            self.liquid.add((x, y, z))
        self.cells[(x, y, z)] = state

    def layers(self) -> list[list[tuple]]:
        order = [((i >> 8) & 15, i & 15, (i >> 4) & 15) for i in range(4096)]  # XZY index -> (x, y, z)
        first = [self.cells.get(p, AIR) for p in order]
        if not self.liquid:
            return [first]
        water = st('water', ('liquid_depth', 'int', 0))
        return [first, [water if p in self.liquid else AIR for p in order]]


def lines(shape: Shape, states: list, axis: str, mult: int):
    """Fill every cell with lines along `axis`: segment s gets states[s * mult % N]
    (halved lines when N > 256, so every state appears)."""
    n = len(states)
    assert math.gcd(mult, n) == 1
    halves = 2 if n > 256 else 1
    s = 0
    for a in range(16):
        for b in range(16):
            for h in range(halves):
                lo, hi = (0, 15) if halves == 1 else ((0, 7), (8, 15))[h]
                state = states[s * mult % n]
                s += 1
                if axis == 'x':
                    shape.fill((lo, a, b), (hi, a, b), state)
                elif axis == 'z':
                    shape.fill((a, b, lo), (a, b, hi), state)
                else:
                    shape.fill((a, lo, b), (a, hi, b), state)


def shapes() -> list[Shape]:
    p = pool()
    out = []

    def shape(sid, dim, cx, cz, sy, note):
        out.append(Shape(sid, dim, cx, cz, sy, note))
        return out[-1]

    shape('fill_stone', 0, 0, ROW_Z, 0, 'one state filling a fresh sub-chunk').fill(
        (0, 0, 0), (15, 15, 15), st('stone'))
    shape('fill_log_x', 0, 0, ROW_Z, 1, 'one state with a property filling a fresh sub-chunk').fill(
        (0, 0, 0), (15, 15, 15), st('oak_log', ('pillar_axis', 'string', 'x')))
    specs = [  # id, chunk x, sub-chunk y, states, line axis, multiplier, with air
        ('n2', 1, 0, 2, 'x', 1, True),
        ('n3', 2, 0, 3, 'z', 2, True),
        ('n5', 3, 0, 5, 'x', 3, False),
        ('n9', 4, 0, 9, 'y', 2, False),
        ('n17', 5, 0, 17, 'z', 3, True),
        ('n33', 6, 0, 33, 'x', 5, False),
        ('n65', 7, 0, 65, 'z', 7, False),
        ('n257', 8, 0, 257, 'x', 7, False),
    ]
    for offset, (sid, cx, sy, n, axis, mult, with_air) in enumerate(specs):
        sh = shape(sid, 0, cx, ROW_Z, sy, f'{n} states{" with air" if with_air else ""}, lines along {axis}')
        first = (offset * 23) % (len(p) - n)
        lines(sh, ([AIR] if with_air else []) + p[first:first + n - with_air], axis, mult)
    sh = shape('unused_entries', 0, 9, ROW_Z, 0, '17 states, then stone over all but two cells')
    lines(sh, p[200:217], 'x', 3)
    sh.fill((0, 0, 0), (15, 15, 15), st('stone'))
    sh.setblock((3, 4, 5), p[201])
    sh.setblock((15, 15, 15), p[202])
    sh = shape('waterlogged', 0, 10, ROW_Z, 0, 'glass box of water; slabs and fences set into it')
    sh.fill((0, 0, 0), (15, 15, 15), st('glass'), 'hollow')
    sh.fill((1, 1, 1), (14, 14, 14), st('water', ('liquid_depth', 'int', 0)))
    fence = st('oak_fence', *[(f'minecraft:connection_{side}', 'byte', 0)
                              for side in ('east', 'north', 'south', 'west')])
    for x in range(2, 14, 3):
        for y in range(2, 14, 3):
            for z in range(2, 14, 3):
                sh.setblock((x, y, z), st('oak_slab', ('minecraft:vertical_half', 'string', 'bottom'))
                            if (x + y + z) % 2 == 0 else fence)
    lines(shape('nether_n5', 1, 1, 0, 3, 'nether: 5 states, lines along z'), p[110:115], 'z', 2)
    lines(shape('end_n2', 2, 8, 8, 4, 'end: 2 states with air, lines along y'), [AIR, st('end_stone')], 'y', 1)
    # No commands: the flat ground as BDS generates and saves it (this record crashed unastar's loader).
    sh = shape('flat_ground', 0, 0, ROW_Z, -4, 'flat world ground as BDS generates it: bedrock, two dirt, grass')
    ground = [st('bedrock', ('infiniburn_bit', 'byte', 0)), st('dirt'), st('dirt'), st('grass_block')]
    for x in range(16):
        for z in range(16):
            for y, state in enumerate(ground):
                sh.cells[(x, y, z)] = state
    return out


# --- BDS sessions --------------------------------------------------------------

class Console:
    def __init__(self, bds: bds_runtime.Bds, sync: tuple[int, int, int]):
        self.bds, self.sync, self.n = bds, sync, 0

    def _line(self, timeout: float):
        try:
            line = self.bds.lines.get(timeout=timeout)
        except queue.Empty:
            if self.bds.process.poll() is not None:
                raise RuntimeError('BDS exited')
            return None
        self.bds.history.append(line)
        return line

    def ask(self, command: str, pattern: str, timeout: float = 15.0) -> list[str]:
        """One command; lines until one matches `pattern` (or the timeout)."""
        self.bds.command(command)
        out, deadline = [], time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self._line(0.2)
            if line is not None:
                out.append(line)
                if re.search(pattern, line):
                    break
        return out

    def run(self, commands: list[str], timeout: float = 900.0) -> list[str]:
        """Commands, then a /testforblock marker in a loaded air cell; every
        line up to the marker's answer."""
        self.n += 1
        x, y, z = self.sync[0] + self.n % 16, self.sync[1] + self.n // 16 % 16, self.sync[2] + self.n // 256
        for command in commands:
            self.bds.command(command)
        self.bds.command(f'testforblock {x} {y} {z} air')
        marker, out, deadline = f'at {x},{y},{z}.', [], time.monotonic() + timeout
        while time.monotonic() < deadline:
            line = self._line(0.5)
            if line is None:
                continue
            if marker in line:
                return out
            out.append(line)
        raise TimeoutError(f'no answer for {marker}')

    def wait_loaded(self, columns, timeout: float = 300.0):
        """Poll one cell per (dim, chunk x, chunk z) until BDS can test it."""
        pending, deadline = set(columns), time.monotonic() + timeout
        while pending:
            if time.monotonic() > deadline:
                raise TimeoutError(f'columns never loaded: {sorted(pending)}')
            for dim, cx, cz in sorted(pending):
                answer = ' '.join(self.ask(f'{DIM_PREFIX[dim]}testforblock {cx * 16} 100 {cz * 16} bedrock',
                                           r'Successfully|The block at|outside of the world|Cannot'))
                if 'Successfully' in answer or 'The block at' in answer:
                    pending.discard((dim, cx, cz))
            if pending:
                time.sleep(2)


def start(level: str, log: Path, **properties) -> bds_runtime.Bds:
    return bds_runtime.Bds(bds_runtime.base_properties(level, gamemode='creative', difficulty='peaceful',
                                                       **properties), log, endstone=False)


def flat_columns(shape_list):
    return {(s.dim, s.cx, s.cz) for s in shape_list} | {(0, cx, ROW_Z) for cx in range(16)}


def build(out: Path):
    world = bds_runtime.SERVER / 'worlds' / FLAT_LEVEL
    if world.exists():
        shutil.rmtree(world)
    shape_list, report = shapes(), {}
    with start(FLAT_LEVEL, out / 'build.log', level_type='FLAT') as bds:
        con = Console(bds, FLAT_SYNC)
        for rule in ('randomtickspeed 0', 'domobspawning false', 'dofiretick false', 'dodaylightcycle false',
                     'doweathercycle false', 'dotiledrops false', 'mobgriefing false'):
            con.ask(f'gamerule {rule}', r'Game rule|ERROR')
        for area in ('tickingarea add 0 0 48 255 0 95 shapes true',
                     'execute in nether run tickingarea add 0 0 0 47 0 15 shapes true',
                     'execute in the_end run tickingarea add 128 0 128 143 0 143 shapes true'):
            print(con.ask(area, r'ticking areas in use|ERROR')[-1])
        con.wait_loaded(flat_columns(shape_list))
        for sh in shape_list:
            lines = con.run(sh.commands)
            placed = sum(1 for line in lines if 'blocks filled' in line or 'Block placed' in line)
            errors = [line for line in lines if 'ERROR' in line and '0 blocks filled' not in line]
            report[sh.id] = {'commands': len(sh.commands), 'placed': placed, 'errors': errors}
            print(sh.id, report[sh.id])
            if errors:
                raise RuntimeError(f'{sh.id}: {errors[:3]}')
    (out / 'build.json').write_text(json.dumps(report, indent=1), encoding='utf-8')
    snapshot(world, out / 'flat-world')


def snapshot(world: Path, to: Path):
    if to.exists():
        shutil.rmtree(to)
    shutil.copytree(world, to)
    print('snapshot', to)


def testforblock(con: Console, dim: int, base, probes: list[tuple]) -> dict:
    """/testforblock (x, y, z, state) cells; {(x, y, z): answer}."""
    bx, by, bz = base
    lines = con.run([f'{DIM_PREFIX[dim]}testforblock {bx + x} {by + y} {bz + z} {state_arg(s)}'
                     for x, y, z, s in probes])
    answers = {}
    for line in lines:
        if (m := re.search(r'at (-?\d+),(-?\d+),(-?\d+)', line)):
            answers.setdefault((int(m[1]) - bx, int(m[2]) - by, int(m[3]) - bz), line.split('] ', 1)[-1])
    return answers


def sample_cells():
    """Cells of every word position class: first, middle and last columns."""
    return [(x, y, z) for x in (0, 8, 15) for z in (0, 15) for y in range(16)]


def probe(out: Path):
    shape_list, results = shapes(), {}
    with start(FLAT_LEVEL, out / 'probe.log', level_type='FLAT') as bds:
        con = Console(bds, FLAT_SYNC)
        con.wait_loaded(flat_columns(shape_list))
        for sh in shape_list:
            probes = [(x, y, z, sh.cells.get((x, y, z), AIR)) for x, y, z in sample_cells()]
            answers = testforblock(con, sh.dim, sh.base(), probes)
            confirmed = [p for p in probes if answers.get(p[:3], '').startswith('Successfully')]
            results[sh.id] = {'asked': len(probes), 'confirmed': [list(p[:3]) + [cell(p[3])] for p in confirmed],
                              'other': {str(k): v for k, v in answers.items() if not v.startswith('Successfully')}}
            print(sh.id, len(probes), 'confirmed', len(confirmed))
    (out / 'probe.json').write_text(json.dumps(results, indent=1), encoding='utf-8')


def natural(out: Path):
    world = bds_runtime.SERVER / 'worlds' / NATURAL_LEVEL
    if world.exists():
        shutil.rmtree(world)
    with start(NATURAL_LEVEL, out / 'natural.log', level_type='DEFAULT', level_seed='12345') as bds:
        con = Console(bds, NATURAL_SYNC)
        for rule in ('randomtickspeed 0', 'domobspawning false', 'dofiretick false'):
            con.ask(f'gamerule {rule}', r'Game rule|ERROR')
        for area in ('tickingarea add -32 0 -32 31 0 31 spawn true',
                     'execute in nether run tickingarea add -32 0 -32 31 0 31 spawn true',
                     'execute in the_end run tickingarea add -32 0 -32 31 0 31 spawn true'):
            print(con.ask(area, r'ticking areas in use|ERROR')[-1])
        con.wait_loaded({(0, 0, 0)} | {(dim, cx, cz) for dim in (0, 1, 2) for cx in (-2, 1) for cz in (-2, 1)})
        time.sleep(30)  # let generation settle
        # BDS saves untouched End chunks only after an edit: set one glass
        # block high in every chunk of every area.
        for dim, y in ((0, 300), (1, 120), (2, 250)):
            for cx in range(-2, 2):
                for cz in range(-2, 2):
                    con.run([f'{DIM_PREFIX[dim]}setblock {cx * 16 + 8} {y} {cz * 16 + 8} glass'])
        time.sleep(10)
    snapshot(world, out / 'natural-world')


def natural_records(db: dict) -> dict[str, tuple]:
    """Natural records the fixture keeps: the first zero-bit storage (BDS
    writes them for uniform generated sections, here inside the End island),
    the first overworld record whose liquid layer holds flowing water, and
    the richest nether and End records (by palette length)."""
    candidates = sorted(((parsed, subchunk(value)) for key, value in db.items()
                         if (parsed := parse_key(key)) is not None), key=lambda c: c[0])
    picked = {}
    for parsed, layers in candidates:
        if any(layer['bits'] == 0 for layer in layers):
            picked.setdefault('natural_bits0', parsed)
        if parsed[0] == 0 and len(layers) == 2 and len(layers[1]['palette']) > 2:
            picked.setdefault('natural_liquid', parsed)
    for dim, name in ((1, 'natural_nether'), (2, 'natural_end')):
        rich = [(len(layers[0]['palette']), parsed) for parsed, layers in candidates if parsed[0] == dim]
        if rich:
            picked[name] = max(rich)[1]
    missing = {'natural_bits0', 'natural_liquid', 'natural_nether', 'natural_end'} - picked.keys()
    if missing:
        raise ValueError(f'the natural world has no record for {sorted(missing)}')
    return picked


def natural_probe(out: Path):
    db = leveldb_read.read_db(out / 'natural-world' / 'db')
    picked, results = natural_records(db), {}
    with start(NATURAL_LEVEL, out / 'natural-probe.log', level_type='DEFAULT', level_seed='12345') as bds:
        con = Console(bds, NATURAL_SYNC)
        con.wait_loaded({(0, 0, 0)} | {(dim, x, z) for dim, x, z, _y in picked.values()})
        for name, (dim, x, z, y) in picked.items():
            first = layer_states(subchunk(db[chunk_key(dim, x, z, y)])[0])
            probes = [(px, py, pz, first[(px << 8) | (pz << 4) | py]) for px, py, pz in sample_cells()]
            answers = testforblock(con, dim, (x * 16, y * 16, z * 16), probes)
            confirmed = [p for p in probes if answers.get(p[:3], '').startswith('Successfully')]
            results[name] = {'key': [dim, x, z, y], 'asked': len(probes),
                             'confirmed': [list(p[:3]) + [cell(p[3])] for p in confirmed]}
            print(name, (dim, x, z, y), len(probes), 'confirmed', len(confirmed))
    (out / 'natural-probe.json').write_text(json.dumps(results, indent=1), encoding='utf-8')


# --- fixture ------------------------------------------------------------------

def record(rid: str, note: str, key: tuple, value: bytes, expected: list[list[tuple]] | None,
           confirmed: list) -> dict:
    """A fixture record. `expected` (the placement plan) must equal the
    independent decode of every layer; natural records have no plan."""
    layers = subchunk(value)
    decoded = [layer_states(layer) for layer in layers]
    if expected is not None and decoded != expected:
        raise ValueError(f'{rid}: the record does not hold the placed blocks')
    states = sorted({c[3] for c in confirmed})
    return {
        'id': rid, 'note': note, 'dimension': key[0], 'chunk': [key[1], key[2]], 'y': key[3],
        'layers': [{'bits': layer['bits'], 'palette': len(layer['palette']),
                    'cells_sha256': cells_sha256(cells)} for layer, cells in zip(layers, decoded)],
        'probe_states': states,
        'probes': [c[:3] + [states.index(c[3])] for c in confirmed],
        'value': value.hex(),
    }


def fixture(out: Path):
    flat = leveldb_read.read_db(out / 'flat-world' / 'db')
    probes = json.loads((out / 'probe.json').read_text(encoding='utf-8'))
    records = []
    for sh in shapes():
        key = (sh.dim, sh.cx, sh.cz, sh.sy)
        confirmed = probes[sh.id]['confirmed']
        if len(confirmed) != probes[sh.id]['asked']:
            raise ValueError(f'{sh.id}: BDS confirmed {len(confirmed)} of {probes[sh.id]["asked"]} cells')
        records.append(record(sh.id, sh.note, key, flat[chunk_key(*key)], sh.layers(), confirmed))
    natural_db = leveldb_read.read_db(out / 'natural-world' / 'db')
    for name, result in json.loads((out / 'natural-probe.json').read_text(encoding='utf-8')).items():
        key = tuple(result['key'])
        records.append(record(name, f'generated by BDS (seed 12345), dimension {key[0]}', key,
                              natural_db[chunk_key(*key)], None, result['confirmed']))
    shape_stats = collections.Counter(layer['bits'] for r in records for layer in r['layers'])
    data = {
        'source': 'BDS 1.26.51 worlds: a flat world built with /fill and /setblock from the console and a '
                  'default world (seed 12345); each world was saved, reloaded and asked /testforblock',
        'generator': 'tools/bds-sweep/subchunk_fixture.py',
        'cell_form': 'cells_sha256: SHA-256 of the 4096 cells of a layer in XZY order (x << 8 | z << 4 | y), '
                     'one line per cell joined by \\n: name|key=<b|i|s>value,... with keys sorted',
        'probes': '[x, y, z, index into probe_states]: first-layer cells BDS confirmed after the reload',
        'records': records,
    }
    # One record per line, like the other BDS fixtures.
    lines = ['{'] + [f' {json.dumps(k)}: {json.dumps(v)},' for k, v in data.items() if k != 'records']
    lines.append(' "records": [')
    lines += [f"  {json.dumps(r, separators=(',', ':'))}," for r in records]
    lines[-1] = lines[-1].rstrip(',')
    lines += [' ]', '}']
    FIXTURE.write_text('\n'.join(lines) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(records)} records, bit widths {dict(sorted(shape_stats.items()))} -> {FIXTURE}')


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in ('build', 'probe', 'natural', 'natural-probe', 'fixture'):
        print(__doc__)
        return 2
    out = Path(sys.argv[2]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    {'build': build, 'probe': probe, 'natural': natural, 'natural-probe': natural_probe,
     'fixture': fixture}[sys.argv[1]](out)
    return 0


if __name__ == '__main__':
    sys.exit(main())
