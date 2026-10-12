"""Synthetic tests only. Parent must run under its finite check lease."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import live_perf as perf


class FrameTests(unittest.TestCase):
    def test_nearest_rank_and_strict_tails(self):
        result = perf.frame_summary([0, 10, 33.33, 50, 100])
        self.assertEqual(result['count'], 5)
        self.assertEqual(result['p50'], 33.33)
        self.assertEqual(result['p95'], 100)
        self.assertEqual(result['p99'], 100)
        self.assertAlmostEqual(result['mean'], 38.666)
        self.assertEqual(result['tails']['33.33']['strictly_greater_count'], 2)
        self.assertEqual(result['tails']['50.0']['fraction'], .2)

    def test_rank_boundary_not_interpolated(self):
        values = list(range(1, 101))
        self.assertEqual(perf.nearest_rank(values, .5), 50)
        self.assertEqual(perf.nearest_rank(values, .95), 95)
        self.assertEqual(perf.nearest_rank(values, .99), 99)
        self.assertEqual(perf.nearest_rank([7], .5), 7)

    def test_newline_millis(self):
        self.assertEqual(perf.parse_csv(b'0\r\n16.667\r\n1e2\r\n'), [0, 16.667, 100])

    def test_invalid_csv(self):
        for raw in (b'', b'\n', b'1\n\n2\n', b'NaN\n', b'inf\n', b'1e999\n',
                    b'-1\n', b'1,2\n', b'ms\n', b'1;2\n', b'\xff\n'):
            with self.subTest(raw=raw), self.assertRaises(ValueError):
                perf.parse_csv(raw)

    def test_frame_count_bounds(self):
        self.assertEqual(len(perf.parse_csv(b'0\n' * perf.MAX_FRAMES)), perf.MAX_FRAMES)
        with self.assertRaises(ValueError):
            perf.parse_csv(b'0\n' * (perf.MAX_FRAMES + 1))
        with self.assertRaises(ValueError):
            perf.frame_summary([float('nan')])


def resource(at, usage, memory=1024):
    return {'kind': 'resource', 'start': {'mono': at}, 'end': {'mono': at + .001},
            'values': {'cpu.stat': {'usage_usec': usage, 'nr_throttled': 0},
                       'memory.current': memory, 'memory.swap.current': 0}, 'errors': {}}


class ResourceTests(unittest.TestCase):
    def test_actual_elapsed_and_gap(self):
        result = perf.resource_summary([resource(10, 1000000, 100), resource(12, 4000000, 200)])
        self.assertEqual(result['cpu_core_seconds'], 3)
        self.assertEqual(result['covered_seconds'], 2)
        self.assertEqual(result['mean_cores_used_over_covered_intervals'], 1.5)
        self.assertEqual(result['gaps']['max_interval_seconds'], 2)
        self.assertTrue(result['cpu_intervals'][0]['over_nominal_gap'])
        self.assertEqual(result['memory_current_bytes']['min'], 100)
        self.assertEqual(result['memory_current_bytes']['max'], 200)

    def test_counter_reset_not_negative_or_crossed(self):
        result = perf.resource_summary([resource(0, 100), resource(1, 20), resource(2, 1000020)])
        self.assertEqual(len(result['excluded_intervals']), 1)
        self.assertEqual(result['excluded_intervals'][0]['reason'], 'counter_reset')
        self.assertEqual(result['cpu_core_seconds'], 1)
        self.assertEqual(result['covered_seconds'], 1)

    def test_missing_cpu_sample_not_bridged(self):
        missing = resource(1, 0)
        missing['values'].pop('cpu.stat')
        missing['errors']['cpu.stat'] = {'message': 'synthetic missing'}
        result = perf.resource_summary([resource(0, 0), missing, resource(2, 1000000)])
        self.assertEqual(result['cpu_intervals'], [])
        self.assertEqual(len(result['read_errors']), 1)

    def test_nonpositive_clock_excluded(self):
        result = perf.resource_summary([resource(2, 0), resource(2, 100)])
        self.assertEqual(result['excluded_intervals'][0]['reason'], 'nonpositive_elapsed')

    def test_malformed_counters(self):
        for text in ('usage_usec -1', 'usage_usec 1\nusage_usec 2', 'usage_usec nan', 'wrong'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                perf.counters(text)


class PathAndBoundsTests(unittest.TestCase):
    def test_phase_escapes_and_symlinks(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'runs'
            root.mkdir()
            (root / 'live-ok').mkdir()
            self.assertEqual(perf.run_path('live-ok', root), root / 'live-ok')
            for phase in ('../live-ok', '/live-ok', 'live-../x', 'live-' + 'a' * 37, 'training'):
                with self.subTest(phase=phase), self.assertRaises(ValueError):
                    perf.run_path(phase, root)
            (root / 'live-escape').symlink_to(Path(directory), target_is_directory=True)
            with self.assertRaises(ValueError):
                perf.run_path('live-escape', root)

    def test_seconds_bounds(self):
        self.assertEqual(perf.seconds_bound('1'), 1)
        self.assertEqual(perf.seconds_bound('120'), 120)
        for value in ('0', '120.01', '-1', 'nan', 'inf'):
            with self.subTest(value=value), self.assertRaises(ValueError):
                perf.seconds_bound(value)

    def test_manifest_resource_ownership_and_limits(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run, cg = root / 'live-test', root / 'cgroup'
            run.mkdir()
            cg.mkdir()
            unit = 'eden-control-123456abcdef.service'
            (cg / unit).mkdir()
            manifest = {'runtime_seconds': 900, 'memory_max_bytes': 8 * 1024**3,
                        'memory_swap_max_bytes': 0, 'state': 'ready', 'unit': unit,
                        'socket': str(run / 'control.sock'), 'last_status': {'commit': perf.PINNED_COMMIT},
                        'verified_limits': {'MemoryMax': str(8 * 1024**3), 'MemorySwapMax': '0',
                                            'RuntimeMaxUSec': '15min', 'ControlGroup': '/' + unit}}
            self.assertEqual(perf.validate_manifest(manifest, run, live=True, cgroup_root=cg)[1], cg / unit)
            for field, value in (('runtime_seconds', 1800), ('memory_max_bytes', 1),
                                 ('memory_swap_max_bytes', 1), ('unit', 'unrelated.service'),
                                 ('socket', str(root / 'other.sock')), ('state', 'stopped')):
                invalid = copy.deepcopy(manifest)
                invalid[field] = value
                with self.subTest(field=field), self.assertRaises(ValueError):
                    perf.validate_manifest(invalid, run, live=True, cgroup_root=cg)
            for field, value in (('ControlGroup', '/../' + unit), ('MemorySwapMax', '1'),
                                 ('RuntimeMaxUSec', '900s junk'), ('ControlGroup', '/another.service')):
                invalid = copy.deepcopy(manifest)
                invalid['verified_limits'][field] = value
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    perf.validate_manifest(invalid, run, live=True, cgroup_root=cg)
            invalid = copy.deepcopy(manifest)
            invalid['last_status']['commit'] = 'wrong'
            with self.assertRaises(ValueError):
                perf.validate_manifest(invalid, run, live=True, cgroup_root=cg)


class StatusAndClosureTests(unittest.TestCase):
    def test_rolling_cache_and_missing_not_frames(self):
        rows = []
        for index, (at, age) in enumerate(((1, 0), (1.25, 250), (2, 0))):
            rows.append({'kind': 'status', 'slot': index, 'scheduled_mono': at,
                         'request': {'mono': at}, 'received': {'mono': at + .01},
                         'rtt_seconds': .01, 'pin_verified': True,
                         'response': {'ok': True, 'result': {'game_fps': 60, 'system_fps': 60,
                             'stats_age_ms': age, 'last_frame_age_ms': age, 'shaders_building': 0,
                             'modal': False, 'running': True, 'powered_on': True}}})
        rows.append({'kind': 'status_missing', 'slot': 3, 'reason': 'deadline_missed'})
        result = perf.status_summary(rows)
        self.assertEqual(result['valid_observations'], 3)
        self.assertEqual(result['adjacent_cache_intervals_overlap'], 1)
        self.assertEqual(len(result['missing_slots']), 1)
        self.assertEqual(result['successful_observation_gaps']['intervals_over_nominal'], 1)
        self.assertNotIn('frame_count', result)

    def test_closed_collection_required(self):
        start = {'kind': 'start', 'format': 1, 'seconds': 1, 'expected_commit': perf.PINNED_COMMIT}
        end = {'kind': 'end'}
        raw = lambda rows: ('\n'.join(json.dumps(row) for row in rows) + '\n').encode()
        self.assertEqual(len(perf.closed_collection(raw([start, end]))), 2)
        for rows in ([start], [end, start], [start, end, end]):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                perf.closed_collection(raw(rows))


if __name__ == '__main__':
    unittest.main()
