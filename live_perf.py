#!/usr/bin/env python3
"""Read-only live status/resource sampling and offline whole-session CSV analysis.

Labels are parent-supplied phase marks, never inferred gameplay phases. No runtime
mutation, process inspection, pair qualification or performance PASS thresholds.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import re
import signal
import statistics
import sys
import time
import uuid

RUNS = Path('/home/intpa/eden-replay/acceptance-fable/runs')
CGROUP = Path('/sys/fs/cgroup')
CLIENT_SOURCE = Path('/home/intpa/eden-cli-control/tools/edenctl.py')
PINNED_COMMIT = '10bcd2d849843b146a79c61a21df615e1bdcfcde'
MAX_FRAMES = 216000
PERIOD = 0.5
RESOURCE_PERIOD = 1.0
MAX_CSV_BYTES = 16 * 1024 * 1024


def finite(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool) and math.isfinite(value)


def seconds_bound(value):
    value = float(value)
    if not math.isfinite(value) or not 1 <= value <= 120:
        raise ValueError('seconds must be finite and in [1, 120]')
    return value


def contained(path, root, *, exists=True):
    root = Path(root).resolve(strict=True)
    path = Path(path).resolve(strict=exists)
    if path == root or not path.is_relative_to(root):
        raise ValueError(f'path escapes owned root: {path}')
    return path


def run_path(phase, root=RUNS):
    if not isinstance(phase, str) or not re.fullmatch(r'live-[A-Za-z0-9][A-Za-z0-9_-]{0,35}', phase):
        raise ValueError('phase must be live-* and at most 41 safe characters')
    path = contained(Path(root) / phase, root)
    if not path.is_dir():
        raise ValueError('existing owned run directory required')
    return path


def duration(value):
    if not isinstance(value, str):
        raise ValueError('missing verified RuntimeMaxUSec')
    tokens = re.findall(r'([0-9]+(?:\.[0-9]+)?)(us|ms|min|s|h|d)', value)
    if not tokens or re.sub(r'([0-9]+(?:\.[0-9]+)?)(us|ms|min|s|h|d)', '', value).strip():
        raise ValueError('invalid verified runtime duration')
    scales = dict(us=1e-6, ms=1e-3, s=1, min=60, h=3600, d=86400)
    return sum(float(n) * scales[u] for n, u in tokens)


def validate_manifest(manifest, run, *, live, cgroup_root=CGROUP):
    if not isinstance(manifest, dict):
        raise ValueError('run.json must be an object')
    if manifest.get('runtime_seconds') != 900 or manifest.get('memory_max_bytes') != 8 * 1024**3 or manifest.get('memory_swap_max_bytes') != 0:
        raise ValueError('run must declare runtime900/8GiB/noSwap')
    limits = manifest.get('verified_limits', {})
    if str(limits.get('MemoryMax')) != str(8 * 1024**3) or str(limits.get('MemorySwapMax')) != '0' or duration(limits.get('RuntimeMaxUSec')) != 900:
        raise ValueError('run limits not verified runtime900/8GiB/noSwap')
    unit = manifest.get('unit', '')
    if not re.fullmatch(r'eden-control-[0-9a-f]{12}\.service', unit):
        raise ValueError('not an owned eden-control unit')
    group = limits.get('ControlGroup', '')
    if not isinstance(group, str) or not group.startswith('/') or '..' in Path(group).parts or Path(group).name != unit:
        raise ValueError('ControlGroup must name the exact owned unit')
    group_path = contained(Path(cgroup_root) / group.lstrip('/'), cgroup_root, exists=live)
    socket_path = contained(manifest.get('socket', ''), run, exists=False)
    if socket_path != Path(run) / 'control.sock':
        raise ValueError('socket is not the owned run control.sock')
    if live and manifest.get('state') != 'ready':
        raise ValueError('collection requires an already-ready live run')
    if not live and manifest.get('state') != 'stopped':
        raise ValueError('CSV analysis requires a stopped run')
    recorded = manifest.get('last_status', {}).get('commit')
    if recorded is not None and recorded != PINNED_COMMIT:
        raise ValueError('manifest status commit differs from expected pin')
    return socket_path, group_path


def read_manifest(run):
    path = contained(run / 'run.json', run)
    raw = path.read_bytes()
    return json.loads(raw), hashlib.sha256(raw).hexdigest()


def stamp():
    return {'mono': time.monotonic(), 'utc': datetime.now(timezone.utc).isoformat()}


def error_text(error):
    return {'type': type(error).__name__, 'message': str(error)}


def emit(stream, row):
    stream.write(json.dumps(row, allow_nan=False, separators=(',', ':')) + '\n')
    stream.flush()


def counters(text):
    result = {}
    for line in text.splitlines():
        key, value = line.split()
        if key in result or not value.isdecimal():
            raise ValueError('invalid or duplicate cgroup counter')
        result[key] = int(value)
    return result


def resource_snapshot(group):
    row = {'start': stamp(), 'values': {}, 'errors': {}}
    for name in ('cpu.stat', 'memory.current', 'memory.swap.current', 'memory.peak',
                 'memory.events', 'memory.swap.events', 'cgroup.events'):
        try:
            path = contained(group / name, group)
            text = path.read_text(encoding='ascii').strip()
            if name in ('cpu.stat', 'memory.events', 'memory.swap.events', 'cgroup.events'):
                value = counters(text)
            elif text.isdecimal():
                value = int(text)
            else:
                raise ValueError('invalid cgroup scalar')
            row['values'][name] = value
        except (OSError, ValueError) as error:
            row['errors'][name] = error_text(error)
    row['end'] = stamp()
    return row


def client_class():
    spec = importlib.util.spec_from_file_location('live_perf_edenctl', CLIENT_SOURCE)
    module = importlib.util.module_from_spec(spec)
    # Loading the existing client must not create artifacts in its source tree.
    old = sys.dont_write_bytecode
    try:
        sys.dont_write_bytecode = True
        spec.loader.exec_module(module)
    finally:
        sys.dont_write_bytecode = old
    return module.Client


def collect(phase, label, seconds):
    seconds = seconds_bound(seconds)
    if not isinstance(label, str) or not label.strip() or len(label) > 160:
        raise ValueError('label must be nonempty and at most 160 characters')
    run = run_path(phase)
    manifest, manifest_sha = read_manifest(run)
    socket_path, group = validate_manifest(manifest, run, live=True)
    Client = client_class()
    target = run / ('perf-' + uuid.uuid4().hex + '.jsonl')
    interrupted = False
    def interrupt(*_):
        nonlocal interrupted
        interrupted = True
    old_handlers = {sig: signal.signal(sig, interrupt) for sig in (signal.SIGTERM, signal.SIGINT)}
    reason, fatal = 'duration_complete', None
    try:
        with target.open('x', encoding='utf-8') as stream:
            start = stamp()
            end_mono = start['mono'] + seconds
            emit(stream, {'kind': 'start', 'format': 1, 'phase': phase, 'label': label,
                          'at': start, 'seconds': seconds, 'status_period_seconds': PERIOD,
                          'resource_period_seconds': RESOURCE_PERIOD, 'rpc_timeout_seconds': 2,
                          'expected_commit': PINNED_COMMIT, 'manifest_sha256': manifest_sha,
                          'manifest': manifest, 'control_group': str(group),
                          'resource_start': resource_snapshot(group),
                          'semantics': 'Parent label only; rolling500ms FPS; no independent frame/tick/drop claims.'})
            status_slot = resource_slot = 0
            try:
                while time.monotonic() < end_mono and not interrupted:
                    now = time.monotonic()
                    next_status = start['mono'] + status_slot * PERIOD
                    next_resource = start['mono'] + resource_slot * RESOURCE_PERIOD
                    if now < min(next_status, next_resource):
                        time.sleep(min(min(next_status, next_resource) - now, 0.1))
                        continue
                    # Skip expired slots instead of burst polling cached statistics.
                    while start['mono'] + (status_slot + 1) * PERIOD <= now:
                        emit(stream, {'kind': 'status_missing', 'slot': status_slot,
                                      'scheduled_mono': start['mono'] + status_slot * PERIOD,
                                      'at': stamp(), 'reason': 'deadline_missed'})
                        status_slot += 1
                    while start['mono'] + (resource_slot + 1) * RESOURCE_PERIOD <= now:
                        emit(stream, {'kind': 'resource_missing', 'slot': resource_slot,
                                      'scheduled_mono': start['mono'] + resource_slot * RESOURCE_PERIOD,
                                      'at': stamp(), 'reason': 'deadline_missed'})
                        resource_slot += 1
                    if now >= start['mono'] + resource_slot * RESOURCE_PERIOD:
                        emit(stream, {'kind': 'resource', 'slot': resource_slot,
                                      'scheduled_mono': start['mono'] + resource_slot * RESOURCE_PERIOD,
                                      **resource_snapshot(group)})
                        resource_slot += 1
                    if now >= start['mono'] + status_slot * PERIOD:
                        request = stamp()
                        row = {'kind': 'status', 'slot': status_slot,
                               'scheduled_mono': start['mono'] + status_slot * PERIOD, 'request': request}
                        try:
                            reply = Client(socket_path, timeout=min(2.0, max(0.001, end_mono - time.monotonic()))).request('status')
                            row['response'] = reply
                            details = reply.get('result')
                            if not reply.get('ok') or not isinstance(details, dict):
                                row['error'] = {'type': 'StatusError', 'message': 'status failed or result missing'}
                            elif details.get('commit') != PINNED_COMMIT:
                                row['error'] = {'type': 'CommitMismatch', 'message': 'status commit differs from expected pin'}
                                reason = 'commit_mismatch'
                            else:
                                row['pin_verified'] = True
                        except Exception as error:
                            row['error'] = error_text(error)
                        row['received'] = stamp()
                        row['rtt_seconds'] = row['received']['mono'] - request['mono']
                        emit(stream, row)
                        status_slot += 1
                        if reason == 'commit_mismatch':
                            break
                if interrupted:
                    reason = 'interrupted'
            except Exception as error:
                reason, fatal = 'collector_error', error_text(error)
            finally:
                # Explicitly retain every unvisited nominal sample slot on early exit.
                for kind, slot, period in (('status_missing', status_slot, PERIOD),
                                           ('resource_missing', resource_slot, RESOURCE_PERIOD)):
                    while start['mono'] + slot * period < end_mono:
                        emit(stream, {'kind': kind, 'slot': slot, 'scheduled_mono': start['mono'] + slot * period,
                                      'at': stamp(), 'reason': reason if reason != 'duration_complete' else 'deadline_missed'})
                        slot += 1
                try:
                    final_manifest, final_sha = read_manifest(run)
                    final_info = {'manifest': final_manifest, 'manifest_sha256': final_sha}
                except Exception as error:
                    final_info = {'error': error_text(error)}
                emit(stream, {'kind': 'end', 'at': stamp(), 'reason': reason, 'error': fatal,
                              'resource_end': resource_snapshot(group), 'run_end_observation': final_info})
    finally:
        for sig, handler in old_handlers.items():
            signal.signal(sig, handler)
    print(str(target))
    return 0 if reason == 'duration_complete' else 2


def nearest_rank(sorted_values, fraction):
    if not sorted_values or not 0 < fraction <= 1:
        raise ValueError('nonempty sorted samples and fraction in (0,1] required')
    return sorted_values[math.ceil(len(sorted_values) * fraction) - 1]


def parse_csv(raw):
    if len(raw) > MAX_CSV_BYTES:
        raise ValueError('CSV exceeds bounded input size')
    text = raw.decode('ascii')
    lines = text.splitlines()
    if not lines or len(lines) > MAX_FRAMES:
        raise ValueError('CSV must contain 1..216000 samples')
    values = []
    for number, line in enumerate(lines, 1):
        if not re.fullmatch(r'\s*(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\s*', line):
            raise ValueError(f'invalid newline-millisecond sample at line {number}')
        value = float(line)
        if not math.isfinite(value) or value < 0:
            raise ValueError(f'nonfinite/negative sample at line {number}')
        values.append(value)
    return values


def frame_summary(values):
    if not values or any(not finite(v) or v < 0 for v in values) or len(values) > MAX_FRAMES:
        raise ValueError('invalid frame samples')
    ordered = sorted(values)
    return {'scope': 'whole-session boot/menu/warmup/combat/results composition intervals; first5 omitted by Eden',
            'units': 'milliseconds', 'quantiles': 'nearest-rank ceil(p*N), one-based',
            'count': len(values), 'mean': statistics.fmean(values),
            'p50': nearest_rank(ordered, .5), 'p95': nearest_rank(ordered, .95),
            'p99': nearest_rank(ordered, .99), 'max': ordered[-1],
            'tails': {str(t): {'strictly_greater_count': sum(v > t for v in values),
                               'fraction': sum(v > t for v in values) / len(values)} for t in (33.33, 50.0)}}


def numeric_summary(values):
    values = [v for v in values if finite(v) and v >= 0]
    return {'count': len(values), 'min': min(values), 'median': statistics.median(values),
            'max': max(values)} if values else {'count': 0}


def gaps(times, period):
    differences = [b - a for a, b in zip(times, times[1:])]
    return {'actual_intervals_seconds': differences, 'nonpositive_intervals': sum(v <= 0 for v in differences),
            'max_interval_seconds': max(differences, default=None),
            'intervals_over_nominal': sum(v > period for v in differences), 'nominal_seconds': period}


def resource_summary(rows):
    memory = [r['values'].get('memory.current') for r in rows]
    intervals, resets = [], []
    for previous, current in zip(rows, rows[1:]):
        elapsed = current['start']['mono'] - previous['start']['mono']
        a, b = previous['values'].get('cpu.stat', {}), current['values'].get('cpu.stat', {})
        if elapsed <= 0:
            resets.append({'at': current['start'], 'reason': 'nonpositive_elapsed'})
            continue
        decreases = [key for key in a.keys() & b.keys() if b[key] < a[key]]
        if decreases:
            resets.append({'at': current['start'], 'reason': 'counter_reset', 'keys': decreases})
            continue
        if 'usage_usec' not in a or 'usage_usec' not in b:
            continue
        delta = {key: b[key] - a[key] for key in a.keys() & b.keys()}
        core_seconds = delta['usage_usec'] / 1e6
        intervals.append({'start_mono': previous['start']['mono'], 'end_mono': current['start']['mono'],
                          'actual_seconds': elapsed, 'cpu_core_seconds': core_seconds,
                          'mean_cores_used': core_seconds / elapsed, 'counter_deltas': delta,
                          'over_nominal_gap': elapsed > RESOURCE_PERIOD})
    covered = sum(r['actual_seconds'] for r in intervals)
    total = sum(r['cpu_core_seconds'] for r in intervals)
    return {'cpu_intervals': intervals, 'excluded_intervals': resets, 'covered_seconds': covered,
            'cpu_core_seconds': total, 'mean_cores_used_over_covered_intervals': total / covered if covered else None,
            'memory_current_bytes': numeric_summary(memory),
            'memory_swap_current_bytes': numeric_summary([r['values'].get('memory.swap.current') for r in rows]),
            'gaps': gaps([r['start']['mono'] for r in rows], RESOURCE_PERIOD),
            'read_errors': [{'at': r['start'], 'errors': r['errors']} for r in rows if r.get('errors')],
            'semantics': 'CPU deltas over actual seconds; one mean core=100% of one core. Memory includes charged cache/descendants, not heap allocations.'}


def status_summary(rows):
    attempts = [r for r in rows if r['kind'] == 'status']
    valid = [r for r in attempts if r.get('pin_verified') and not r.get('error')]
    results = [r['response']['result'] for r in valid]
    # Cache has no unique update ID. Age-derived update intervals can overlap;
    # report ambiguity rather than falsely deduplicating by equal FPS values.
    update_intervals = []
    for row, result in zip(valid, results):
        age = result.get('stats_age_ms')
        if finite(age) and age >= 0:
            update_intervals.append({'earliest_mono': row['request']['mono'] - (age + 1) / 1000,
                                     'latest_mono': row['received']['mono'] - age / 1000})
    return {'semantics': 'Rolling500ms FPS polling observations, not independent frames; repeated cache observations retained. No independence or time-below-rate inferred.',
            'attempts': len(attempts), 'valid_observations': len(valid),
            'missing_slots': [r for r in rows if r['kind'] == 'status_missing'],
            'failures': [r for r in attempts if r.get('error')],
            'game_fps_poll_observations': numeric_summary([r.get('game_fps') for r in results]),
            'system_fps_poll_observations': numeric_summary([r.get('system_fps') for r in results]),
            'stats_age_ms': numeric_summary([r.get('stats_age_ms') for r in results]),
            'last_frame_age_ms': numeric_summary([r.get('last_frame_age_ms') for r in results]),
            'missing_status_fields': {key: sum(key not in r for r in results) for key in
                                     ('game_fps', 'system_fps', 'stats_age_ms', 'last_frame_age_ms',
                                      'shaders_building', 'modal', 'running', 'powered_on', 'loading_complete')},
            'rtt_seconds': numeric_summary([r.get('rtt_seconds') for r in attempts]),
            'request_lateness_seconds': numeric_summary([r['request']['mono'] - r['scheduled_mono'] for r in attempts]),
            'gaps': gaps([r['request']['mono'] for r in attempts], PERIOD),
            'successful_observation_gaps': gaps([r['received']['mono'] for r in valid], PERIOD),
            'possible_cached_update_intervals': update_intervals,
            'cache_interval_semantics': 'Request/response bracket minus stats age, with 1ms age quantization uncertainty; no unique update ID.',
            'adjacent_cache_intervals_overlap': sum(b['earliest_mono'] <= a['latest_mono'] for a, b in zip(update_intervals, update_intervals[1:])),
            'shader_work_observations': sum(finite(r.get('shaders_building')) and r['shaders_building'] > 0 for r in results),
            'modal_observations': sum(bool(r.get('modal')) for r in results),
            'not_running_observations': sum(r.get('running') is not True for r in results),
            'not_powered_on_observations': sum(r.get('powered_on') is not True for r in results)}


def closed_collection(raw):
    if len(raw) > 32 * 1024 * 1024:
        raise ValueError('collection exceeds input bound')
    rows = [json.loads(line) for line in raw.decode('utf-8').splitlines()]
    if len(rows) < 2 or rows[0].get('kind') != 'start' or rows[-1].get('kind') != 'end':
        raise ValueError('closed start/end collection required')
    if any(r.get('kind') in ('start', 'end') for r in rows[1:-1]):
        raise ValueError('multiple collection boundaries')
    if rows[0].get('format') != 1 or rows[0].get('expected_commit') != PINNED_COMMIT:
        raise ValueError('unsupported collection or wrong commit pin')
    seconds_bound(rows[0]['seconds'])
    return rows


def read_bounded(path, limit):
    with Path(path).open('rb') as stream:
        raw = stream.read(limit + 1)
    if len(raw) > limit:
        raise ValueError('input exceeds byte bound')
    return raw


def analyze(collection, csv):
    collection = Path(collection).resolve(strict=True)
    run = run_path(collection.parent.name)
    collection = contained(collection, run)
    raw = read_bounded(collection, 32 * 1024 * 1024)
    rows = closed_collection(raw)
    if rows[0]['phase'] != run.name:
        raise ValueError('collection phase differs from owning run')
    manifest, manifest_sha = read_manifest(run)
    validate_manifest(manifest, run, live=False)
    original = Path(csv).resolve(strict=True)
    csv_raw = read_bounded(original, MAX_CSV_BYTES)
    frames = frame_summary(parse_csv(csv_raw))
    token = uuid.uuid4().hex
    archive = run / ('perf-frames-' + token + '.csv')
    output = run / ('perf-analysis-' + token + '.json')
    with archive.open('xb') as stream:
        stream.write(csv_raw)
    resources = [r for r in rows if r['kind'] == 'resource']
    report = {'format': 1, 'phase': run.name, 'label': rows[0]['label'],
              'collection': str(collection), 'collection_sha256': hashlib.sha256(raw).hexdigest(),
              'collection_start': rows[0], 'collection_end': rows[-1],
              'closed_run_manifest': manifest, 'closed_run_manifest_sha256': manifest_sha,
              'csv': {'original_path': str(original), 'preserved_path': str(archive),
                      'sha256': hashlib.sha256(csv_raw).hexdigest(), 'bytes': len(csv_raw), **frames},
              'status': status_summary(rows), 'resources': resource_summary(resources),
              'resource_missing_slots': [r for r in rows if r['kind'] == 'resource_missing'],
              'whole_run_resources': {'launcher_peak_bytes_observed': manifest.get('memory_peak_bytes_observed'),
                                     'last_resources': manifest.get('last_resources'),
                                     'unit_before_stop': manifest.get('unit_before_stop'),
                                     'collection_start_snapshot': rows[0]['resource_start'],
                                     'collection_end_snapshot': rows[-1]['resource_end']},
              'qualification': 'No pair matching or performance verdict. CSV is whole session; label is parent mark. Parent must qualify fixture/cache/mode/shutdown and gaps.'}
    with output.open('x', encoding='utf-8') as stream:
        json.dump(report, stream, allow_nan=False, indent=2)
        stream.write('\n')
    print(str(output))
    return 0


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    live = commands.add_parser('collect')
    live.add_argument('phase')
    live.add_argument('label')
    live.add_argument('--seconds', required=True, type=seconds_bound)
    offline = commands.add_parser('analyze')
    offline.add_argument('collection', type=Path)
    offline.add_argument('csv', type=Path)
    args = parser.parse_args(argv)
    try:
        return collect(args.phase, args.label, args.seconds) if args.command == 'collect' else analyze(args.collection, args.csv)
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(json.dumps({'error': error_text(error)}, allow_nan=False), file=sys.stderr)
        return 2


if __name__ == '__main__':
    sys.exit(main())
