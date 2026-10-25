"""Turn raw sweep results into measured data for `crates/unastar-data`.

    python tools/bds-sweep/summarize.py drops SWEEP_DIR [SWEEP_DIR ...]
    python tools/bds-sweep/summarize.py armor-slots SWEEP_DIR
    python tools/bds-sweep/summarize.py fuel SWEEP_DIR
    python tools/bds-sweep/summarize.py compare-drops SWEEP_DIR_A SWEEP_DIR_B

Writes into `crates/unastar-data/data/measured/` and records every artefact
(SHA-256, sweep parameters, sample counts, run times) in `manifest.json`
there. Raw `results.jsonl`/logs stay in the sweep directories (outside the
repository). See that directory's README for the schemas.
"""

from __future__ import annotations

import hashlib
import json
import math
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


def summarize_drops(sweeps: list[Path]) -> None:
    """The first sweep is the measurement; further sweeps of the same job are
    repeat runs whose agreement is recorded as `repeatability`."""
    primary, repeats = sweeps[0], sweeps[1:]
    rows = results(primary)
    by_block: dict[str, list[dict]] = defaultdict(list)
    for row in rows:
        by_block[row['block']].append(row)
    files = []
    for block, block_rows in sorted(by_block.items()):
        cases = []
        for row in sorted(block_rows, key=lambda r: r['key']):
            table, fit = loot_table(row['outcomes'])
            cases.append({
                'states': row['states'],
                'tool': row['key'].rsplit('|', 1)[1],
                'tool_item': row['tool'],
                'samples': row['samples'],
                'command_failures': row['command_failures'],
                'block_unchanged': row['unchanged'],
                'fit': fit,
                'observed': row['outcomes'],
                'loot_table': table,
            })
        name = block.removeprefix('minecraft:')
        files.append(write(f'block_drops/{name}.json', {'block': block, 'cases': cases}))
    update_manifest('block_drops', {
        **provenance('drops', sweeps),
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


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    kind, dirs = sys.argv[1], [Path(arg).resolve() for arg in sys.argv[2:]]
    if kind == 'drops':
        summarize_drops(dirs)
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
    else:
        print(__doc__)
        return 2
    return 0


if __name__ == '__main__':
    sys.exit(main())
