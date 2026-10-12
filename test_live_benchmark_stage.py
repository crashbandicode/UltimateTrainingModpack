"""Temp-profile integration tests, including actual base backup/restore code.

No native process, live profile, systemd, build, or runtime is used.
"""
from contextlib import ExitStack
import json
from pathlib import Path
import shutil
import tempfile
import time
import unittest
from unittest.mock import patch

import live_acceptance as s
import live_benchmark_stage as b


class StageTests(unittest.TestCase):
    def setUp(self):
        self.stack = ExitStack()
        self.addCleanup(self.stack.close)
        self.root = Path(self.stack.enter_context(tempfile.TemporaryDirectory())).resolve()
        research = self.root / 'research'
        research.mkdir()
        runtime = self.root / 'runtime'
        profile = runtime / 'profile'
        trees = {'config': profile / 'config', 'saves': profile / 'data/save', 'sdmc': profile / 'data/sdmc'}
        for path in trees.values():
            path.mkdir(parents=True)
        settings = trees['sdmc'] / 'ultimate/TrainingModpack'
        settings.mkdir(parents=True)
        plugins = trees['sdmc'] / 'plugins'
        plugins.mkdir()
        arc = plugins / 'libarcropolis.nro'
        arc.write_bytes(b'original arc')
        (settings / 'training_modpack_menu.conf').write_text('{"menu":{"macro_source":[1],"recording_crop":[1]}}')
        (trees['config'] / 'eden').mkdir()
        (trees['config'] / 'eden/qt-config.ini').write_text('[Debugging]\nrecord_frame_times=false\nrecord_frame_times\\default=true\n\n[Controls]\nfoo=bar\n')
        (trees['saves'] / 'original.bin').write_bytes(b'original saves')
        cache = profile / 'cache'
        cache.mkdir()
        (cache / 'original.bin').write_bytes(b'original cache')
        nro = research / 'normal.nro'
        nro.write_bytes(b'synthetic frozen NRO')
        fixture = research / 'live-acceptance-27-fixture2'
        saves = fixture / 'after/saves-1'
        data = saves / b.SYSTEM_DATA_REL
        data.parent.mkdir(parents=True)
        data.write_bytes(b'fixture system data')
        (data.parent / 'replay202610124136.bin').write_bytes(b'fixture native replay')
        prewarmed = fixture / 'prewarmed-cache'
        (prewarmed / 'eden').mkdir(parents=True)
        (prewarmed / 'nvidia').mkdir()
        (prewarmed / 'eden/cache.bin').write_bytes(b'prewarmed Eden')
        (prewarmed / 'nvidia/cache.bin').write_bytes(b'prewarmed NVIDIA')
        (fixture / 'prewarmed-cache-manifest.json').write_text(json.dumps(s.tree(prewarmed)))
        (fixture / 'closure-verification.json').write_text(json.dumps({
            'state': 'restored_fixed_fixture_warmup_saved', 'restoration_exact': True, 'preserved_saves': str(saves)}))
        (fixture / 'receipt.json').write_text(json.dumps({
            'state': 'restored', 'revision': 27, 'mode': 'normal', 'profile': str(profile),
            'campaign': fixture.name, 'after': [{'tree': 'saves', 'path': str(saves), 'manifest': s.tree(saves)}]}))
        for module, key, value in (
                (b, 'HERE', research), (b, 'FIXTURE2', fixture), (b, 'CACHE', cache),
                (s, 'HERE', research), (s, 'ROOT', runtime), (s, 'PROFILE', profile), (s, 'TREES', trees),
                (s, 'SETTINGS', settings), (s, 'PLUGINS', plugins), (s, 'PROTECTED', {}), (s, 'ARC', s.sha(arc))):
            self.stack.enter_context(patch.object(module, key, value))
        self.stack.enter_context(patch.object(s, 'stopped'))
        self.stack.enter_context(patch.object(s, 'gate', return_value={'available_kib': 20 * 1024**2}))
        self.stack.enter_context(patch.object(s, 'deadline', return_value=time.time() + 3600))
        self.stack.enter_context(patch.object(s, 'frozen', return_value=(
            {}, {'libtraining_modpack.nro': s.sha(nro)}, {'libtraining_modpack.nro': nro})))
        self.token = '27-bench-test'
        self.stage = b.bench_stage(self.token)
        self.before = {name: s.tree(path) for name, path in s.TREES.items()}
        self.before_cache = s.tree(b.CACHE)

    def prepare(self, enabled=True):
        return b.prepare(self.stage, self.token, enabled, 'synthetic-deadline')

    def disk_receipt(self):
        return json.loads((self.stage / 'receipt.json').read_text())

    def restore_and_check(self):
        result = b.restore(self.stage, self.disk_receipt())
        self.assertEqual(result['state'], 'restored')
        self.assertEqual({name: s.tree(path) for name, path in s.TREES.items()}, self.before)
        self.assertEqual(s.tree(b.CACHE), self.before_cache)
        return result

    def test_full_fixture_ready_and_exact_restore_on(self):
        receipt = self.prepare()
        self.assertEqual(b.check_ready(self.stage, receipt)['state'], 'ready')
        self.assertEqual(s.tree(s.TREES['saves']), receipt['benchmark']['source']['saves_manifest'])
        self.assertEqual(s.tree(b.CACHE), receipt['benchmark']['source']['cache_manifest'])
        self.assertIn('foo=bar', (s.TREES['config'] / 'eden/qt-config.ini').read_text())
        self.assertEqual((s.SETTINGS / 'replay_capture.ini').read_text(), '[capture]\nenabled=on\n')
        (b.CACHE / 'postrun.bin').write_bytes(b'postrun cache')
        (s.TREES['saves'] / 'postrun.bin').write_bytes(b'postrun save')
        result = self.restore_and_check()
        self.assertTrue(any((Path(item['path']) / 'postrun.bin').exists() for item in result['benchmark']['after_cache']))
        self.assertTrue(any((Path(item['path']) / 'postrun.bin').exists() for item in result['after'] if item['tree'] == 'saves'))
        self.restore_and_check()  # Idempotent validation, not another overwrite.

    def test_off_condition_and_stale_ready_refusal(self):
        receipt = self.prepare(False)
        self.assertFalse(b.check_ready(self.stage, receipt)['capture_on'])
        (s.TREES['config'] / 'eden/qt-config.ini').write_text('drift')
        with self.assertRaisesRegex(RuntimeError, 'Qt config drifted'):
            b.check_ready(self.stage, receipt)
        self.restore_and_check()

    def test_corrupted_full_save_source_refused_before_profile_mutation(self):
        source = b.fixture()
        (Path(source['saves']) / 'extra.bin').write_bytes(b'unreceipted')
        with self.assertRaisesRegex(RuntimeError, 'Full native saves'):
            self.prepare()
        self.assertFalse(self.stage.exists())
        self.assertEqual(s.tree(b.CACHE), self.before_cache)

    def test_parent_symlink_escape_refused(self):
        real = b.FIXTURE2 / 'after'
        renamed = b.FIXTURE2 / 'original-after'
        real.rename(renamed)
        real.symlink_to(renamed, target_is_directory=True)
        with self.assertRaisesRegex(RuntimeError, 'redirects'):
            self.prepare()
        self.assertFalse(self.stage.exists())

    def test_prepare_interruption_before_cache_rename(self):
        original = Path.rename
        def fail(path, target):
            if path == b.CACHE:
                raise OSError('injected before original-cache rename')
            return original(path, target)
        with patch.object(Path, 'rename', fail), self.assertRaises(OSError):
            self.prepare()
        self.restore_and_check()

    def test_prepare_interruption_after_cache_rename(self):
        original = Path.rename
        def fail(path, target):
            result = original(path, target)
            if path == b.CACHE:
                raise OSError('injected after original-cache rename')
            return result
        with patch.object(Path, 'rename', fail), self.assertRaises(OSError):
            self.prepare()
        self.restore_and_check()

    def partial_copy(self, target):
        original = shutil.copytree
        def fail(source, destination, *args, **kwargs):
            if Path(destination) == target:
                target.mkdir()
                (target / 'partial-copy.bin').write_bytes(b'partial retained')
                raise OSError('injected partial copy')
            return original(source, destination, *args, **kwargs)
        return patch.object(shutil, 'copytree', fail)

    def test_prepare_partial_cache_copy_is_preserved_and_restored(self):
        with self.partial_copy(b.CACHE), self.assertRaises(OSError):
            self.prepare()
        result = self.restore_and_check()
        self.assertTrue(any((Path(item['path']) / 'partial-copy.bin').exists() for item in result['benchmark']['after_cache']))

    def test_prepare_partial_saves_copy_is_preserved_and_restored(self):
        with self.partial_copy(s.TREES['saves']), self.assertRaises(OSError):
            self.prepare()
        result = self.restore_and_check()
        self.assertTrue(any((Path(item['path']) / 'partial-copy.bin').exists() for item in result['after'] if item['tree'] == 'saves'))

    def test_restore_interruption_after_cache_preservation(self):
        self.prepare()
        (b.CACHE / 'postrun.bin').write_bytes(b'postrun')
        original = Path.rename
        def fail(path, target):
            result = original(path, target)
            if path == b.CACHE:
                raise OSError('injected after postrun-cache rename')
            return result
        with patch.object(Path, 'rename', fail), self.assertRaises(OSError):
            b.restore(self.stage, self.disk_receipt())
        result = self.restore_and_check()
        self.assertTrue((Path(result['benchmark']['after_cache'][0]['path']) / 'postrun.bin').exists())

    def test_restore_partial_copy_preserves_both_postrun_and_partial(self):
        self.prepare()
        (b.CACHE / 'postrun.bin').write_bytes(b'postrun')
        with self.partial_copy(b.CACHE), self.assertRaises(OSError):
            b.restore(self.stage, self.disk_receipt())
        result = self.restore_and_check()
        paths = [Path(item['path']) for item in result['benchmark']['after_cache']]
        self.assertTrue(any((path / 'postrun.bin').exists() for path in paths))
        self.assertTrue(any((path / 'partial-copy.bin').exists() for path in paths))

    def test_fixture_drift_blocks_ready_but_not_recovery_of_owned_originals(self):
        receipt = self.prepare()
        (b.FIXTURE2 / 'prewarmed-cache/eden/cache.bin').write_bytes(b'changed external source')
        with self.assertRaisesRegex(RuntimeError, 'immutable manifest'):
            b.check_ready(self.stage, receipt)
        self.restore_and_check()

    def test_already_restored_cache_drift_refused(self):
        self.prepare()
        self.restore_and_check()
        (b.CACHE / 'unexpected.bin').write_bytes(b'drift')
        with self.assertRaisesRegex(RuntimeError, 'Previously restored cache drifted'):
            b.restore(self.stage, self.disk_receipt())

    def test_wrong_tokens_refused(self):
        for token in ('27', '26-bench-a', '27-fixture2', '27-bench-../escape', '27-bench-' + 'a' * 17):
            with self.subTest(token=token), self.assertRaises(RuntimeError):
                b.bench_stage(token)


if __name__ == '__main__':
    unittest.main()
