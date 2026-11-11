"""Distil `sweep.py knockback --items cadence|cadence2 --observer
--client-capture` runs into the committed replication fixture
(`crates/unastar/src/entity/mob/net/testdata/bds-1.26.51-replication.json`).

    python tools/bds-sweep/replication_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Per trial it keeps the actor's state at every tick (Endstone samples:
position, ground contact, pitch and yaw), where each viewer stood (the sweep
player and the observer client), the hits and timed commands, and, from each
viewer's packet capture, the actor's `AddActor`, every `MoveActorDelta` and
every `SetActorMotion`, each tied to the sample it reports. A delta that
carries a position is matched to the sample with that position; a
rotation-only delta and a motion packet are counted in 50 ms ticks from the
last matched packet (`timed`). Only observed values are kept: no packet
bytes, no login data.
"""

from __future__ import annotations

import json
import struct
import sys
from pathlib import Path


class Reader:
    def __init__(self, data: bytes) -> None:
        self.data, self.at = data, 0

    def u8(self) -> int:
        value = self.data[self.at]
        self.at += 1
        return value

    def varint(self) -> int:
        value = shift = 0
        while True:
            byte = self.u8()
            value |= (byte & 0x7F) << shift
            shift += 7
            if not byte & 0x80:
                return value

    def zigzag(self) -> int:
        value = self.varint()
        return (value >> 1) ^ -(value & 1)

    def f32(self) -> float:
        (value,) = struct.unpack_from('<f', self.data, self.at)
        self.at += 4
        return value

    def string(self) -> str:
        size = self.varint()
        text = self.data[self.at:self.at + size].decode('utf-8')
        self.at += size
        return text

    def optional(self, read):
        return read() if self.u8() else None


def packets(capture: Path):
    """(t_ms, decoded) for the actor packets this fixture uses: AddActor (13),
    SetActorMotion (40), MoveActorDelta (111) and MotionPredictionHints (157)."""
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c' or record['id'] not in (13, 40, 111, 157):
            continue
        reader = Reader(bytes.fromhex(record['hex']))
        reader.varint()  # header
        if record['id'] == 13:
            reader.zigzag()
            decoded = {'kind': 'add', 'rid': reader.varint(), 'type': reader.string(),
                       'pos': [reader.f32() for _ in range(3)], 'vel': [reader.f32() for _ in range(3)]}
        elif record['id'] == 40:
            decoded = {'kind': 'motion', 'rid': reader.varint(), 'motion': [reader.f32() for _ in range(3)],
                       'tick': reader.varint()}
        elif record['id'] == 157:
            decoded = {'kind': 'hint', 'rid': reader.varint(), 'motion': [reader.f32() for _ in range(3)],
                       'ground': reader.u8() != 0}
        else:
            decoded = {'kind': 'delta', 'rid': reader.varint(), 'pos': [reader.optional(reader.f32) for _ in range(3)],
                       'rot': [reader.optional(reader.u8) for _ in range(3)]}
            decoded.update(ground=reader.u8() != 0, force=reader.u8() != 0, force_local=reader.u8() != 0,
                           completion=reader.u8() != 0, ticks=reader.varint())
        yield record['t_ms'], decoded


def r6(values):
    return [None if v is None else round(v, 6) for v in values]


# Packets of one batch arrive within a few milliseconds of each other;
# batches of consecutive ticks are about 50 ms apart.
SAME_BATCH_MS = 6


def wire_for(rid: int, capture: list, samples: list) -> list[dict]:
    """The actor's packets as one viewer received them, each tied to a sample.
    A delta with a position is matched to the sample with that position (an
    anchor; the `AddActor` batch is an anchor for sample 1). Any other packet
    goes with an anchor whose batch it arrived in, else it is counted in
    50 ms ticks from the anchor before it, kept strictly between that anchor
    and the next one (`timed`)."""
    mine = [(t_ms, decoded) for t_ms, decoded in capture if decoded.get('rid') == rid]
    anchors: list[tuple[int, int]] = []
    matched: dict[int, int] = {}
    cursor = 0
    for order, (t_ms, decoded) in enumerate(mine):
        if decoded['kind'] == 'add':
            anchors.append((t_ms, 1))
        elif decoded['kind'] == 'delta' and any(v is not None for v in decoded['pos']):
            for candidate in range(cursor, len(samples)):
                pos = samples[candidate]['mob']['pos']
                if all(v is None or abs(v - pos[axis]) < 2e-5 for axis, v in enumerate(decoded['pos'])):
                    matched[order] = candidate
                    anchors.append((t_ms, candidate))
                    cursor = candidate
                    break
    if mine and mine[0][1]['kind'] != 'add':
        raise SystemExit(f'actor {rid}: {mine[0][1]["kind"]} before its AddActor')

    def timed_index(t_ms: int) -> int:
        before = [a for a in anchors if a[0] <= t_ms + SAME_BATCH_MS]
        after = [a for a in anchors if a[0] >= t_ms - SAME_BATCH_MS]
        prev, nxt = before[-1], (after[0] if after else None)
        if t_ms - prev[0] <= SAME_BATCH_MS:
            return prev[1]
        if nxt is not None and nxt[0] - t_ms <= SAME_BATCH_MS:
            return nxt[1]
        index = max(prev[1] + round((t_ms - prev[0]) / 50), prev[1] + 1)
        if nxt is not None:
            index = min(index, nxt[1] - 1)
        return index

    events = []
    for order, (t_ms, decoded) in enumerate(mine):
        kind = decoded['kind']
        if kind == 'add':
            events.append({'sample': 0, 'add': {'pos': r6(decoded['pos']), 'vel': r6(decoded['vel'])}})
            continue
        timed = order not in matched
        index = timed_index(t_ms) if timed else matched[order]
        if index >= len(samples):
            break
        event = {'sample': index}
        if timed:
            event['timed'] = True
        if kind == 'delta':
            event.update(delta=r6(decoded['pos']), rot=decoded['rot'], ground=decoded['ground'],
                         ticks=decoded['ticks'])
            if decoded['force'] or decoded['force_local'] or decoded['completion']:
                event['flags'] = [decoded['force'], decoded['force_local'], decoded['completion']]
        elif kind == 'motion':
            event.update(motion=r6(decoded['motion']), tick=decoded['tick'])
        else:
            event.update(hint=r6(decoded['motion']), ground=decoded['ground'])
        events.append(event)
    return events


def positions(samples: list, viewer: str) -> list[list]:
    """Where a viewer stood: `[sample, x, y, z]` (feet) whenever it changed."""
    out = []
    for index, sample in enumerate(samples):
        if viewer not in sample:
            return []
        pos = r6(sample[viewer]['pos'])
        if not out or out[-1][1:] != pos:
            out.append([index, *pos])
    return out


def distil(trial: dict, captures: dict, job_item: dict) -> dict:
    samples = trial['samples']
    by_tick = {sample['tick']: index for index, sample in enumerate(samples)}
    hits = []
    for event in trial['knockbacks']:
        # A melee hit lands during packet handling: the sample two plugin
        # ticks later is the first one whose move carries it. A `/damage`
        # action lands in the plugin step itself: the next sample carries it.
        delay = 2 if job_item['hits'] else 1
        hits.append({'sample': by_tick[event['tick'] + delay],
                     'source': r6(event['source']['pos']) if event['source'] else None,
                     'push': r6(event['knockback'])})
    viewers = {}
    for viewer, capture in captures.items():
        place = positions(samples, viewer)
        if not place:
            continue
        viewers[viewer] = {'positions': place, 'wire': wire_for(trial['runtime_id'], capture, samples)}
    return {
        'actor': trial['mob'],
        'frozen': trial['frozen'],
        'spec': {k: job_item[k] for k in ('at', 'hits', 'settle', 'tail', 'player', 'observer', 'setup',
                                          'actions', 'effects', 'tool') if k in job_item},
        # x, y, z, on ground (0/1), pitch, yaw of the actor after each tick.
        'samples': [r6(s['mob']['pos']) + [int(s['mob']['ground'])] + r6(s['mob']['rot']) for s in samples],
        'hits': hits,
        'viewers': viewers,
    }


def main() -> int:
    out, dirs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    trials, sources = {}, []
    for directory in dirs:
        loaded = json.loads((directory / 'loaded.json').read_text(encoding='utf-8'))
        sources.append({'sweep': directory.name, **loaded})
        captures = {'player': list(packets(directory / 'client' / 'packets.jsonl'))}
        if (directory / 'observer' / 'packets.jsonl').exists():
            captures['observer'] = list(packets(directory / 'observer' / 'packets.jsonl'))
        job = json.loads((directory / 'job.json').read_text(encoding='utf-8'))
        items = {item['key']: item for item in job['items']}
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            trial = json.loads(line)
            trials[trial['key']] = distil(trial, captures, items[trial['key']])
    fixture = {
        'bds_version': '1.26.51',
        'protocol': sources[0]['protocol_version'],
        'sources': sources,
        'method': 'tools/bds-sweep/sweep.py knockback --items cadence / cadence2 --observer --client-capture '
                  '--view-distance 10 --tick-distance 10; Endstone per-tick samples, ActorKnockbackEvent, '
                  'packet captures of two clients (player, observer)',
        'trials': trials,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8', newline='\n')
    print(f'{len(trials)} trials -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
