"""Aggregate only observed benchmark rows; preserve missing public APIs as gaps."""
import json
import statistics
import sys
from pathlib import Path


def median(rows, metric):
    values = [r[metric] for r in rows if isinstance(r.get(metric), (float, int))]
    return statistics.median(values) if values else None


def fmt(value):
    return '—' if value is None else f'{value:,.2f}'


def main():
    source, destination = map(Path, sys.argv[1:])
    metadata = json.loads((source / 'metadata.json').read_text(encoding='utf-8-sig'))
    rows = []
    for file in sorted(source.glob('[0-9]*-*.jsonl')):
        if not 1 <= int(file.name.split('-')[0]) <= metadata['rounds']:
            continue
        for line in file.read_text(encoding='utf-8-sig').splitlines():
            if line.strip():
                row = json.loads(line)
                row['round'] = int(file.name.split('-')[0])
                rows.append(row)
    if not rows:
        raise ValueError('No measured rows')
    groups = {}
    for row in rows:
        key = row['area'], row['name'], row['operation'], row['implementation']
        groups.setdefault(key, []).append(row)
    metrics = ('ns_per_op', 'allocs_per_op', 'bytes_per_op', 'payload_bytes',
               'msgs_per_sec', 'mb_per_sec', 'p50_us', 'p99_us')
    aggregate = [{**dict(zip(('area','name','operation','implementation'), key)),
                  **{metric: median(values, metric) for metric in metrics},
                  'rounds': len(values)} for key, values in sorted(groups.items())]
    (source / 'summary.json').write_text(json.dumps({'metadata': metadata, 'rows': aggregate}, indent=2))
    index = {(r['area'], r['name'], r['operation'], r['implementation']): r for r in aggregate}
    report = ['# Cross-implementation benchmark report', '',
              f"Measured UTC: {metadata['utc']}. Median of {metadata['rounds']} alternating rounds; {metadata['count']} sequential echoes per size per round.", '',
              f"Machine: {metadata['os']} / {metadata['architecture']}; {metadata.get('cpu')}; {metadata['processors']} logical processors.",
              f"Toolchains: `{metadata['rust']}`; `{metadata['go']}`; Go GOMAXPROCS={metadata['go_cpu']}.",
              f"Source base: `{metadata['commit']}`; protocolgen corpus: `{metadata['corpus_commit']}`.", '', metadata['conditions'], '',
              'Ratios are ours/Go ns/op (below 1 is faster); throughput is request payload only, decimal MB/s, one completed echo per message. Medians are descriptive, not significance tests.', '']
    ratios = []
    for area in sorted({r['area'] for r in aggregate}):
        report += [f'## {area}', '']
        if area == 'loopback':
            report += ['| Size / operation | Side | ns/echo | msgs/s | MB/s | p50 µs | p99 µs | ours/Go | Winner |',
                       '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |']
        else:
            report += ['| Case | Operation | Side | ns/op | allocs/op | allocated B/op | ours/Go | Winner |',
                       '| --- | --- | --- | ---: | ---: | ---: | ---: | --- |']
        for r in (r for r in aggregate if r['area'] == area):
            go = index.get((area, r['name'], r['operation'], 'go'))
            ratio = r['ns_per_op'] / go['ns_per_op'] if go and go['ns_per_op'] and r['implementation'] != 'go' else None
            winner = '—' if ratio is None else ('ours' if ratio < 1 else 'Go')
            if ratio is not None:
                ratios.append((ratio, area, r['name'], r['operation'], r['implementation']))
            if area == 'loopback':
                report.append(f"| {r['name']} / {r['operation']} | {r['implementation']} | {fmt(r['ns_per_op'])} | {fmt(r['msgs_per_sec'])} | {fmt(r['mb_per_sec'])} | {fmt(r['p50_us'])} | {fmt(r['p99_us'])} | {fmt(ratio)} | {winner} |")
            else:
                report.append(f"| {r['name']} | {r['operation']} | {r['implementation']} | {fmt(r['ns_per_op'])} | {fmt(r['allocs_per_op'])} | {fmt(r['bytes_per_op'])} | {fmt(ratio)} | {winner} |")
        report.append('')
    contract = []
    for estimates in sorted((source / 'contract-criterion').glob('*/*/new/estimates.json')):
        result = json.loads(estimates.read_text())
        contract.append((estimates.parents[2].name, estimates.parents[1].name, result['median']['point_estimate']))
    if contract:
        report += ['## Protocolgen contract baseline (third implementation)', '',
                   'Criterion median estimates; independent sampling, not the alternating custom-loop estimator. Length framing matches the original protocolgen benchmark; see README for the Valentine boundary caveat.', '',
                   '| Case | Operation | Contract ns/op |', '| --- | --- | ---: |']
        report += [f'| {name} | {operation} | {fmt(ns)} |' for operation, name, ns in contract]
        report.append('')
    excluded = set()
    for log in source.glob('[0-9]*-*-codec.stderr.log'):
        if not 1 <= int(log.name.split('-')[0]) <= metadata['rounds']:
            continue
        for line in log.read_text(encoding='utf-8-sig').splitlines():
            if not line.startswith('{'):
                continue
            record = json.loads(line)
            if record.get('area') == 'coverage' and record.get('status') == 'unavailable':
                excluded.add((record['name'], record['implementation'], record.get('reason', 'no public typed API')))
    report += ['## Corpus exclusions and API coverage', '',
               'Corpus encoder: gophertunnel `85d4022854e1` (`v1.25.3-0.20261019151115-85d4022854e1`), as pinned by protocolgen retail/go.mod. Requested live-master reference: HashimTheArab/gophertunnel `80c811b6186016b3860c358368cfa47e507f26e9`; go-raknet: `87a6b7e53799290d70ac25bcd03645d7ead00d2c`. The newer corpus encoder is not silently substituted for the requested reference.', '',
               '| Case | Implementation | Excluded because |', '| --- | --- | --- |']
    for name, implementation, reason in sorted(excluded):
        report.append(f"| {name} | {implementation} | {str(reason).replace('|', '/').replace(chr(10), ' ')} |")
    report += ['', 'Borrowed encoding has no public API. Go RakNet datagram/split micro APIs are private; only Rust micro timings are available, with no speed ratio or winner. Go compression-only rows have no equivalent jolyne comparison in this run.', '']
    interop = json.loads((source / 'interop.json').read_text(encoding='utf-8-sig'))
    if isinstance(interop, dict):
        interop = [interop]
    report += ['## Transport interop sanity', '', '| Direction | Sizes | Echoes per size | Result |', '| --- | --- | ---: | --- |']
    for result in interop:
        report.append(f"| {result['direction']} | {','.join(map(str,result['sizes']))} | {result['count']} | {result['status']} |")
        if result.get('error'):
            report += ['', 'Failure evidence: `' + result['error'].replace('`', "'").replace('\n', ' ') + '`']
    report += ['', '## Highest measured optimization gaps', '']
    for ratio, area, name, operation, implementation in sorted(ratios, reverse=True)[:3]:
        report.append(f'- {area} / {name} / {operation} ({implementation}): {ratio:.2f}× Go time. Profile this measured boundary before attributing a cause.')
    report += ['', '## Limits and reproducibility', '',
               'See [README.md](README.md), [Go API caveats](go/CAVEATS.md), and [Rust API caveats](../../crates/bench-compare/README.md). Missing rows are not zero-cost operations: private transport internals and untyped borrowed variants cannot be compared through public APIs.', '',
               'Go GC accounting and Rust allocator accounting are not equivalent retained-memory measurements. Timing loops include destruction; Go GC may run during later iterations. Small timings on Windows depend on clock precision and scheduling. No production implementation was optimized for this run.', '',
               'Raw per-round JSONL, stderr, metadata, interop outcomes and summary.json remain in the ignored results directory. Run `./tools/bench-compare/run.ps1 -Contract` to regenerate.']
    destination.write_text('\n'.join(report) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
