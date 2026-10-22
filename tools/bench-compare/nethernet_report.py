"""Append measured NetherNet tables without replacing the existing benchmark record."""
import json
from pathlib import Path
import statistics
import sys

MARKER = '<!-- nethernet-bench:start -->'
METRICS = ('ns_per_op', 'allocs_per_op', 'bytes_per_op', 'payload_bytes',
           'msgs_per_sec', 'mb_per_sec', 'p50_us', 'p99_us', 'max_us')
SIDES = ('rust-owned', 'cinnabar', 'go')


def fmt(value):
    return '—' if value is None else f'{value:,.2f}'


def clean(value):
    return str(value).replace('|', '/').replace('\r', ' ').replace('\n', ' ')


def render(source):
    metadata = json.loads((source / 'metadata.json').read_text(encoding='utf-8-sig'))
    rounds = metadata['rounds']
    groups = {}
    exclusions = set()
    clocks = []
    for file in sorted(source.glob('[0-9]*-*.jsonl')):
        number = int(file.name.split('-')[0])
        if not 1 <= number <= rounds:
            continue
        for line in file.read_text(encoding='utf-8-sig').splitlines():
            if not line.strip():
                continue
            row = json.loads(line)
            key = tuple(row[k] for k in ('area', 'name', 'operation', 'implementation'))
            group = groups.setdefault(key, {})
            if number in group:
                raise ValueError(f'Duplicate round {number}: {key}')
            group[number] = row
    if not groups:
        raise ValueError('No NetherNet measured rows')
    aggregate = []
    for key, observations in sorted(groups.items()):
        if set(observations) != set(range(1, rounds + 1)):
            raise ValueError(f'Missing round for {key}: {sorted(observations)}')
        row = dict(zip(('area', 'name', 'operation', 'implementation'), key))
        for metric in METRICS:
            values = [r[metric] for r in observations.values()
                      if isinstance(r.get(metric), (int, float))]
            row[metric] = statistics.median(values) if values else None
        row['rounds'] = len(observations)
        aggregate.append(row)
    for file in sorted(source.glob('[0-9]*-*.stderr.log')):
        number = int(file.name.split('-')[0])
        if not 1 <= number <= rounds:
            continue
        for line in file.read_text(encoding='utf-8-sig').splitlines():
            if not line.startswith('{'):
                continue
            record = json.loads(line)
            if record.get('area') == 'coverage' and record.get('status') == 'unavailable':
                exclusions.add((record['name'], record['implementation'], record['reason']))
            if record.get('event') == 'benchmark_clock':
                clocks.append((number, record))
    lines = ['# NetherNet cross-implementation measurements', '',
             f"Measured UTC: {metadata.get('utc', 'not recorded')}; median of {rounds} alternating rounds. "
             f"Echoes: {metadata.get('count', 100)} for 32/512 B, {metadata.get('split_count', 10)} for 4/64 KiB, "
             'one untimed readiness echo; fresh connection per size and round.', '',
             metadata.get('conditions', ''), '',
             'Measurement only: no production optimization. Throughput counts completed reliable-channel echoes and request payload only (decimal MB/s), not both directions or saturation bandwidth. '
             'Setup spans offer/answer, ICE, DTLS and SCTP/data-channel opening; readiness is excluded. '
             'Each table cell is the median of its per-round statistic, not a pooled percentile.', '',
             '## NetherNet pins and configuration', '',
             '| Component | Version / configuration |', '| --- | --- |']
    for key, value in sorted(metadata.get('versions', {}).items()):
        lines.append(f'| {clean(key)} | {clean(value)} |')
    lines += ['', 'Stack parameters, public API boundaries and buffer asymmetries: '
              '[ours](../../crates/bench-compare/README.md), [Go](go/CAVEATS.md), '
              '[Cinnabar](cinnabar/README.md). Local signaling uses no public STUN/TURN service or credentials. '
              'The interop adapter exchanges complete signals as JSON strings over benchmark-local loopback TCP; '
              'this bridge is measurement code, not a production transport change.', '']
    index = {tuple(r[k] for k in ('area', 'name', 'operation', 'implementation')): r for r in aggregate}
    for area, title in (('nethernet_framing', 'framing'), ('nethernet_signaling', 'signaling'),
                        ('nethernet_discovery', 'discovery')):
        segment_header = ' Segments |' if area == 'nethernet_framing' else ''
        segment_separator = ' ---: |' if area == 'nethernet_framing' else ''
        lines += [f'## NetherNet {title} micro', '',
                  f'| Case | Operation |{segment_header} tokio-nethernet ns/op | Cinnabar ns/op | Go ns/op |',
                  f'| --- | --- |{segment_separator} ---: | ---: | ---: |']
        cases = sorted({(r['name'], r['operation']) for r in aggregate if r['area'] == area})
        for name, operation in cases:
            records = [index.get((area, name, operation, side)) for side in SIDES]
            segment_cell = ''
            if area == 'nethernet_framing':
                segments = (int(name.removeprefix('message_')) + 16382) // 16383 if name.startswith('message_') else '—'
                segment_cell = f' {segments} |'
            lines.append(f'| {name} | {operation} |{segment_cell} ' + ' | '.join(
                fmt(r['ns_per_op']) if r else '—' for r in records) + ' |')
        if not cases:
            lines += ['', 'No public comparable micro API produced measurements; see coverage below.']
        lines += ['']
        if area == 'nethernet_framing':
            lines += ['Cinnabar micro uses 16,383-byte segment payloads plus a one-byte countdown. '
                      'Encode consumes every generated segment; reassembly includes segment decode. '
                      'The oversize case exceeds the 255-segment encode limit; invalid-count/sequence rows exercise actual public rejection paths.', '']
    gaps = []
    lines += ['## NetherNet loopback: setup and reliable echo', '',
              '| Size | Operation | Side | Samples/round | Mean µs | p50 µs | p99 µs | Max µs | msgs/s | MB/s | Ours/reference time |',
              '| --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
    for r in (r for r in aggregate if r['area'] == 'nethernet_loopback'):
        setup = r['operation'] == 'setup'
        size = r['payload_bytes'] or int(r['name'])
        samples = 1 if setup else metadata.get('split_count', 10) if size >= 4096 else metadata.get('count', 100)
        ours = index.get((r['area'], r['name'], r['operation'], 'rust-owned'))
        ratio = ours['ns_per_op'] / r['ns_per_op'] if ours and r['implementation'] != 'rust-owned' and r['ns_per_op'] else None
        lines.append(f"| {r['name']} | {r['operation']} | {r['implementation']} | {samples} | "
                     f"{fmt(r['ns_per_op'] / 1000)} | {fmt(r['p50_us'])} | {fmt(r['p99_us'])} | {fmt(r['max_us'])} | "
                     f"{fmt(None if setup else r['msgs_per_sec'])} | {fmt(None if setup else r['mb_per_sec'])} | {fmt(ratio)} |")
    lines += ['', 'Ten-sample p99 equals the maximum; it is not a stable population tail estimate. '
              'Setup has one observation per connection; setup percentile columns, if emitted, describe that single sample.', '']
    for r in aggregate:
        ours = index.get((r['area'], r['name'], r['operation'], 'rust-owned'))
        if ours and r['implementation'] != 'rust-owned' and r['ns_per_op']:
            gaps.append((ours['ns_per_op'] / r['ns_per_op'], r))
    lines += ['## NetherNet public API coverage', '',
              '| Case | Side | Unavailable because |', '| --- | --- | --- |']
    lines += [f'| {clean(name)} | {clean(side)} | {clean(reason)} |'
              for name, side, reason in sorted(exclusions)]
    lines += ['', 'Missing cells are unavailable APIs, not zero time. No private framing algorithm was copied to manufacture a comparison.', '',
              '## NetherNet interop sanity', '', '| Direction | Result | Evidence |', '| --- | --- | --- |']
    interop = json.loads((source / 'interop.json').read_text(encoding='utf-8-sig'))
    if isinstance(interop, dict):
        interop = [interop]
    for item in interop:
        lines.append(f"| {clean(item['direction'])} | {clean(item['status'])} | {clean(item.get('error', 'All four payload sizes verified byte-for-byte'))} |")
    if clocks:
        lines += ['', '## NetherNet Go QPC validation', '',
                  '| Round | Clock | Frequency Hz | Tick ns | Minimum positive ns |',
                  '| ---: | --- | ---: | ---: | ---: |']
        for number, clock in clocks:
            lines.append(f"| {number} | {clock['clock']} | {clock['frequency_hz']} | {fmt(clock['tick_ns'])} | {fmt(clock['minimum_positive_ns'])} |")
    lines += ['', '## NetherNet top three measured gaps', '']
    for ratio, row in sorted(gaps, key=lambda pair: pair[0], reverse=True)[:3]:
        lines.append(f"- {row['area']} / {row['name']} / {row['operation']}: ours takes {ratio:.2f}× {row['implementation']} time. Profile this boundary before attributing a cause.")
    lines += ['', 'Reproduce: `./tools/bench-compare/run.ps1 -Area nethernet -Rounds 5`. '
              'Raw per-round JSONL, stderr, metadata, interop logs and summary.json are retained in the ignored results directory. '
              'Other workers may run concurrently on disjoint allowed CPUs; affinity does not eliminate shared package power/cache contention.', '']
    return '\n'.join(lines), dict(metadata=metadata, rows=aggregate, interop=interop)


def update(destination, section):
    existing = destination.read_text(encoding='utf-8') if destination.exists() else ''
    prefix = existing.split(MARKER, 1)[0].rstrip()
    destination.write_text(prefix + '\n\n' + MARKER + '\n\n' + section, encoding='utf-8')


def main():
    source, destination = map(Path, sys.argv[1:])
    section, summary = render(source)
    (source / 'summary.json').write_text(json.dumps(summary, indent=2), encoding='utf-8')
    update(destination, section)


if __name__ == '__main__':
    main()
