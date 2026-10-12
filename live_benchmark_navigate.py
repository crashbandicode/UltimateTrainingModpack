"""Bounded cold-fixture menu navigation after the parent boots the pinned clone.

Stops at the expected rules menu for parent phase inspection. Native loading_complete
does not prove title-screen readiness. No stage or fighter confirmation follows the
batch. Wall-time cursor movement is approximate and must be inspected separately.
"""
import json
import re
import sys
import time

import live_perf as perf


def navigate(phase):
    if not re.fullmatch(r'live-r27-(?:bench|slow)-[a-z0-9-]{1,20}', phase):
        raise ValueError('Only parent benchmark phases are eligible')
    run = perf.run_path(phase)
    manifest, _ = perf.read_manifest(run)
    socket, _ = perf.validate_manifest(manifest, run, live=True)
    if time.time() - manifest['started_at'] > 90:
        raise ValueError('Only fresh cold-boot navigation; retain the480s stop margin')
    client = perf.client_class()(socket, timeout=2)
    with (run / 'cold-navigation.jsonl').open('x') as journal:
        def call(method, params=None):
            reply = client.request(method, params or {})
            perf.emit(journal, {'method': method, 'params': params or {}, 'response': reply, **perf.stamp()})
            if not reply.get('ok'):
                raise RuntimeError(json.dumps(reply))
            return reply['result']

        def buttons(actions):
            for key, duration, wait in actions:
                call('input.button', {'button': key, 'state': 'tap', 'duration_ms': duration,
                                      'lease_ms': max(1000, duration + 300)})
                time.sleep(max(wait, duration / 1000 + .2))

        def shot(name):
            path = run / f'navigation-{name}.png'
            if path.exists():
                raise ValueError('Refuse reused screenshot evidence')
            call('ui.screenshot', {'target': 'all', 'path': str(path)})

        try:
            deadline = time.monotonic() + 40
            while True:
                status = call('status')
                if status.get('commit') != perf.PINNED_COMMIT or status.get('modal'):
                    raise ValueError('Unexpected runtime pin or frontend modal')
                if status.get('running') and status.get('loading_complete'):
                    break
                if time.monotonic() >= deadline:
                    raise TimeoutError('Cold game did not finish loading')
                time.sleep(1)
            time.sleep(20)
            buttons([('a', 100, 4)] * 4)
            shot('preset')
        finally:
            call('input.reset')
    print(json.dumps({'navigation': str(run / 'cold-navigation.jsonl'),
                       'next': 'Inspect actual menu before proceeding to stage and fighter selection'}))


if __name__ == '__main__':
    navigate(sys.argv[1])
