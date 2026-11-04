"""Turn raw sweep results into measured data for `crates/unastar-data`.

    python tools/bds-sweep/summarize.py drops SWEEP_DIR [SWEEP_DIR ...] [repeat=SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py armor-slots SWEEP_DIR
    python tools/bds-sweep/summarize.py fuel SWEEP_DIR
    python tools/bds-sweep/summarize.py blocks SWEEP_DIR
    python tools/bds-sweep/summarize.py redstone SWEEP_DIR
    python tools/bds-sweep/summarize.py utility SWEEP_DIR [RERUN_DIR ...]
    python tools/bds-sweep/summarize.py compare-drops SWEEP_DIR_A SWEEP_DIR_B

Writes into `crates/unastar-data/data/measured/` and records every artefact
(SHA-256, sweep parameters, sample counts, run times) in `manifest.json`
there. Raw `results.jsonl`/logs stay in the sweep directories (outside the
repository). See that directory's README for the schemas. `blocks`,
`redstone` and `utility` instead write the observed-results fixtures
`crates/unastar/src/server/game/block_behaviour/testdata/bds_1_26_51_blocks.json`,
`bds_1_26_51_redstone.json` and `bds_1_26_51_utility.json` next to it (from
`results.jsonl` and the packets the client logged in `client.log`).
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


# ---------------------------------------------------------------- blocks

BLOCKS_FIXTURE = (REPO / 'crates' / 'unastar' / 'src' / 'server' / 'game' / 'block_behaviour' / 'testdata'
                  / 'bds_1_26_51_blocks.json')
# Level event ids seen in the blocks job, with their usual protocol names.
LEVEL_EVENTS = {1002: 'SoundLaunch', 1005: 'SoundFuse', 2000: 'ParticlesShoot', 2001: 'ParticlesDestroyBlock',
                2005: 'ParticlesCropGrowth', 2025: 'ParticlesExplosion', 2026: 'ParticlesBlockExplosion',
                3506: 'CauldronFillWater', 3507: 'CauldronTakeWater', 3512: 'CauldronFillLava',
                3513: 'CauldronTakeLava', 3600: 'StartBlockCracking', 3601: 'StopBlockCracking',
                3602: 'UpdateBlockCracking', 3603: 'ParticlesCrackBlockDown', 3604: 'ParticlesCrackBlockUp',
                3605: 'ParticlesCrackBlockNorth', 3606: 'ParticlesCrackBlockSouth',
                3607: 'ParticlesCrackBlockWest', 3608: 'ParticlesCrackBlockEast', 9801: 'SleepingPlayers'}
_VEC = r'Vec3 \{ x: (\S+), y: (\S+), z: (\S+) \}'
_TRACE = {
    'LevelSoundEventPacket': re.compile(rf'sound_event: "([^"]*)", position: {_VEC}, data: (-?\d+), '
                                        r'actor_identifier: "([^"]*)", is_baby: (\w+), is_global: (\w+)'),
    'LevelEventPacket': re.compile(rf'event_id: (-?\d+), position: {_VEC}, data: (-?\d+)'),
    'UpdateBlockPacket': re.compile(r'block_position: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, '
                                    r'block_runtime_id: (\d+), flags: (\d+), layer: (\d+)'),
    'AddItemActorPacket': re.compile(r'item: CerealizerNetworkItemStackDescriptorSerializedData \{ id: (-?\d+), '
                                     r'stacksize: (\d+)'),
    'BlockEventPacket': re.compile(r'block_position: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, '
                                   r'event_type: (-?\d+), event_value: (-?\d+)'),
}
_SUB_CHUNK_BLOCK = re.compile(r'pos: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, runtime_id: (\d+), '
                              r'update_flags: (\d+)')
_MARK = re.compile(r'"bds-sweep:mark (\S+)"')
_TRACE_LINE = re.compile(r' trace (\w+)\((.*)\)$')


def block_text(state: dict) -> str:
    states = ','.join(f'{k}={json.dumps(v)}' for k, v in sorted(state['states'].items()))
    return f'{state["name"]}[{states}]' if states else state['name']


def trace_packets(client_log: Path, names: dict[int, str], detail: UtilityTrace | None = None) -> dict[str, dict]:
    """Server packets the client logged after each `SCENARIO/STEP` mark,
    decoded and grouped: identical packets (same kind, position and data)
    become one entry with a `times` count, in first-seen order. Runtime ids
    that match a block state seen in the job are written as that state.
    `detail` decodes the actor, block actor, text, spawn and attribute
    packets the utility scenarios trace."""
    steps: dict[str, dict] = {}
    current = None

    def block(runtime_id: int):
        return names.get(runtime_id & 0xFFFFFFFF, runtime_id)

    def vec(match, start):
        return [round(float(match.group(i)), 3) for i in range(start, start + 3)]

    for line in client_log.read_text(encoding='utf-8', errors='replace').splitlines():
        if 'plugin message' in line and (mark := _MARK.search(line)):
            current = steps.setdefault(mark.group(1), {})
            continue
        trace = _TRACE_LINE.search(line)
        if current is None or trace is None:
            continue
        kind, body = trace.groups()
        match = _TRACE[kind].search(body) if kind in _TRACE else None
        detailed = detail.entries(kind, body) if detail is not None else None
        entries = detailed or []
        if detailed is not None:
            pass
        elif kind == 'LevelSoundEventPacket' and match:
            entry = {'sound': match.group(1), 'position': vec(match, 2), 'data': block(int(match.group(5)))}
            if match.group(6):
                entry['actor'] = match.group(6)
            entries.append(('sounds', entry))
        elif kind == 'LevelEventPacket' and match:
            event = int(match.group(1))
            entries.append(('level_events', {
                'event': event, 'name': LEVEL_EVENTS.get(event), 'position': vec(match, 2),
                'data': block(int(match.group(5))) if event == 2001 else int(match.group(5))}))
        elif kind == 'UpdateBlockPacket' and match:
            entry = {'position': [int(match.group(i)) for i in (1, 2, 3)], 'block': block(int(match.group(4)))}
            if (int(match.group(5)), int(match.group(6))) != (3, 0):
                entry.update(flags=int(match.group(5)), layer=int(match.group(6)))
            entries.append(('block_updates', entry))
        elif kind == 'UpdateSubChunkBlocksPacket':
            standard, _, extra = body.partition('blocks_changed_extras')
            for layer, part in ((0, standard), (1, extra)):
                for found in _SUB_CHUNK_BLOCK.finditer(part):
                    entry = {'position': [int(found.group(i)) for i in (1, 2, 3)],
                             'block': block(int(found.group(4))), 'via': 'UpdateSubChunkBlocks'}
                    if (int(found.group(5)), layer) != (3, 0):
                        entry.update(flags=int(found.group(5)), layer=layer)
                    entries.append(('block_updates', entry))
        elif kind == 'AddItemActorPacket' and match:
            entries.append(('item_actors', {'item_network_id': int(match.group(1)), 'count': int(match.group(2))}))
        elif kind == 'BlockEventPacket' and match:
            entries.append(('block_events', {'position': [int(match.group(i)) for i in (1, 2, 3)],
                                             'event_type': int(match.group(4)), 'event_value': int(match.group(5))}))
        else:
            entries.append(('other', {'packet': kind, 'debug': body[:300]}))
        for group, entry in entries:
            bucket = current.setdefault(group, [])
            for existing in bucket:
                if {k: v for k, v in existing.items() if k != 'times'} == entry:
                    existing['times'] = existing.get('times', 1) + 1
                    break
            else:
                bucket.append(entry)
    return steps


def rust_bytes(text: str) -> bytes:
    """The bytes of the body of a Rust byte-string literal as `{:?}` prints
    it (`\\xNN`, `\\n`, `\\t`, `\\r`, `\\0`, `\\\\`, `\\"`, `\\'`)."""
    raw = text.encode('utf-8')
    out = bytearray()
    i = 0
    while i < len(raw):
        if raw[i] == 0x5C:
            escape = raw[i + 1]
            if escape == ord('x'):
                out.append(int(raw[i + 2:i + 4], 16))
                i += 4
                continue
            out.append({ord('n'): 10, ord('t'): 9, ord('r'): 13, ord('0'): 0}.get(escape, escape))
            i += 2
            continue
        out.append(raw[i])
        i += 1
    return bytes(out)


class NetworkNbt:
    """Network (little-endian, varint) NBT reader."""

    def __init__(self, data: bytes) -> None:
        self.data = data
        self.pos = 0

    def byte(self) -> int:
        self.pos += 1
        return self.data[self.pos - 1]

    def unsigned(self) -> int:
        value = shift = 0
        while True:
            byte = self.byte()
            value |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                return value

    def signed(self) -> int:
        value = self.unsigned()
        return (value >> 1) ^ -(value & 1)

    def take(self, n: int) -> bytes:
        self.pos += n
        return self.data[self.pos - n:self.pos]

    def string(self) -> str:
        return self.take(self.unsigned()).decode('utf-8', 'replace')

    def payload(self, tag: int):
        import struct
        if tag == 1:
            return struct.unpack('<b', self.take(1))[0]
        if tag == 2:
            return struct.unpack('<h', self.take(2))[0]
        if tag in (3, 4):
            return self.signed()
        if tag == 5:
            return round(struct.unpack('<f', self.take(4))[0], 4)
        if tag == 6:
            return round(struct.unpack('<d', self.take(8))[0], 6)
        if tag == 7:
            return list(self.take(self.signed()))
        if tag == 8:
            return self.string()
        if tag == 9:
            inner = self.byte()
            return [self.payload(inner) for _ in range(self.signed())]
        if tag == 10:
            return self.compound()
        if tag in (11, 12):
            return [self.signed() for _ in range(self.signed())]
        raise ValueError(f'NBT tag {tag}')

    def compound(self) -> dict:
        out = {}
        while self.pos < len(self.data):
            tag = self.byte()
            if tag == 0:
                break
            name = self.string()
            out[name] = self.payload(tag)
        return out

    def root(self) -> dict:
        tag = self.byte()
        self.string()
        return self.payload(tag)


_ACTOR_IDS = r'target_actor_id: ActorUniqueId \{ actor_unique_id: (-?\d+) \}, target_runtime_id: ActorRuntimeId \{ actor_runtime_id: (\d+) \}'
_DETAIL = {
    'AddActorPacket': re.compile(rf'{_ACTOR_IDS}, actor_type: "([^"]*)", position: {_VEC}, velocity: {_VEC}'),
    'AddItemActorPacket': re.compile(rf'{_ACTOR_IDS}, item: CerealizerNetworkItemStackDescriptorSerializedData '
                                     rf'\{{ id: (-?\d+), stacksize: (\d+), auxvalue: (\d+).*?position: {_VEC}, '
                                     rf'velocity: {_VEC}'),
    'RemoveActorPacket': re.compile(r'actor_unique_id: (-?\d+)'),
    'BlockActorDataPacket': re.compile(r'block_position: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, '
                                       r'actor_data_tags: Nbt\(b"(.*)"\) \}$'),
    'LevelEventGenericPacket': re.compile(r'event_id: (-?\d+), __ctd__: NbtCompoundBody\(b"(.*)"\) \}$'),
    'ActorEventPacket': re.compile(r'actor_runtime_id: (\d+) \}, event_id: (\w+), data: (-?\d+)'),
    'SetActorDataPacket': re.compile(r'actor_runtime_id: (\d+) \}'),
    'SetActorMotionPacket': re.compile(rf'actor_runtime_id: (\d+) \}}, motion: {_VEC}'),
    'TextPacket': re.compile(r'message_type: (\w+), message: "((?:[^"\\]|\\.)*)"(?:, parameter_list: \[(.*?)\])?'),
    'SetSpawnPositionPacket': re.compile(
        r'spawn_position_type: (\w+), block_position: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, '
        r'dimensiontype: DimensionType \{ value: (\d+) \}, spawn_block_pos: BlockPos \{ x: (-?\d+), y: (-?\d+), '
        r'z: (-?\d+) \}'),
    'PlaySoundPacket': re.compile(r'name: "([^"]*)", position: BlockPos \{ x: (-?\d+), y: (-?\d+), z: (-?\d+) \}, '
                                  r'volume: ([^,]*), pitch: ([^,]*)'),
}
_DATA_ITEM = re.compile(r'DataItemEntry \{ id: (\d+), payload: \w+\(\w+ \{ type_: \w+, value: ([^{}]*?) \}\) \}')
_ATTRIBUTE = re.compile(r'AttributeData \{ min_value: [^,]*, max_value: [^,]*, current_value: ([^,]*), [^}]*?'
                        r'name: "minecraft:player\.(hunger|saturation)"')
# Actor data ids kept for non-player actors: flags and the TNT fuse.
ACTOR_DATA_IDS = {'0': 'flags', '55': 'fuse'}


def _rust_str(text: str) -> str:
    return re.sub(r'\\(.)', lambda m: {'n': '\n', 't': '\t'}.get(m.group(1), m.group(1)), text)


class UtilityTrace:
    """Decodes the packets the utility scenarios trace; remembers the
    actors the log added so actor packets name them and the player's own
    actor data stays out."""

    def __init__(self, items: dict[int, str]) -> None:
        self.items = items
        self.runtime: dict[int, str] = {}
        self.unique: dict[int, str] = {}
        self.hunger: dict[str, float] = {}

    def entries(self, kind: str, body: str) -> list | None:
        pattern = _DETAIL.get(kind)
        if kind == 'UpdateAttributesPacket':
            # Only changes: the player's attributes are resent often.
            values = {name: float(value) for value, name in _ATTRIBUTE.findall(body)
                      if self.hunger.get(name) != float(value)}
            self.hunger.update(values)
            return [('hunger', values)] if values else []
        if pattern is None:
            return None
        match = pattern.search(body)
        if match is None:
            return [('other', {'packet': kind, 'debug': body[:300]})]
        g = match.group

        def vec(start: int) -> list[float]:
            return [round(float(g(i)), 4) for i in range(start, start + 3)]

        if kind == 'AddActorPacket':
            self.runtime[int(g(2))] = self.unique[int(g(1))] = g(3)
            entry = {'actor': g(3), 'runtime_id': int(g(2)), 'position': vec(4), 'velocity': vec(7)}
            data = {ACTOR_DATA_IDS[i]: v for i, v in _DATA_ITEM.findall(body) if i in ACTOR_DATA_IDS}
            if data.get('fuse', '-1') != '-1':
                entry['fuse'] = int(data['fuse'])
            return [('add_actor', entry)]
        if kind == 'AddItemActorPacket':
            item = self.items.get(int(g(3)), int(g(3)))
            self.runtime[int(g(2))] = self.unique[int(g(1))] = f'minecraft:item ({item})'
            entry = {'item': item, 'count': int(g(4)), 'runtime_id': int(g(2)), 'position': vec(6),
                     'velocity': vec(9)}
            if int(g(5)):
                entry['aux'] = int(g(5))
            return [('add_item_actor', entry)]
        if kind == 'RemoveActorPacket':
            return [('remove_actor', {'actor': self.unique.get(int(g(1)), int(g(1)))})]
        if kind == 'BlockActorDataPacket':
            return [('block_actor_data', {'position': [int(g(i)) for i in (1, 2, 3)],
                                          'nbt': NetworkNbt(rust_bytes(g(4))).root()})]
        if kind == 'LevelEventGenericPacket':
            data = NetworkNbt(rust_bytes(g(2))).compound()
            positions = {}
            for key in list(data):
                if found := re.fullmatch(r'pos(\d+)([xyz])', key):
                    positions.setdefault(int(found.group(1)), {})[found.group(2)] = data.pop(key)
            if positions:
                data['positions'] = [[p['x'], p['y'], p['z']] for _, p in sorted(positions.items())]
            return [('level_events_generic', {'event': int(g(1)), 'name': LEVEL_EVENTS.get(int(g(1))), **data})]
        runtime_id = int(g(1)) if kind in ('ActorEventPacket', 'SetActorDataPacket', 'SetActorMotionPacket') else None
        if runtime_id is not None and runtime_id not in self.runtime:
            return []
        if kind == 'ActorEventPacket':
            return [('actor_events', {'actor': self.runtime[runtime_id], 'runtime_id': runtime_id, 'event': g(2),
                                      'data': int(g(3))})]
        if kind == 'SetActorDataPacket':
            data = {ACTOR_DATA_IDS[i]: int(v) for i, v in _DATA_ITEM.findall(body) if i in ACTOR_DATA_IDS}
            if data.get('fuse') == -1:
                del data['fuse']
            if self.runtime[runtime_id].startswith('minecraft:item'):
                data.pop('flags', None)
            return [('actor_data', {'actor': self.runtime[runtime_id], 'runtime_id': runtime_id, **data})]
        if kind == 'SetActorMotionPacket':
            return [('actor_motion', {'actor': self.runtime[runtime_id], 'runtime_id': runtime_id,
                                      'motion': vec(2)})]
        if kind == 'TextPacket':
            message = _rust_str(g(2))
            if 'bds-sweep:' in message or '"translate":"commands.' in message or 'multiplayer.player.joined' in message:
                return []
            entry = {'type': g(1), 'message': message.strip()}
            if g(3):
                entry['parameters'] = [_rust_str(p) for p in re.findall(r'"((?:[^"\\]|\\.)*)"', g(3))]
            return [('messages', entry)]
        if kind == 'SetSpawnPositionPacket':
            return [('spawn_position', {'type': g(1), 'block_position': [int(g(i)) for i in (2, 3, 4)],
                                        'dimension': int(g(5)), 'spawn_block_pos': [int(g(i)) for i in (6, 7, 8)]})]
        if kind == 'PlaySoundPacket':
            return [('play_sounds', {'name': g(1), 'position': [int(g(i)) / 8 for i in (2, 3, 4)],
                                     'volume': float(g(5)), 'pitch': float(g(6))})]
        return None


def _drops(items: list[dict]) -> list[dict]:
    totals: dict[str, list[int]] = {}
    for item in items:
        total = totals.setdefault(item['item'], [0, 0])
        total[0] += item['count']
        total[1] += 1
    return [{'item': name, 'count': count, 'stacks': stacks} for name, (count, stacks) in sorted(totals.items())]


def _cells(states: dict, positions: dict) -> dict:
    return {label: {'pos': positions.get(label), 'name': s['name'], 'states': s['states']}
            for label, s in states.items()}


def _changes(changes: list) -> list[dict]:
    return [{'tick': tick, 'cell': cell, 'block': block_text(state)} for tick, cell, state in changes]


def _series(samples: list[dict], field: str) -> dict:
    """Per plant: [[tick, value], ...] at the first sample and every change."""
    out: dict[str, list] = {}
    for sample in samples:
        for key, value in sample[field].items():
            series = out.setdefault(key, [])
            if not series or series[-1][1] != value:
                series.append([sample['tick'], value])
    return dict(sorted(out.items()))


def blocks_step(step: dict, packets: dict) -> dict:
    out = {'step': step['step'].split('/', 1)[1], 'action': step['action']}
    positions = step.get('cells', {})
    if step['action'] == 'use':
        out.update(clicked=step['block'], face=step['face'], click=step['click'],
                   player={'feet': step['look']['position'], 'yaw': step['look']['server'][0],
                           'pitch': step['look']['server'][1]},
                   used=bool(step['interactions']), changed=step['changed'],
                   held_after=step['held_after'], cells=_cells(step['after'], positions))
    elif step['action'] == 'setblock':
        out.update(command=step['command'], same_tick=_cells(step['same_tick'], positions),
                   changes=_changes(step['changes']), cells=_cells(step['after'], positions),
                   drops=_drops(step['drops']))
    elif step['action'] == 'mine':
        out.update(block=step['block'], face=step['face'],
                   events=[{'tick': t, 'event': kind, 'pos': pos} for t, kind, pos in step['events']],
                   changes=_changes(step['changes']), cells=_cells(step['after'], positions),
                   drops=_drops(step['drops']))
    elif step['action'] == 'random_ticks':
        samples = step['samples']
        full = {key: next((s['tick'] for s in samples if s['growth'].get(key) == 7), None)
                for key in samples[0]['growth']}
        out.update(randomtickspeed=step['randomtickspeed'], seconds=step['seconds'],
                   sample_ticks=samples[1]['tick'] - samples[0]['tick'] if len(samples) > 1 else None,
                   growth=_series(samples, 'growth'),
                   farmland_moisturized_amount=_series(samples, 'moisturized_amount'),
                   first_sample_at_growth_7=dict(sorted(full.items())),
                   final_growth=dict(sorted((k, v['states'].get('growth')) for k, v in step['after'].items())))
        if any(s['other'] for s in samples):
            out['not_wheat'] = [s for s in samples if s['other']][:3]
    step_packets = packets.get(step['step'], {})
    if step_packets and step['action'] == 'random_ticks':
        # One block update per growth stage; only the totals matter here.
        out['packet_counts'] = {group: sum(e.get('times', 1) for e in entries)
                                for group, entries in step_packets.items()}
        out['block_updates_seen'] = sorted({e['block'] for e in step_packets.get('block_updates', [])
                                            if isinstance(e['block'], str)})
    elif step_packets:
        out['packets'] = step_packets
    return out


def compact_json(value, indent: int = 0, width: int = 180) -> str:
    """JSON with every value that fits in `width` columns on one line."""
    flat = json.dumps(value, ensure_ascii=False)
    if len(flat) + indent <= width or not isinstance(value, (dict, list)) or not value:
        return flat
    pad = ' ' * (indent + 1)
    if isinstance(value, dict):
        items = [f'{pad}{json.dumps(k)}: {compact_json(v, indent + 1, width)}' for k, v in value.items()]
        return '{\n' + ',\n'.join(items) + '\n' + ' ' * indent + '}'
    items = [pad + compact_json(v, indent + 1, width) for v in value]
    return '[\n' + ',\n'.join(items) + '\n' + ' ' * indent + ']'


def summarize_blocks(sweep: Path) -> None:
    """The observed-results fixture of a `blocks` sweep: per scenario step
    the watched cells' states, tick delays, drops and the server packets."""
    rows = {row['key']: row for row in results(sweep)}
    names: dict[int, str] = {}
    for row in rows.values():
        for step in row['steps']:
            for key in ('before', 'after', 'same_tick'):
                for state in (step.get(key) or {}).values():
                    if isinstance(state, dict) and 'runtime_id' in state:
                        names[state['runtime_id']] = block_text(state)
            for _, _, state in step.get('changes', []):
                names[state['runtime_id']] = block_text(state)
    packets = trace_packets(sweep / 'client.log', names)
    finished = [r for r in runs(sweep) if r['status'] == 'done'] or runs(sweep)
    stamp = finished[-1]['started']
    fixture = {
        'source': f'measured on BDS 1.26.51 with the bds-sweep blocks job, {stamp[:4]}-{stamp[4:6]}-{stamp[6:8]} UTC',
        'conventions': {
            'arena': 'stone floor at y=-61, survival player standing on it (feet y=-60), noon, clear sky',
            'faces': '0 down, 1 up, 2 north (-z), 3 south (+z), 4 west (-x), 5 east (+x)',
            'yaw': 'yaw 0 south, 90 west, 180 north, 270 east',
            'ticks': 'server ticks after the command or order; a change at tick n was first seen n ticks later',
            'cells': 'block name and full state map of each watched cell after the step',
            'packets': 'server packets received after the step, identical ones merged with a "times" count; '
                       'block runtime ids are written as the block state they denote; block updates have '
                       'flags 3 and layer 0 unless listed',
        },
    }
    for key in ('doors', 'torches', 'crops'):
        if key in rows:
            fixture[key] = [blocks_step(step, packets) for step in rows[key]['steps']]
    BLOCKS_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    BLOCKS_FIXTURE.write_bytes((compact_json(fixture) + '\n').encode('utf-8'))
    print(f'{BLOCKS_FIXTURE.relative_to(REPO)}: {", ".join(rows)}')


REDSTONE_FIXTURE = BLOCKS_FIXTURE.with_name('bds_1_26_51_redstone.json')
REDSTONE_SCENARIOS = ('redstone_dust', 'repeaters', 'redstone_torch', 'comparators', 'buttons', 'pressure_plates',
                      'iron_door_lever', 'note_blocks', 'fire')


def _snapshots(step: dict, start: int) -> list[dict]:
    """Every `sample_every` ticks from tick 0: the labels of the watched
    cells per block name (air only counted)."""
    state = {label: s['name'] for label, s in step['before'].items()}
    changes = sorted(step['changes'], key=lambda c: c[0])
    out, index = [], 0
    for tick in range(0, step['ticks'] + 1, step['sample_every']):
        while index < len(changes) and changes[index][0] - start <= tick:
            state[changes[index][1]] = changes[index][2]['name']
            index += 1
        row: dict = {'tick': tick}
        for label, name in state.items():
            if name == 'minecraft:air':
                row['air'] = row.get('air', 0) + 1
            else:
                row.setdefault(name.removeprefix('minecraft:'), []).append(label)
        out.append(row)
    return out


def redstone_step(step: dict, packets: dict) -> dict:
    """One step of a redstone/fire scenario: watched cells before, the
    per-tick changes counted from the trigger cell's first change (or from
    the command / order when there is no trigger), the cells after, and the
    sounds and level events the client received."""
    out = {'step': step['step'].split('/', 1)[1], 'action': step['action']}
    if 'look' in step:
        out.update(clicked=step['block'], face=step['face'],
                   player={'feet': step['look']['position'], 'yaw': step['look']['server'][0],
                           'pitch': step['look']['server'][1]})
    if 'interactions' in step:
        out['used'] = bool(step['interactions'])
    if step['action'] == 'use':
        out['cells'] = {label: {'pos': step['cells'][label], 'block': block_text(s)}
                        for label, s in step['after'].items()}
    else:
        if 'commands' in step:
            out['commands'] = step['commands']
            if not all(step['succeeded']):
                out['failed_commands'] = [c for c, ok in zip(step['commands'], step['succeeded']) if not ok]
        if 'stopped_after' in step:
            out['stop_order_after_ticks'] = step['stopped_after']
        start = 0
        trigger = step.get('trigger')
        if trigger:
            out['trigger'] = trigger
            first = next((tick for tick, cell, _ in step['changes'] if cell == trigger), None)
            if first is None:
                out['trigger_changed'] = False
            else:
                start = first
                out['trigger_seen_ticks_after_order'] = first
        out['tick_0_server_tick'] = step['origin_tick'] + start
        out['watched_ticks'] = step['ticks'] - start
        out['positions'] = step['cells']
        out['before'] = {label: block_text(s) for label, s in step['before'].items()}
        changes: list[dict] = []
        for tick, cell, state in step['changes']:
            if not changes or changes[-1]['tick'] != tick - start:
                changes.append({'tick': tick - start, 'cells': {}})
            changes[-1]['cells'][cell] = block_text(state)
        out['changes'] = changes
        out['after'] = {label: block_text(s) for label, s in step['after'].items()}
        if step.get('sample_every'):
            out['samples_every'] = step['sample_every']
            out['samples'] = _snapshots(step, start)
        if 'difficulty' in step:
            out['difficulty'] = step['difficulty']
    if 'items' in step:
        out['item_actors'] = [{'item': i['item'], 'count': i['count'], 'position': i['position']}
                              for i in step['items']]
    received = packets.get(step['step'], {})
    for group in ('sounds', 'level_events', 'block_events'):
        if received.get(group):
            out[group] = received[group]
    if received.get('block_updates'):
        out['block_update_packets'] = sum(e.get('times', 1) for e in received['block_updates'])
    return out


def summarize_redstone(sweep: Path) -> None:
    """The observed-results fixture of the redstone and fire scenarios of a
    `blocks` sweep."""
    rows = {row['key']: row for row in results(sweep) if row['key'] in REDSTONE_SCENARIOS}
    names: dict[int, str] = {}
    for row in rows.values():
        for runtime_id, state in row.get('palette', {}).items():
            names[int(runtime_id)] = block_text(state)
        for step in row['steps']:
            for key in ('before', 'after'):
                for state in (step.get(key) or {}).values():
                    names[state['runtime_id']] = block_text(state)
            for _, _, state in step.get('changes', []):
                names[state['runtime_id']] = block_text(state)
    packets = trace_packets(sweep / 'client.log', names)
    finished = [r for r in runs(sweep) if r['status'] == 'done'] or runs(sweep)
    stamp = finished[-1]['started']
    fixture = {
        'source': f'measured on BDS 1.26.51 with the bds-sweep blocks job (redstone and fire scenarios), '
                  f'{stamp[:4]}-{stamp[4:6]}-{stamp[6:8]} UTC',
        'conventions': {
            'arena': 'stone floor at y=-61 (x -10..24, z -12..24), survival player standing on it (feet y=-60), '
                     'noon, clear sky, peaceful (fire steps name their difficulty); the redstone scenarios run at '
                     'randomtickspeed 0, fire at randomtickspeed 1 with doFireTick on',
            'faces': '0 down, 1 up, 2 north (-z), 3 south (+z), 4 west (-x), 5 east (+x)',
            'yaw': 'yaw 0 south, 90 west, 180 north, 270 east',
            'placement': 'levers, repeaters, comparators, buttons, torches and doors are clicked into place by '
                         'the player (steps with action "use"); dust, lamps, plates, note blocks, stone, '
                         'redstone blocks and fire are /setblock',
            'ticks': 'the watched cells are read once per server tick; tick 0 is the tick in which the trigger '
                     'cell (the clicked lever/button/comparator, or the pressed plate) was first seen changed, '
                     'or the tick of the command when a step has no trigger; a change at tick n was first seen '
                     'n ticks after tick 0; tick_0_server_tick is a running server tick count, so steps can be '
                     'compared for even/odd phase',
            'changes': 'per tick, the watched cells that changed and their new state in block state notation',
            'samples': 'fire: every samples_every ticks, the watched cells per block name (air only counted)',
            'packets': 'sounds and level events received by the player after the step started, identical ones '
                       'merged with a "times" count; block runtime ids in sound data are written as the block '
                       'state they denote; block_update_packets counts the block update packets',
        },
    }
    for key in REDSTONE_SCENARIOS:
        if key in rows:
            fixture[key] = [redstone_step(step, packets) for step in rows[key]['steps']]
    REDSTONE_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    REDSTONE_FIXTURE.write_bytes((compact_json(fixture) + '\n').encode('utf-8'))
    print(f'{REDSTONE_FIXTURE.relative_to(REPO)}: {", ".join(rows)}')


UTILITY_FIXTURE = BLOCKS_FIXTURE.with_name('bds_1_26_51_utility.json')
UTILITY_SCENARIOS = ('observer', 'dispensers', 'tnt', 'note_block_use', 'rails', 'composter', 'cauldron', 'campfire',
                     'beehive', 'bell', 'jukebox', 'flower_pot', 'cake', 'end_portal', 'nether_portal',
                     'respawn_anchor', 'bed')
_REGION_LABEL = re.compile(r'-?\d+,-?\d+,-?\d+')


def utility_step(step: dict, packets: dict) -> dict:
    """One step of a utility scenario: the click or commands, the watched
    cells before/after (cells of an `x,y,z` region only when they changed),
    per-tick changes from the order, actors that appeared or went away, the
    held item after the step and the packets the player received."""
    out: dict = {'step': step['step'].split('/', 1)[1]}
    if 'look' in step:
        out['use'] = {'clicked': step['block'], 'face': step['face'], 'click': step['click'],
                      'player': {'feet': step['look']['position'], 'yaw': step['look']['server'][0],
                                 'pitch': step['look']['server'][1]}}
        out['used'] = bool(step['interactions'])
    if 'commands' in step:
        out['commands'] = step['commands']
        if not all(step['succeeded']):
            out['failed_commands'] = [c for c, ok in zip(step['commands'], step['succeeded']) if not ok]
    if step.get('scheduled'):
        out['scheduled'] = [{'tick': tick, 'command': command, 'succeeded': ok}
                            for tick, command, ok in step['scheduled']]
    out['origin_server_tick'] = step['origin_tick']
    out['watched_ticks'] = step['ticks']
    changed = {cell for _, cell, _ in step['changes']}
    shown = [label for label in step['cells'] if not _REGION_LABEL.fullmatch(label) or label in changed]
    out['positions'] = {label: step['cells'][label] for label in shown if not _REGION_LABEL.fullmatch(label)}
    out['before'] = {label: block_text(step['before'][label]) for label in shown}
    changes: list[dict] = []
    for tick, cell, state in step['changes']:
        if not changes or changes[-1]['tick'] != tick:
            changes.append({'tick': tick, 'cells': {}})
        changes[-1]['cells'][cell] = block_text(state)
    out['changes'] = changes
    out['after'] = {label: block_text(step['after'][label]) for label in shown}
    regions = len(step['cells']) - len(out['positions'])
    if regions:
        out['region_cells_watched'] = regions
    actors = []
    for entry in step['actors']:
        actor = {k: v for k, v in entry.items() if k != 'id'}
        if actor.get('data') == 0:
            del actor['data']
        actors.append(actor)
    if actors:
        out['actors'] = actors
    out['held_after'] = step['held_after']
    for key in ('level_time_before', 'level_time_after'):
        if key in step:
            out[key] = step[key]
    received = packets.get(step['step'], {})
    for group in ('sounds', 'play_sounds', 'level_events', 'level_events_generic', 'block_events',
                  'block_actor_data', 'add_actor', 'add_item_actor', 'remove_actor', 'actor_events', 'messages',
                  'spawn_position', 'hunger'):
        if received.get(group):
            out[group] = received[group]
    if received.get('actor_data'):
        out['actor_data'] = _actor_series(received['actor_data'], ('fuse', 'flags'))
    if received.get('actor_motion'):
        out['actor_motion'] = _actor_series(received['actor_motion'], ('motion',))
    if received.get('block_updates'):
        out['block_update_packets'] = sum(e.get('times', 1) for e in received['block_updates'])
    return out


def _actor_series(entries: list[dict], fields: tuple[str, ...]) -> list[dict]:
    """Per actor: packet count and each field's values in arrival order
    (repeats of the previous value dropped); a countdown by one is written
    as `first..last`."""
    actors: dict[int, dict] = {}
    for entry in entries:
        actor = actors.setdefault(entry['runtime_id'], {'actor': entry['actor'], 'runtime_id': entry['runtime_id'],
                                                         'packets': 0})
        actor['packets'] += entry.get('times', 1)
        for field in fields:
            if field in entry:
                values = actor.setdefault(field, [])
                if not values or values[-1] != entry[field]:
                    values.append(entry[field])
    for actor in actors.values():
        fuse = actor.get('fuse')
        if fuse and len(fuse) > 3 and all(b == a - 1 for a, b in zip(fuse, fuse[1:])):
            actor['fuse'] = f'{fuse[0]}..{fuse[-1]}'
        if 'motion' in actor and len(actor['motion']) > 4:
            motion = actor['motion']
            actor['motion'] = motion[:3] + [f'… {len(motion) - 4} more …', motion[-1]]
    return [actor for actor in actors.values() if any(field in actor for field in fields)]


def summarize_utility(sweeps: list[Path]) -> None:
    """The observed-results fixture of the utility scenarios of `blocks`
    sweeps; a scenario a later sweep finished without an error replaces the
    earlier one (reruns of single scenarios)."""
    rows: dict[str, tuple[dict, Path]] = {}
    for sweep in sweeps:
        for row in results(sweep):
            if row['key'] in UTILITY_SCENARIOS and (row['key'] not in rows or not row.get('error')):
                rows[row['key']] = (row, sweep)
    import native
    items = native.item_network_ids()
    packets: dict[Path, dict] = {}
    for sweep in dict.fromkeys(sweep for _, sweep in rows.values()):
        names: dict[int, str] = {}
        for row, source in rows.values():
            if source != sweep:
                continue
            for runtime_id, state in row.get('palette', {}).items():
                names[int(runtime_id)] = block_text(state)
            for step in row['steps']:
                for key in ('before', 'after'):
                    for state in (step.get(key) or {}).values():
                        names[state['runtime_id']] = block_text(state)
                for _, _, state in step.get('changes', []):
                    names[state['runtime_id']] = block_text(state)
        packets[sweep] = trace_packets(sweep / 'client.log', names, UtilityTrace(items))
    finished = [r for r in runs(sweeps[0]) if r['status'] == 'done'] or runs(sweeps[0])
    stamp = finished[-1]['started']
    fixture = {
        'source': f'measured on BDS 1.26.51 with the bds-sweep blocks job, {stamp[:4]}-{stamp[4:6]}-{stamp[6:8]} UTC',
        'conventions': {
            'arena': 'stone floor at y=-61 (x -10..24, z -12..24), survival player standing on it (feet y=-60), '
                     'noon, clear sky, peaceful (the cake scenario runs on easy), randomtickspeed 0; the player has '
                     'resistance V so explosions leave it alive; every scenario starts from a fresh arena with '
                     'no actors',
            'faces': '0 down, 1 up, 2 north (-z), 3 south (+z), 4 west (-x), 5 east (+x)',
            'yaw': 'yaw 0 south, 90 west, 180 north, 270 east',
            'placement': 'steps with "use" are the player\'s right-clicks with the held item (placements, levers, '
                         'block uses); everything else is /setblock or the listed commands',
            'ticks': 'the watched cells and the actors are read once per server tick; tick 0 is the server tick '
                     'in which the plugin sent the use order or ran the commands; a change at tick n was first '
                     'seen n ticks later; the client sends the click about a tick after the order',
            'cells': 'before/after/changes give block states in block state notation; cells labelled "x,y,z" '
                     'belong to a watched region and are listed only when they changed',
            'actors': 'non-player actors that appeared (first seen position and velocity in blocks per tick, '
                      'item stack for item actors) or went away (last seen position), by tick',
            'held_after': 'the selected hotbar stack after the step: [item, count, data], null when empty',
            'packets': 'packets the player received after the step started (attributed by arrival after the '
                       'step mark), identical ones merged with a "times" count: sounds (LevelSoundEvent), '
                       'play_sounds (PlaySound, block position / 8), level_events and level_events_generic (id, '
                       'usual protocol name, data; posNx/y/z fields listed as positions), block_events, '
                       'block_actor_data (network NBT decoded), add_actor (identifier, runtime id, position, '
                       'velocity, fuse when set), add_item_actor (item, count, position, velocity), remove_actor, '
                       'actor_events, actor_data (non-player actors only: per actor the packet count, the fuse '
                       'values, a countdown by one written first..last, and the flags of non-item actors), '
                       'actor_motion (SetActorMotion per actor), messages (TextPacket, harness command feedback '
                       'left out), spawn_position, hunger (changes of player.hunger/player.saturation); block '
                       'runtime ids in sound and level event data are written as the block state they denote; '
                       'block_update_packets counts the block update packets',
        },
    }
    for key in UTILITY_SCENARIOS:
        if key in rows:
            row, sweep = rows[key]
            fixture[key] = ({'failed': row['error']} if row.get('error')
                            else [utility_step(step, packets[sweep]) for step in row['steps']])
    UTILITY_FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    UTILITY_FIXTURE.write_bytes((compact_json(fixture) + '\n').encode('utf-8'))
    print(f'{UTILITY_FIXTURE.relative_to(REPO)}: {", ".join(rows)}')


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
    elif kind == 'blocks':
        summarize_blocks(dirs[0])
    elif kind == 'redstone':
        summarize_redstone(dirs[0])
    elif kind == 'utility':
        summarize_utility(dirs)
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
