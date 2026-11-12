"""Compare movement traces of BDS, unastar, bedsim and acacia-physics tick by tick.

    python tools/movement-diff/compare.py BDS.json UNASTAR.json BEDSIM.json [ACACIA.json] [--report OUT.md]
                                          [--scenario NAME] [--ticks]

Every trace file is one implementation's states after every input of every
scenario (`movement-diff simulate`, `movement-diff extract`, the bedsim
runner, the acacia runner). Values are compared bit for bit through their `bits`:

* the wire position on every tick (BDS has it on every tick);
* the velocity where both traces have it (BDS: the ticks it corrected);
* ground contact where both have it.

For each scenario and pair of implementations the report gives the first
tick and field where they differ, by how much (absolute and in f32 ULPs),
and how many ticks agree. `--ticks` adds every differing tick of the chosen
scenario.
"""

from __future__ import annotations

import argparse
import json
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

AXES = 'xyz'
ORDER = ['bds', 'unastar', 'bedsim', 'acacia']
PAIRS = [('unastar', 'bds'), ('bedsim', 'bds'), ('acacia', 'bds'), ('unastar', 'bedsim'),
         ('acacia', 'bedsim'), ('acacia', 'unastar')]


def bits_to_float(bits: str) -> float:
    return struct.unpack('<f', struct.pack('<I', int(bits, 16)))[0]


def f32_text(value: float) -> str:
    """The shortest decimal that reads back as the same f32."""
    target = struct.pack('<f', value)
    for digits in range(1, 10):
        text = f'{value:.{digits}g}'
        if struct.pack('<f', float(text)) == target:
            return text
    return repr(value)


def ordered(bits: str) -> int:
    """The f32 as an integer that orders like the float (for ULP distances)."""
    value = int(bits, 16)
    return -(value & 0x7fffffff) if value & 0x80000000 else value


def ulps(a: str, b: str) -> int:
    return abs(ordered(a) - ordered(b))


@dataclass
class Difference:
    tick: int
    field: str
    left: float | bool
    right: float | bool
    ulps: int | None

    @property
    def size(self) -> float | None:
        if isinstance(self.left, bool):
            return None
        return abs(self.left - self.right)

    def describe(self) -> str:
        if self.ulps is None:
            return f'tick {self.tick} {self.field}: {self.left} vs {self.right}'
        return (f'tick {self.tick} {self.field}: {f32_text(self.left)} vs {f32_text(self.right)} '
                f'(diff {self.size:.3g}, {self.ulps} ULP)')


def differences(left: list[dict], right: list[dict]) -> tuple[list[Difference], int]:
    """Every differing field of two scenario traces, and the ticks compared."""
    out: list[Difference] = []
    right_by_tick = {state['tick']: state for state in right}
    compared = 0
    for state in left:
        other = right_by_tick.get(state['tick'])
        if other is None:
            continue
        compared += 1
        for field in ('position', 'velocity'):
            if field not in state or field not in other:
                continue
            for axis, (a, b) in enumerate(zip(state[field]['bits'], other[field]['bits'])):
                if int(a, 16) != int(b, 16):
                    out.append(Difference(state['tick'], f'{field}.{AXES[axis]}',
                                          bits_to_float(a), bits_to_float(b), ulps(a, b)))
        if state.get('on_ground') is not None and other.get('on_ground') is not None \
                and state['on_ground'] != other['on_ground']:
            out.append(Difference(state['tick'], 'on_ground', state['on_ground'], other['on_ground'], None))
    return out, compared


def load(paths: list[Path]) -> dict[str, dict]:
    traces = {}
    for path in paths:
        data = json.loads(path.read_text(encoding='utf-8'))
        traces[data['implementation']] = data
    return traces


def verdict(diffs: list[Difference], compared: int) -> str:
    if compared == 0:
        return 'n/a'
    if not diffs:
        return 'equal'
    first = diffs[0]
    return f'differs t{first.tick} {first.field}'


def report(traces: dict[str, dict], only: str | None, ticks: bool) -> str:
    names = sorted(set().union(*(t['scenarios'].keys() for t in traces.values())))
    if only:
        names = [n for n in names if n == only]
    lines = ['# Movement differential: BDS 1.26.51, unastar, bedsim', '']
    for implementation in ORDER:
        if implementation in traces:
            lines.append(f'- **{implementation}**: {traces[implementation]["source"]}')
    pairs = [(a, b) for a, b in PAIRS if a in traces and b in traces]
    label = {'bds': 'BDS'}
    lines += ['', '| scenario | ticks | '
              + ' | '.join(f'{label.get(a, a)} = {label.get(b, b)}' for a, b in pairs) + ' |',
              '|---|---|' + '---|' * len(pairs)]
    totals = {pair: 0 for pair in pairs}
    details = []
    for name in names:
        row = [name]
        lengths = {len(t['scenarios'][name]['ticks']) for t in traces.values() if name in t['scenarios']}
        row.append('/'.join(str(n) for n in sorted(lengths)))
        for left, right in pairs:
            if name not in traces[left]['scenarios'] or name not in traces[right]['scenarios']:
                row.append('n/a')
                continue
            diffs, compared = differences(traces[left]['scenarios'][name]['ticks'],
                                          traces[right]['scenarios'][name]['ticks'])
            row.append(verdict(diffs, compared))
            if compared and not diffs:
                totals[(left, right)] += 1
            if diffs:
                largest = max((d for d in diffs if d.size is not None), key=lambda d: d.size, default=None)
                ticks_off = len({d.tick for d in diffs})
                text = f'- `{name}` {left} vs {right}: first {diffs[0].describe()}; {ticks_off} of {compared} ticks differ'
                if largest:
                    text += f'; largest {largest.describe()}'
                details.append(text)
                if ticks:
                    details += [f'  - {d.describe()}' for d in diffs]
        lines.append('| ' + ' | '.join(row) + ' |')
    lines.append(f'| **equal** | {len(names)} | ' + ' | '.join(str(totals[pair]) for pair in pairs) + ' |')
    if details:
        lines += ['', '## Divergences', ''] + details
    return '\n'.join(lines) + '\n'


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('traces', nargs='+', type=Path)
    parser.add_argument('--report', type=Path, help='write the markdown report here')
    parser.add_argument('--scenario', help='only this scenario')
    parser.add_argument('--ticks', action='store_true', help='list every differing tick')
    args = parser.parse_args()
    text = report(load(args.traces), args.scenario, args.ticks)
    if args.report:
        args.report.write_text(text, encoding='utf-8')
    sys.stdout.write(text)
    return 0


if __name__ == '__main__':
    sys.exit(main())
