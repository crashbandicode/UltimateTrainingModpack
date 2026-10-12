"""Synthetic quota restoration only; never calls systemd or an emulator."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import live_benchmark_pulse as pulse


class PulseTests(unittest.TestCase):
    def exercise(self, original='max 100000', fail_after_set=False, interrupt=False):
        calls, rows, waits = [], [], []
        with tempfile.TemporaryDirectory() as directory:
            group = Path(directory)
            (group / 'cpu.max').write_text(original)
            (group / 'cpu.stat').write_text('usage_usec 100\nnr_throttled 0\nthrottled_usec 0\n')

            def fake_quota(unit, value):
                calls.append(value)
                (group / 'cpu.max').write_text('100000 100000' if value else 'max 100000')
                if value and fail_after_set:
                    raise RuntimeError('synthetic response loss after property applied')

            with patch.object(pulse, 'quota', side_effect=fake_quota), patch.object(pulse.time, 'sleep', side_effect=waits.append):
                error = None
                try:
                    pulse.pulse(group, 'eden-control-123456abcdef.service',
                                lambda kind, **data: rows.append((kind, data)), lambda: interrupt)
                except (ValueError, RuntimeError) as exc:
                    error = exc
            restored = (group / 'cpu.max').read_text()
        return calls, rows, waits, restored, error

    def test_finite_pulse_and_exact_restore(self):
        calls, rows, waits, restored, error = self.exercise()
        self.assertIsNone(error)
        self.assertEqual(calls, ['100%', ''])
        self.assertEqual(waits, [10, 5])
        self.assertEqual(restored, 'max 100000')
        self.assertEqual([kind for kind, _ in rows], ['original', 'pulse_begin', 'pulse_end', 'restored'])

    def test_lost_response_still_restores(self):
        calls, rows, waits, restored, error = self.exercise(fail_after_set=True)
        self.assertIsInstance(error, RuntimeError)
        self.assertEqual(calls, ['100%', ''])
        self.assertEqual(restored, 'max 100000')
        self.assertEqual(rows[-1][0], 'restored')

    def test_existing_quota_or_prior_interrupt_never_mutates(self):
        calls, _, _, _, error = self.exercise(original='50000 100000')
        self.assertIsInstance(error, ValueError)
        self.assertEqual(calls, [])
        calls, rows, _, _, error = self.exercise(interrupt=True)
        self.assertIsNone(error)
        self.assertEqual(calls, [])
        self.assertEqual(rows[-1][0], 'interrupted_before_pulse')


if __name__ == '__main__':
    unittest.main()
