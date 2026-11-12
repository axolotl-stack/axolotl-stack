"""Turn raw sweep results into measured data for `crates/unastar-data`.

    python tools/bds-sweep/summarize.py drops SWEEP_DIR [SWEEP_DIR ...] [repeat=SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py armor-slots SWEEP_DIR
    python tools/bds-sweep/summarize.py armor-wear SWEEP_DIR
    python tools/bds-sweep/summarize.py fuel SWEEP_DIR
    python tools/bds-sweep/summarize.py mining SWEEP_DIR [SWEEP_DIR ...]   # first sweep wins per cell
    python tools/bds-sweep/summarize.py attack SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py compare-drops SWEEP_DIR_A SWEEP_DIR_B
    python tools/bds-sweep/summarize.py scenarios SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py placements SWEEP_DIR

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



def summarize_armor_wear(sweep: Path) -> None:
    """`armor_wear.json`: per armour item its slot and the durability damage
    one `/damage <player> D entity_attack` hit added to the worn piece, per D."""
    table = {}
    for row in results(sweep):
        table[row['item']] = {'slot': row['slot'],
                              'wear': {str(hit['damage']): hit['wear'] for hit in row['hits']}}
    files = [write('armor_wear.json', dict(sorted(table.items())))]
    update_manifest('armor_wear', {
        **provenance('armor-wear', [sweep]),
        'method': 'sweep player (survival, max health 2000, natural regeneration off) wearing one breakable piece '
                  '(the slot armor_slots.json assigns) takes one `/damage <player> D entity_attack` per D, spaced past '
                  'the hurt cooldown; the piece is read back through Endstone before and after every hit',
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


def wear_per_trial(before: dict | None, trials: list[dict]) -> list[int | None]:
    """Durability damage each trial added to the held stack (`held_before`,
    then each trial's `held_after`; `None` for a trial after which the stack
    was gone, i.e. it broke)."""
    out = []
    previous = (before or {}).get('damage', 0)
    for trial in trials:
        after = trial.get('held_after')
        if after is None:
            out.append(None)
            continue
        out.append(after['damage'] - previous)
        previous = after['damage']
    return out


def merged_results(sweeps: list[Path]) -> list[dict]:
    """Rows of several sweeps by key; the first sweep that has a key wins."""
    chosen: dict[str, dict] = {}
    for sweep in sweeps:
        for row in results(sweep):
            chosen.setdefault(row['key'], row)
    return list(chosen.values())


def summarize_attack(sweeps: list[Path]) -> None:
    """`attack_damage.json`: per held item the damage event value of each
    measured trial's first hit on the target and whether all trials agree
    (the warm-up trial after each held-item change is skipped). Rows of a
    survival sweep also record `wear`, the durability damage each trial
    (warm-up included, first) added to the held stack."""
    table = {}
    for row in merged_results(sweeps):
        measured = [trial for trial in row['trials'] if not trial.get('warmup')]
        firsts = [trial['damage'][0] if trial['damage'] else None for trial in measured]
        entry = {
            'tool_item': row['tool'],
            'damage': firsts,
            'consistent': len(set(firsts)) == 1 and firsts[0] is not None,
        }
        if 'held_before' in row:
            entry['gamemode'] = row.get('gamemode', 'creative')
            if row['tool']:
                entry['wear'] = wear_per_trial(row['held_before'], row['trials'])
        table[row['key']] = entry
    files = [write('attack_damage.json', dict(sorted(table.items())))]
    update_manifest('attack_damage', {
        **provenance('attack', sweeps),
        'method': 'sweep player (creative, or survival where `gamemode` says so; on the ground, not sprinting) '
                  'holding the item attacks a freshly spawned minecraft:pig through an InventoryTransaction '
                  'use-item-on-actor Attack; value of the Endstone ActorDamageEvent for the pig, three trials '
                  'after one warm-up attack; `wear`: change of the durability damage of the held stack per trial '
                  '(warm-up first), read back through Endstone',
        'files': files,
    })


def summarize_mining(sweeps: list[Path]) -> None:
    """`mining_ticks.json` from one or more mining sweeps (tool slice, held
    items, effect levels); the first sweep that has a cell wins. Variants of
    sweeps that read the held stack back also record `wear`, the durability
    damage each trial added."""
    table: dict[str, dict] = defaultdict(dict)
    for row in merged_results(sweeps):
        state, variant = row['key'].split('|', 1)
        if 'stack_error' in row:
            table[state][variant] = {'tool_item': row['tool'], 'stack_error': row['stack_error']}
            continue
        ticks = sorted(t for t in row['ticks'] if t is not None)
        trial = row['trials'][0]
        entry = {
            'ticks': row['ticks'],
            'median': ticks[len(ticks) // 2] if ticks else None,
            'tool_item': row['tool'],
            'commands': row['commands'],
            'on_ground': trial['on_ground'],
            'in_water': trial['in_water'],
        }
        if 'held_before' in row and row['tool']:
            entry['wear'] = wear_per_trial(row['held_before'], row['trials'])
        table[state][variant] = entry
    files = [write('mining_ticks.json', dict(sorted(table.items())))]
    update_manifest('mining_ticks', {
        **provenance('mining', sweeps),
        'method': 'sweep player in survival facing the block keeps sending StartDestroyBlock then '
                  'ContinueDestroyBlock (no prediction); BDS breaks the block itself; server ticks from the '
                  'LEFT_CLICK_BLOCK interaction to the BlockBreakEvent (resolution +/-1 tick); three trials per '
                  'tool-slice, effect and empty-hand cell, one per held-item cell; `wear`: change of the durability '
                  'damage of the held stack per trial, read back through Endstone',
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


def connection_faces(rows: list) -> dict:
    """Blocks a fence, pane, bars or wall treats differently from their
    collision face: `never` connect although the face toward the centre is
    covered, `always` connect although it is not. Every centre and every
    measured state of the block must agree."""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import native
    shapes = json.loads((REPO / 'crates' / 'unastar-data' / 'output' / 'shapes.json').read_text(encoding='utf-8'))
    ids = {(e['name'], json.dumps(e['states'], sort_keys=True)): rid
           for rid, e in enumerate(native.block_states(typed=True))}
    verdicts: dict[str, set] = defaultdict(set)
    for block, states, centres in rows:
        if any(part in block for part in CONNECTION_FAMILIES):
            continue
        boxes = shapes['shapes'][shapes['states'][ids[(block, json.dumps(states, sort_keys=True))]][0]] or []
        # The neighbour stands east of the centre: its west face (x = 0) faces it.
        covered = face_covered(boxes, 0.0)
        for centre, centre_states in centres.items():
            value = centre_states.get('wall_connection_type_east', centre_states.get('minecraft:connection_east'))
            verdicts[block].add('match' if (value not in (False, 'none')) == covered else
                                ('always' if not covered else 'never'))
    mixed = {block: sorted(v) for block, v in verdicts.items() if len(v) > 1}
    if mixed:
        raise SystemExit(f'connection behaviour differs between states or centres: {mixed}')
    return {
        'method': 'derived from block_updates/connections.json: blocks whose observed connection differs '
                  'from the collision-face rule (a fence, pane, bars or wall connects to a neighbour whose '
                  'collision boxes cover the face toward it), for every centre and measured state',
        'never': sorted(b for b, v in verdicts.items() if v == {'never'}),
        'always': sorted(b for b, v in verdicts.items() if v == {'always'}),
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
        files.append(write_rows(f'block_updates/{family}.json', header, out))
        if family == 'connections':
            files.append(write('block_updates/connection_faces.json', connection_faces(out)))
        if family == 'surfaces':
            files.append(write('block_updates/support_holders.json', support_holders(out)))
        if family == 'soils':
            files.append(write('block_updates/plant_soils.json', {
                'method': 'derived from block_updates/soils.json: the blocks (default state) each plant stays on',
                'plants': {plant: sorted(soil for soil, _, stays in soils if stays) for plant, _, soils in out},
            }))
    update_manifest('block_updates', {
        **provenance('scenarios', sweeps),
        'method': 'block scenarios (tools/bds-sweep/scenarios.py) run by the endstone-sweep `scenarios` job '
                  'on a superflat world with random ticks off: blocks set through the Endstone block API with '
                  'or without the block update a placement triggers, read back after the stated wait; '
                  'see each file\'s `method`',
        'files': files,
    })


# ---------------------------------------------------------------- placements

PLACEMENT_FIXTURE = REPO / 'crates' / 'unastar' / 'src' / 'server' / 'game' / 'testdata' / 'bds-placements.json'


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
                     'LevelSoundEventPacket', 'BlockActorDataPacket', 'OpenSignPacket', 'PlayerHotbarPacket'}


def placement_decoded(record: dict) -> bool:
    if record['name'] == 'InventoryContentPacket':
        # Packet id 0x31, then the container id: 0 is the main inventory.
        return record['hex'][2:4] == '00'
    return record['name'] in PLACEMENT_DECODED


def summarize_placements(sweep: Path) -> None:
    """The `placements` sweep as a unastar test fixture: each case's setup,
    the client's click, what BDS stored and every packet between the case's
    marker and its end marker (noise left out), hex as recorded for the
    packets the test decodes."""
    rows = {row['key']: row for row in results(sweep)}
    job = json.loads((sweep / 'job.json').read_text(encoding='utf-8'))
    packets: dict[str, list] = {}
    current = None
    for line in (sweep / 'client-capture' / 'packets.jsonl').read_text(encoding='utf-8').splitlines():
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
            if placement_decoded(record):
                packet['hex'] = record['hex']
            packets[current].append(packet)
    cases = []
    for item in job['items']:
        row = rows[item['key']]
        cases.append({**{k: item[k] for k in ('key', 'setup', 'hand', 'player', 'click', 'read')},
                      'origin': job['arena']['placements']['origin'],
                      'bds': {'reads': row['reads'], 'held': row['held']},
                      'packets': packets.get(item['key'], [])})
    fixture = {
        'source': f'BDS {BDS_VERSION}, tools/bds-sweep placements job at {harness_revision()}, '
                  f'sweep {sweep.name}: the scripted player (survival) clicks with the stack in hotbar slot 0; '
                  'packets are the client capture between the case marker and its end marker',
        'cases': cases,
    }
    PLACEMENT_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    PLACEMENT_FIXTURE.write_bytes((json.dumps(fixture, indent=1) + '\n').encode('utf-8'))
    print(f'{len(cases)} cases -> {PLACEMENT_FIXTURE}')


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
    elif kind == 'armor-wear':
        summarize_armor_wear(dirs[0])
    elif kind == 'fuel':
        summarize_fuel(dirs[0])
    elif kind == 'mining':
        summarize_mining(dirs)
    elif kind == 'attack':
        summarize_attack(dirs)
    elif kind == 'scenarios':
        summarize_scenarios(dirs)
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
