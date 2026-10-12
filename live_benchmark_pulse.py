"""One independently restoring CPU-quota pulse; requires a separate parent grant.

Start alongside a measured slow-* cycle's collector. Delay10s, quota100% for5s,
then restore originally unlimited quota. No adaptive escalation or drop claims.
"""
import json
from pathlib import Path
import re
import signal
import subprocess
import sys
import time

import live_perf as perf

PS = ['/usr/bin/pwsh', '-NoLogo', '-NoProfile', '-NonInteractive', '-Command']


def quota(unit, value):
    return subprocess.run(PS + [f"systemctl --user set-property --runtime '{unit}' 'CPUQuota={value}'"],
                          check=True, timeout=5, capture_output=True, text=True)


def pulse(group, unit, record, interrupted):
    original = (group / 'cpu.max').read_text().strip()
    if original != 'max 100000':
        raise ValueError('Expected original unlimited quota/max 100000; no mutation')
    record('original', cpu_max=original, counters=perf.counters((group / 'cpu.stat').read_text()))
    time.sleep(10)
    if interrupted():
        record('interrupted_before_pulse')
        return
    try:
        # Even a failed command may have applied the property before its response
        # failed. The finally block always attempts the original quota afterward.
        quota(unit, '100%')
        effective = (group / 'cpu.max').read_text().strip()
        record('pulse_begin', cpu_max=effective, counters=perf.counters((group / 'cpu.stat').read_text()))
        if effective != '100000 100000':
            raise RuntimeError('CPU quota not observed at exactly one core')
        time.sleep(5)
        record('pulse_end', cpu_max=(group / 'cpu.max').read_text().strip(),
               counters=perf.counters((group / 'cpu.stat').read_text()))
    finally:
        quota(unit, '')
        restored = (group / 'cpu.max').read_text().strip()
        record('restored', cpu_max=restored, counters=perf.counters((group / 'cpu.stat').read_text()))
        if restored != original:
            raise RuntimeError('Original CPU quota restoration did not verify')


def main(phase):
    if not re.fullmatch(r'live-r27-slow-[a-z0-9-]{1,20}', phase):
        raise ValueError('Only explicitly owned slow-* phase is eligible')
    run = perf.run_path(phase)
    manifest, manifest_sha = perf.read_manifest(run)
    _, group = perf.validate_manifest(manifest, run, live=True)
    if time.time() - manifest['started_at'] + 30 > 480:
        raise ValueError('Insufficient parent480s normal-stop margin')
    interrupted = False

    def handle_signal(*_):
        nonlocal interrupted
        interrupted = True

    previous = {sig: signal.signal(sig, handle_signal) for sig in (signal.SIGINT, signal.SIGTERM)}
    try:
        with (run / 'quota-pulse.jsonl').open('x') as stream:
            def record(kind, **fields):
                perf.emit(stream, {'kind': kind, **perf.stamp(), **fields})
            record('start', manifest_sha256=manifest_sha, unit=manifest['unit'],
                   control_group=str(group), semantics='Observed quota/throttling, not a presentation-drop injector')
            pulse(group, manifest['unit'], record, lambda: interrupted)
            record('end', interrupted=interrupted)
    finally:
        for sig, handler in previous.items():
            signal.signal(sig, handler)


if __name__ == '__main__':
    main(sys.argv[1])
