"""Turn raw sweep results into measured data for `crates/unastar-data`.

    python tools/bds-sweep/summarize.py drops SWEEP_DIR [SWEEP_DIR ...] [repeat=SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py armor-slots SWEEP_DIR
    python tools/bds-sweep/summarize.py fuel SWEEP_DIR
    python tools/bds-sweep/summarize.py compare-drops SWEEP_DIR_A SWEEP_DIR_B
    python tools/bds-sweep/summarize.py scenarios SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py placements SWEEP_DIR
    python tools/bds-sweep/summarize.py circuits SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py random-ticks SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py liquids SWEEP_DIR [SWEEP_DIR ...]

Writes into `crates/unastar-data/data/measured/` and records every artefact
(SHA-256, sweep parameters, sample counts, run times) in `manifest.json`
there. Raw `results.jsonl`/logs stay in the sweep directories (outside the
repository). See that directory's README for the schemas.
"""

from __future__ import annotations

import hashlib
import json
import math
import re
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
MEASURED = REPO / 'crates' / 'unastar-data' / 'data' / 'measured'
BDS_VERSION = '1.26.51'


def results(sweep: Path) -> list[dict]:
    """The rows of the sweep's current job (rows of items a later job no
    longer lists are dropped); every listed item must be finished."""
    rows = [json.loads(line) for line in (sweep / 'results.jsonl').read_text(encoding='utf-8').splitlines()
            if line.strip()]
    job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
    expected = {item['key'] for item in job['items']}
    missing = expected - {row['key'] for row in rows}
    if missing:
        raise SystemExit(f'{sweep}: {len(missing)} items unfinished (rerun sweep.py with --out {sweep})')
    return [row for row in rows if row['key'] in expected]


def runs(sweep: Path) -> list[dict]:
    path = sweep / 'runs.jsonl'
    return [json.loads(line) for line in path.read_text(encoding='utf-8').splitlines() if line.strip()]


def harness_revision() -> str:
    return subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=REPO, capture_output=True, text=True,
                          check=True).stdout.strip()


def write(relative: str, value) -> dict:
    path = MEASURED / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(value, indent=1, sort_keys=True) + '\n'
    path.write_bytes(text.encode('utf-8'))
    return {'path': relative, 'sha256': hashlib.sha256(text.encode('utf-8')).hexdigest()}


def write_cases(relative: str, block: str, cases: list[dict]) -> dict:
    """`{"block": ..., "cases": [...]}` with one compact case per line, so
    diffs stay per case while the files stay small."""
    lines = ',\n'.join(json.dumps(case, separators=(',', ':'), sort_keys=True) for case in cases)
    text = f'{{"block":{json.dumps(block)},"cases":[\n{lines}\n]}}\n'
    path = MEASURED / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode('utf-8'))
    return {'path': relative, 'sha256': hashlib.sha256(text.encode('utf-8')).hexdigest()}


def update_manifest(family: str, entry: dict) -> None:
    path = MEASURED / 'manifest.json'
    manifest = json.loads(path.read_text(encoding='utf-8')) if path.exists() else {'bds_version': BDS_VERSION}
    manifest.setdefault('families', {})[family] = entry
    path.write_bytes((json.dumps(manifest, indent=1, sort_keys=True) + '\n').encode('utf-8'))


def provenance(kind: str, sweeps: list[Path]) -> dict:
    jobs = [json.loads((s / 'job.json').read_text(encoding='utf-8')) for s in sweeps]
    return {
        'source': 'bds_sweep',
        'bds_version': BDS_VERSION,
        'harness': f'tools/bds-sweep (sweep.py {kind}, endstone-sweep plugin) at {harness_revision()}',
        'sweeps': [{'dir': s.name, 'items': len(j['items']), 'runs': runs(s)} for s, j in zip(sweeps, jobs)],
    }


# ---------------------------------------------------------------- drops

def parse_outcome(outcome: str) -> list[tuple[str, int, int]]:
    """`name@data x count;...` → [(name, data, count)]; empty string = nothing."""
    stacks = []
    for part in filter(None, outcome.split(';')):
        name_data, count = part.rsplit('x', 1)
        name, data = name_data.rsplit('@', 1)
        stacks.append((name, int(data), int(count)))
    return stacks


def marginals(outcomes: dict[str, int]) -> dict[tuple[str, int], Counter]:
    """Per (item, data): distribution of the total count over all samples."""
    samples = sum(outcomes.values())
    per_item: dict[tuple[str, int], Counter] = defaultdict(Counter)
    keys = {(n, d) for outcome in outcomes for n, d, _ in parse_outcome(outcome)}
    for outcome, times in outcomes.items():
        totals: Counter = Counter()
        for name, data, count in parse_outcome(outcome):
            totals[(name, data)] += count
        for key in keys:
            per_item[key][totals.get(key, 0)] += times
    for key in keys:
        assert sum(per_item[key].values()) == samples
    return per_item


def loot_table(outcomes: dict[str, int]) -> tuple[dict, str]:
    """Pack loot-table JSON (`pools`/`entries`/`set_count`/`set_data`) fitted to
    one observed outcome histogram, and the fit kind: `exact` when every
    sample was identical, `empirical` otherwise (one pool per item, entry
    weights = observed frequencies of each count, `empty` for zero; items
    are treated as independent)."""
    pools = []
    deterministic = len(outcomes) == 1
    for (name, data), counts in sorted(marginals(outcomes).items()):
        entries = []
        for count, times in sorted(counts.items()):
            if count == 0:
                entries.append({'type': 'empty', 'weight': times})
                continue
            functions = []
            if count != 1:
                functions.append({'function': 'set_count', 'count': count})
            if data:
                functions.append({'function': 'set_data', 'data': data})
            entry = {'type': 'item', 'name': name, 'weight': times}
            if functions:
                entry['functions'] = functions
            entries.append(entry)
        if deterministic:
            for entry in entries:
                entry.pop('weight')
        pools.append({'rolls': 1, 'entries': entries})
    return {'pools': pools}, 'exact' if deterministic else 'empirical'


def merge_states(cases: list[dict]) -> list[dict]:
    """Turns `states`/`placed_states` into lists and folds exact cases that
    differ only in the block state (same tools, samples and outcome) into
    one case listing every such state."""
    out: list[dict] = []
    groups: dict[str, dict] = {}
    for case in cases:
        case = {**case, 'states': [case['states']]}
        if 'placed_states' in case:
            case['placed_states'] = [case['placed_states']]
        if case['fit'] == 'exact' and 'command_failures' not in case:
            key = json.dumps([sorted(case['tools']), case['samples'], case['observed']], sort_keys=True)
            if key in groups:
                groups[key]['states'] += case['states']
                groups[key]['placed_states'] += case['placed_states']
                continue
            groups[key] = case
        out.append(case)
    for case in out:
        # BDS normalises bits to booleans; keep what it stored only where it
        # differs from the request.
        if case.get('placed_states') == case['states']:
            del case['placed_states']
    return out


def summarize_drops(sources: list[Path], repeats: list[Path]) -> None:
    """Merges measurement sweeps by cell key, the first sweep that has a cell
    wins (list the sweep with more samples first, e.g. the 400-sample
    first-state sweep before the all-states sweep). `repeats` are repeat runs
    of the first sweep; their agreement is recorded as `repeatability`."""
    primary = sources[0]
    chosen: dict[str, dict] = {}
    for source in sources:
        for row in results(source):
            chosen.setdefault(row['key'], row)
    by_block: dict[str, list[dict]] = defaultdict(list)
    for row in chosen.values():
        by_block[row['block']].append(row)
    files = []
    for block, block_rows in sorted(by_block.items()):
        cases: list[dict] = []
        merged: dict[str, dict] = {}
        for row in sorted(block_rows, key=lambda r: r['key']):
            tool = row['key'].rsplit('|', 1)[1]
            if row.get('crashed'):
                # Setting this state crashed the server (Endstone access
                # violation in Block.set_data); nothing was measured.
                cases.append({'states': row['states'], 'tools': [tool], 'fit': 'server_crash',
                              'observed': None, 'loot_table': None})
                continue
            placed = row['placed']
            if placed['type'] != row['block']:
                # BDS stores another block for this state (flowing liquids,
                # bubble columns, soul fire without soul soil).
                table, fit = None, 'not_placeable'
            elif not row['unchanged']:
                # The block left the arena during sampling (buttons, frames,
                # wall banners without support pop off); `observed` mixes its
                # own drop with samples taken on air.
                table, fit = None, 'unstable_block'
            else:
                table, fit = loot_table(row['outcomes'])
            case = {
                'states': row['states'],
                'placed_states': placed['states'],
                'tools': [tool],
                'samples': row['samples'],
                'fit': fit,
                'observed': row['outcomes'],
                'loot_table': table,
            }
            if row['command_failures']:
                case['command_failures'] = row['command_failures']
            # Tools with the same exact outcome on the same state share a case.
            if fit == 'exact' and not row['command_failures']:
                group = json.dumps([row['states'], placed['states'], row['samples'], row['outcomes']],
                                   sort_keys=True)
                if group in merged:
                    merged[group]['tools'].append(tool)
                    continue
                merged[group] = case
            cases.append(case)
        name = block.removeprefix('minecraft:')
        files.append(write_cases(f'block_drops/{name}.json', block, merge_states(cases)))
    update_manifest('block_drops', {
        **provenance('drops', sources + repeats),
        'method': '`/loot spawn <p> mine <block> mainhand` executed as the sweep player holding the tool '
                  '(Endstone ItemStack + ItemMeta enchants), item actors collected and removed after each sample',
        'repeatability': [{'repeat': r.name, **compare(primary, r)} for r in repeats],
        'files': files,
    })


def total_variation(a: dict[str, int], b: dict[str, int]) -> float:
    na, nb = sum(a.values()), sum(b.values())
    return 0.5 * sum(abs(a.get(k, 0) / na - b.get(k, 0) / nb) for k in set(a) | set(b))


def compare(first: Path, second: Path) -> dict:
    """Repeatability of two drop sweeps: cells where both runs saw a single
    outcome must be identical; other cells are compared by total variation
    distance against the heuristic bound 2*sqrt(k/n) (k distinct outcomes, n
    samples per run), which sampling noise alone rarely exceeds."""
    a = {row['key']: row for row in results(first)}
    b = {row['key']: row for row in results(second)}
    mismatches = []
    deterministic = random = 0
    worst = (0.0, '')
    for key in sorted(a.keys() & b.keys()):
        oa, ob = a[key]['outcomes'], b[key]['outcomes']
        if len(oa) == 1 and len(ob) == 1:
            deterministic += 1
            if oa != ob:
                mismatches.append(f'deterministic {key}: {oa} vs {ob}')
            continue
        random += 1
        tv = total_variation(oa, ob)
        k = len(set(oa) | set(ob))
        bound = 2 * math.sqrt(k / min(sum(oa.values()), sum(ob.values())))
        if tv > worst[0]:
            worst = (tv, key)
        if tv > bound:
            mismatches.append(f'distribution {key}: tv={tv:.3f} > {bound:.3f}')
    return {'cells': len(a.keys() & b.keys()), 'deterministic_cells': deterministic,
            'random_cells': random, 'mismatches': mismatches,
            'largest_total_variation': {'value': round(worst[0], 4), 'cell': worst[1]}}


def compare_drops(first: Path, second: Path) -> int:
    report = compare(first, second)
    print(json.dumps(report, indent=1))
    return 1 if report['mismatches'] else 0


# ---------------------------------------------------------------- items

def toughness_range(damage: float, per_hit: float, resolution: float, defense: float):
    """Toughness interval consistent with `per_hit` +- `resolution` under
    damage * (1 - (def - damage / (2 + tough / 4)) / 25), or None when the
    measurement cannot be told apart from the clamped def / 5 reduction."""
    clamped = damage * (1 - min(20, defense / 5) / 25)
    if per_hit > clamped - 2 * resolution:
        return None
    bounds = []
    for value in (per_hit - resolution, per_hit + resolution):
        reduction = 25 * (1 - value / damage)
        if defense - reduction <= 0:
            return None
        bounds.append(4 * (damage / (defense - reduction) - 2))
    return [round(min(bounds), 2), round(max(bounds), 2)]


def summarize_armor_values(sweep: Path) -> None:
    """`armor_values.json`: per item the measured health lost per hit for each
    damage amount, `defense` and, where the measurement resolves it, a
    `toughness` interval, both under the armour formula
    damage * (1 - min(20, max(def / 5, def - damage / (2 + tough / 4))) / 25).
    A 1000-point hit is clamped to def / 5, which gives the defence exactly;
    small hits show the toughness only when they land clearly below the
    clamped value (the health read-out resolves 1 / hits per hit)."""
    rows = {row['key']: row for row in results(sweep)}
    baseline = {hit['damage']: hit['per_hit'] for hit in rows['none']['hits']}
    table = {}
    for key, row in sorted(rows.items()):
        if key == 'none':
            continue
        if row['worn'] != row['item']:
            raise SystemExit(f'{key}: slot {row["slot"]} held {row["worn"]}')
        hits = {hit['damage']: (hit['per_hit'], 1 / hit['hits']) for hit in row['hits']}
        defense = round(125 * (1 - hits[1000][0] / baseline[1000]), 2)
        ranges = {str(d): toughness_range(d, per_hit, resolution, defense)
                  for d, (per_hit, resolution) in sorted(hits.items()) if d != 1000}
        table[row['item']] = {
            'slot': row['slot'],
            'defense': defense,
            'toughness_range': {d: r for d, r in ranges.items() if r is not None} or None,
            'per_hit': {str(d): v for d, (v, _) in sorted(hits.items())},
        }
    files = [write('armor_values.json', table)]
    update_manifest('armor_values', {
        **provenance('armor-values', [sweep]),
        'method': 'sweep player (survival, max health 2000, natural regeneration off) wearing one item takes '
                  '`/damage <player> D entity_attack`; health lost summed over repeated hits spaced past the hurt '
                  'cooldown; baseline without armour: ' + json.dumps(baseline),
        'files': files,
    })



def summarize_armor(sweep: Path) -> None:
    table = {}
    for row in results(sweep):
        table[row['item']] = {slot: value['holds'] == row['item'] for slot, value in row['slots'].items()}
    files = [write('armor_slots.json', dict(sorted(table.items())))]
    update_manifest('armor_slots', {
        **provenance('armor-slots', [sweep]),
        'method': '`/replaceitem entity <player> slot.armor.<slot>|slot.weapon.offhand 0 <item>` from the '
                  'console; true when the slot holds the item afterwards (Endstone PlayerInventory)',
        'files': files,
    })


def summarize_fuel(sweep: Path) -> None:
    table = {}
    for row in results(sweep):
        if row['timed_out']:
            raise SystemExit(f'{row["item"]} still burning at max_ticks: rerun with a larger --max-ticks')
        if row['lit']:
            table[row['item']] = row['burn_ticks']
    files = [write('fuel_burn_ticks.json', dict(sorted(table.items())))]
    update_manifest('fuel_burn_ticks', {
        **provenance('fuel', [sweep]),
        'method': 'furnace with 1 cobblestone input and 1 candidate fuel (/replaceitem block); server ticks '
                  'from the first tick the block is lit_furnace to the first tick it is furnace again '
                  '(+/-1 tick); items that do not light it within 40 ticks are omitted',
        'files': files,
    })


def summarize_attack(sweep: Path) -> None:
    """`attack_damage.json`: per held item the damage event value of each
    measured trial's first hit on the target and whether all trials agree
    (the warm-up trial after each held-item change is skipped)."""
    table = {}
    for row in results(sweep):
        measured = [trial for trial in row['trials'] if not trial.get('warmup')]
        firsts = [trial['damage'][0] if trial['damage'] else None for trial in measured]
        table[row['key']] = {
            'tool_item': row['tool'],
            'damage': firsts,
            'consistent': len(set(firsts)) == 1 and firsts[0] is not None,
        }
    files = [write('attack_damage.json', dict(sorted(table.items())))]
    update_manifest('attack_damage', {
        **provenance('attack', [sweep]),
        'method': 'sweep player (creative, on the ground, not sprinting) holding the item attacks a freshly '
                  'spawned minecraft:pig through an InventoryTransaction use-item-on-actor Attack; value of the '
                  'Endstone ActorDamageEvent for the pig, three trials after one warm-up attack',
        'files': files,
    })


def summarize_mining(sweep: Path) -> None:
    table: dict[str, dict] = defaultdict(dict)
    for row in results(sweep):
        state, variant = row['key'].split('|', 1)
        ticks = sorted(t for t in row['ticks'] if t is not None)
        trial = row['trials'][0]
        table[state][variant] = {
            'ticks': row['ticks'],
            'median': ticks[len(ticks) // 2] if ticks else None,
            'tool_item': row['tool'],
            'commands': row['commands'],
            'on_ground': trial['on_ground'],
            'in_water': trial['in_water'],
        }
    files = [write('mining_ticks.json', dict(sorted(table.items())))]
    update_manifest('mining_ticks', {
        **provenance('mining', [sweep]),
        'method': 'sweep player in survival facing the block keeps sending StartDestroyBlock then '
                  'ContinueDestroyBlock (no prediction); BDS breaks the block itself; server ticks from the '
                  'LEFT_CLICK_BLOCK interaction to the BlockBreakEvent, three trials (resolution +/-1 tick)',
        'files': files,
    })


# ---------------------------------------------------------------- scenarios

def write_rows(relative: str, header: dict, rows: list) -> dict:
    """`{header..., "rows": [...]}` with one compact row per line."""
    head = json.dumps(header, separators=(',', ':'), sort_keys=True)[:-1]
    lines = ',\n'.join(json.dumps(row, separators=(',', ':'), sort_keys=True) for row in rows)
    text = f'{head},"rows":[\n{lines}\n]}}\n'
    path = MEASURED / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode('utf-8'))
    return {'path': relative, 'sha256': hashlib.sha256(text.encode('utf-8')).hexdigest(), 'rows': len(rows)}


def scenario_parts(row: dict) -> list:
    return [json.loads(part) for part in row['key'].split('|')[1:]]


SCENARIO_HEADERS = {
    'stairs': {
        'family': 'stairs',
        'row': '[centre_weirdo, centre_upside_down, neighbours, order, corners]',
        'method': 'oak stairs (minecraft:corner none) set with a block update at the cell origin and '
                  'one or two neighbouring stairs ([side, block, weirdo_direction, upside_down_bit]); '
                  'order neighbour_first places the neighbours first, centre_first the centre first; '
                  '`corners` is the stored minecraft:corner of the centre and then of each neighbour, '
                  'read 1 tick after the last placement',
    },
    'connections': {
        'family': 'connections',
        'row': '[neighbour_block, neighbour_states, centres]',
        'method': 'the neighbour (its stored states) is set with a block update one block east of the '
                  'centre, then the centre is set with a block update; `centres` maps each centre block '
                  'to its stored states read 3 ticks later (walls settle a tick after placement); rows '
                  'whose neighbour did not stay in place are left out',
    },
    'wall_above': {
        'family': 'wall_above',
        'row': '[above_block, above_states, walls]',
        'method': 'cobblestone walls: `straight` (walls east and west of the centre), `end` (a wall '
                  'east) and `lone`; the walls are set with block updates, 3 ticks later the block above '
                  'the centre, and the centre is read 3 ticks after that; `walls` maps each shape to '
                  'the centre stored states; rows whose block above did not stay in place are left out',
    },
    'gate_in_wall': {
        'family': 'gate_in_wall',
        'row': '[cardinal_direction, open_bit, side_block, sides, in_wall_bit]',
        'method': 'side blocks set with block updates beside the cell origin, then a fence_gate with '
                  'in_wall_bit false; in_wall_bit read 3 ticks later',
    },
    'surfaces': {
        'family': 'surfaces',
        'row': '[block, states, side, holders]',
        'method': 'the holder (the default state of every block and each further collision shape) is set without a '
                  'block update on the support side of the cell origin, the attached block (`block`, `states`) '
                  'at the origin with one, then stone with one on another side as a poke; `holders` lists '
                  '[holder_block, holder_states, stays] read 2 ticks later (rows whose holder did not stay in '
                  'place are left out)',
    },
    'soils': {
        'family': 'soils',
        'row': '[plant, states, soils]',
        'method': 'the default state of every block is set without a block update below the cell origin, the plant '
                  'at the origin with one, then stone above the plant with one as a poke; `soils` lists '
                  '[soil_block, soil_states, stays] read 2 ticks later (rows whose soil did not stay are left out)',
    },
    'junctions': {
        'family': 'junctions',
        'row': '[above_block, above_states, walls]',
        'method': 'cobblestone walls: `cross` (walls on all four sides of the centre) and `t` (north, east and '
                  'south); the walls are set with block updates, 3 ticks later the block above the centre, '
                  'read 3 ticks after that; `walls` maps each shape to the centre stored states',
    },
    'halves': {
        'family': 'halves',
        'row': '[block, floor, removed, placed, after, items]',
        'method': 'the floor below the cell origin without a block update, the lower and upper half with '
                  'block updates, then the lower half, the upper half or the floor removed with a block update; '
                  '`placed` and `after` are [origin, above] read 2 ticks after each step, `items` the item '
                  'actors that appeared',
    },
    'stairs_blocks': {
        'family': 'stairs_blocks',
        'row': '[block, centre_weirdo, centre_upside_down, side, neighbour_weirdo, neighbour_upside_down, corners]',
        'method': 'every stair block but oak: the neighbour (the same block, minecraft:corner none) set with a block '
                  'update on `side` of the cell origin, then the centre; `corners` is the stored minecraft:corner of '
                  'the centre and the neighbour, read 1 tick later',
    },
    'connections_blocks': {
        'family': 'connections_blocks',
        'row': '[neighbour_block, neighbour_states, centres]',
        'method': 'as `connections`, for every fence, pane, bars and wall block as centre and a curated set of '
                  'neighbours (one block of each connection kind round 1 found, every fence gate and stair '
                  'orientation measured there, every fence, pane, bars and wall); rows whose neighbour did not stay '
                  'are left out',
    },
    'wall_heights_blocks': {
        'family': 'wall_heights_blocks',
        'row': '[wall, above_block, above_states, walls]',
        'method': 'as `wall_above`, for every wall block (its neighbours the same block) under a curated set of '
                  'blocks; `walls` maps each shape to the centre stored states; rows whose block above did not stay '
                  'are left out',
    },
    'multiface_support': {
        'family': 'multiface_support',
        'row': '[block, states, order, placed_states, removals]',
        'method': 'every state of vine, glow_lichen, sculk_vein and resin_clump set with a block update inside six '
                  'stone neighbours set without; the neighbours are removed with block updates one at a time, 2 ticks '
                  'apart (forward: east, west, south, north, up, down; backward the reverse); `removals` lists '
                  '[side removed, block, states] read after each removal',
    },
    'rails': {
        'family': 'rails',
        'row': '[centre, sides, order, reads]',
        'method': 'a rail, powered rail, detector rail or activator rail at the cell origin with, on each side '
                  '(north, east, south, west), no rail, a rail on the floor or a rail on a stone one block up '
                  '(`sides`); `neighbours_first` sets the neighbours (with block updates) before the centre, '
                  '`centre_first` after it; `reads` are [block, states] of the centre and then of each neighbour '
                  'rail in side order, 2 ticks after the last change',
    },
    'soils_more': {
        'family': 'soils_more',
        'row': '[plant, states, soils]',
        'method': 'as `soils`, for the plants round 1 did not sweep, on the default state of every block and on '
                  'water (source, flowing, depth 3) and lava set in the floor; `soils` lists [soil_block, '
                  'soil_states, stays] read 2 ticks later (rows whose soil did not stay are left out)',
    },
    'coral_death': {
        'family': 'coral_death',
        'row': '[kind, cell, repeat, dead_by, last]',
        'method': 'cells 16 blocks apart (`tools/bds-sweep/coral.py` LAYOUT, so water a cell brings reaches no '
                  'other): a live coral block, plant, floor fan or wall fan (on a stone east of it) set with a '
                  'block update in air, or over a water source set in the floor (`*_over_water`), read every 10 '
                  'ticks for 300 ticks; `dead_by` is the first read (ticks) that found it dead (null: alive at '
                  'tick 300), `last` the block read at tick 300',
    },
    'coral_water': {
        'family': 'coral_water',
        'row': '[kind, cell, repeat, before, dead_by, last]',
        'method': 'as `coral_death`: a coral block set with a block update (`block_set`) or by `/setblock` '
                  '(`block_setblock`), set and then a stone set east of it 40 ticks later '
                  '(`block_then_neighbour`), set beside a walled-in water source that is removed 40 ticks later '
                  '(`block_water_goes`) or kept while a stone is set (`block_water_stays`); a coral plant and a '
                  'floor fan set by `/setblock`, alone or with a stone set beside them 40 ticks later; `before` '
                  'is the block read just before that second change, `dead_by` the first read after it (or after '
                  'the set) that found it dead',
    },
    'coral_timing': {
        'family': 'coral_timing',
        'row': '[kind, cell, repeat, trace]',
        'method': 'one read per tick (`timeline`): a coral block set with a block update or by `/setblock` at '
                  'tick 0, placed by a click of the scripted player at tick 0 (one click per wave), a coral block '
                  'whose walled-in water source is removed at tick 0 (the block set 20 ticks before), and a coral '
                  'plant set by `/setblock` with a stone set beside it at tick 0; `trace` is [tick, block] for the '
                  'first read and each change (a read at tick t sees the world after t server ticks)',
    },
    'support': {
        'family': 'support',
        'row': '[block, states, placed_states, forward, backward]',
        'method': 'six stone neighbours set without block updates, the block set with one, then the '
                  'neighbours removed with block updates one at a time, 2 ticks apart: forward in the '
                  'order east, west, south, north, up, down and backward in the reverse order; '
                  'forward/backward = [side whose removal left the cell without the block or null, '
                  'what the cell held then, item actors that appeared in the cell]',
    },
}


def face_covered(boxes: list, x_face: float) -> bool:
    """Whether collision boxes cover the whole square of the block face at
    x = `x_face` (0 or 1); the same check unastar runs."""
    rects = [[max(0.0, min(1.0, v)) for v in (z0, z1, y0, y1)]
             for x0, y0, z0, x1, y1, z1 in boxes if (x0 <= 0.0 if x_face == 0 else x1 >= 1.0)]
    us = sorted({0.0, 1.0} | {r[0] for r in rects} | {r[1] for r in rects})
    vs = sorted({0.0, 1.0} | {r[2] for r in rects} | {r[3] for r in rects})
    return all(any(r[0] <= u0 and u1 <= r[1] and r[2] <= v0 and v1 <= r[3] for r in rects)
               for u0, u1 in zip(us, us[1:]) for v0, v1 in zip(vs, vs[1:]))


CONNECTION_FAMILIES = ('_fence', 'glass_pane', '_bars', '_wall', 'fence_gate', 'border_block')


def connection_faces(family: str, rows: list) -> dict:
    """Blocks a fence, pane, bars or wall treats differently from their
    collision face: `never` connect although the face toward the centre is
    covered, `always` connect although it is not, over the rows just
    summarized and the committed rows of the other connection family. Every
    centre and every measured state of a listed block agree; a block whose
    states differ lists the states that do not follow the rule in
    `never_states` / `always_states`."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import native
    other = 'connections_blocks' if family == 'connections' else 'connections'
    path = MEASURED / 'block_updates' / f'{other}.json'
    committed = json.loads(path.read_text(encoding='utf-8'))['rows'] if path.exists() else []
    shapes = json.loads((REPO / 'crates' / 'unastar-data' / 'output' / 'shapes.json').read_text(encoding='utf-8'))
    ids = {(e['name'], json.dumps(e['states'], sort_keys=True)): rid
           for rid, e in enumerate(native.block_states(typed=True))}
    verdicts: dict[str, set] = defaultdict(set)
    state_verdicts: dict[tuple[str, str], set] = defaultdict(set)
    for block, states, centres in committed + rows:
        if any(part in block for part in CONNECTION_FAMILIES):
            continue
        frozen = json.dumps(states, sort_keys=True)
        boxes = shapes['shapes'][shapes['states'][ids[(block, frozen)]][0]] or []
        # The neighbour stands east of the centre: its west face (x = 0) faces it.
        covered = face_covered(boxes, 0.0)
        for centre, centre_states in centres.items():
            value = centre_states.get('wall_connection_type_east', centre_states.get('minecraft:connection_east'))
            verdict = ('match' if (value not in (False, 'none')) == covered else
                       ('always' if not covered else 'never'))
            verdicts[block].add(verdict)
            state_verdicts[(block, frozen)].add(verdict)
    mixed = {key: sorted(v) for key, v in state_verdicts.items() if len(v) > 1}
    if mixed:
        raise SystemExit(f'connection behaviour differs between centres: {mixed}')
    by_state = {kind: sorted([block, json.loads(frozen)] for (block, frozen), v in state_verdicts.items()
                             if len(verdicts[block]) > 1 and v == {kind})
                for kind in ('never', 'always')}
    return {
        'method': 'derived from block_updates/connections.json and connections_blocks.json: blocks whose '
                  'observed connection differs from the collision-face rule (a fence, pane, bars or wall connects '
                  'to a neighbour whose collision boxes cover the face toward it), for every centre and measured '
                  'state; the *_states lists name single states of blocks whose other states follow the rule',
        'never': sorted(b for b, v in verdicts.items() if v == {'never'}),
        'always': sorted(b for b, v in verdicts.items() if v == {'always'}),
        'never_states': by_state['never'],
        'always_states': by_state['always'],
    }


def face_rects(boxes: list, face: str) -> list:
    """Rectangles `(u0, u1, v0, v1)` of the boxes lying on a block face."""
    out = []
    for x0, y0, z0, x1, y1, z1 in boxes:
        if face == 'up' and y1 >= 1.0 or face == 'down' and y0 <= 0.0:
            out.append((x0, x1, z0, z1))
        elif face == 'west' and x0 <= 0.0 or face == 'east' and x1 >= 1.0:
            out.append((z0, z1, y0, y1))
        elif face == 'north' and z0 <= 0.0 or face == 'south' and z1 >= 1.0:
            out.append((x0, x1, y0, y1))
    return [tuple(max(0.0, min(1.0, v)) for v in r) for r in out]


def rects_cover(rects: list, area: tuple) -> bool:
    u0, u1, v0, v1 = area
    us = sorted({u0, u1} | {min(max(r[0], u0), u1) for r in rects} | {min(max(r[1], u0), u1) for r in rects})
    vs = sorted({v0, v1} | {min(max(r[2], v0), v1) for r in rects} | {min(max(r[3], v0), v1) for r in rects})
    return all(any(r[0] <= a and b <= r[1] and r[2] <= c and d <= r[3] for r in rects)
               for a, b in zip(us, us[1:]) if b > a for c, d in zip(vs, vs[1:]) if d > c)


def falling_blocks() -> set[str]:
    """Blocks with the native `falling` flag (unastar-data's normalized
    protocolgen block table)."""
    text = (REPO / 'crates' / 'unastar-data' / 'output' / 'blocks.kdl').read_text(encoding='utf-8')
    return {m.group(1) for m in re.finditer(r'^block (\S+) .*\\"falling\\":true', text, re.M)}


# Holder rules unastar evaluates on the holder's collision shape, on the face
# toward the attached block.
HOLDER_RULES = {
    'present': lambda block, boxes, face: block != 'minecraft:air',
    'collision': lambda block, boxes, face: bool(boxes),
    'face_full': lambda block, boxes, face: rects_cover(face_rects(boxes, face), (0.0, 1.0, 0.0, 1.0)),
    'face_centre': lambda block, boxes, face: rects_cover(face_rects(boxes, face), (7 / 16, 9 / 16, 7 / 16, 9 / 16)),
}
TOWARD = {'down': 'up', 'up': 'down', 'east': 'west'}


def support_holders(rows: list) -> dict:
    """Per attached block (`block/side`, the support side): the holder rule
    that explains the most of `surfaces.json` and the measured exceptions
    (whole blocks, then single states)."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import native
    shapes = json.loads((REPO / 'crates' / 'unastar-data' / 'output' / 'shapes.json').read_text(encoding='utf-8'))
    ids = {(e['name'], json.dumps(e['states'], sort_keys=True)): rid
           for rid, e in enumerate(native.block_states(typed=True))}
    falling = falling_blocks()
    classes = {}
    for block, states, side, cases in rows:
        face = TOWARD[side]
        # A gravity block as holder varies with its own fall: identical
        # stored states both held and dropped the attached block.
        cases = [case for case in cases if case[0] not in falling]
        best = None
        for rule, predicate in HOLDER_RULES.items():
            by_block: dict[str, list] = defaultdict(list)
            for holder, holder_states, stays in cases:
                rid = ids[(holder, json.dumps(holder_states, sort_keys=True))]
                boxes = shapes['shapes'][shapes['states'][rid][0]] or []
                by_block[holder].append((holder_states, predicate(holder, boxes, face), stays))
            always, never, states_always, states_never = [], [], [], []
            for holder, values in sorted(by_block.items()):
                if all(p == s for _, p, s in values):
                    continue
                if all(s for _, _, s in values):
                    always.append(holder)
                elif not any(s for _, _, s in values):
                    never.append(holder)
                else:
                    for holder_states, p, s in values:
                        if p != s:
                            (states_always if s else states_never).append([holder, holder_states])
            cost = len(always) + len(never) + len(states_always) + len(states_never)
            if best is None or cost < best[0]:
                best = (cost, {'rule': rule, 'always': always, 'never': never,
                               'always_states': states_always, 'never_states': states_never,
                               'cases': len(cases)})
        classes[f'{block}/{side}'] = best[1]
    return {
        'method': 'derived from block_updates/surfaces.json: for each attached block and support side, the '
                  'holder rule (present: any block but air; collision: any collision box; face_full: '
                  'collision covers the face toward the attached block; face_centre: collision covers the '
                  'middle 2/16 square of that face) with the fewest exceptions, then the holders that '
                  'always or never hold it against the rule (every measured state agreeing) and single '
                  'holder states that differ; gravity blocks as holders are left out (they vary with their '
                  'own fall)',
        'classes': classes,
    }


def plant_soils(family: str, rows: list) -> dict:
    """The blocks (default state, or a liquid) each plant stays on, from
    the rows just summarized and the committed rows of the other soil
    family."""
    other = 'soils_more' if family == 'soils' else 'soils'
    path = MEASURED / 'block_updates' / f'{other}.json'
    committed = json.loads(path.read_text(encoding='utf-8'))['rows'] if path.exists() else []
    plants, plant_states = {}, {}
    for plant, _, soils in committed + rows:
        verdicts: dict[str, set] = defaultdict(set)
        for soil, _, stays in soils:
            verdicts[soil].add(stays)
        plants[plant] = sorted(soil for soil, v in verdicts.items() if v == {True})
        mixed = sorted([soil, states] for soil, states, stays in soils if stays and len(verdicts[soil]) > 1)
        if mixed:
            plant_states[plant] = mixed
    return {
        'method': 'derived from block_updates/soils.json and block_updates/soils_more.json: the blocks (default '
                  'state, or a liquid set in the floor) each plant stays on; `plant_states` lists the states a '
                  'plant stays on for soil blocks whose measured states differ (a water source but not flowing '
                  'water)',
        'plants': dict(sorted(plants.items())),
        'plant_states': dict(sorted(plant_states.items())),
    }


def merge_manifest_files(family: str, entry: dict) -> dict:
    """`entry` with the files and sweeps of the committed manifest entry of
    `family` that this summary did not rewrite, so families summarized from
    different sweeps keep their provenance."""
    path = MEASURED / 'manifest.json'
    if not path.exists():
        return entry
    old = json.loads(path.read_text(encoding='utf-8')).get('families', {}).get(family)
    if not old:
        return entry
    written = {f['path'] for f in entry['files']}
    files = [f for f in old.get('files', []) if f['path'] not in written] + entry['files']
    seen = {s['dir'] for s in entry['sweeps']}
    sweeps = [s for s in old.get('sweeps', []) if s['dir'] not in seen] + entry['sweeps']
    harness = old.get('harness')
    harnesses = sorted({h for h in [harness, entry['harness']] if h})
    return {**entry, 'files': sorted(files, key=lambda f: f['path']), 'sweeps': sweeps,
            'harness': '; '.join(harnesses)}


def summarize_scenarios(sweeps: list[Path]) -> None:
    rows = [row for sweep in sweeps for row in results(sweep)]
    by_family: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        by_family[row['key'].split('|', 1)[0]].append(row)
    files = []
    for family, header in SCENARIO_HEADERS.items():
        out = []
        found = by_family.get(family, [])
        if not found:
            continue
        if family == 'stairs':
            for row in found:
                parts = scenario_parts(row)
                if parts[0] in ('one', 'material'):
                    (cw, cu), side, (nw, nu) = parts[1], parts[2], parts[3]
                    block = 'minecraft:oak_stairs' if parts[0] == 'one' else 'minecraft:stone_brick_stairs'
                    neighbours = [[side, block, nw, nu]]
                    order = parts[4] if parts[0] == 'one' else 'neighbour_first'
                else:
                    (cw, cu), a, av, b, bv = parts[1:]
                    neighbours = [[a, 'minecraft:oak_stairs', *av], [b, 'minecraft:oak_stairs', *bv]]
                    order = 'neighbour_first'
                corners = [states['minecraft:corner'] for _, states in row['reads']['after']]
                out.append([cw, cu, neighbours, order, corners])
        elif family == 'connections':
            grouped: dict[str, list] = {}
            for row in found:
                centre, block, _requested = scenario_parts(row)
                (_, centre_states), (neighbour, neighbour_states) = row['reads']['after']
                if neighbour != block:
                    continue
                k = json.dumps([block, neighbour_states], sort_keys=True)
                grouped.setdefault(k, [block, neighbour_states, {}])[2][centre] = centre_states
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:2], sort_keys=True))
        elif family == 'wall_above':
            grouped = {}
            for row in found:
                shape, block, _requested = scenario_parts(row)
                (_, centre_states), (above, above_states) = row['reads']['after']
                if above != block:
                    continue
                k = json.dumps([block, above_states], sort_keys=True)
                grouped.setdefault(k, [block, above_states, {}])[2][shape] = centre_states
            out = sorted((r for r in grouped.values() if len(r[2]) == 3),
                         key=lambda r: json.dumps(r[:2], sort_keys=True))
        elif family == 'gate_in_wall':
            for row in found:
                direction, open_, side_block, sides = scenario_parts(row)
                (_, states), = row['reads']['after']
                out.append([direction, open_, side_block, sides, states['in_wall_bit']])
        elif family == 'surfaces':
            grouped = {}
            for row in found:
                block, states, side, holder, _requested = scenario_parts(row)
                (_, _), (now, holder_states) = row['reads']['after']
                if now != holder:
                    continue
                (here, _), _ = row['reads']['after']
                k = json.dumps([block, states, side], sort_keys=True)
                grouped.setdefault(k, [block, states, side, []])[3].append([holder, holder_states, here == block])
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:3], sort_keys=True))
            for entry in out:
                entry[3].sort(key=lambda case: json.dumps(case, sort_keys=True))
        elif family == 'soils':
            grouped = {}
            for row in found:
                plant, states, soil, _requested = scenario_parts(row)
                (here, _), (now, soil_states) = row['reads']['after']
                if now != soil:
                    continue
                k = json.dumps([plant, states], sort_keys=True)
                grouped.setdefault(k, [plant, states, []])[2].append([soil, soil_states, here == plant])
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:2], sort_keys=True))
            for entry in out:
                entry[2].sort(key=lambda case: json.dumps(case, sort_keys=True))
        elif family == 'junctions':
            grouped = {}
            for row in found:
                shape, block, _requested = scenario_parts(row)
                (_, centre_states), (above, above_states) = row['reads']['after']
                if above != block:
                    continue
                k = json.dumps([block, above_states], sort_keys=True)
                grouped.setdefault(k, [block, above_states, {}])[2][shape] = centre_states
            out = sorted((r for r in grouped.values() if len(r[2]) == 2),
                         key=lambda r: json.dumps(r[:2], sort_keys=True))
        elif family == 'halves':
            for row in found:
                block, floor, removed = scenario_parts(row)
                out.append([block, floor, removed, row['reads']['placed'], row['reads']['after'], row['items']])
            out.sort(key=lambda r: json.dumps(r[:3]))
        elif family == 'support':
            orders = {'forward': ['east', 'west', 'south', 'north', 'up', 'down']}
            orders['backward'] = list(reversed(orders['forward']))
            grouped = {}
            for row in found:
                order, block, states = scenario_parts(row)
                (placed, placed_states), = row['reads']['placed']
                popped = None
                held = None
                for side in orders[order]:
                    (now, _), = row['reads'][f'without_{side}']
                    if now != block:
                        popped, held = side, now
                        break
                if placed != block:
                    popped, held = 'placement', placed
                k = json.dumps([block, states], sort_keys=True)
                entry = grouped.setdefault(k, [block, states, placed_states, None, None])
                entry[3 if order == 'forward' else 4] = [popped, held, row['items']]
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:2], sort_keys=True))
        elif family == 'stairs_blocks':
            for row in found:
                block, (cw, cu), side, (nw, nu) = scenario_parts(row)
                corners = [states['minecraft:corner'] for _, states in row['reads']['after']]
                out.append([block, cw, cu, side, nw, nu, corners])
            out.sort(key=lambda r: json.dumps(r[:6]))
        elif family == 'connections_blocks':
            grouped = {}
            for row in found:
                centre, block, _requested = scenario_parts(row)
                (_, centre_states), (neighbour, neighbour_states) = row['reads']['after']
                if neighbour != block:
                    continue
                k = json.dumps([block, neighbour_states], sort_keys=True)
                grouped.setdefault(k, [block, neighbour_states, {}])[2][centre] = centre_states
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:2], sort_keys=True))
        elif family == 'wall_heights_blocks':
            grouped = {}
            for row in found:
                wall, shape, block, _requested = scenario_parts(row)
                (_, centre_states), (above, above_states) = row['reads']['after']
                if above != block:
                    continue
                k = json.dumps([wall, block, above_states], sort_keys=True)
                grouped.setdefault(k, [wall, block, above_states, {}])[3][shape] = centre_states
            out = sorted((r for r in grouped.values() if len(r[3]) == 3),
                         key=lambda r: json.dumps(r[:3], sort_keys=True))
        elif family == 'multiface_support':
            orders = {'forward': ['east', 'west', 'south', 'north', 'up', 'down']}
            orders['backward'] = list(reversed(orders['forward']))
            for row in found:
                order, block, states = scenario_parts(row)
                (_, placed_states), = row['reads']['placed']
                removals = []
                for side in orders[order]:
                    (now, now_states), = row['reads'][f'without_{side}']
                    removals.append([side, now, now_states])
                out.append([block, states, order, placed_states, removals])
            out.sort(key=lambda r: json.dumps(r[:3], sort_keys=True))
        elif family == 'rails':
            for row in found:
                centre, sides, order = scenario_parts(row)
                out.append([centre, sides, order, row['reads']['after']])
            out.sort(key=lambda r: json.dumps(r[:3]))
        elif family in ('coral_death', 'coral_water'):
            for row in found:
                kind, cell, repeat = scenario_parts(row)
                reads = row['reads']
                dead_by = next((tick for tick in range(10, 310, 10)
                                if reads[f't{tick}'][0][0].startswith('minecraft:dead_')), None)
                last = reads['t300'][0][0]
                if family == 'coral_death':
                    out.append([kind, cell, repeat, dead_by, last])
                else:
                    before = reads['before'][0][0] if 'before' in reads else None
                    out.append([kind, cell, repeat, before, dead_by, last])
            out.sort(key=lambda r: json.dumps(r[:3]))
        elif family == 'coral_timing':
            for row in found:
                kind, cell, repeat = scenario_parts(row)
                trace = [[tick, block] for tick, _, block, _ in row['traces']['trace']]
                out.append([kind, cell, repeat, trace])
            out.sort(key=lambda r: json.dumps(r[:3]))
        elif family == 'soils_more':
            grouped = {}
            for row in found:
                plant, states, soil, requested = scenario_parts(row)
                (here, _), (now, soil_states) = row['reads']['after']
                if now != soil:
                    continue
                k = json.dumps([plant, states], sort_keys=True)
                grouped.setdefault(k, [plant, states, []])[2].append([soil, soil_states, here == plant])
            out = sorted(grouped.values(), key=lambda r: json.dumps(r[:2], sort_keys=True))
            for entry in out:
                entry[2].sort(key=lambda case: json.dumps(case, sort_keys=True))
        files.append(write_rows(f'block_updates/{family}.json', header, out))
        if family in ('connections', 'connections_blocks'):
            files.append(write('block_updates/connection_faces.json', connection_faces(family, out)))
        if family == 'surfaces':
            files.append(write('block_updates/support_holders.json', support_holders(out)))
        if family in ('soils', 'soils_more'):
            files.append(write('block_updates/plant_soils.json', plant_soils(family, out)))
    update_manifest('block_updates', merge_manifest_files('block_updates', {
        **provenance('scenarios', sweeps),
        'method': 'block scenarios (tools/bds-sweep/scenarios.py) run by the endstone-sweep `scenarios` job '
                  'on a superflat world with random ticks off: blocks set through the Endstone block API with '
                  'or without the block update a placement triggers, read back after the stated wait; '
                  'see each file\'s `method`',
        'files': files,
    }))


# ---------------------------------------------------------------- placements

PLACEMENT_DIR = REPO / 'crates' / 'unastar' / 'src' / 'server' / 'game' / 'testdata'


def placement_fixture(group: str) -> Path:
    """Round 1's clicks (group `blocks`) keep their file; every later group
    of `placements.py` has its own."""
    return PLACEMENT_DIR / ('bds-placements.json' if group == 'blocks' else f'bds-placements-{group}.json')


def block_hashes() -> dict[int, tuple[str, dict]]:
    """Network hash -> (block, states) of every canonical state, from
    unastar-data's `output/blocks.kdl` (byte properties as booleans, the
    Endstone form the other measured files use)."""
    out: dict[int, tuple[str, dict]] = {}
    block = None
    text = (REPO / 'crates' / 'unastar-data' / 'output' / 'blocks.kdl').read_text(encoding='utf-8')
    row = re.compile(r'network_hash=(\d+) properties_json="((?:[^"\\]|\\.)*)"')
    for line in text.splitlines():
        if line.startswith('block '):
            block = line.split()[1]
        elif line.startswith('    state ') and block:
            match = row.search(line)
            if not match:
                continue
            states = {}
            for name, typed in json.loads(match.group(2).replace('\\"', '"')).items():
                kind, value = next(iter(typed.items()))
                states[name] = bool(value) if kind == 'byte' else value
            out[int(match.group(1))] = (block, states)
    return out


def _slot_zero(hex_text: str) -> tuple[int, int]:
    """(item network id, block runtime id) of slot 0 of a main-inventory
    `InventoryContent` (packet id, container 0, slot count, then the first
    descriptor: i16 id, u16 count, varuint aux, optional zigzag net id,
    varuint block runtime id)."""
    data = bytes.fromhex(hex_text)
    pos = 0

    def uvarint() -> int:
        nonlocal pos
        value = shift = 0
        while True:
            byte = data[pos]
            pos += 1
            value |= (byte & 0x7F) << shift
            if not byte & 0x80:
                return value
            shift += 7

    uvarint()  # packet id
    if uvarint() != 0:
        raise ValueError('not the main inventory')
    uvarint()  # slot count
    item = int.from_bytes(data[pos:pos + 2], 'little', signed=True)
    pos += 4  # id, count
    uvarint()  # aux
    present = data[pos]
    pos += 1
    if present:
        uvarint()  # zigzag stack net id
    return item, uvarint()


def summarize_item_blocks(sweep: Path) -> None:
    """The `item-blocks` sweep: per item the block BDS describes a stack of
    it with (null for an item that is no block item), from the client's
    capture of the inventory BDS sends after each `/replaceitem`."""
    rows = {row['key']: row for row in results(sweep)}
    hashes = block_hashes()
    seen: dict[str, tuple[int, int]] = {}
    current = None
    for line in (sweep / 'client-capture' / 'packets.jsonl').read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c':
            continue
        if record['name'] == 'TextPacket':
            text = bytes.fromhex(record['hex']).decode('utf-8', 'replace')
            if 'bds-sweep:mark ' in text:
                marker = text.split('bds-sweep:mark ', 1)[1]
                current = ''.join(ch for ch in marker if ch.isalnum() or ch in '_.')
                continue
        if current and record['name'] == 'InventoryContentPacket' and record['hex'][2:4] == '00':
            seen[current] = _slot_zero(record['hex'])
    table, refused = {}, []
    for key, row in sorted(rows.items()):
        if not row['given']:
            refused.append(row['item'])
            continue
        if key not in seen:
            raise SystemExit(f'{row["item"]}: given but no inventory captured')
        _, block = seen[key]
        if block == 0:
            table[row['item']] = None
            continue
        name, states = hashes[block]
        table[row['item']] = {'block': name, 'states': states}
    files = [write('item_blocks.json', {'items': table, 'not_given': refused})]
    update_manifest('item_blocks', {
        **provenance('item-blocks', [sweep]),
        'method': '`/replaceitem entity <player> slot.hotbar 0 <item> 1` from the console for every item of the '
                  'item registry; the block runtime id of hotbar slot 0 in the next main-inventory '
                  '`InventoryContent` the scripted client received (null: no block); `not_given` lists the '
                  'items the command refuses',
        'files': files,
    })
    blocks = sum(1 for value in table.values() if value)
    print(f'{len(table)} items ({blocks} with a block), {len(refused)} not given')
# Packets of the client capture that are not answers to the click.
PLACEMENT_NOISE = {'PlayerAuthInputPacket', 'LevelChunkPacket', 'NetworkStackLatencyPacket', 'SetTimePacket',
                   'NetworkChunkPublisherUpdatePacket', 'MoveActorDeltaPacket', 'CurrentStructureFeaturePacket',
                   'SyncWorldClocksPacket', 'TextPacket'}


# Packets the placement test decodes keep their bytes; the others (the full
# player resync BDS sends after a click: attributes, actor data, the armour,
# UI and offhand windows) are listed by name only.
PLACEMENT_DECODED = {'InventoryTransactionPacket', 'UpdateBlockPacket', 'UpdateSubChunkBlocksPacket',
                     'LevelSoundEventPacket', 'BlockActorDataPacket', 'OpenSignPacket', 'PlayerHotbarPacket',
                     'LevelEventPacket', 'BlockEventPacket', 'AnimatePacket'}
# Groups whose cases also keep the player's attributes (hunger after eating).
PLACEMENT_ATTRIBUTE_GROUPS = {'leftovers'}


def placement_decoded(record: dict, attributes: bool = False) -> bool:
    if attributes and record['name'] == 'UpdateAttributesPacket':
        return True
    if record['name'] == 'InventoryContentPacket':
        # Packet id 0x31, then the container id: 0 is the main inventory.
        return record['hex'][2:4] == '00'
    return record['name'] in PLACEMENT_DECODED


def capture_by_marker(path: Path, attributes: frozenset = frozenset()) -> dict[str, list]:
    """A client capture cut at the `bds-sweep:mark <key>` / `bds-sweep:mark
    end` chat markers: key -> the packets in between (noise left out, hex
    kept for the packets the test decodes, and for `UpdateAttributes` in the
    cases whose key is in `attributes`)."""
    packets: dict[str, list] = {}
    current = None
    if not path.exists():
        return packets
    for line in path.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['name'] == 'TextPacket' and record['dir'] == 's2c':
            text = bytes.fromhex(record['hex']).decode('utf-8', 'replace')
            if 'bds-sweep:mark ' in text:
                marker = text.split('bds-sweep:mark ', 1)[1]
                key = ''.join(ch for ch in marker if ch.isalnum() or ch in '_-')
                current = None if key == 'end' else key
                if current:
                    packets[current] = []
                continue
        if current and record['name'] not in PLACEMENT_NOISE:
            packet = {'dir': record['dir'], 'name': record['name']}
            if placement_decoded(record, current in attributes):
                packet['hex'] = record['hex']
            packets[current].append(packet)
    return packets


# Case fields copied from the job item into the fixture.
PLACEMENT_FIELDS = ('key', 'setup', 'hand', 'player', 'click', 'read', 'before', 'after', 'read_later', 'watcher',
                    'watcher_hand')
# What the plugin recorded per case.
PLACEMENT_RESULTS = ('reads', 'held', 'items', 'timeline', 'read_tick', 'reads_later', 'read_later_tick',
                     'items_later')


def summarize_placements(sweep: Path) -> None:
    """The `placements` sweep as unastar test fixtures, one file per case
    group: each case's setup and steps, what BDS stored and every packet
    between the case's marker and its end marker (noise left out, hex as
    recorded for the packets the test decodes), for the clicking player and,
    when the sweep had one, the watching player."""
    rows = {row['key']: row for row in results(sweep)}
    job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
    attributes = frozenset(item['key'] for item in job['items']
                           if item.get('group') in PLACEMENT_ATTRIBUTE_GROUPS)
    clicker = capture_by_marker(sweep / 'client-capture' / 'packets.jsonl', attributes)
    watcher = capture_by_marker(sweep / 'watcher-capture' / 'packets.jsonl')
    arena = job['arena']['placements']
    groups: dict[str, list] = defaultdict(list)
    for item in job['items']:
        row = rows[item['key']]
        case = {k: item[k] for k in PLACEMENT_FIELDS if k in item}
        case['origin'] = arena['origin']
        case['bds'] = {k: row[k] for k in PLACEMENT_RESULTS if k in row}
        case['packets'] = clicker.get(item['key'], [])
        if job.get('watcher'):
            case.setdefault('watcher', arena['watcher'])
            case['watcher_packets'] = watcher.get(item['key'], [])
        groups[item.get('group', 'blocks')].append(case)
    for group, cases in groups.items():
        fixture = {
            'source': f'BDS {BDS_VERSION}, tools/bds-sweep placements job at {harness_revision()}, '
                      f'sweep {sweep.name}, group {group}: the scripted player (survival) clicks with the stack '
                      'in hotbar slot 0; packets are the client capture between the case marker and its end '
                      'marker' + ('; watcher_packets are the capture of a second scripted player standing '
                                  'outside the arena' if job.get('watcher') else ''),
            'cases': cases,
        }
        path = placement_fixture(group)
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes((json.dumps(fixture, indent=1) + '\n').encode('utf-8'))
        print(f'{len(cases)} cases -> {path}')


# ---------------------------------------------------------------- circuits

CIRCUIT_METHOD = (
    'sweep.py scenarios --family <redstone family> (tools/bds-sweep/circuits.py): each circuit is built in a '
    'cell of its own with block updates (`build`), left `wait` ticks, then run for a `timeline` of N server '
    'ticks in which `events` ([tick, event]) are applied at their tick and every `watch` offset is read once '
    'per tick after them. `trace` lists [tick, watch index, block, states] for the first read and every '
    'change. `witness` is the tick (2 or 3) at which a dust beside a redstone block set at tick 0 outside '
    'the cells lit in the same wave: BDS works redstone every other tick, and an update shows at the reads '
    'whose tick has the parity of `witness`. Offsets are relative to the cell origin on the superflat grass '
    '(the floor is y=-1); `minecraft:` is left out of block names.'
)
CIRCUIT_ROW = '[name, witness, steps, trace]'


def short(name: str) -> str:
    return name.removeprefix('minecraft:')


def short_steps(steps: list) -> list:
    """The job's steps with `minecraft:` left out of block names."""
    def event(e: list) -> list:
        if e[0] == 'set':
            return ['set', e[1], short(e[2]), e[3], e[4]]
        if e[0] == 'build':
            return ['build', [[o, short(b), st, ph] for o, b, st, ph in e[1]]]
        if e[0] == 'setblock':
            return ['setblock', e[1], short(e[2]), e[3], e[4]]
        if e[0] == 'drop':
            return ['drop', e[1], short(e[2]), e[3]]
        return e
    out = []
    for step in steps:
        if step[0] == 'timeline':
            out.append(['timeline', step[1], [[t, event(e)] for t, e in step[3]], step[4]])
        elif step[0] == 'wait':
            out.append(step)
        else:
            out.append(event(step))
    return out


def circuit_blocks(rows_by_family: dict[str, list]) -> dict:
    """Facts the server reads at runtime, derived from the per-block
    families: blocks a repeater powers through (`conductors`), blocks that
    cut dust climbing past them (`cuts`) and, per measured block state,
    whether power climbs onto dust on top of it and comes down its far side
    (`climb`)."""
    conductors, cuts, climb = set(), set(), []
    for name, witness, steps, trace in rows_by_family.get('redstone_conductors', []):
        block = 'minecraft:' + name[1]
        first = {k: b for t, k, b, st in trace if t == 0}
        lit = [t for t, k, b, st in trace if k == 3 and t > 0 and b == 'lit_redstone_lamp']
        powered = [t for t, k, b, st in trace if k == 1 and b == 'powered_repeater']
        # The block must still be there, the lamp dark before the repeater
        # powered it and lit in the same update (a block that reacts to the
        # power on its own, such as a sculk sensor, lights it later).
        if lit and powered and lit[0] == powered[0] and first.get(3) == 'redstone_lamp' \
                and first.get(2) == short(block) and block != 'minecraft:redstone_wire':
            conductors.add(block)
    for name, witness, steps, trace in rows_by_family.get('redstone_dust_cut', []):
        block = 'minecraft:' + name[1]
        first = {k: b for t, k, b, st in trace if t == 0}
        # Cut: the dust on the stone stays dark in the update the dust below
        # lights (a door or plate set there may pop off later and let the
        # power climb).
        lit = [t for t, k, b, st in trace if k == 1 and st.get('redstone_signal', 0) > 0]
        top_lit = [t for t, k, b, st in trace if k == 2 and st.get('redstone_signal', 0) > 0]
        if first.get(3) == short(block) and lit and not (top_lit and top_lit[0] <= lit[0]):
            cuts.add(block)
    for name, witness, steps, trace in rows_by_family.get('redstone_dust_blocks', []):
        block, states = 'minecraft:' + name[1], name[2]

        # Ever lit: the dust on top may pop later, when the power it
        # carries changes the block under it (a trapdoor opening).
        def lit(k):
            return any(kk == k and b == 'redstone_wire' and st.get('redstone_signal', 0) > 0
                       for t, kk, b, st in trace)
        climb.append([block, states, lit(3), lit(4)])
    climb.sort(key=lambda r: (r[0], json.dumps(r[1], sort_keys=True)))
    return {'method': 'derived from redstone_conductors.json, redstone_dust_cut.json and '
                      'redstone_dust_blocks.json by summarize.py circuits',
            'conductors': sorted(conductors), 'cuts': sorted(cuts),
            'row': '[block, states, power climbs onto dust on top, power comes down the far side]',
            'rows': climb}


def summarize_circuits(sweeps: list[Path]) -> None:
    """Redstone circuit families to `redstone/<family>.json` (one row per
    line) and the derived `redstone/blocks.json`."""
    by_family: dict[str, list] = defaultdict(list)
    for sweep in sweeps:
        job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
        items = {item['key']: item for item in job['items']}
        for row in results(sweep):
            family, rest = row['key'].split('|', 1)
            if not family.startswith('redstone_'):
                continue
            witness = row['timing']['witness_tick']
            if witness not in (2, 3):
                raise SystemExit(f'{sweep}: {row["key"]}: no witness tick ({witness})')
            trace = [[t, k, short(b), st] for t, k, b, st in row['traces']['trace']]
            name = json.loads(rest)
            # Block names inside a row name are shortened like the steps.
            name = [short(p) if isinstance(p, str) else p for p in name]
            by_family[family].append([name, witness, short_steps(items[row['key']]['steps']), trace])
    files = []
    for family, rows in sorted(by_family.items()):
        rows.sort(key=lambda r: json.dumps(r[0], sort_keys=True))
        files.append(write_rows(f'redstone/{family}.json',
                                {'family': family, 'method': CIRCUIT_METHOD, 'row': CIRCUIT_ROW}, rows))
    derived = circuit_blocks(by_family)
    if derived['conductors'] or derived['rows']:
        rows = derived.pop('rows')
        files.append(write_rows('redstone/blocks.json', derived, rows))
    entry = {**provenance('scenarios', sweeps), 'files': files,
             'method': 'redstone circuits replayed tick by tick: ' + CIRCUIT_METHOD}
    update_manifest('redstone', merge_manifest_files('redstone', entry))
    for f in files:
        print(f['path'], f.get('rows', ''), f['sha256'][:12])


# ---------------------------------------------------------------- random ticks

RANDOM_METHOD = (
    'sweep.py scenarios --family <random family> (tools/bds-sweep/random_ticks.py): a 16 x 16 field of '
    'identical cells is laid out on the superflat floor without block updates (`build`, offsets from the '
    'field origin, the floor at y=-1), `randomtickspeed` is set to `speed`, and every `watch` position is '
    'read every `read_every` ticks; `reads` holds, per read, how many watched positions held each '
    '[block, states] (`minecraft:` left out). The replay test lays out the same field in unastar with a '
    'seeded random source and compares the histograms with a chi-square test.'
)
RANDOM_ROW = '[name, speed, read_every, build, watch, reads]'


def summarize_random_ticks(sweeps: list[Path]) -> None:
    """Random tick fields to `random_ticks/<family>.json` (one row per
    line)."""
    by_family: dict[str, list] = defaultdict(list)
    for sweep in sweeps:
        job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
        items = {item['key']: item for item in job['items']}
        for row in results(sweep):
            family, rest = row['key'].split('|', 1)
            if not family.startswith('random_'):
                continue
            steps = items[row['key']]['steps']
            build = [[o, short(b), st] for o, b, st, physics in steps[0][1]]
            speed = int(steps[1][1].split()[-1])
            waits = [s[1] for s in steps if s[0] == 'wait']
            watch = next(s[2] for s in steps if s[0] == 'read')
            reads = []
            for index in range(1, len(waits) + 1):
                counts = Counter(json.dumps([short(b), st], sort_keys=True)
                                 for b, st in row['reads'][f'r{index}'])
                reads.append([[json.loads(label), n] for label, n in sorted(counts.items())])
            name = [short(p) if isinstance(p, str) else p for p in json.loads(rest)]
            by_family[family].append([name, speed, waits[0], build, watch, reads])
    files = []
    for family, rows in sorted(by_family.items()):
        rows.sort(key=lambda r: json.dumps(r[0], sort_keys=True))
        files.append(write_rows(f'random_ticks/{family}.json',
                                {'family': family, 'method': RANDOM_METHOD, 'row': RANDOM_ROW}, rows))
    entry = {**provenance('scenarios', sweeps), 'files': files,
             'method': 'random tick fields compared by distribution: ' + RANDOM_METHOD}
    update_manifest('random_ticks', merge_manifest_files('random_ticks', entry))
    for f in files:
        print(f['path'], f.get('rows', ''), f['sha256'][:12])


# ---------------------------------------------------------------- liquids

LIQUID_METHOD = (
    'sweep.py scenarios --family <liquid family> (tools/bds-sweep/liquids.py): each cell is laid out on the '
    'superflat floor (the floor at y=-1) at `base`; `steps` are the job steps (offsets from `base`; `start` sets '
    'a liquid without a block update and a stone on it with one, since BDS leaves a liquid set by a command '
    'still until a neighbour changes); in a `timeline` every `watch` position is read once per tick after the '
    "tick's events and `trace` keeps [tick, index, block, states] for the first read and each "
    'change (the read at tick t shows the world after game tick t - 1). `packets` are the UpdateBlock (`block`) '
    'and UpdateSubChunkBlocks (`subchunk`) packets the scripted player was sent in each tick, per entry '
    '[dx, dy, dz, layer, flags, block, states] relative to `base`, in packet and entry order.'
)
LIQUID_ROW = '[name, base, steps, trace, packets]'
LIQUID_TRACE_FAMILIES = ('liquids_pilot', 'liquids_order', 'liquids_order2', 'liquids_flow', 'liquids_logged',
                         'liquids_chunks', 'liquids_nether', 'liquids_shapes')


def _uvarint(data: bytes, i: int) -> tuple[int, int]:
    value = shift = 0
    while True:
        byte = data[i]
        i += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            return value, i


def _zigzag(value: int) -> int:
    return (value >> 1) ^ -(value & 1)


def tick_packets(capture: Path, hashes: dict) -> dict[tuple[str, int], list]:
    """(wave, tick) -> the block packets the player was sent after the
    `bds-sweep:tick <wave> <tick>` marker and before the next one:
    [kind, [[x, y, z, layer, flags, block, states], ...]]."""
    out: dict[tuple[str, int], list] = defaultdict(list)
    current = None
    if not capture.exists():
        return out
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c':
            continue
        data = bytes.fromhex(record['hex'])
        if record['name'] == 'TextPacket':
            text = data.decode('utf-8', 'replace')
            if 'bds-sweep:tick ' in text:
                wave, tick = text.split('bds-sweep:tick ', 1)[1].strip('\x00 ').split()[:2]
                tick = ''.join(ch for ch in tick if ch.isalnum())
                current = None if tick == 'end' else (wave, int(tick))
            continue
        if current is None or record['name'] not in ('UpdateBlockPacket', 'UpdateSubChunkBlocksPacket'):
            continue
        i = _uvarint(data, 0)[1]
        entries = []
        if record['name'] == 'UpdateBlockPacket':
            x, i = _uvarint(data, i)
            y, i = _uvarint(data, i)
            z, i = _uvarint(data, i)
            rid, i = _uvarint(data, i)
            flags, i = _uvarint(data, i)
            layer, i = _uvarint(data, i)
            block, states = hashes.get(rid, (f'#{rid}', {}))
            entries.append([_zigzag(x), _zigzag(y), _zigzag(z), layer, flags, short(block), states])
            kind = 'block'
        else:
            for _ in range(3):
                i = _uvarint(data, i)[1]
            for layer in (0, 1):
                count, i = _uvarint(data, i)
                for _ in range(count):
                    x, i = _uvarint(data, i)
                    y, i = _uvarint(data, i)
                    z, i = _uvarint(data, i)
                    rid, i = _uvarint(data, i)
                    flags, i = _uvarint(data, i)
                    i = _uvarint(data, i)[1]
                    i = _uvarint(data, i)[1]
                    block, states = hashes.get(rid, (f'#{rid}', {}))
                    entries.append([_zigzag(x), _zigzag(y), _zigzag(z), layer, flags, short(block), states])
            kind = 'subchunk'
        out[current].append([kind, entries])
    return out


def liquid_cell_packets(packets: dict, wave: str, base: list, half: int, ticks: int) -> list:
    """The packets of one cell: [tick, kind, entries] for every packet with
    an entry within `half` blocks (x and z) of `base`, entries outside the
    cell left out, positions relative to `base`."""
    out = []
    for tick in range(ticks):
        for kind, entries in packets.get((wave, tick), []):
            mine = [[x - base[0], y - base[1], z - base[2], *rest] for x, y, z, *rest in entries
                    if abs(x - base[0]) <= half and abs(z - base[2]) <= half]
            if mine:
                out.append([tick, kind, mine])
    return out


def liquid_trace(row: dict) -> list:
    return [[t, k, short(b), st] for t, k, b, st in row['traces']['trace']]


def liquid_block_table(water: list[dict], lava: list[dict], holders: list[dict], logged_water: dict) -> dict:
    """Per block (first palette state): what spreading water and lava do to
    it (`breaks` with what it drops, `fills` its liquid layer, `blocks`, or
    `becomes` another block), and whether it keeps water it is set into and
    through which sides that water spreads (`holds`, -x +x -z +z)."""
    rows: dict[str, dict] = defaultdict(dict)
    for liquid, cells in (('water', water), ('lava', lava)):
        for row in cells:
            name = json.loads(row['key'].split('|', 1)[1])
            if row.get('crashed'):
                rows[short(name)][liquid] = 'crashed'
                continue
            target = [x for x in row['traces']['trace'] if x[1] == 1]
            final = target[-1]
            if final[2] in ('minecraft:water', 'minecraft:lava'):
                outcome = {'breaks': sorted(short(item) for item, _data, count in row['items'] for _ in range(count))}
            elif liquid == 'water' and short(name) in logged_water:
                outcome = 'fills'
            elif len(target) > 1 and short(final[2]) != short(name):
                outcome = {'becomes': short(final[2])}
            else:
                outcome = 'blocks'
            rows[short(name)][liquid] = outcome
    for row in holders:
        name = short(json.loads(row['key'].split('|', 1)[1]))
        if row.get('crashed'):
            rows[name]['holds'] = 'crashed'
            continue
        trace = row['traces']['trace']
        here = [x for x in trace if x[1] == 0]
        final = short(here[-1][2])
        sides = [any(x[1] == k and x[2] == 'minecraft:water' for x in trace) for k in (1, 2, 3, 4)]
        if final == name and any(sides):
            rows[name]['holds'] = {'sides': sides}
        elif final == 'water':
            # Popped once set (no support) and left the water as a source.
            rows[name]['holds'] = 'popped'
        else:
            rows[name]['holds'] = 'no'
    return {name: rows[name] for name in sorted(rows)}


def summarize_liquids(sweeps: list[Path]) -> None:
    """Liquid sweeps to `liquids/<family>.json` (trace families, one row per
    line, with the packets of each tick) and `liquids/blocks.json` (the
    per-block table from `liquid_blocks_water`, `liquid_blocks_lava` and
    `liquid_holders`)."""
    hashes = block_hashes()
    by_family: dict[str, list] = defaultdict(list)
    tables: dict[str, list] = defaultdict(list)
    logged_water: set[str] = set()
    for sweep in sweeps:
        job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
        items = {item['key']: item for item in job['items']}
        arena = job['arena']['scenarios']
        spacing = arena['spacing']
        half = (spacing if isinstance(spacing, int) else min(spacing)) // 2
        packets = tick_packets(sweep / 'client-capture' / 'packets.jsonl', hashes)
        rows = results(sweep)
        # Older rows lack `base`: a wave's rows are recorded in cell order.
        in_wave: Counter = Counter()
        for row in rows:
            if 'base' not in row and 'timing' in row:
                index = in_wave[row['timing']['start_tick']]
                in_wave[row['timing']['start_tick']] += 1
                columns, origin = arena['columns'], arena['origin']
                space_x, space_z = (spacing, spacing) if isinstance(spacing, int) else spacing
                row['base'] = [origin[0] + (index % columns) * space_x, origin[1],
                               origin[2] + (index // columns) * space_z]
        for row in rows:
            family, rest = row['key'].split('|', 1)
            if family in ('liquid_blocks_water', 'liquid_blocks_lava', 'liquid_holders'):
                tables[family].append(row)
                if family == 'liquid_blocks_water' and not row.get('crashed'):
                    # Water spreading into the block's liquid layer: a layer-1
                    # water entry at the block (1 east of the cell) after tick 0.
                    cell = liquid_cell_packets(packets, str(row['wave']), row['base'], half, 12)
                    if any(layer == 1 and (dx, dy, dz) == (1, 0, 0) and block == 'water' and tick > 0
                           for tick, _, entries in cell for dx, dy, dz, layer, _f, block, _s in entries):
                        logged_water.add(short(json.loads(rest)))
                continue
            if family not in LIQUID_TRACE_FAMILIES:
                continue
            steps = items[row['key']]['steps']
            ticks = next(step[1] for step in steps if step[0] == 'timeline')
            cell = liquid_cell_packets(packets, str(row.get('wave')), row['base'], half, ticks) \
                if 'wave' in row else []
            name = [short(p) if isinstance(p, str) else p for p in json.loads(f'[{rest.replace("|", ",")}]')]
            by_family[family].append([name, row['base'], short_steps(steps), liquid_trace(row), cell])
    files = []
    for family, rows in sorted(by_family.items()):
        rows.sort(key=lambda r: json.dumps(r[0], sort_keys=True))
        header = {'family': family, 'method': LIQUID_METHOD, 'row': LIQUID_ROW}
        files.append(write_rows(f'liquids/{family}.json', header, rows))
    if tables:
        table = liquid_block_table(tables['liquid_blocks_water'], tables['liquid_blocks_lava'],
                                   tables['liquid_holders'], logged_water)
        header = {'method': (
            'Per block, its first palette state set without a block update: `water` and `lava` say what a '
            'source woken beside it (the cell walled in with stone) did to it within 12 (water) or 40 (lava) '
            'ticks: `{"breaks": [items it dropped]}` (the liquid took its place), `"fills"` (water entered its '
            'liquid layer: a layer-1 UpdateBlock), `"blocks"` (nothing reached it), `{"becomes": block}` (it '
            'turned into another block) or `"crashed"` (setting it crashed the harness). `holds` says whether it '
            'kept a still water source it was set into with a block update: `{"sides": [-x, +x, -z, +z]}` '
            '(the sides that water spread through once woken), `"popped"` (the block broke on its own and left '
            'the source), `"no"` or `"crashed"`.'),
            'row': '[block, {water, lava, holds}]'}
        files.append(write_rows('liquids/blocks.json', header, [[name, value] for name, value in table.items()]))
    entry = {**provenance('scenarios', sweeps), 'files': files,
             'method': 'liquid flow replayed tick by tick: ' + LIQUID_METHOD}
    update_manifest('liquids', merge_manifest_files('liquids', entry))
    for f in files:
        print(f['path'], f.get('rows', ''), f['sha256'][:12])


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    kind, dirs = sys.argv[1], [Path(arg).resolve() for arg in sys.argv[2:]]
    if kind == 'drops':
        args = sys.argv[2:]
        summarize_drops([Path(a).resolve() for a in args if not a.startswith('repeat=')],
                        [Path(a.removeprefix('repeat=')).resolve() for a in args if a.startswith('repeat=')])
    elif kind == 'compare-drops':
        return compare_drops(*dirs)
    elif kind == 'armor-slots':
        summarize_armor(dirs[0])
    elif kind == 'armor-values':
        summarize_armor_values(dirs[0])
    elif kind == 'fuel':
        summarize_fuel(dirs[0])
    elif kind == 'mining':
        summarize_mining(dirs[0])
    elif kind == 'attack':
        summarize_attack(dirs[0])
    elif kind == 'scenarios':
        summarize_scenarios(dirs)
    elif kind == 'circuits':
        summarize_circuits(dirs)
    elif kind == 'random-ticks':
        summarize_random_ticks(dirs)
    elif kind == 'liquids':
        summarize_liquids(dirs)
    elif kind == 'item-blocks':
        summarize_item_blocks(dirs[0])
    elif kind == 'placements':
        summarize_placements(dirs[0])
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
