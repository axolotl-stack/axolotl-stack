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
               'msgs_per_sec', 'mb_per_sec', 'p50_us', 'p99_us', 'max_us')
    aggregate = [{**dict(zip(('area','name','operation','implementation'), key)),
                  **{metric: median(values, metric) for metric in metrics},
                  'rounds': len(values)} for key, values in sorted(groups.items())]
    (source / 'summary.json').write_text(json.dumps({'metadata': metadata, 'rows': aggregate}, indent=2))
    index = {(r['area'], r['name'], r['operation'], r['implementation']): r for r in aggregate}
    report = ['# Cross-implementation benchmark report', '',
              f"Measured UTC: {metadata['utc']}. Median of {metadata['rounds']} alternating rounds; measured echoes per connection: {metadata['count']} for 32/512 B and {metadata.get('split_count', metadata['count'])} for 4096/65536 B; untimed readiness echoes per connection: {metadata.get('warmup_echoes', 'not recorded')}.", '',
              f"Machine: {metadata['os']} / {metadata['architecture']}; {metadata.get('cpu')}; {metadata['processors']} logical processors.",
              f"Toolchains: `{metadata['rust']}`; `{metadata['go']}`; Go GOMAXPROCS={metadata['go_cpu']}; Tokio multi-thread workers={metadata.get('rust_workers', 'not recorded')}.",
              f"Source base: `{metadata['commit']}`; protocolgen corpus: `{metadata['corpus_commit']}`.", '', metadata['conditions'], '',
              'Ratios are row-side/Go ns/op (below 1 is faster); throughput is request payload only, decimal MB/s, one completed echo per message. Contract, Valentine and Cinnabar use the same custom-loop estimator in alternating rounds. Medians are descriptive, not significance tests.', '']
    if metadata.get('cinnabar_raknet_commit'):
        report += [f"Cinnabar pins: raknet/raknet-tokio `{metadata['cinnabar_raknet_commit']}`; bedrock-net `{metadata['cinnabar_bedrock_net_commit']}` (root crate only; provider-verification disabled).", '']
    if metadata.get('rust_release_profile'):
        report += [f"Release flags: {metadata['rust_release_profile']}.", '']
    clocks = []
    for file in sorted(source.glob('[0-9]*-go-loopback.stderr.log')):
        round_number = int(file.name.split('-')[0])
        if not 1 <= round_number <= metadata['rounds']:
            continue
        for line in file.read_text(encoding='utf-8-sig').splitlines():
            if line.startswith('{'):
                evidence = json.loads(line)
                if evidence.get('event') == 'benchmark_clock':
                    clocks.append((round_number, evidence))
    if clocks:
        report += ['## Go transport clock validation', '',
                   '| Round | Clock | Frequency Hz | Tick ns | Minimum observed positive ns |',
                   '| ---: | --- | ---: | ---: | ---: |']
        report += [f"| {number} | {clock['clock']} | {clock['frequency_hz']} | {fmt(clock['tick_ns'])} | {fmt(clock['minimum_positive_ns'])} |" for number, clock in clocks]
        report += ['', 'Startup validation is outside timing. Windows transport uses QueryPerformanceCounter; codec/batch aggregate Go benchmarks retain Go testing.Benchmark over long intervals.', '']
    ratios = []
    sides = ('rust-owned', 'rust-borrowed', 'contract', 'cinnabar', 'go', 'flate2-rust')
    for area in sorted({r['area'] for r in aggregate}):
        report += [f'## {area}', '']
        if area == 'loopback':
            report += ['| Size / operation | Side | Samples | Mean µs | msgs/s | MB/s | p50 µs | p99 µs | Max µs | Side/Go |',
                       '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |']
            for r in (r for r in aggregate if r['area'] == area):
                go = index.get((area, r['name'], r['operation'], 'go'))
                ratio = r['ns_per_op'] / go['ns_per_op'] if go and go['ns_per_op'] and r['implementation'] != 'go' else None
                samples = metadata.get('split_count', metadata['count']) if r['payload_bytes'] >= 4096 else metadata['count']
                tail = fmt(r['p99_us']) + (' †' if samples < 100 else '')
                report.append(f"| {r['name']} / {r['operation']} | {r['implementation']} | {samples} | {fmt(r['ns_per_op'] / 1000)} | {fmt(r['msgs_per_sec'])} | {fmt(r['mb_per_sec'])} | {fmt(r['p50_us'])} | {tail} | {fmt(r['max_us'])} | {fmt(ratio)} |")
                if ratio is not None:
                    ratios.append((ratio, area, r['name'], r['operation'], r['implementation']))
            report += ['', '† Low-sample p99: for ten samples, nearest-rank p99 is the maximum, not a stable population tail estimate. All implementations use identical counts and fresh connections. Columns are medians of per-round mean/percentile/max statistics, not pooled samples.', '']
            continue
        report += ['| Case | Operation | Valentine/jolyne owned ns/op | Valentine borrowed ns/op | Contract ns/op | Cinnabar ns/op | Go ns/op | flate2-rust ns/op | Fastest measured |',
                   '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |']
        cases = sorted({(r['name'], r['operation']) for r in aggregate if r['area'] == area})
        for name, operation in cases:
            records = [index.get((area, name, operation, side)) for side in sides]
            available = [r for r in records if r is not None]
            winner = min(available, key=lambda r: r['ns_per_op'])['implementation'] if len(available) > 1 else 'unmatched'
            report.append(f"| {name} | {operation} | " + ' | '.join(fmt(r['ns_per_op']) if r else '—' for r in records) + f' | {winner} |')
            go = index.get((area, name, operation, 'go'))
            if go and go['ns_per_op']:
                ratios.extend((r['ns_per_op'] / go['ns_per_op'], area, name, operation, r['implementation']) for r in available if r['implementation'] != 'go')
        report += ['', '<details><summary>Allocation requests (median per round)</summary>', '',
                   '| Case | Operation | Side | allocs/op | allocated B/op |',
                   '| --- | --- | --- | ---: | ---: |']
        for r in (r for r in aggregate if r['area'] == area):
            report.append(f"| {r['name']} | {r['operation']} | {r['implementation']} | {fmt(r['allocs_per_op'])} | {fmt(r['bytes_per_op'])} |")
        report += ['', '</details>', '']
    excluded = set()
    for log in source.glob('[0-9]*-*.stderr.log'):
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
    report += ['', 'Borrowed encoding has no public API. Go RakNet datagram/split/ACK micro APIs and Cinnabar split assembly are private. Missing cells mean unavailable, not zero cost. Batch framing excludes typed packet decoding; callback-vs-owned API and compression backend asymmetries are documented in the READMEs. Jolyne has no public isolated encryption API.', '']
    interop = json.loads((source / 'interop.json').read_text(encoding='utf-8-sig'))
    if isinstance(interop, dict):
        interop = [interop]
    report += ['## Transport interop sanity', '', '| Direction | Sizes | Echoes per size | Result |', '| --- | --- | ---: | --- |']
    for result in interop:
        counts = [result.get('split_count', result['count']) if size >= 4096 else result['count'] for size in result['sizes']]
        report.append(f"| {result['direction']} | {','.join(map(str,result['sizes']))} | {','.join(map(str, counts))} | {result['status']} |")
        if result.get('error'):
            report += ['', 'Failure evidence: `' + result['error'].replace('`', "'").replace('\n', ' ') + '`']
    crosscheck_path = Path(__file__).with_name('cinnabar') / 'upstream-check.json'
    if crosscheck_path.exists():
        crosscheck = json.loads(crosscheck_path.read_text(encoding='utf-8-sig'))
        report += ['', '## Upstream RakNet self-benchmark cross-check', '',
                   crosscheck['conclusion'], '',
                   'This separate diagnostic does not enter the alternating comparison medians. Exact upstream commands, configuration audit and limitations: [upstream-check.md](cinnabar/upstream-check.md); reproduction: [upstream-check.ps1](cinnabar/upstream-check.ps1).', '']
    diagnostic = json.loads(Path(__file__).with_name('diagnostics.json').read_text())
    failure = diagnostic['failure']
    report += ['', '## Reference split/retransmission finding (separate diagnostic runs)', '',
               f"An uninstrumented {failure['direction']} run of `{failure['command']}` failed at {failure['measured_size']} B, measured message index {failure['measured_message_index']}: `{failure['error']}`. No Rust peer participated in that failure; no public listener API overrides this private limit.", '',
               'The coordinator-approved timed matrix retains 100 small-message echoes and uses ten measured plus one readiness echo for split-message sizes on both implementations. Larger-count failures remain failures: the harness neither retries nor changes production/reference sources.', '',
               '| Direction | Diagnostic attempt | Completed split groups | Duplicate indexes after completion | Exit code |',
               '| --- | ---: | ---: | ---: | ---: |']
    for observed in diagnostic['observations']:
        report.append(f"| {observed['direction']} | {observed['attempt']} | {observed['completed_groups']} | {observed['duplicate_indexes_after_completion']} | {observed['exit_code']} |")
    report += ['', diagnostic['interpretation'], '',
               '| Late datagram sequence | Split ID | Fragment index | Fragment count |',
               '| ---: | ---: | ---: | ---: |']
    for example in diagnostic['late_examples']:
        report.append(f"| {example['datagram_sequence']} | {example['split_id']} | {example['fragment_index']} | {example['fragment_count']} |")
    report += ['', 'The successful Go-to-Go trace with late fragments covered 17 distinct completed split IDs. All instrumented captures used GOMAXPROCS=4, 100 measured echoes plus readiness; the exact failing standalone run did not record concurrency. The public UDP socket observer is disabled for timed measurements. Reproduction and distilled evidence: [diagnostics.json](diagnostics.json); raw packet metadata logs remain ignored under results/diagnostics/.', '']
    report += ['', '## Highest measured optimization gaps', '']
    for ratio, area, name, operation, implementation in sorted(ratios, reverse=True)[:3]:
        report.append(f'- {area} / {name} / {operation} ({implementation}): {ratio:.2f}× Go time. Profile this measured boundary before attributing a cause.')
    report += ['', '## Limits and reproducibility', '',
               'See [README.md](README.md), [Go API caveats](go/CAVEATS.md), and [Rust API caveats](../../crates/bench-compare/README.md). Missing rows are not zero-cost operations: private transport internals and untyped borrowed variants cannot be compared through public APIs.', '',
               'Go GC accounting and Rust allocator accounting are not equivalent retained-memory measurements. Timing loops include destruction; Go GC may run during later iterations. Small timings on Windows depend on clock precision and scheduling. No production implementation was optimized for this run.', '',
               'Raw per-round JSONL, stderr, metadata, interop outcomes and summary.json remain in the ignored results directory. Run `./tools/bench-compare/run.ps1` to regenerate.']
    if any(r['area'] == 'loopback' and r['implementation'] == 'go' and r['p50_us'] == 0 for r in aggregate):
        report += ['', 'Observed Go p50 RTT values include zero. Clock quantization may contribute [INFERENCE]; these values must not be interpreted as physically zero RTT. Throughput uses the complete elapsed exchange interval.']
    destination.write_text('\n'.join(report) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
