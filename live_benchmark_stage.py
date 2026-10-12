"""Stopped, parent-owned fixture staging; a separate finite lease is required.

prepare TOKEN on|off CLEANUP_ISO [deluxe6c39]
ready TOKEN
restore TOKEN

No launch/boot. Full native saves and full Eden/NVIDIA cache start identically.
Original trees and every interrupted/postrun copy are preserved, never deleted.
"""
import json
from pathlib import Path
import re
import shutil
import sys
import time

import live_acceptance as s

HERE = Path(__file__).resolve().parent
FIXTURE2 = HERE / 'live-acceptance-27-fixture2'
CACHE = s.PROFILE / 'cache'
SYSTEM_DATA_REL = ('0000000000000000/D5D8291B55A016597329A04D037CD073/'
                   '01006A800016E000/save_data/system_data.bin')


def direct(path):
    path = Path(path)
    s.require(path.is_absolute() and path.resolve() == path and not path.is_symlink(),
              f'Path redirects outside its recorded location: {path}')
    return path


def validate_token(token):
    s.require(re.fullmatch(r'27-(?:bench|slow)-[a-z0-9][a-z0-9-]{0,15}', token),
              'Expected short 27-bench-* or 27-slow-* token')


def bench_stage(token):
    validate_token(token)
    return direct(HERE / f'live-acceptance-{token}')


def fixture():
    direct(FIXTURE2)
    names = ('receipt.json', 'closure-verification.json', 'prewarmed-cache-manifest.json')
    pins = {name: s.sha(direct(FIXTURE2 / name)) for name in names}
    receipt, closure, cache_manifest = [json.loads((FIXTURE2 / name).read_text()) for name in names]
    s.require(receipt['state'] == 'restored' and receipt['revision'] == 27
              and receipt['mode'] == 'normal' and receipt['profile'] == str(s.PROFILE)
              and receipt['campaign'] == FIXTURE2.name, 'Wrong fixture receipt')
    s.require(closure['state'] == 'restored_fixed_fixture_warmup_saved'
              and closure['restoration_exact'] is True, 'Fixture not qualified/restored')
    saves = direct(Path(closure['preserved_saves']))
    s.require(saves.parent == FIXTURE2 / 'after' and saves.name.startswith('saves-'),
              'Wrong preserved saves location')
    candidates = [item['manifest'] for item in receipt['after']
                  if item['tree'] == 'saves' and item['path'] == str(saves)]
    s.require(len(candidates) == 1, 'Preserved saves lack a unique receipt manifest')
    saves_manifest = candidates[0]
    s.require(SYSTEM_DATA_REL in saves_manifest['files'] and s.tree(saves) == saves_manifest,
              'Full native saves differ from the immutable fixture receipt')
    cache = direct(FIXTURE2 / 'prewarmed-cache')
    s.require(s.tree(cache) == cache_manifest, 'Full cache differs from immutable manifest')
    return {'pins': pins, 'saves': str(saves), 'saves_manifest': saves_manifest,
            'cache_manifest': cache_manifest}


def owned(stage, receipt=None):
    direct(stage)
    direct(CACHE)
    s.owned_profile()
    token = stage.name.removeprefix('live-acceptance-')
    s.require(stage == bench_stage(token), 'Unexpected stage path')
    if receipt is not None:
        s.require(receipt['profile'] == str(s.PROFILE) and receipt['campaign'] == stage.name
                  and receipt['revision'] == 27 and receipt['mode'] == 'normal', 'Wrong campaign receipt')
        if 'benchmark' in receipt:
            s.require(receipt['benchmark']['token'] == token, 'Benchmark token mismatch')
        for run in receipt['runs']:
            for key in ('launcher_pid', 'guard_pid', 'normal_stop_pid'):
                pid = run.get(key)
                s.require(pid is None or not Path(f'/proc/{pid}').exists(), f'Owned {key} still present')
    s.stopped()


def set_frame_csv(path):
    direct(path)
    lines = path.read_text().splitlines()
    s.require(lines.count('[Debugging]') == 1, 'Expected one Debugging section')
    # These exact global keys are the only changes to the original Qt config.
    lines = [line for line in lines if not line.startswith(('record_frame_times=', 'record_frame_times\\default='))]
    offset = lines.index('[Debugging]') + 1
    lines[offset:offset] = ['record_frame_times=true', 'record_frame_times\\default=false']
    path.write_text('\n'.join(lines) + '\n')


def prepare(stage, token, capture_on, cleanup, coexistence=None):
    s.require(stage == bench_stage(token), 'Stage/token mismatch')
    owned(stage)
    source = fixture()
    original_cache = s.tree(CACHE)
    # Existing full config/save/SDMC backups and frozen27 checks happen first.
    receipt = s.prepare(stage, 27, 'normal', cleanup, coexistence)
    bench = receipt['benchmark'] = {
        'token': token, 'source': source, 'capture_on': capture_on,
        'before_cache_manifest': original_cache, 'overlay_state': 'cache_rename_pending',
        'cache_restored': False, 'after_cache': [],
    }
    s.write_receipt(stage, receipt)
    # Atomic rename is the original-cache backup. If interrupted either side,
    # restore can distinguish untouched original from the verified backup.
    backup = direct(stage / 'before/cache')
    s.require(not backup.exists(), 'Existing cache backup')
    CACHE.rename(backup)
    s.require(s.tree(backup) == original_cache, 'Original cache changed during backup')
    bench['overlay_state'] = 'cache_copy_pending'
    s.write_receipt(stage, receipt)
    shutil.copytree(FIXTURE2 / 'prewarmed-cache', CACHE)
    s.require(s.tree(CACHE) == source['cache_manifest'], 'Prewarmed cache copy mismatch')
    bench['overlay_state'] = 'saves_replace_pending'
    s.write_receipt(stage, receipt)
    # Do not combine new system_data with old replay files. Keep the whole tree.
    saves_side = direct(stage / 'original-saves-side')
    s.require(not saves_side.exists(), 'Existing saves side directory')
    s.TREES['saves'].rename(saves_side)
    shutil.copytree(Path(source['saves']), s.TREES['saves'])
    s.require(s.tree(s.TREES['saves']) == source['saves_manifest'], 'Full saves overlay mismatch')
    bench['overlay_state'] = 'config_pending'
    s.write_receipt(stage, receipt)
    config = s.TREES['config'] / 'eden/qt-config.ini'
    set_frame_csv(config)
    ini = direct(s.SETTINGS / 'replay_capture.ini')
    ini.write_text(f'[capture]\nenabled={"on" if capture_on else "off"}\n')
    receipt['capture_ini_sha256'] = s.sha(ini)
    bench.update(overlay_state='ready', config_sha256=s.sha(config))
    s.write_receipt(stage, receipt)
    check_ready(stage, receipt)
    return receipt


def check_ready(stage, receipt):
    owned(stage, receipt)
    bench = receipt['benchmark']
    s.require(receipt['state'] == 'installed' and bench['overlay_state'] == 'ready'
              and not bench['cache_restored'], 'Benchmark overlay is not ready')
    s.require(fixture() == bench['source'], 'Immutable fixture source changed after preparation')
    s.require(s.tree(s.TREES['saves']) == bench['source']['saves_manifest'], 'Staged native saves drifted')
    s.require(s.tree(CACHE) == bench['source']['cache_manifest'], 'Staged complete cache drifted')
    config = direct(s.TREES['config'] / 'eden/qt-config.ini')
    s.require(s.sha(config) == bench['config_sha256'], 'Staged Qt config drifted')
    ini = direct(s.SETTINGS / 'replay_capture.ini')
    s.require(s.sha(ini) == receipt['capture_ini_sha256'] and ini.read_text()
              == f'[capture]\nenabled={"on" if bench["capture_on"] else "off"}\n', 'Capture condition drifted')
    s.require(s.inventory() == {'libarcropolis.nro': s.ARC, **receipt['payloads']}, 'Staged plugins drifted')
    if receipt.get('deluxe'):
        s.require(s.sha(direct(s.TREES['sdmc'] / 'ultimate/ssbu_online_deluxe_rewrite/config.ini'))
                  == receipt['deluxe']['config_sha256'], 'Deluxe configuration drifted')
    s.verify_protected(receipt)
    return {'state': 'ready', 'token': bench['token'], 'capture_on': bench['capture_on'],
            'source_pins': bench['source']['pins']}


def restore(stage, receipt):
    owned(stage, receipt)
    s.verify_protected(receipt)
    bench = receipt.get('benchmark')
    if bench is None:
        # Interruption inside s.prepare precedes every cache/saves overlay.
        return s.restore(stage, receipt)
    original = bench['before_cache_manifest']
    if bench.get('restore_state') == 'done':
        s.require(receipt['state'] == 'restored' and s.tree(CACHE) == original,
                  'Previously restored cache drifted')
        return s.restore(stage, receipt)  # Verifies all original profile trees too.
    backup = direct(stage / 'before/cache')
    if bench['cache_restored']:
        s.require(s.tree(CACHE) == original, 'Restored cache drifted during tree restoration')
    elif not backup.exists():
        # The only pre-backup operation was a receipt write; cache is untouched.
        s.require(bench['overlay_state'] == 'cache_rename_pending' and s.tree(CACHE) == original,
                  'Missing original cache backup with a changed live cache')
    else:
        s.require(s.tree(backup) == original, 'Original cache backup drifted')
        after = direct(stage / 'after')
        after.mkdir(exist_ok=True)
        # Validate previously recorded destinations before any more mutation.
        for item in bench['after_cache']:
            path = direct(Path(item['path']))
            s.require(path.parent == after and path.name.startswith('cache-'), 'Wrong after-cache path')
            if path.exists():
                s.require(s.tree(path) == item['manifest'], 'Preserved after-cache drifted')
        if CACHE.exists():
            destination = direct(after / f'cache-{time.time_ns()}')
            s.require(not destination.exists(), 'After-cache destination already exists')
            bench['after_cache'].append({'path': str(destination), 'manifest': s.tree(CACHE)})
            s.write_receipt(stage, receipt)  # Destination and hash BEFORE rename.
            CACHE.rename(destination)
        # An interrupted copy is preserved on the next attempt exactly as above.
        # An interrupted rename leaves CACHE absent and needs only this copy.
        shutil.copytree(backup, CACHE)
        s.require(s.tree(CACHE) == original, 'Original cache restoration mismatch')
    bench['cache_restored'] = True
    s.write_receipt(stage, receipt)
    # Restoration depends on verified owned backups, not on external source being
    # available. Source drift blocks prepare/ready, never recovery of originals.
    receipt = s.restore(stage, receipt)
    s.require(s.tree(CACHE) == original, 'Final cache restoration mismatch')
    receipt['benchmark']['restore_state'] = 'done'
    s.write_receipt(stage, receipt)
    return receipt


def main():
    mode, token, *args = sys.argv[1:]
    stage = bench_stage(token)
    if mode == 'prepare':
        s.require(len(args) in (2, 3) and args[0] in ('on', 'off'),
                  'prepare TOKEN on|off CLEANUP_ISO [deluxe6c39]')
        receipt = prepare(stage, token, args[0] == 'on', args[1], args[2] if len(args) == 3 else None)
        result = {'state': receipt['state'], 'overlay_state': receipt['benchmark']['overlay_state']}
    else:
        s.require(mode in ('restore', 'ready') and not args, 'restore|ready TOKEN')
        receipt = json.loads(direct(stage / 'receipt.json').read_text())
        if mode == 'ready':
            result = check_ready(stage, receipt)
        else:
            receipt = restore(stage, receipt)
            result = {'state': receipt['state'], 'restore_state': receipt.get('benchmark', {}).get('restore_state')}
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    main()
