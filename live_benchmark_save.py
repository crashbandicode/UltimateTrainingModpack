"""Confirm the parent-verified native Save Replay dialog (No initially selected).

Measure request-to-first-readable-bundle time for a fixed10s outside-combat window.
Guest confirmation timing and canonical validity require separate stopped evidence.
Requires an existing owned run and an applicable parent runtime grant.
"""
import json
from pathlib import Path
import re
import sys
import time

import live_perf as perf

CAPTURES = perf.RUNS.parent / 'profile/data/eden/sdmc/ultimate/TrainingModpack/replay_captures'


def confirm(phase, assertion='Native Save Replay dialog visible with No selected'):
    if not re.fullmatch(r'live-r27-(?:bench|slow)-[a-z0-9-]{1,20}', phase):
        raise ValueError('Expected owned benchmark/slowdown phase')
    run = perf.run_path(phase)
    manifest, _ = perf.read_manifest(run)
    socket, _ = perf.validate_manifest(manifest, run, live=True)
    if time.time() - manifest['started_at'] + 20 > 465:
        raise ValueError('Insufficient parent480s stop margin')
    if CAPTURES.resolve() != CAPTURES:
        raise ValueError('Redirected capture path')
    before = set(CAPTURES.glob('*.smd'))
    client = perf.client_class()(socket, timeout=2)
    with (run / 'save-publication.jsonl').open('x') as out:
        def record(kind, **fields):
            perf.emit(out, {'kind': kind, **perf.stamp(), **fields})

        def call(method, params=None):
            reply = client.request(method, params or {})
            record('rpc', method=method, params=params or {}, response=reply)
            if not reply.get('ok'):
                raise RuntimeError(json.dumps(reply))

        record('start', control_assertion=assertion,
               semantics='Request-to-first-readable bundle; not native confirmation or isolated writer duration')
        try:
            call('input.button', {'button': 'right', 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
            time.sleep(.3)
            start = time.monotonic()
            record('confirmation_request')
            call('input.button', {'button': 'a', 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
            observed = set()
            while time.monotonic() - start < 10:
                for bundle in sorted(set(CAPTURES.glob('*.smd')) - before - observed):
                    try:
                        if bundle.resolve() != bundle or not re.fullmatch(r'[a-f0-9]+\.smd', bundle.name):
                            raise ValueError('Unexpected published bundle path')
                        native_name = bytes.fromhex(bundle.stem).decode('ascii')
                        metadata = json.loads(perf.read_bounded(bundle / 'manifest.json', 1024 * 1024))
                        for name in ('timeline.jsonl', f'{bundle.stem}.tmrc'):
                            path = bundle / name
                            if path.resolve() != path:
                                raise ValueError('Redirected bundle member')
                            with path.open('rb') as data:
                                if not data.read(1):
                                    raise ValueError('Empty bundle member')
                        record('first_readable', bundle=str(bundle), native_name=native_name,
                               request_elapsed_seconds=time.monotonic() - start, manifest=metadata,
                               validation='Members readable; exact native association and projection checked after stop')
                        observed.add(bundle)
                    except (OSError, ValueError) as error:
                        record('not_yet_readable', bundle=str(bundle), error=str(error))
                time.sleep(.25)
            record('end', readable_bundles=[str(p) for p in sorted(observed)],
                   actual_window_seconds=time.monotonic() - start)
        finally:
            call('input.reset')


if __name__ == '__main__':
    confirm(sys.argv[1])
