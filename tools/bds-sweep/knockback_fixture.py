"""Distil `sweep.py knockback --client-capture` runs into the committed
knockback fixture (`crates/unastar/src/entity/mob/combat/testdata/
bds-1.26.51-knockback.json`).

    python tools/bds-sweep/knockback_fixture.py OUT.json SWEEP_DIR [SWEEP_DIR ...]

Per trial it keeps the mob's position and ground contact at every tick
(Endstone samples), every knockback the mob received (the attacker's
position, the velocity change BDS applied and the sample whose move is the
first one carrying it) and, from the sweep player's packet capture, every
`MoveActorDelta` that carried a position and every `SetActorMotion` about
the mob, each matched to the sample it reports. Only observed values are
kept: no packet bytes, no login data.
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
    """(t_ms, id, decoded) for the server packets this fixture uses."""
    for line in capture.read_text(encoding='utf-8').splitlines():
        record = json.loads(line)
        if record['dir'] != 's2c' or record['id'] not in (13, 40, 111):
            continue
        reader = Reader(bytes.fromhex(record['hex']))
        reader.varint()  # header
        if record['id'] == 13:  # AddActor
            reader.zigzag()
            decoded = {'rid': reader.varint(), 'type': reader.string()}
        elif record['id'] == 40:  # SetActorMotion
            decoded = {'rid': reader.varint(), 'motion': [reader.f32(), reader.f32(), reader.f32()],
                       'tick': reader.varint()}
        else:  # MoveActorDelta
            decoded = {'rid': reader.varint(), 'pos': [reader.optional(reader.f32) for _ in range(3)]}
            for _ in range(3):
                reader.optional(reader.u8)
            decoded.update(ground=reader.u8() != 0, force=reader.u8() != 0, force_local=reader.u8() != 0,
                           completion=reader.u8() != 0, ticks=reader.varint())
        yield record['t_ms'], record['id'], decoded


def wire_for(trial: dict, capture: list, samples: list) -> list[dict]:
    """The mob's position updates and motion packets, each tied to the index
    of the sample it reports. A position update is matched by its position;
    a motion packet goes with the update in its batch, or, for a far mob
    whose updates BDS sends only every few ticks, counts 50 ms ticks from
    the last update (or from the `AddActor`, sample 0)."""
    rid = trial['runtime_id']
    events, cursor, last = [], 0, None
    for t_ms, kind, decoded in capture:
        if decoded.get('rid') != rid:
            continue
        if kind == 13:
            last = (t_ms, 0)
        elif kind == 111 and any(v is not None for v in decoded['pos']):
            for index in range(cursor, len(samples)):
                pos = samples[index]['mob']['pos']
                if all(v is None or abs(v - pos[axis]) < 1e-4 for axis, v in enumerate(decoded['pos'])):
                    break
            else:
                # The mob keeps moving between the last sample and its removal.
                if cursor >= len(samples) - 2:
                    break
                raise SystemExit(f'{trial["key"]}: no sample matches MoveActorDelta {decoded}')
            cursor = index
            last = (t_ms, index)
            assert not (decoded['force'] or decoded['force_local'] or decoded['completion']), decoded
            events.append({'sample': index, 'delta': [None if v is None else round(v, 6) for v in decoded['pos']],
                           'ground': decoded['ground'], 'ticks': decoded['ticks']})
        elif kind == 40:
            if last is None:
                raise SystemExit(f'{trial["key"]}: SetActorMotion before the mob was added at {t_ms}')
            # Packets of one batch are stamped as they are decoded, within a
            # few milliseconds of each other; ticks are 50 ms apart.
            index = last[1] + round((t_ms - last[0]) / 50)
            if index >= len(samples):
                break
            events.append({'sample': index, 'motion': [round(v, 6) for v in decoded['motion']],
                           'tick': decoded['tick']})
    return events


def distil(trial: dict, capture: list, spec: dict) -> dict:
    samples = trial['samples']
    by_tick = {sample['tick']: index for index, sample in enumerate(samples)}
    hits = []
    for event in trial['knockbacks']:
        # The knockback lands during packet handling; the sample two plugin
        # ticks later is the first one whose move carries it.
        index = by_tick[event['tick'] + 2]
        hits.append({'sample': index, 'source': [round(v, 6) for v in event['source']['pos']],
                     'push': [round(v, 6) for v in event['knockback']],
                     'mob_ground': event['state']['ground']})
    return {
        'mob': trial['mob'],
        'frozen': trial['frozen'],
        'spec': spec,
        'positions': [[round(v, 6) for v in sample['mob']['pos']] + [int(sample['mob']['ground'])]
                      for sample in samples],
        'hits': hits,
        'wire': wire_for(trial, capture, samples),
    }


# What the job did in a trial (positions, hits, effects, commands).
SPEC_KEYS = ('at', 'hits', 'settle', 'tail', 'player', 'setup', 'effects', 'tool')


def main() -> int:
    out, dirs = Path(sys.argv[1]), [Path(arg) for arg in sys.argv[2:]]
    trials, sources = {}, []
    for directory in dirs:
        loaded = json.loads((directory / 'loaded.json').read_text(encoding='utf-8'))
        capture = list(packets(directory / 'client-capture' / 'packets.jsonl'))
        sources.append({'sweep': directory.name, **loaded})
        job = json.loads((directory / 'job.json').read_text(encoding='utf-8'))
        specs = {item['key']: {k: item[k] for k in SPEC_KEYS if k in item} for item in job['items']}
        for line in (directory / 'results.jsonl').read_text(encoding='utf-8').splitlines():
            trial = json.loads(line)
            trials[trial['key']] = distil(trial, capture, specs[trial['key']])
    fixture = {
        'bds_version': '1.26.51',
        'protocol': sources[0]['protocol_version'],
        'sources': sources,
        'method': 'tools/bds-sweep/sweep.py knockback --client-capture (--items slice / air); '
                  'Endstone per-tick samples and ActorKnockbackEvent, player-side packet capture',
        'trials': trials,
    }
    out.write_text(json.dumps(fixture, separators=(',', ':')) + '\n', encoding='utf-8')
    print(f'{len(trials)} trials -> {out} ({out.stat().st_size} bytes)')
    return 0


if __name__ == '__main__':
    sys.exit(main())
