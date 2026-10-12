"""Synthetic publication timing and stopped-inspection failure recovery."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import live_benchmark_save as save
import live_benchmark_close as close


class PublicationTests(unittest.TestCase):
    def exercise(self, publish):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            captures = root / 'captures'
            clock = [0.]
            calls = []

            def sleep(seconds):
                clock[0] += seconds
                if publish and clock[0] >= 1.3 and not captures.exists():
                    identity = b'replay202610129999.bin'.hex()
                    bundle = captures / f'{identity}.smd'
                    bundle.mkdir(parents=True)
                    (bundle / 'manifest.json').write_text('{"format":"synthetic"}')
                    (bundle / 'timeline.jsonl').write_text('{}\n')
                    (bundle / f'{identity}.tmrc').write_bytes(b'synthetic cache')

            class Client:
                def __init__(self, *args, **kwargs):
                    pass

                def request(self, method, params):
                    calls.append((clock[0], method, params))
                    return {'ok': True, 'result': {'guest_delivery': 'not_claimed'}}

            with patch.object(save, 'CAPTURES', captures), \
                 patch.object(save.perf, 'run_path', return_value=root), \
                 patch.object(save.perf, 'read_manifest', return_value=({'started_at': 0}, 'pin')), \
                 patch.object(save.perf, 'validate_manifest', return_value=(root / 'sock', root)), \
                 patch.object(save.perf, 'client_class', return_value=Client), \
                 patch.object(save.time, 'monotonic', side_effect=lambda: clock[0]), \
                 patch.object(save.time, 'time', side_effect=lambda: clock[0]), \
                 patch.object(save.time, 'sleep', side_effect=sleep):
                save.confirm('live-r27-bench-synthetic')
            rows = [json.loads(line) for line in (root / 'save-publication.jsonl').read_text().splitlines()]
        return rows, calls

    def test_on_records_first_readable_but_keeps_same_observation_window(self):
        rows, calls = self.exercise(True)
        readable = [r for r in rows if r['kind'] == 'first_readable']
        self.assertEqual(len(readable), 1)
        self.assertEqual(readable[0]['native_name'], 'replay202610129999.bin')
        self.assertAlmostEqual(readable[0]['request_elapsed_seconds'], 1.)
        self.assertAlmostEqual(rows[-2]['actual_window_seconds'], 10.)
        self.assertEqual(calls[-1][1], 'input.reset')

    def test_off_records_no_publication_without_inventing_zero_latency(self):
        rows, calls = self.exercise(False)
        self.assertFalse(any(r['kind'] == 'first_readable' for r in rows))
        self.assertEqual(rows[-2]['readable_bundles'], [])
        self.assertAlmostEqual(rows[-2]['actual_window_seconds'], 10.)
        self.assertEqual([c[2].get('button') for c in calls if c[1] == 'input.button'], ['right', 'a'])

    def test_wrong_phase_refused_before_control(self):
        with self.assertRaises(ValueError):
            save.confirm('unowned')


class ClosureTests(unittest.TestCase):
    def test_failed_inspection_still_restores_and_retains_error(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            stage = root / 'stage'
            stage.mkdir()
            run = root / 'run'
            run.mkdir()
            tree = {'files': {}, 'directories': []}
            receipt = {'state': 'installed', 'runs': [{'phase': 'live-r27-bench-synthetic',
                       'launcher_pid': 999999991, 'guard_pid': 999999992, 'normal_stop_pid': 999999993}],
                       'before': {'config': tree}, 'benchmark': {'before_cache_manifest': tree}}
            (stage / 'receipt.json').write_text(json.dumps(receipt))
            restored = {**receipt, 'state': 'restored', 'restored_at': 123}
            with patch.object(close.stage_helper, 'bench_stage', return_value=stage), \
                 patch.object(close.stage_helper, 'CACHE', root / 'cache'), \
                 patch.object(close.stage_helper, 'restore', return_value=restored) as restore, \
                 patch.object(close.perf, 'run_path', return_value=run), \
                 patch.object(close.perf, 'read_manifest', return_value=({'state': 'stopped'}, 'pin')), \
                 patch.object(close.perf, 'validate_manifest'), \
                 patch.object(close.s, 'ROOT', root), \
                 patch.object(close.s, 'TREES', {'config': root / 'config'}), \
                 patch.object(close.s, 'stopped'), \
                 patch.object(close.s, 'tree', return_value=tree), \
                 patch.object(close.s, 'verify_protected'), \
                 patch.object(close.s, 'inventory', return_value={'libarcropolis.nro': close.s.ARC}):
                self.assertEqual(close.close('27-bench-synthetic'), 1)
                restore.assert_called_once()
            report = json.loads((stage / 'benchmark-closure.json').read_text())
            self.assertTrue(report['restoration_exact'])
            self.assertEqual(report['errors'][0]['type'], 'FileNotFoundError')
            self.assertFalse(report['performance_acceptance'])


if __name__ == '__main__':
    unittest.main()
