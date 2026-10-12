"""Closed real-image gates; no emulator, profile mutation or native controls."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from PIL import Image, ImageDraw

import live_benchmark_finish as finish


class DialogTests(unittest.TestCase):
    def test_separate_native_run_matches_despite_different_results_background(self):
        image = finish.perf.RUNS / 'live-r27-fixture2/fixture2-native-save.png'
        self.assertTrue(finish.dialog_match(image)['matched'])

    def test_css_combat_and_results_refused(self):
        for phase, image in (('live-r27-bench-b1off', 'fixed-ready.png'),
                             ('live-r27-bench-b1off', 'measure-window-end.png'),
                             ('live-r27-bench-pilot3', 'pilot3-warmup-details.png')):
            with self.subTest(image=image):
                self.assertFalse(finish.dialog_match(finish.perf.RUNS / phase / image)['matched'])

    def test_same_dialog_with_unselected_no_or_different_question_is_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            for box in ((430, 315, 875, 365), (235, 545, 628, 632)):
                with self.subTest(box=box), Image.open(finish.REFERENCE) as image:
                    ImageDraw.Draw(image).rectangle(box, fill='white')
                    path = Path(directory) / 'changed.png'
                    image.save(path)
                    self.assertFalse(finish.dialog_match(path)['matched'])

    def test_reference_drift_fails_closed(self):
        with patch.object(finish, 'REFERENCE_SHA', '0' * 64):
            with self.assertRaisesRegex(ValueError, 'reference changed'):
                finish.dialog_match(finish.REFERENCE)


if __name__ == '__main__':
    unittest.main()
