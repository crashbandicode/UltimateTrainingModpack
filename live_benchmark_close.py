"""Parent-only stopped-run evidence preservation and benchmark restoration.

Run under a separately recorded finite inspection grant. This reports evidence;
it does not infer combat boundaries, pair validity, or a performance verdict.
"""
import json
from pathlib import Path
import re
import subprocess
import sys
import time

import live_acceptance as s
import live_benchmark_stage as stage_helper
import live_perf as perf

INSPECTOR = Path('/tmp/opencode/named-macros-host-target/debug/capture-archive-check')
INSPECTOR_SHA = '1569c2121a21fe45aa1d7577389201b7c1abb02f588e64e6316aed90f84d58d8'


def close(token):
    stage = stage_helper.bench_stage(token)
    receipt = json.loads((stage / 'receipt.json').read_text())
    s.require(receipt['state'] == 'installed' and len(receipt['runs']) == 1,
              'Expected one stopped run in an installed benchmark campaign')
    owned = receipt['runs'][0]
    run_dir = perf.run_path(owned['phase'])
    run, _ = perf.read_manifest(run_dir)
    perf.validate_manifest(run, run_dir, live=False)
    s.stopped()
    pids = [owned[key] for key in ('launcher_pid', 'guard_pid', 'normal_stop_pid')]
    identity = run_dir / 'runtime-identity.json'
    if identity.exists():
        pids.append(json.loads(identity.read_text())['pid'])
    stop_log = s.ROOT / f'normal-stop-{owned["phase"]}.log'
    if stop_log.exists():
        first = stop_log.read_text().splitlines()
        if first:
            pids.append(json.loads(first[0])['pid'])
    s.require(all(not Path(f'/proc/{pid}').exists() for pid in pids), 'Owned runtime PID still present')
    output = stage / 'benchmark-closure.json'
    s.require(not output.exists(), 'Closure evidence already exists; inspect instead of overwriting')
    report = {'at': time.time(), 'token': token, 'phase': owned['phase'],
              'run_manifest': run, 'absent_pids': pids, 'errors': [],
              'performance_acceptance': False,
              'qualification': 'Parent must inspect screenshots, cache/fixture equality and pair comparability.'}
    try:
        log = (run_dir / 'console.log').read_text(errors='replace')
        report['capture_enabled_log'] = re.findall(r'\[ReplayCapture\] live_enabled=(true|false)', log)
        report['native_bound_keys'] = [list(map(int, pair)) for pair in re.findall(
            r'\[ReplayCapture\] save_bound generation=(\d+) index=(\d+)', log)]
        report['persisted_archive_ids'] = re.findall(r'\[ReplayCapture\] persisted id=([a-f0-9]+)', log)
        # Preserve the entire native saves tree through the stager's restore below.
        # Compare replay names to the exact fixture, never an inferred latest buffer.
        fixture_receipt = json.loads((stage_helper.FIXTURE2 / 'receipt.json').read_text())
        fixture_closure = json.loads((stage_helper.FIXTURE2 / 'closure-verification.json').read_text())
        baseline = next(item['manifest'] for item in fixture_receipt['after']
                        if item['tree'] == 'saves' and item['path'] == fixture_closure['preserved_saves'])
        saves = s.tree(s.TREES['saves'])
        new_replays = {name: digest for name, digest in saves['files'].items()
                       if re.fullmatch(r'replay\d+\.bin', Path(name).name)
                       and name not in baseline['files']}
        report['new_native_replays'] = new_replays
        captures = s.SETTINGS / 'replay_captures'
        if captures.exists():
            s.require(s.sha(INSPECTOR) == INSPECTOR_SHA, 'Stopped inspector pin mismatch')
            with (stage / 'benchmark-inspector.json').open('x') as out, \
                    (stage / 'benchmark-inspector.log').open('x') as err:
                subprocess.run(s.PS + [f"& '{INSPECTOR}' '{captures}'"], stdout=out, stderr=err,
                               check=True, timeout=90)
            inspection = json.loads((stage / 'benchmark-inspector.json').read_text())
            report['inspection'] = inspection
            report['capture_native_association'] = [
                {'replay_id': capture['replay_id'],
                 'new_native_file_present': capture['replay_id'] in {Path(n).name for n in new_replays},
                 'persisted_log_present': capture['replay_id'].encode().hex() in report['persisted_archive_ids']}
                for capture in inspection['captures']]
        else:
            report['inspection'] = {'captures': [], 'skipped': [], 'directory_present': False}
        # Each run lasts more than a minute and is serialized; preserve promptly.
        # Still refuse ambiguous/missing CSV provenance instead of picking newest.
        csvs = [p for p in (s.PROFILE / 'data/eden/log').glob('*_01006A800016E000.csv')
                if run['started_at'] <= p.stat().st_mtime <= run['ended_at'] + 30]
        s.require(len(csvs) == 1, f'Expected one shutdown CSV, found: {[str(p) for p in csvs]}')
        csv = csvs[0]
        raw = perf.read_bounded(csv, perf.MAX_CSV_BYTES)
        with (stage / 'whole-session-frame-times.csv').open('xb') as out:
            out.write(raw)
        report['csv'] = {'source': str(csv), 'sha256': s.sha(csv), **perf.frame_summary(perf.parse_csv(raw))}
        before = set(run_dir.glob('perf-analysis-*.json'))
        for collection in sorted(run_dir.glob('perf-*.jsonl')):
            s.require(perf.analyze(collection, csv) == 0, 'Collection analysis failed')
        report['analyses'] = [str(p) for p in sorted(set(run_dir.glob('perf-analysis-*.json')) - before)]
    except Exception as error:
        report['errors'].append({'type': type(error).__name__, 'message': str(error)})
    finally:
        # Evidence inspection failure must not leave an otherwise restorable clone staged.
        try:
            receipt = stage_helper.restore(stage, receipt)
            report['restoration_exact'] = (
                {k: s.tree(v) for k, v in s.TREES.items()} == receipt['before']
                and s.tree(stage_helper.CACHE) == receipt['benchmark']['before_cache_manifest'])
            s.verify_protected(receipt)
            s.require(report['restoration_exact'], 'Post-restore verification failed')
            s.require(s.inventory() == {'libarcropolis.nro': s.ARC}, 'ARC-only restoration failed')
            report['restored_at'] = receipt.get('restored_at')
        except Exception as error:
            report['errors'].append({'type': type(error).__name__, 'restoration_error': str(error)})
        with output.open('x') as out:
            json.dump(report, out, indent=2)
            out.write('\n')
    print(json.dumps({'report': str(output), 'errors': report['errors'],
                      'restoration_exact': report.get('restoration_exact', False)}))
    return 1 if report['errors'] else 0


if __name__ == '__main__':
    sys.exit(close(sys.argv[1]))
