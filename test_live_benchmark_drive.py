"""Synthetic timing/control boundaries; no emulator or live-profile access."""
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import live_benchmark_drive as drive


class Clock:
    now = 0

    def sleep(self, seconds):
        self.now += seconds


class DriveTests(unittest.TestCase):
    def exercise(self, mode, elapsed=0, collector_exit=0, pulse=False, deluxe=False,
                 save=False, finish_error=None):
        clock = Clock()
        calls = []

        def finish(*args, **kwargs):
            calls.append((clock.now, 'finish', {}))
            if finish_error:
                raise ValueError(finish_error)

        def stop(*args, **kwargs):
            calls.append((clock.now, 'normal_stop', {'timeout': kwargs['timeout']}))

        class Client:
            def __init__(self, *args, **kwargs):
                pass

            def request(self, method, params):
                calls.append((clock.now, method, params))
                result = {'commit': drive.perf.PINNED_COMMIT, 'running': True, 'modal': False}
                return {'ok': True, 'result': result}

        class Child:
            pid = 123456789
            returncode = None

            def __init__(self, *args, **kwargs):
                assert kwargs['start_new_session']
                self.start = clock.now

            def wait(self, timeout):
                clock.now = max(clock.now, self.start + 40)
                self.returncode = collector_exit

            def poll(self):
                return self.returncode

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch.object(drive.perf, 'run_path', return_value=root), \
                 patch.object(drive.perf, 'read_manifest', return_value=({'started_at': -elapsed}, 'unused')), \
                 patch.object(drive.perf, 'validate_manifest', return_value=(root / 'sock', root)), \
                 patch.object(drive.perf, 'client_class', return_value=Client), \
                 patch.object(drive.time, 'time', side_effect=lambda: clock.now), \
                 patch.object(drive.time, 'monotonic', side_effect=lambda: clock.now), \
                 patch.object(drive.time, 'sleep', side_effect=clock.sleep), \
                 patch.object(drive.subprocess, 'Popen', Child), \
                  patch.object(drive, 'deluxe_snapshot', side_effect=lambda _: {'observed_at': clock.now}), \
                  patch('live_benchmark_finish.finish', side_effect=finish), \
                  patch.object(drive.subprocess, 'run', side_effect=stop):
                error = None
                try:
                    drive.run_cycle('live-r27-slow-synthetic' if pulse else 'live-r27-bench-synthetic', mode, pulse, deluxe, save)
                except (ValueError, RuntimeError) as exc:
                    error = exc
                file = root / f'drive-{mode}.jsonl'
                rows = [json.loads(line) for line in file.read_text().splitlines()] if file.exists() else []
        return clock.now, calls, rows, error

    def test_measured_window_bracketed_and_same_bounded_inputs(self):
        elapsed, calls, rows, error = self.exercise('measure')
        self.assertIsNone(error)
        self.assertEqual(elapsed, 55)
        self.assertEqual([t for t, method, _ in calls if method == 'ui.screenshot'], [15, 55])
        self.assertEqual(len([c for c in calls if c[1] == 'input.stick']), 8)
        self.assertEqual([c[2]['x'] for c in calls if c[1] == 'input.stick'], [.375, -.375] * 4)
        self.assertEqual(sum(c[1] == 'input.button' and c[2]['button'] == 'a' for c in calls), 8)
        self.assertEqual(rows[-2]['kind'], 'end')
        self.assertEqual(calls[-1][1], 'input.reset')

    def test_warmup_does_not_collect_or_claim_measurement(self):
        elapsed, calls, rows, error = self.exercise('warmup')
        self.assertIsNone(error)
        self.assertEqual(elapsed, 110)
        self.assertFalse(any(row['kind'] == 'collector' for row in rows))
        self.assertEqual([t for t, method, _ in calls if method == 'ui.screenshot'], [15, 110])

    def test_parent_deadline_refuses_before_any_control_request(self):
        for mode, elapsed in (('measure', 341), ('warmup', 351)):
            with self.subTest(mode=mode):
                _, calls, rows, error = self.exercise(mode, elapsed)
                self.assertIsInstance(error, ValueError)
                self.assertEqual(calls, [])
                self.assertEqual(rows, [])

    def test_collector_failure_retained_and_inputs_released(self):
        _, calls, rows, error = self.exercise('measure', collector_exit=2)
        self.assertIsInstance(error, RuntimeError)
        self.assertIn('exit2', str(error))
        self.assertEqual(calls[-1][1], 'input.reset')
        self.assertFalse(any(row['kind'] == 'end' for row in rows))

    def test_postmeasurement_finish_runs_after_window_and_always_stops(self):
        for failure in (None, 'native dialog mismatch'):
            with self.subTest(failure=failure):
                _, calls, _, error = self.exercise('measure', save=True, finish_error=failure)
                self.assertEqual([t for t, method, _ in calls if method == 'finish'], [55])
                self.assertEqual(calls[-1][1:], ('normal_stop', {'timeout': 60}))
                self.assertEqual(calls[-2][1], 'input.reset')
                self.assertEqual(isinstance(error, ValueError), failure is not None)

    def test_explicit_slowdown_controller_is_recorded_and_bounded(self):
        elapsed, _, rows, error = self.exercise('measure', pulse=True)
        self.assertIsNone(error)
        self.assertEqual(elapsed, 55)
        controller = next(row for row in rows if row['kind'] == 'pulse_controller')
        self.assertEqual((controller['delay'], controller['hold']), (10, 5))
        with self.assertRaises(ValueError):
            drive.run_cycle('live-r27-bench-synthetic', 'measure', True)

    def test_deluxe_observations_bracket_collector_without_midwindow_reads(self):
        _, _, rows, error = self.exercise('measure', deluxe=True)
        self.assertIsNone(error)
        self.assertEqual([(r['kind'], r['observed_at']) for r in rows if r['kind'].startswith('deluxe_')],
                         [('deluxe_start', 15), ('deluxe_end', 55)])

    def test_deluxe_status_refuses_stale_or_inactive_mode(self):
        fields = ('build=0.1.0-2db70f90aaae-dirty-1791663838\nengine_status=Active\n'
                  'policy_effective=lesslag\npolicy_failed=false\nactive_textures=2\n'
                  'allocated_textures=3\nauto_profile=false\nplatform=emulator\nscene_ready=true\n'
                  'scene_connection=offline\nscene_status=singles\nfps_boost_status=Off\n')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'status.txt'
            path.write_text(fields)
            with patch.object(drive, 'DELUXE_STATUS', path):
                self.assertEqual(drive.deluxe_snapshot(0)['raw'], fields)
                with self.assertRaises(ValueError):
                    drive.deluxe_snapshot(path.stat().st_mtime + 1)
                path.write_text(fields.replace('policy_effective=lesslag', 'policy_effective=vanilla'))
                with self.assertRaises(ValueError):
                    drive.deluxe_snapshot(0)


if __name__ == '__main__':
    unittest.main()
