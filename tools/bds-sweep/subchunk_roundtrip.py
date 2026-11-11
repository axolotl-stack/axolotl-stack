"""Sub-chunk records between BDS 1.26.51 and unastar: what BDS writes, and what it reads.

One BDS-saved flat world (`source`) holds a setup for every question below. The
same edit plan is then applied twice to copies of it: by BDS from the console
(`bds-edit`) and by unastar's LevelDB provider (`unastar-edit`, through
`cargo run -p unastar --example subchunk_roundtrip`). `compare` puts every
sub-chunk record of the edited columns side by side, byte for byte, and checks
unastar's records with an independent decode. `bds-verify` reopens the world
unastar edited in BDS and asks /testforblock for every edited cell and a
sample of untouched ones; `generate` and `generated-verify` do the same for a
world unastar generated.

    source OUT            flat world: palette shapes (full palettes of every width,
                          unused entries, waterlogged blocks), zero-bit End sections,
                          nether and End shapes; BDS's periodic save writes them; snapshot
    plan OUT              the edit plan, chosen from the source world
    bds-edit OUT          BDS applies the plan to a copy; snapshot
    unastar-edit OUT      unastar applies the plan to a copy
    compare OUT           BDS's and unastar's records of the edited columns; what BDS
                          wrote in the edit session (LevelDB sequence numbers)
    bds-verify OUT        BDS reopens unastar's world: /testforblock; then one block per
                          waterlogged section, so BDS saves the liquid layer it read
    generate OUT          unastar generates columns in all three dimensions (Java-style
                          generator, seed 12345) into a world with the source level.dat
    generated-verify OUT  BDS opens it: /testforblock along vertical lines
    fixture OUT           write crates/unastar/tests/fixtures/bds_1_26_51_subchunk_edits.json:
                          each edited section before the edits, the writes, and BDS's
                          record after them

BDS_RUNTIME and BDS_PORT pick the runtime (README, "Parallel slots"). The
setups answer: the order of palette entries after edits, within and across
sessions; what happens to an entry no cell uses any more, and when a full
palette grows; zero-bit storages and edits to them; whether BDS rewrites the
unchanged sub-chunks of an edited column; the liquid storage once no cell holds
water; records of sub-chunks cleared to air and of air below a used one; the
NBT `version` of entries, newer blocks included. Results:
`docs/solutions/storage/bds-compacts-palettes-only-when-a-chunk-unloads.md` and
`docs/solutions/storage/data3d-copy-markers-below-a-stored-section-stop-bds.md`.
"""

from __future__ import annotations

import collections
import contextlib
import json
import shutil
import struct
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import bds_runtime  # noqa: E402
import leveldb_read  # noqa: E402
from subchunk_fixture import (AIR, DIM_PREFIX, WORDS, Console, Shape, _Nbt, cell, chunk_key,  # noqa: E402
                              layer_states, lines, parse_key, pool, sample_cells, st, start, state_arg,
                              subchunk, testforblock)

REPO = Path(__file__).resolve().parents[2]
LEVEL = 'subchunk-roundtrip'
ROW = 8  # overworld chunk z of the setups (chunks x 0..15)
SYNC = (240, 200, 128)  # loaded air cells (chunk 15, 8) for console sync markers
GENERATED_SYNC = (0, 300, 0)
END_AREA = range(-2, 2)  # End chunks -2..1 on both axes: the island
NETHER_COLUMNS = [(1, x, 0) for x in range(3)]
GOLD, DIAMOND, EMERALD, IRON, LAPIS, STONE, GLASS, END_STONE = (st(n) for n in (
    'gold_block', 'diamond_block', 'emerald_block', 'iron_block', 'lapis_block', 'stone', 'glass', 'end_stone'))
WATER = st('water', ('liquid_depth', 'int', 0))
SLAB = st('oak_slab', ('minecraft:vertical_half', 'string', 'bottom'))
FENCE = st('oak_fence', *[(f'minecraft:connection_{side}', 'byte', 0)
                          for side in ('east', 'north', 'south', 'west')])
# Blocks of the 1.21.x drops, to see their NBT `version`; none has an actor.
NEWER = ['pale_oak_planks', 'pale_moss_block', 'resin_block', 'resin_bricks', 'chiseled_resin_bricks',
         'tuff_bricks', 'polished_tuff', 'chiseled_tuff_bricks', 'copper_grate', 'chiseled_copper']
# Setup columns whose plan edits only BDS applies (water flows after them).
MEASURE_ONLY = {(0, 15, ROW)}
WIDTHS = [1, 2, 3, 4, 5, 6, 8]  # sections 0..6 of column 14 hold full palettes of these widths


def setblock(dim: int, pos, state) -> str:
    return f'{DIM_PREFIX[dim]}setblock {pos[0]} {pos[1]} {pos[2]} {state_arg(state)}'


def fill(dim: int, a, b, state, mode: str = '') -> str:
    return (f'{DIM_PREFIX[dim]}fill {a[0]} {a[1]} {a[2]} {b[0]} {b[1]} {b[2]} {state_arg(state)}'
            + (f' {mode}' if mode else ''))


def ow(cx: int, x: int, y: int, z: int) -> tuple[int, int, int]:
    """Absolute position of local (x, z) in setup column cx; y is absolute."""
    return cx * 16 + x, y, ROW * 16 + z


def water_box(dim: int, x: int, y: int, z: int) -> list[str]:
    """Glass shell of one sub-chunk (corner x, y, z) filled with still water."""
    return [fill(dim, (x, y, z), (x + 15, y + 15, z + 15), GLASS, 'hollow'),
            fill(dim, (x + 1, y + 1, z + 1), (x + 14, y + 14, z + 14), WATER)]


def index(x: int, y: int, z: int) -> int:
    """XZY cell index of local coordinates."""
    return (x << 8) | (z << 4) | y


# --- source world ------------------------------------------------------------

def source_commands() -> list[tuple[str, list[str]]]:
    """(setup, commands) of the source session, in order."""
    p, out = pool(), []

    def sets(cx, cells):
        return [setblock(0, ow(cx, x, y, z), s) for (x, y, z), s in cells]

    out.append(('order', sets(0, [((0, 0, 0), GOLD), ((1, 0, 0), DIAMOND), ((0, 1, 0), EMERALD),
                                  ((5, 5, 5), IRON)])))
    out.append(('unused_same_session', sets(1, [((0, 0, 0), STONE), ((1, 0, 0), GOLD), ((2, 0, 0), DIAMOND),
                                                ((1, 0, 0), AIR), ((3, 0, 0), EMERALD)])))
    out.append(('unused_then_grow', sets(2, [((0, 0, 0), STONE), ((1, 0, 0), GOLD), ((2, 0, 0), DIAMOND),
                                             ((1, 0, 0), AIR)])))
    sh = Shape('lines_then_stone', 0, 3, ROW, 0, '')
    lines(sh, p[0:5], 'x', 2)
    sh.fill((0, 0, 0), (15, 15, 15), STONE)
    sh.setblock((3, 4, 5), p[1])
    out.append(('lines_then_stone', sh.commands))
    out.append(('waterlogged', water_box(0, 4 * 16, 0, ROW * 16) + sets(4, [
        ((3, 3, 3), SLAB), ((6, 3, 3), SLAB), ((9, 3, 3), SLAB), ((3, 6, 3), FENCE)])))
    out.append(('single_waterlogged', water_box(0, 5 * 16, 0, ROW * 16) + sets(5, [((5, 5, 5), SLAB)])))
    out.append(('clear', [fill(0, ow(6, 0, 0, 0), ow(6, 15, 15, 15), STONE)] + sets(6, [((0, 32, 0), GOLD)])))
    out.append(('gap', sets(7, [((0, 100, 0), GOLD)])))
    out.append(('newer_blocks', sets(8, [((i, 0, 0), st(name)) for i, name in enumerate(NEWER)])))
    out.append(('uniform_again', [fill(0, ow(9, 0, 0, 0), ow(9, 15, 15, 15), STONE)]
                + sets(9, [((0, 0, 0), GOLD), ((0, 0, 0), STONE)])))
    out.append(('rewrite', sets(10, [((0, 0, 0), STONE), ((0, 16, 0), GOLD), ((0, 32, 0), DIAMOND)])))
    out.append(('control', sets(11, [((0, 0, 0), STONE)])))
    widths = []
    for sy, bits in enumerate(WIDTHS):
        widths += sets(14, [((i % 16, sy * 16, i // 16), p[i]) for i in range((1 << bits) - 1)])
    out.append(('full_palettes', widths))
    out.append(('remove_waterlogged', water_box(0, 15 * 16, 0, ROW * 16) + sets(15, [((5, 5, 5), SLAB)])))
    # Section 2 of column 9: the liquid layer's water unused before the edit
    # session, which changes the first layer only.
    out.append(('liquid_unused', water_box(0, 9 * 16, 32, ROW * 16)
                + sets(9, [((5, 37, 5), SLAB), ((5, 37, 5), STONE)])))
    # Section 2 of column 8: a waterlogged slab, later air in both layers.
    out.append(('liquid_then_air', water_box(0, 8 * 16, 32, ROW * 16) + sets(8, [((5, 37, 5), SLAB)])))
    nether = Shape('nether_lines', 1, 1, 0, 3, '')
    lines(nether, p[110:115], 'z', 2)
    out.append(('nether_lines', nether.commands))
    out.append(('end_waterlogged', water_box(2, 16, 96, 16)
                + [setblock(2, (19, 99, 19), SLAB), setblock(2, (22, 99, 19), SLAB)]))
    # BDS saves a generated chunk only once a block in it changed (flat overworld,
    # nether and End alike): one glass block high in each column the plan edits
    # that has no setup.
    out.append(('save', [setblock(0, ow(cx, 8, 250, 8), GLASS) for cx in (12, 13)]
                + [setblock(1, (cx * 16 + 8, 120, 8), GLASS) for cx in (0, 2)]
                + [setblock(2, (cx * 16 + 8, 250, cz * 16 + 8), GLASS) for cx in END_AREA for cz in END_AREA]))
    return out


def columns() -> set[tuple[int, int, int]]:
    return ({(0, cx, ROW) for cx in range(16)} | set(NETHER_COLUMNS)
            | {(2, cx, cz) for cx in END_AREA for cz in END_AREA})


def snapshot(world: Path, to: Path):
    if to.exists():
        shutil.rmtree(to)
    shutil.copytree(world, to)
    print('snapshot', to)


def slot_world() -> Path:
    return bds_runtime.SERVER / 'worlds' / LEVEL


@contextlib.contextmanager
def session(out: Path, log: str, copy_from: Path | None, keep: Path | None = None, **properties):
    """A BDS session on this harness's slot world, fresh or a copy of
    `copy_from`. After a clean stop the world is copied to `keep`; it leaves
    the slot either way, also when the session fails."""
    world = slot_world()
    if world.exists():
        shutil.rmtree(world)
    if copy_from is not None:
        shutil.copytree(copy_from, world)
    try:
        with start(LEVEL, out / log, **properties) as bds:
            yield bds
        if keep is not None:
            snapshot(world, keep)
    finally:
        shutil.rmtree(world, ignore_errors=True)


def run_logged(con: Console, name: str, commands: list[str]) -> dict:
    output = con.run(commands)
    errors = [line for line in output if any(word in line for word in ('ERROR', 'Syntax', 'Unknown', 'rror:'))
              and '0 blocks filled' not in line]
    print(name, len(commands), 'commands', len(errors), 'errors', errors[:3])
    return {'commands': len(commands), 'errors': errors}


def step_source(out: Path):
    report = {}
    with session(out, 'source.log', None, out / 'source-world', level_type='FLAT') as bds:
        con = Console(bds, SYNC)
        for rule in ('randomtickspeed 0', 'domobspawning false', 'dofiretick false', 'dodaylightcycle false',
                     'doweathercycle false', 'dotiledrops false', 'mobgriefing false'):
            con.ask(f'gamerule {rule}', r'Game rule|ERROR')
        for area in ('tickingarea add 0 0 128 255 0 143 rt true',
                     'execute in nether run tickingarea add 0 0 0 47 0 15 rt true',
                     'execute in the_end run tickingarea add -32 0 -32 31 0 31 rt true'):
            print(con.ask(area, r'ticking areas in use|ERROR|already')[-1])
        con.wait_loaded(columns())
        for name, commands in source_commands():
            report[name] = run_logged(con, name, commands)
        # Let the periodic save write every chunk as it is in memory, so the
        # source records do not depend on the race at stop.
        time.sleep(100)
    (out / 'source.json').write_text(json.dumps(report, indent=1), encoding='utf-8')


# --- decoding helpers ----------------------------------------------------------

def db_entries(db_dir: Path) -> dict[bytes, tuple[int, bytes | None]]:
    """Newest (sequence, value; None for a deletion) of every key."""
    newest: dict[bytes, tuple[int, bytes | None]] = {}
    for path in sorted(Path(db_dir).iterdir()):
        if path.suffix in ('.ldb', '.sst'):
            entries = leveldb_read._table_entries(path)
        elif path.suffix == '.log':
            entries = leveldb_read._log_entries(path)
        else:
            continue
        for key, sequence, value in entries:
            if key not in newest or newest[key][0] < sequence:
                newest[key] = (sequence, value)
    return newest


def live(entries) -> dict[bytes, bytes]:
    return {key: value for key, (_, value) in entries.items() if value is not None}


def dim_range(dim: int) -> range:
    return {0: range(-4, 20), 1: range(0, 8), 2: range(0, 16)}[dim]


def column_index(key: bytes):
    """(dim, x, z, tag bytes) of a chunk record key, else None."""
    if len(key) in (9, 10):
        x, z = struct.unpack_from('<ii', key)
        return 0, x, z, key[8:]
    if len(key) in (13, 14):
        x, z, dim = struct.unpack_from('<iii', key)
        if dim in (1, 2):
            return dim, x, z, key[12:]
    return None


def layers_of(value: bytes | None) -> list[list[tuple]]:
    """Cells of both layers (XZY order); a missing record or layer is air."""
    layers = [] if value is None else [layer_states(layer) for layer in subchunk(value)]
    while len(layers) < 2:
        layers.append([AIR] * 4096)
    return layers


def describe_layers(value: bytes | None) -> list[str]:
    if value is None:
        return ['no record']
    out = []
    for i, layer in enumerate(subchunk(value)):
        names = [s[0].removeprefix('minecraft:') for s in layer['palette']]
        used = set(layer['indices'])
        unused = [names[j] for j in range(len(names)) if j not in used]
        shown = names if len(names) <= 12 else names[:6] + ['...'] + names[-5:]
        out.append(f'layer {i}: {layer["bits"]} bits, {len(names)} entries {shown}'
                   + (f', unused {unused if len(unused) <= 8 else len(unused)}' if unused else ''))
    return out


def entry_versions(value: bytes) -> collections.Counter:
    """NBT `version` of every palette entry of a record."""
    out = collections.Counter()
    off = 3
    for _ in range(value[1]):
        bits = value[off] >> 1
        off += 1
        n = 1
        if bits:
            off += 4 * WORDS[bits]
            n = struct.unpack_from('<i', value, off)[0]
            off += 4
        for _ in range(n):
            nbt = _Nbt(value, off)
            nbt.take(1)
            nbt.string()
            out[nbt.payload(10)['version'][1]] += 1
            off = nbt.off
    return out


# --- the edit plan -------------------------------------------------------------

def solid_cells(layers, name: str, count: int) -> list[tuple[int, int, int]]:
    """Cells of `name` whose six neighbours inside the section are `name` too."""
    first, out = layers[0], []
    for x in range(1, 15):
        for z in range(1, 15):
            for y in range(1, 15):
                around = [index(x, y, z)] + [index(x + dx, y + dy, z + dz) for dx, dy, dz in (
                    (1, 0, 0), (-1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1), (0, 0, -1))]
                if all(first[i][0] == name for i in around):
                    out.append((x, y, z))
                    if len(out) == count:
                        return out
    return out


def step_plan(out: Path):
    """BDS commands of the edit session; `measure` edits are BDS-only."""
    db = leveldb_read.read_db(out / 'source-world' / 'db')
    p = pool()
    plan: list[dict] = []

    def edit(dim, pos, state, note, measure=False):
        plan.append({'dim': dim, 'pos': list(pos), 'state': state, 'note': note, 'measure': measure})

    edit(0, ow(0, 2, 0, 0), LAPIS, 'order: an entry added in a later session')
    edit(0, ow(2, 3, 0, 0), EMERALD, 'a full 2-bit palette with an unused entry grows')
    edit(0, ow(3, 7, 7, 7), p[10], 'a palette with unused entries, after a reload')
    edit(0, ow(4, 3, 9, 3), SLAB, 'slab set into water')
    edit(0, ow(4, 6, 9, 3), FENCE, 'fence set into water')
    edit(0, ow(4, 6, 3, 3), STONE, 'stone over a waterlogged slab')
    edit(0, ow(4, 3, 12, 3), GLASS, 'glass over still water')
    edit(0, ow(5, 5, 5, 5), STONE, 'stone over the only waterlogged slab')
    plan.append({'dim': 0, 'fill': [list(ow(6, 0, 0, 0)), list(ow(6, 15, 15, 15))], 'state': AIR,
                 'note': 'a section cleared to air', 'measure': False})
    edit(0, ow(10, 1, 16, 0), EMERALD, 'one section of a column with three')
    edit(0, ow(12, 0, -60, 0), GOLD, 'flat ground: a block added')
    edit(0, ow(12, 1, -61, 0), AIR, 'flat ground: grass removed')
    edit(0, ow(13, 0, 48, 0), GOLD, 'a block in a section with no record')
    edit(0, ow(9, 15, 47, 15), GOLD, 'first layer changed, liquid layer with an unused entry')
    edit(0, ow(8, 5, 37, 5), STONE, 'air in both layers, a liquid storage left: stone over the slab')
    plan.append({'dim': 0, 'fill': [list(ow(8, 0, 32, 0)), list(ow(8, 15, 47, 15))], 'state': AIR,
                 'note': 'air in both layers, a liquid storage left: the section cleared', 'measure': False})
    for sy, bits in enumerate(WIDTHS):
        n = (1 << bits) - 1
        edit(0, ow(14, 0, sy * 16 + 8, 0), p[300 + sy], f'full {bits}-bit palette: one more entry')
        for i in sorted({0, n - 1}):
            edit(0, ow(14, i % 16, sy * 16, i // 16), AIR, f'full {bits}-bit palette: a placed block removed')
    edit(0, ow(15, 5, 5, 5), AIR, 'air over the only waterlogged slab', measure=True)

    # Nether: the lines section, and generated netherrack away from lava.
    edit(1, (16 + 5, 48 + 5, 5), GOLD, 'nether: a block into the lines section')
    edit(1, (16 + 8, 48 + 8, 8), AIR, 'nether: a block of the lines section removed')
    for cx, state in ((0, DIAMOND), (2, EMERALD)):
        for sy in range(1, 7):
            value = db.get(chunk_key(1, cx, 0, sy))
            cells = solid_cells(layers_of(value), 'minecraft:netherrack', 2) if value else []
            if len(cells) == 2:
                (x0, y0, z0), (x1, y1, z1) = cells
                edit(1, (cx * 16 + x0, sy * 16 + y0, z0), state, f'nether: generated section {sy}, a block set')
                edit(1, (cx * 16 + x1, sy * 16 + y1, z1), AIR, f'nether: generated section {sy}, a block removed')
                break
        else:
            raise ValueError(f'nether chunk {cx}: no netherrack section')

    # End: zero-bit sections, and a generated surface section.
    zero = sorted(parsed for key, value in db.items()
                  if (parsed := parse_key(key)) and parsed[0] == 2 and value[1] == 1 and value[3] == 0)
    print('zero-bit End sections:', zero)
    by_column = collections.defaultdict(list)
    for _dim, x, z, y in zero:
        by_column[(x, z)].append(y)
    ranked = sorted(by_column, key=lambda c: (-len(by_column[c]), c))
    if len(ranked) < 2:
        raise ValueError(f'too few zero-bit End columns: {dict(by_column)}')
    (ax, az), (bx, bz) = ranked[:2]
    ay, by = by_column[(ax, az)][0], by_column[(bx, bz)][0]
    edit(2, (ax * 16, ay * 16, az * 16), GOLD, 'end: a block into a zero-bit section')
    edit(2, (bx * 16, by * 16, bz * 16), GOLD, 'end: a block into a zero-bit section, then back')
    edit(2, (bx * 16, by * 16, bz * 16), END_STONE, 'end: a block into a zero-bit section, then back')
    surface = None
    for cx in END_AREA:
        for cz in END_AREA:
            for sy in range(16):
                value = db.get(chunk_key(2, cx, cz, sy))
                if not value or value[3] == 0:
                    continue
                layers = layers_of(value)
                if {s[0] for s in layers[0]} != {'minecraft:air', 'minecraft:end_stone'}:
                    continue
                solid = solid_cells(layers, 'minecraft:end_stone', 1)
                if solid:
                    surface = (cx, cz, sy, layers, solid[0])
                    break
            if surface:
                break
        if surface:
            break
    if surface is None:
        raise ValueError('no End surface section')
    cx, cz, sy, layers, (x, y, z) = surface
    air = next((ax_, ay_, az_) for ax_ in range(16) for az_ in range(16) for ay_ in range(16)
               if layers[0][index(ax_, ay_, az_)] == AIR)
    edit(2, (cx * 16 + x, sy * 16 + y, cz * 16 + z), AIR, 'end: generated surface section, end stone removed')
    edit(2, (cx * 16 + air[0], sy * 16 + air[1], cz * 16 + air[2]), DIAMOND,
         'end: generated surface section, a block in the air')
    edit(2, (19, 102, 19), SLAB, 'end: slab set into water')
    edit(2, (22, 99, 19), STONE, 'end: stone over a waterlogged slab')
    (out / 'plan.json').write_text(json.dumps({'plan': plan, 'zero_bit': zero}, indent=1), encoding='utf-8')
    print(len(plan), 'edits')


def load_plan(out: Path) -> list[dict]:
    plan = json.loads((out / 'plan.json').read_text(encoding='utf-8'))['plan']
    for edit in plan:
        name, states = edit['state']
        edit['state'] = name, tuple(tuple(s) for s in states)
    return plan


def plan_cells(edit: dict) -> list[tuple[int, int, int]]:
    if 'fill' in edit:
        (x0, y0, z0), (x1, y1, z1) = edit['fill']
        return [(x, y, z) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1) for z in range(z0, z1 + 1)]
    return [tuple(edit['pos'])]


def plan_column(edit: dict) -> tuple[int, int, int]:
    x, _, z = plan_cells(edit)[0]
    return edit['dim'], x >> 4, z >> 4


def waterloggable(state: tuple) -> bool:
    return state[0].endswith('_slab') or state[0].endswith('_fence')


def apply_plan(source: dict[bytes, bytes], plan: list[dict], include_measure: bool):
    """Edits as /setblock applies them, one cell at a time: a waterloggable block
    set into still water (or over a waterlogged block) keeps the water as the
    second layer; any other block clears it. Returns the edited sections
    {(dim, x, z, y): [layer 0, layer 1]} and the per-cell writes
    (edit number, dim, position, state, liquid)."""
    sections: dict[tuple, list[list[tuple]]] = {}
    writes = []
    for number, edit in enumerate(plan):
        if edit['measure'] and not include_measure:
            continue
        for x, y, z in plan_cells(edit):
            key = (edit['dim'], x >> 4, z >> 4, y >> 4)
            if key not in sections:
                sections[key] = layers_of(source.get(chunk_key(*key)))
            first, liquid = sections[key]
            i = index(x & 15, y & 15, z & 15)
            state = edit['state']
            in_water = first[i] == WATER or liquid[i] == WATER
            liquid[i] = WATER if waterloggable(state) and in_water else AIR
            first[i] = state
            writes.append((number, edit['dim'], (x, y, z), state, liquid[i]))
    return sections, writes


def json_state(state: tuple) -> list:
    name, states = state
    return [name, [list(s) for s in states]]


def plan_commands(plan: list[dict]) -> list[str]:
    return [fill(edit['dim'], *edit['fill'], edit['state']) if 'fill' in edit
            else setblock(edit['dim'], edit['pos'], edit['state']) for edit in plan]


# --- BDS and unastar edits -------------------------------------------------------

def step_bds_edit(out: Path):
    """BDS applies the plan, then the edited chunks unload: BDS's save of a chunk
    it unloads is deterministic, while at shutdown its periodic saver races
    the unload saves (the two write different palettes)."""
    plan = load_plan(out)
    with session(out, 'bds-edit.log', out / 'source-world', out / 'bds-edited-world') as bds:
        con = Console(bds, SYNC)
        con.wait_loaded(columns())
        report = run_logged(con, 'plan', plan_commands(plan))
        for prefix in DIM_PREFIX.values():
            print((con.ask(f'{prefix}tickingarea remove_all', r'icking|ERROR') or ['no answer'])[-1])
        time.sleep(90)
    (out / 'bds-edit.json').write_text(json.dumps(report, indent=1), encoding='utf-8')


def cargo_example(*args: str):
    command = ['cargo', 'run', '--quiet', '-p', 'unastar', '--example', 'subchunk_roundtrip', '--', *args]
    print(' '.join(command))
    subprocess.run(command, cwd=REPO, check=True)


def step_unastar_edit(out: Path):
    source = leveldb_read.read_db(out / 'source-world' / 'db')
    _sections, writes = apply_plan(source, load_plan(out), include_measure=False)
    ops = [{'dim': dim, 'pos': list(pos), 'primary': json_state(state),
            'liquid': json_state(liquid) if liquid != AIR else None} for _, dim, pos, state, liquid in writes]
    (out / 'unastar-ops.json').write_text(json.dumps({'ops': ops}), encoding='utf-8')
    world = out / 'unastar-edited-world'
    snapshot(out / 'source-world', world)
    cargo_example('edit', str(world / 'db'), str(out / 'unastar-ops.json'))


# --- comparison ------------------------------------------------------------------

def step_compare(out: Path):
    source_entries = db_entries(out / 'source-world' / 'db')
    source = live(source_entries)
    bds_entries = db_entries(out / 'bds-edited-world' / 'db')
    bds = live(bds_entries)
    unastar = live(db_entries(out / 'unastar-edited-world' / 'db'))
    plan = load_plan(out)
    source_max = max(sequence for sequence, _ in source_entries.values())
    edited = sorted({plan_column(edit) for edit in plan})
    expected, _ = apply_plan(source, plan, include_measure=True)
    report = {'written': {}, 'columns': {}, 'summary': collections.Counter()}

    print('== records BDS wrote in the edit session (sequence above the source maximum)')
    written = collections.defaultdict(list)
    for key, (sequence, value) in bds_entries.items():
        if sequence > source_max and (parsed := column_index(key)):
            dim, x, z, tag = parsed
            name = f'/{struct.unpack("b", tag[1:2])[0]}' if tag[:1] == b'/' else tag.hex()
            written[(dim, x, z)].append(name + ('' if value is not None else ' deleted'))
    for column, keys in sorted(written.items()):
        print(' ', column, 'edited' if column in edited else 'NOT EDITED', sorted(keys))
        report['written'][str(column)] = sorted(keys)

    print('== edited columns: source, BDS and unastar records')
    versions = collections.Counter()
    for column in edited:
        dim, cx, cz = column
        rows = []
        for y in dim_range(dim):
            key = chunk_key(dim, cx, cz, y)
            src, b, u = source.get(key), bds.get(key), unastar.get(key)
            if src is None and b is None and u is None:
                continue
            for value in (b, u):
                if value is not None:
                    versions.update(entry_versions(value))
            measure = column in MEASURE_ONLY
            if measure:
                kind = 'measure only'
            elif b == u:
                kind = 'bytes equal'
            else:
                kind = 'cells equal' if layers_of(b) == layers_of(u) else 'CELLS DIFFER'
            planned = expected.get((dim, cx, cz, y))
            if planned is not None and not measure and layers_of(u) != planned:
                kind += ', UNASTAR NOT AS PLANNED'
            if planned is not None and layers_of(b) != planned:
                kind += ', bds not as planned'
            report['summary'][kind] += 1
            row = {'y': y, 'kind': kind, 'source': describe_layers(src), 'bds': describe_layers(b),
                   'unastar': None if measure else describe_layers(u)}
            rows.append(row)
            print(f'  {column} y {y}: {kind}')
            for label in ('source', 'bds', 'unastar'):
                if row[label] is not None and (kind != 'bytes equal' or label == 'source'):
                    for line in row[label]:
                        print(f'      {label}: {line}')
            if planned is not None and layers_of(b) != planned:
                got = layers_of(b)
                for layer in range(2):
                    diff = [i for i in range(4096) if got[layer][i] != planned[layer][i]]
                    for i in diff[:4]:
                        print(f'      layer {layer} cell {(i >> 8, i & 15, (i >> 4) & 15)}: planned '
                              f'{cell(planned[layer][i])}, BDS {cell(got[layer][i])}')
        report['columns'][str(column)] = rows
    print('NBT versions of entries in edited records:', dict(versions))

    print('== every unastar sub-chunk record: persistent form, independent decode')
    bad = 0
    for key, value in unastar.items():
        if parse_key(key):
            try:
                subchunk(value)
            except ValueError as error:
                bad += 1
                print('  undecodable', parse_key(key), error)
    print('  undecodable:', bad)

    print('== other records of three edited columns (tag bytes)')
    for column in edited[:3]:
        for label, db in (('source', source), ('bds', bds), ('unastar', unastar)):
            tags = sorted(parsed[3].hex() for key in db if (parsed := column_index(key))
                          and parsed[:3] == column and parsed[3][:1] != b'/')
            print(f'  {column} {label}: {tags}')
    report['summary'] = dict(report['summary'])
    report['versions'] = {str(k): v for k, v in versions.items()}
    report['undecodable_unastar_records'] = bad
    print('summary', report['summary'])
    (out / 'compare.json').write_text(json.dumps(report, indent=1), encoding='utf-8')


# --- BDS reads what unastar wrote ---------------------------------------------------

def thin_sample() -> list[tuple[int, int, int]]:
    return [(x, y, z) for x in (0, 8, 15) for z in (0, 15) for y in range(0, 16, 2)]


def step_bds_verify(out: Path):
    source = leveldb_read.read_db(out / 'source-world' / 'db')
    plan = load_plan(out)
    expected, writes = apply_plan(source, plan, include_measure=False)
    edited_cells = collections.defaultdict(set)
    for _, dim, (x, y, z), _state, _liquid in writes:
        edited_cells[(dim, x >> 4, z >> 4, y >> 4)].add((x & 15, y & 15, z & 15))
    sections = dict(expected)
    for dim, cx, cz in {plan_column(edit) for edit in plan if not edit['measure']}:
        for y in dim_range(dim):
            key = (dim, cx, cz, y)
            if key not in sections and (value := source.get(chunk_key(*key))) is not None:
                sections[key] = layers_of(value)
    results = {'sections': {}, 'asked': 0, 'confirmed': 0, 'edited_asked': 0, 'edited_confirmed': 0}
    with session(out, 'bds-verify.log', out / 'unastar-edited-world', out / 'bds-verified-world') as bds:
        con = Console(bds, SYNC)
        con.wait_loaded(columns())
        for key, layers in sorted(sections.items()):
            dim, cx, cz, sy = key
            edited = edited_cells.get(key, set())
            sample = sample_cells() if key in expected else thin_sample()
            cells = sorted(set(sample) | (edited if len(edited) <= 64 else set()))
            probes = [(x, y, z, layers[0][index(x, y, z)]) for x, y, z in cells]
            answers = testforblock(con, dim, (cx * 16, sy * 16, cz * 16), probes)
            confirmed = {p[:3] for p in probes if answers.get(p[:3], '').startswith('Successfully')}
            e_asked = sum(1 for p in probes if p[:3] in edited)
            e_ok = sum(1 for p in probes if p[:3] in edited and p[:3] in confirmed)
            results['asked'] += len(probes)
            results['confirmed'] += len(confirmed)
            results['edited_asked'] += e_asked
            results['edited_confirmed'] += e_ok
            failed = {str(p[:3]): [cell(p[3]), answers.get(p[:3], 'no answer')]
                      for p in probes if p[:3] not in confirmed}
            results['sections'][str(key)] = {'edited': key in expected, 'asked': len(probes),
                                             'confirmed': len(confirmed), 'edited_asked': e_asked,
                                             'edited_confirmed': e_ok, 'failed': failed}
            print(key, 'edited' if key in expected else 'untouched', 'asked', len(probes), 'confirmed',
                  len(confirmed), f'(edited cells {e_ok}/{e_asked})', f'FAILED {list(failed.items())[:3]}'
                  if failed else '')
        # /testforblock sees the first layer only. A stone block in a glass
        # corner of each section with water in its second layer makes BDS save
        # that section as it read it.
        resave = sorted(key for key, layers in expected.items() if WATER in layers[1])
        run_logged(con, 'resave', [setblock(dim, (cx * 16 + 15, sy * 16 + 15, cz * 16 + 15), STONE)
                                   for dim, cx, cz, sy in resave])
        time.sleep(5)
    after = leveldb_read.read_db(out / 'bds-verified-world' / 'db')
    results['liquid_resave'] = {}
    for key in resave:
        saved = layers_of(after.get(chunk_key(*key)))
        want = [list(layer) for layer in expected[key]]
        want[0][index(15, 15, 15)] = STONE
        result = {'water_cells': want[1].count(WATER), 'layer0_equal': saved[0] == want[0],
                  'layer1_equal': saved[1] == want[1]}
        results['liquid_resave'][str(key)] = result
        print('resaved', key, result)
    print('asked', results['asked'], 'confirmed', results['confirmed'], 'edited cells',
          results['edited_confirmed'], '/', results['edited_asked'])
    (out / 'bds-verify.json').write_text(json.dumps(results, indent=1), encoding='utf-8')


# --- a world unastar generated --------------------------------------------------------

GENERATED = [(0, 0, 0), (0, 1, 0), (0, -1, 2), (1, 0, 0), (1, 1, 1), (2, 0, 0), (2, 1, -1)]


def generated_probes() -> list[list[int]]:
    """Two full-height vertical lines per generated column."""
    probes = []
    for dim, cx, cz in GENERATED:
        bottom, top = (-64, 320) if dim == 0 else (0, 128 if dim == 1 else 256)
        for lx, lz in ((0, 0), (9, 7)):
            probes += [[dim, cx * 16 + lx, y, cz * 16 + lz] for y in range(bottom, top)]
    return probes


def step_generate(out: Path):
    world = out / 'unastar-generated-world'
    if world.exists():
        shutil.rmtree(world)
    world.mkdir(parents=True)
    for name in ('level.dat', 'levelname.txt'):
        shutil.copyfile(out / 'source-world' / name, world / name)
    spec = {'seed': 12345, 'columns': [list(c) for c in GENERATED], 'probes': generated_probes()}
    (out / 'generate.json').write_text(json.dumps(spec), encoding='utf-8')
    cargo_example('generate', str(world / 'db'), str(out / 'generate.json'), str(out / 'generated-cells.json'))


def parse_cell(text: str) -> tuple:
    name, _, states = text.partition('|')
    parsed = []
    for part in filter(None, states.split(',')):
        key, _, value = part.partition('=')
        kind = {'b': 'byte', 'i': 'int', 's': 'string'}[value[0]]
        parsed.append((key, kind, value[1:] if kind == 'string' else int(value[1:])))
    return name, tuple(sorted(parsed))


def step_generated_verify(out: Path):
    cells = json.loads((out / 'generated-cells.json').read_text(encoding='utf-8'))['cells']
    db = leveldb_read.read_db(out / 'unastar-generated-world' / 'db')
    disagree = 0  # the independent decode of unastar's records against unastar's report
    for dim, x, y, z, first, liquid in cells:
        layers = layers_of(db.get(chunk_key(dim, x >> 4, z >> 4, y >> 4)))
        i = index(x & 15, y & 15, z & 15)
        disagree += cell(layers[0][i]) != first or cell(layers[1][i]) != liquid
    print('cells where the independent decode disagrees with unastar:', disagree)
    by_section = collections.defaultdict(list)
    for dim, x, y, z, first, _liquid in cells:
        by_section[(dim, x >> 4, z >> 4, y >> 4)].append((x & 15, y & 15, z & 15, parse_cell(first)))
    results = {'asked': 0, 'confirmed': 0, 'non_air_asked': 0, 'non_air_confirmed': 0,
               'decode_disagreements': disagree, 'sections': {}}
    with session(out, 'generated-verify.log', out / 'unastar-generated-world') as bds:
        con = Console(bds, GENERATED_SYNC)
        for dim, cx, cz in GENERATED:
            print(con.ask(f'{DIM_PREFIX[dim]}tickingarea add {cx * 16} 0 {cz * 16} {cx * 16 + 15} 0 '
                          f'{cz * 16 + 15} g{dim}_{cx}_{cz} true', r'ticking areas in use|ERROR|already')[-1])
        con.wait_loaded(set(GENERATED))
        for key, probes in sorted(by_section.items()):
            dim, cx, cz, sy = key
            answers = testforblock(con, dim, (cx * 16, sy * 16, cz * 16), probes)
            confirmed = {p[:3] for p in probes if answers.get(p[:3], '').startswith('Successfully')}
            results['asked'] += len(probes)
            results['confirmed'] += len(confirmed)
            results['non_air_asked'] += sum(1 for p in probes if p[3] != AIR)
            results['non_air_confirmed'] += sum(1 for p in probes if p[3] != AIR and p[:3] in confirmed)
            failed = {str(p[:3]): [cell(p[3]), answers.get(p[:3], 'no answer')]
                      for p in probes if p[:3] not in confirmed}
            results['sections'][str(key)] = {'asked': len(probes), 'confirmed': len(confirmed), 'failed': failed}
            if failed:
                print(key, 'asked', len(probes), 'confirmed', len(confirmed), list(failed.items())[:3])
    print('asked', results['asked'], 'confirmed', results['confirmed'], 'non-air',
          results['non_air_confirmed'], '/', results['non_air_asked'])
    (out / 'generated-verify.json').write_text(json.dumps(results, indent=1), encoding='utf-8')


# --- fixture ------------------------------------------------------------------

EDITS_FIXTURE = REPO / 'crates' / 'unastar' / 'tests' / 'fixtures' / 'bds_1_26_51_subchunk_edits.json'


def sparse_build(value: bytes) -> list | None:
    """Placements that rebuild a one-layer record from air, when the record
    is a few blocks set into air: palette entry 0 is air, every entry is used
    and few cells are not air. Each entry's cells are placed in palette order,
    so the palette comes out in BDS's order. None otherwise."""
    layers = subchunk(value)
    if len(layers) != 1:
        return None
    layer = layers[0]
    palette, indices = layer['palette'], layer['indices']
    if palette[0] != AIR or set(indices) != set(range(len(palette))):
        return None
    if sum(1 for i in indices if i) > 300:
        return None
    return [(cell_index, entry) for entry in range(1, len(palette))
            for cell_index, i in enumerate(indices) if i == entry]


def step_fixture(out: Path):
    """Write the edit fixture: every section the plan edits (but for the
    measure-only one), BDS's record before the edits, the cell writes and the
    SHA-256 of the record BDS saved when the chunk unloaded. Writes whose cells
    BDS did not end with as planned fail the step."""
    import hashlib
    source = leveldb_read.read_db(out / 'source-world' / 'db')
    bds = leveldb_read.read_db(out / 'bds-edited-world' / 'db')
    plan = load_plan(out)
    expected, writes = apply_plan(source, plan, include_measure=False)
    states: list[str] = []

    def state_index(state: tuple) -> int:
        text = cell(state)
        if text not in states:
            states.append(text)
        return states.index(text)

    sections = []
    for key in sorted(expected):
        dim, cx, cz, sy = key
        before, after = source.get(chunk_key(*key)), bds.get(chunk_key(*key))
        if layers_of(after) != expected[key]:
            raise ValueError(f'{key}: BDS did not save the planned cells')
        entry = {'dimension': dim, 'chunk': [cx, cz], 'y': sy}
        build = sparse_build(before) if before else None
        if before is None:
            entry['before'] = None
        elif build is not None:
            palette = subchunk(before)[0]['palette']
            entry['before_build'] = [[i >> 8, i & 15, (i >> 4) & 15, state_index(palette[e])] for i, e in build]
            entry['before_sha256'] = hashlib.sha256(before).hexdigest()
        else:
            entry['before'] = before.hex()
        entry['writes'] = []
        mine = [w for w in writes if (w[1], w[2][0] >> 4, w[2][2] >> 4, w[2][1] >> 4) == key]
        for number in sorted({w[0] for w in mine}):
            cells = [[x & 15, y & 15, z & 15, state_index(state), state_index(liquid) if liquid != AIR else -1]
                     for n, _dim, (x, y, z), state, liquid in mine if n == number]
            if len(cells) == 4096 and all(c[3:] == cells[0][3:] for c in cells):
                entry['writes'].append({'fill': cells[0][3:]})  # every cell of the section
            else:
                entry['writes'] += cells
        entry['after_sha256'] = hashlib.sha256(after).hexdigest() if after else None
        entry['after_len'] = len(after) if after else 0
        entry['note'] = sorted({e['note'] for e in plan if not e['measure'] and any(
            (e['dim'], x >> 4, z >> 4, y >> 4) == key for x, y, z in plan_cells(e))})
        sections.append(entry)
    data = {
        'source': 'BDS 1.26.51: a flat world saved by BDS (overworld setups, generated nether and End), then '
                  'edited with /setblock and /fill; the edited chunks were unloaded (ticking areas removed) so '
                  'BDS saved them through its unload path',
        'generator': 'tools/bds-sweep/subchunk_roundtrip.py (steps source, plan, bds-edit, fixture)',
        'states': 'cell form name|key=<b|i|s>value,... (keys sorted), indexed by the writes and builds',
        'before': 'BDS record before the edits (hex), or before_build: [x, y, z, state] placements into an '
                  'air section that rebuild it, with its before_sha256; null when the section had no record',
        'writes': '[x, y, z, state, liquid state or -1] in order (both layers of one cell, as /setblock left '
                  'them); {"fill": [state, liquid]} writes every cell of the section',
        'after': 'SHA-256 and length of the record BDS saved when the chunk unloaded; null when BDS deleted it',
        'state_table': states,
        'sections': sections,
    }
    lines_ = ['{'] + [f' {json.dumps(k)}: {json.dumps(v)},' for k, v in data.items() if k != 'sections']
    lines_.append(' "sections": [')
    lines_ += [f"  {json.dumps(s, separators=(',', ':'))}," for s in sections]
    lines_[-1] = lines_[-1].rstrip(',')
    lines_ += [' ]', '}']
    EDITS_FIXTURE.write_text('\n'.join(lines_) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(sections)} sections, {len(states)} states -> {EDITS_FIXTURE} '
          f'({EDITS_FIXTURE.stat().st_size} bytes)')


STEPS = {'source': step_source, 'plan': step_plan, 'bds-edit': step_bds_edit, 'unastar-edit': step_unastar_edit,
         'compare': step_compare, 'bds-verify': step_bds_verify, 'generate': step_generate,
         'generated-verify': step_generated_verify, 'fixture': step_fixture}


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in STEPS:
        print(__doc__)
        return 2
    out = Path(sys.argv[2]).resolve()
    out.mkdir(parents=True, exist_ok=True)
    STEPS[sys.argv[1]](out)
    return 0


if __name__ == '__main__':
    sys.exit(main())
