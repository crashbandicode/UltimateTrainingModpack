"""Parent-owned bounded native match controls; a separate runtime grant is required.

Only invoke warmup/measure after visually verifying the ready-to-fight fixture.
Start/end screenshots are evidence for later parent qualification, not an oracle.
No launch, settings mutation, quota change, automatic phase detection or verdict.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import time

import live_perf as perf

PS = ['/usr/bin/pwsh', '-NoLogo', '-NoProfile', '-NonInteractive', '-Command']
DELUXE_STATUS = (perf.RUNS.parent / 'profile/data/eden/sdmc/ultimate/'
                 'ssbu_online_deluxe_rewrite/renderer-status.txt')


def deluxe_snapshot(started_at):
    if DELUXE_STATUS.resolve() != DELUXE_STATUS or DELUXE_STATUS.stat().st_mtime < started_at:
        raise ValueError('Missing fresh owned Deluxe renderer status')
    raw = perf.read_bounded(DELUXE_STATUS, 65536)
    text = raw.decode('utf-8')
    fields = dict(line.split('=', 1) for line in text.splitlines() if '=' in line)
    expected = {'build': '0.1.0-2db70f90aaae-dirty-1791663838',
                'engine_status': 'Active', 'policy_effective': 'lesslag',
                'policy_failed': 'false', 'active_textures': '2', 'allocated_textures': '3',
                'auto_profile': 'false', 'platform': 'emulator', 'scene_ready': 'true',
                'scene_connection': 'offline', 'scene_status': 'singles', 'fps_boost_status': 'Off'}
    if any(fields.get(key) != value for key, value in expected.items()):
        raise ValueError(f'Deluxe active combat mode did not verify: {fields}')
    return {'path': str(DELUXE_STATUS), 'sha256': hashlib.sha256(raw).hexdigest(),
            'mtime': DELUXE_STATUS.stat().st_mtime, 'raw': text}


def run_cycle(phase, mode, pulse=False, deluxe=False, save=False):
    if not re.fullmatch(r'live-r27-(?:bench|slow)-[a-z0-9-]{1,20}', phase):
        raise ValueError('Only parent benchmark/slowdown phases are eligible')
    if pulse and (mode != 'measure' or not phase.startswith('live-r27-slow-')):
        raise ValueError('Quota pulse requires a separately granted slow-* measured cycle')
    if save and mode != 'measure':
        raise ValueError('Post-match saving belongs only to the measured cycle')
    run = perf.run_path(phase)
    manifest, _ = perf.read_manifest(run)
    socket, _ = perf.validate_manifest(manifest, run, live=True)
    # Reserve the rest of the match, native results/save and normal shutdown,
    # not just the collection. Slowdown also needs extra match-completion time.
    total = 115 if mode == 'warmup' else (155 if pulse else 125)
    if time.time() - manifest['started_at'] + total > 465:
        raise ValueError('Insufficient parent480s normal-stop margin for this cycle')
    client = perf.client_class()(socket, timeout=2)
    target = run / f'drive-{mode}.jsonl'
    child = None
    pulse_child = None
    with target.open('x') as journal:
        def record(kind, **data):
            perf.emit(journal, {'kind': kind, **perf.stamp(), **data})

        def call(method, params=None):
            response = client.request(method, params or {})
            record('rpc', method=method, params=params or {}, response=response)
            if not response.get('ok'):
                raise RuntimeError(json.dumps(response))
            return response['result']

        def shot(name):
            path = run / f'{mode}-{name}.png'
            if path.exists():
                raise ValueError('Refuse reused screenshot evidence')
            return call('ui.screenshot', {'target': 'all', 'path': str(path)})

        state = call('status')
        if state.get('commit') != perf.PINNED_COMMIT or not state.get('running') or state.get('modal'):
            raise ValueError('Pinned running non-modal game required')
        record('start', phase=phase, mode=mode, parent_assertion='ready-to-fight fixed fixture')
        try:
            call('input.button', {'button': 'plus', 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
            launch = time.monotonic()
            time.sleep(15)
            shot('window-start')
            if deluxe:
                record('deluxe_start', **deluxe_snapshot(manifest['started_at']))
            if mode == 'measure':
                log_path = run / 'measure-collector.log'
                with log_path.open('x') as log:
                    command = f"& /usr/bin/python3 -B '{Path(perf.__file__).resolve()}' collect '{phase}' parent-marked-fixed-match-window --seconds 40"
                    child = subprocess.Popen(PS + [command], stdin=subprocess.DEVNULL, stdout=log,
                                             stderr=subprocess.STDOUT, start_new_session=True)
                record('collector', pid=child.pid, log=str(log_path), seconds=40)
                if pulse:
                    pulse_log = run / 'measure-pulse.log'
                    with pulse_log.open('x') as log:
                        pulse_script = Path(__file__).with_name('live_benchmark_pulse.py').resolve()
                        pulse_child = subprocess.Popen(PS + [f"& /usr/bin/python3 -B '{pulse_script}' '{phase}'"],
                                                       stdin=subprocess.DEVNULL, stdout=log,
                                                       stderr=subprocess.STDOUT, start_new_session=True)
                    record('pulse_controller', pid=pulse_child.pid, log=str(pulse_log), delay=10, hold=5)
            pattern_start = time.monotonic()
            # Same short wall-time pattern in every warmup and measurement.
            # Frontend delivery is journaled; native consumption is not inferred.
            for index in range(8):
                time.sleep(max(0, pattern_start + 3 * index - time.monotonic()))
                call('input.stick', {'player': 0, 'stick': 'left', 'x': .375 if index % 2 == 0 else -.375,
                                     'y': .2, 'lease_ms': 2000})
                call('input.button', {'player': 0, 'button': 'a', 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
                if index % 3 == 0:
                    call('input.button', {'player': 0, 'button': 'x', 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
            time.sleep(max(0, pattern_start + 24 - time.monotonic()))
            call('input.reset')
            if child is not None:
                child.wait(timeout=max(1, launch + 58 - time.monotonic()))
                if child.returncode != 0:
                    raise RuntimeError(f'Collector failed with exit{child.returncode}; preserve raw log')
                if pulse_child is not None:
                    pulse_child.wait(timeout=5)
                    if pulse_child.returncode != 0:
                        raise RuntimeError(f'Pulse controller failed with exit{pulse_child.returncode}; inspect quota restoration')
            else:
                time.sleep(max(0, launch + 110 - time.monotonic()))
            shot('window-end')
            if deluxe and mode == 'measure':
                record('deluxe_end', **deluxe_snapshot(manifest['started_at']))
            record('end', elapsed=time.monotonic() - launch,
                   qualification='Parent must inspect both screenshots and native result; no automatic performance verdict')
            if save:
                from live_benchmark_finish import finish
                finish(phase, slow=pulse)
        finally:
            try:
                call('input.reset')
            finally:
                try:
                    if pulse_child is not None and pulse_child.poll() is None:
                        # Let the independent bounded controller finish restoring
                        # the quota even after GUI failure. It never needs GUI.
                        pulse_child.wait(timeout=30)
                finally:
                    if child is not None and child.poll() is None:
                        # Only the new collector and its wrapper belong to this
                        # group; never signal the parent's process group.
                        os.killpg(child.pid, signal.SIGTERM)
                        try:
                            child.wait(timeout=5)
                        except subprocess.TimeoutExpired:
                            os.killpg(child.pid, signal.SIGKILL)
                            child.wait(timeout=5)
                    if save:
                        # Complete this bounded operation without waiting for a
                        # model/tool round trip. Exact restoration is separate.
                        current, _ = perf.read_manifest(run)
                        if current.get('state') != 'stopped':
                            subprocess.run(PS + [f"& /usr/bin/python3 '{perf.RUNS.parent}/acceptance.py' stop '{phase}'"],
                                           check=True, timeout=60)
    print(json.dumps({'drive': str(target), 'mode': mode}))


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=('warmup', 'measure'))
    parser.add_argument('phase')
    parser.add_argument('--pulse', action='store_true', help='Only under an explicit slow-* quota-pulse grant')
    parser.add_argument('--deluxe', action='store_true', help='Require fresh pinned active LessLag at combat boundaries')
    parser.add_argument('--save', action='store_true', help='After collection, verify native dialog pixels, save and stop')
    args = parser.parse_args()
    run_cycle(args.phase, args.mode, args.pulse, args.deluxe, args.save)
