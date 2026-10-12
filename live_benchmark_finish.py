"""Bounded post-measurement results/save flow using a pinned native-dialog image.

No GUI work runs during collection. Saving is refused unless both question text
and the selected No button match the inspected native Save Replay reference.
This is a control precondition, never archive/native association acceptance.
"""
import hashlib
import json
import time

from PIL import Image, ImageChops

import live_perf as perf
import live_benchmark_save as publication

REFERENCE = perf.RUNS / 'live-r27-bench-b1off/native-save-dialog.png'
REFERENCE_SHA = '2153896a0eec637b9d60abd3b35da6ab89fa95a025ac0aa35eb7a09712d399fc'


def dialog_match(path):
    raw = perf.read_bounded(REFERENCE, 4 * 1024 * 1024)
    if hashlib.sha256(raw).hexdigest() != REFERENCE_SHA:
        raise ValueError('Save-dialog reference changed')
    with Image.open(REFERENCE) as reference, Image.open(path) as actual:
        if actual.size != (1280, 800) or reference.size != actual.size:
            return {'matched': False, 'reason': 'image dimensions'}
        overlaps = []
        for box in ((430, 315, 875, 365), (235, 545, 628, 632)):
            masks = [im.crop(box).convert('L').point(lambda p: 255 if p < 70 else 0)
                     for im in (reference, actual)]
            intersection = ImageChops.darker(*masks).histogram()[255]
            union = ImageChops.lighter(*masks).histogram()[255]
            overlaps.append(intersection / union if union else 0.)
        return {'matched': min(overlaps) >= .985, 'dark_pixel_iou': overlaps,
                'reference_sha256': REFERENCE_SHA}


def finish(phase, slow=False):
    run = perf.run_path(phase)
    manifest, _ = perf.read_manifest(run)
    socket, _ = perf.validate_manifest(manifest, run, live=True)
    delay = 55 if slow else 35
    if time.time() - manifest['started_at'] + delay + 25 > 465:
        raise ValueError('Insufficient results/save/stop margin')
    client = perf.client_class()(socket, timeout=2)
    with (run / 'finish-results.jsonl').open('x') as out:
        def call(method, params=None):
            reply = client.request(method, params or {})
            perf.emit(out, {'method': method, 'params': params or {}, 'reply': reply, **perf.stamp()})
            if not reply.get('ok'):
                raise RuntimeError(json.dumps(reply))

        def shot(name):
            path = run / f'finish-{name}.png'
            if path.exists():
                raise ValueError('Refuse reused screenshot')
            call('ui.screenshot', {'target': 'all', 'path': str(path)})
            return path

        try:
            time.sleep(delay)
            shot('results-before')
            for key, wait in (('a', 2), ('a', 2), ('y', 1)):
                call('input.button', {'button': key, 'state': 'tap', 'duration_ms': 100, 'lease_ms': 1000})
                time.sleep(wait)
            matched = dialog_match(shot('dialog'))
            perf.emit(out, {'kind': 'dialog_match', **matched, **perf.stamp()})
            if not matched['matched']:
                raise ValueError('Actual native Save Replay/No image did not match; no confirmation issued')
            publication.confirm(phase, assertion='Pinned question + selected-No image matched; parent reviews retained screenshots')
            shot('after-save')
        finally:
            call('input.reset')
