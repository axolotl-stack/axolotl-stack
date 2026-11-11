"""Cuts `order_probe` captures into the committed `pvp-race-*` fixtures.

    python pvp_race_fixture.py OUT_DIR CASE=CAPTURE_DIR [CASE=CAPTURE_DIR ...]

For every case it writes `OUT_DIR/pvp-race-CASE.jsonl`: for the first trial
of each variant, every client packet and the server packets that show the
outcome (hurt, health, knockback, deaths, corrections, actor flags, sounds,
chat and swing relays, spawns, container opens), from three ticks before
the race tick (more for cases that set up earlier) to fifteen after. One JSON object per packet: `variant`,
`trial`, `rel` (client tick relative to the race tick), `t_us`, `player`
(the sender for `c2s`, the receiver for `s2c`), `dir`, `batch` (the game
batch of a sent packet), `name` and `hex` (header included, no batch
length). The first line of each file is a header: the runtime ids, the
server properties and every variant's outcome distribution over all trials
(`pvp_race_summary.py`), which is the determinism evidence.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pvp_race_summary as summary  # noqa: E402

KEEP_S2C = {
    'ActorEventPacket', 'UpdateAttributesPacket', 'SetActorMotionPacket', 'AnimatePacket',
    'CorrectPlayerMovePredictionPacket', 'SetActorDataPacket', 'LevelSoundEventPacket',
    'RespawnPacket', 'TextPacket', 'AddActorPacket', 'ContainerOpenPacket', 'MovePlayerPacket',
    'CommandOutputPacket',
}
WINDOW = (-3, 15)
# Cases kept as their header only (outcome distributions): no replay needs the packets.
HEADER_ONLY = {'reach-sweep'}
# Cases whose trials set up state earlier than three ticks before the race.
LEAD_IN = {'crit': 12, 'kill': 21, 'both-low': 21, 'lag': 13, 'sprint': 3, 'model': 21,
           'model-strict': 21, 'follow-up': 21, 'two-attackers': 21}
# Before this tick only the set-up (commands and other non-input packets) is kept,
# except where a test counts the inputs of the lead-in.
INPUTS_FROM = {'crit': -12}

# Sent only to the player they are about; everything else is kept as the
# bystander C received it.
OWN_ONLY = {'CorrectPlayerMovePredictionPacket', 'ContainerOpenPacket', 'RespawnPacket'}


def kept(record, runtime_ids) -> bool:
    name, who = record['name'], record['player']
    if record['dir'] == 'c2s':
        return True
    if name not in KEEP_S2C:
        return False
    if name in OWN_ONLY:
        return True
    if name == 'MovePlayerPacket':
        # Teleports only (the per-tick position broadcasts are not evidence).
        return 'position_mode: Normal' not in record['debug']
    return who == 'C'


def cut(case: str, capture: Path, out: Path) -> int:
    events = capture / 'events.jsonl'
    trials = summary.parse(events)
    groups: dict[str, list] = {}
    for trial in trials:
        groups.setdefault(trial['variant'], []).append(trial)
    outcomes = {}
    for variant, group in groups.items():
        counts: dict[str, int] = {}
        for trial in group:
            text = summary.outcome(trial)
            counts[text] = counts.get(text, 0) + 1
        outcomes[variant] = [{'count': n, 'events': text.split('\n      ')}
                             for text, n in sorted(counts.items(), key=lambda kv: -kv[1])]
    runtime_ids = {}
    first_trial = {}
    for trial in trials:
        first_trial.setdefault(trial['variant'], trial['trial'])
    session = json.loads((capture / 'session.json').read_text(encoding='utf-8'))
    # Trials in which C sends more than idle inputs (C's inputs are kept then).
    c_acts = {}
    for trial in trials:
        c_acts[(trial['variant'], trial['trial'])] = any(
            ' C ' in sent and 'PlayerAuthInput' not in sent for sent in trial['sent'])
    # Race ticks of the kept trials. A record belongs to the trial whose window holds its
    # tick: the race tick's own idle inputs and a trial's set-up are logged before its mark.
    races = []
    for line in events.open(encoding='utf-8'):
        record = json.loads(line)
        if 'mark' not in record:
            continue
        mark = record['mark']
        if mark.startswith('runtime id'):
            _, _, who, rid = mark.split()
            runtime_ids[who] = int(rid)
            continue
        fields = dict(part.split('=', 1) for part in mark.split())
        if first_trial.get(fields['variant']) == int(fields['trial']):
            races.append((record['tick'], fields['variant'], int(fields['trial'])))
    lead = LEAD_IN.get(case, -WINDOW[0])
    rows = []
    for line in events.open(encoding='utf-8'):
        record = json.loads(line)
        if 'mark' in record or case in HEADER_ONLY:
            continue
        owner = next(((race, variant, trial) for race, variant, trial in races
                      if -lead <= record['tick'] - race <= WINDOW[1]), None)
        if owner is None:
            continue
        race, variant, trial = owner
        rel = record['tick'] - race
        if not kept(record, runtime_ids):
            continue
        if (record['dir'] == 'c2s' and record['name'] == 'PlayerAuthInputPacket'
                and rel < INPUTS_FROM.get(case, WINDOW[0])):
            continue
        if record['dir'] == 's2c' and rel < WINDOW[0]:
            continue
        if record['dir'] == 'c2s' and record['player'] == 'C' and not c_acts.get((variant, trial)):
            continue
        rows.append({'variant': variant, 'trial': trial, 'rel': rel, 't_us': record['t_us'],
                     'player': record['player'], 'dir': record['dir'], 'batch': record['batch'],
                     'name': record['name'], 'hex': record['hex']})
    command = session['probe_command']
    join_order = command[command.index('--join-order') + 1] if '--join-order' in command else 'ABC'
    header = {'case': case, 'bds_version': session['bds_version'], 'server': session['server'],
              'join_order': join_order,
              'server_properties': session['server_properties'], 'runtime_ids': runtime_ids,
              'trials_per_variant': {v: len(g) for v, g in groups.items()}, 'outcomes': outcomes}
    path = out / f'pvp-race-{case}.jsonl'
    with path.open('w', encoding='utf-8', newline='\n') as f:
        f.write(json.dumps(header, separators=(',', ':')) + '\n')
        for row in rows:
            f.write(json.dumps(row, separators=(',', ':')) + '\n')
    return len(rows)


def main() -> int:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for spec in sys.argv[2:]:
        case, capture = spec.split('=', 1)
        n = cut(case, Path(capture), out)
        size = (out / f'pvp-race-{case}.jsonl').stat().st_size
        print(f'{case}: {n} packets, {size // 1024} KiB')
    return 0


if __name__ == '__main__':
    sys.exit(main())
