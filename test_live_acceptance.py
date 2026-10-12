"""Restoration fault tests; all fixtures live under /tmp/opencode, no real profile."""
import json
import os
from pathlib import Path
import shutil
import tempfile
import time
import unittest
from unittest.mock import patch

import live_acceptance as staging


class LaunchBoundaryTests(unittest.TestCase):
    def test_campaigns_share_candidate_pins_but_not_receipt_paths(self):
        self.assertEqual(staging.campaign_stage("27"), (27, staging.HERE / "live-acceptance-27"))
        revision, stage = staging.campaign_stage("27-deluxe-a")
        self.assertEqual(revision, 27)
        self.assertEqual(stage, staging.HERE / "live-acceptance-27-deluxe-a")
        for token in ("../27", "27/other", "027", "22-old", "27-", "27-" + "x" * 25):
            with self.assertRaisesRegex(RuntimeError, "Expected REV"):
                staging.campaign_stage(token)

    def test_phase_limit_matches_launcher_socket_before_any_spawn(self):
        boundary = "live-" + "x" * 36
        self.assertEqual(len(os.fsencode(staging.ROOT / "runs" / boundary / "control.sock")), 100)
        with patch.object(staging, "frozen", side_effect=RuntimeError("pins reached")) as pins, \
                patch.object(staging.subprocess, "Popen") as spawn:
            for phase in (boundary + "x", "live-" + "x" * 64):
                with self.assertRaisesRegex(RuntimeError, "at most 41 characters"):
                    staging.start(None, {"state": "installed"}, phase, "unused")
            pins.assert_not_called()
            spawn.assert_not_called()
            # The longest valid phase reaches pin validation without launching.
            with self.assertRaisesRegex(RuntimeError, "pins reached"):
                staging.start(None, {"state": "installed", "revision": 24, "mode": "trace"},
                              boundary, "unused")
            pins.assert_called_once_with(24, "trace")
            spawn.assert_not_called()


class CoexistenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="live-coexist-test-", dir="/tmp/opencode")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        profile = self.root / "profile"
        trees = {name: profile / name for name in ("config", "saves", "sdmc")}
        protected = {name: profile / name for name in ("keys", "load")}
        for path in (*trees.values(), *protected.values()):
            path.mkdir(parents=True)
            (path / "original").write_bytes(b"baseline")
        plugins = trees["sdmc"] / "plugins"
        plugins.mkdir()
        (plugins / "libarcropolis.nro").write_bytes(b"ARC")
        settings = trees["sdmc"] / "ultimate/TrainingModpack"
        settings.mkdir(parents=True)
        (settings / "training_modpack_menu.conf").write_text(json.dumps({"menu": {"unrelated": 7}}))
        training = self.root / "training.nro"
        training.write_bytes(b"Training fixture")
        fixture = self.root / "deluxe"
        fixture.mkdir()
        deluxe = fixture / staging.DELUXE_NAME
        deluxe.write_bytes(b"D" * 2428928)
        digest = staging.sha(deluxe)
        (fixture / "manifest.json").write_text(json.dumps({"nro": staging.DELUXE_NAME,
            "nro_sha256": digest, "nro_size": 2428928}))
        replacements = {"ROOT": self.root, "PROFILE": profile, "TREES": trees,
            "PROTECTED": protected, "PLUGINS": plugins, "SETTINGS": settings,
            "ARC": staging.sha(plugins / "libarcropolis.nro"), "DELUXE_FIXTURE": fixture,
            "DELUXE_SHA": digest, "stopped": lambda: None, "gate": lambda: {"fixture": True},
            "deadline": lambda _: time.time() + 3600,
            "frozen": lambda *_: ({}, {"libtraining_modpack.nro": staging.sha(training)},
                                  {"libtraining_modpack.nro": training})}
        for key, value in replacements.items():
            item = patch.object(staging, key, value)
            item.start()
            self.addCleanup(item.stop)
        self.stage = self.root / "live-acceptance-27-coexist"
        self.before = {name: staging.tree(path) for name, path in trees.items()}

    def test_bad_fixture_is_rejected_before_backup_or_profile_mutation(self):
        (staging.DELUXE_FIXTURE / staging.DELUXE_NAME).write_bytes(b"corrupted")
        with self.assertRaisesRegex(RuntimeError, "Deluxe fixture pin mismatch"):
            staging.prepare(self.stage, 27, "normal", "fixture", "deluxe6c39")
        self.assertFalse(self.stage.exists())
        self.assertEqual({n: staging.tree(p) for n, p in staging.TREES.items()}, self.before)

    def test_coexistence_install_preserves_baseline_and_restores_all_outputs(self):
        receipt = staging.prepare(self.stage, 27, "normal", "fixture", "deluxe6c39")
        self.assertEqual(receipt["campaign"], self.stage.name)
        self.assertEqual(staging.sha(staging.PLUGINS / staging.DELUXE_NAME), staging.DELUXE_SHA)
        config = staging.TREES["sdmc"] / "ultimate/ssbu_online_deluxe_rewrite/config.ini"
        self.assertEqual(config.read_text(), staging.DELUXE_CONFIG)
        self.assertEqual(receipt["deluxe"]["config_sha256"], staging.sha(config))
        config.with_name("runtime-evidence.txt").write_text("native observations")
        restored = staging.restore(self.stage, receipt)
        self.assertEqual({n: staging.tree(p) for n, p in staging.TREES.items()}, self.before)
        after = next(Path(row["path"]) for row in restored["after"] if row["tree"] == "sdmc")
        self.assertEqual((after / "ultimate/ssbu_online_deluxe_rewrite/runtime-evidence.txt").read_text(),
                         "native observations")
        self.assertTrue((after / "plugins" / staging.DELUXE_NAME).is_file())

    def test_changed_deluxe_config_blocks_launch_before_spawn(self):
        receipt = staging.prepare(self.stage, 27, "normal", "fixture", "deluxe6c39")
        config = staging.TREES["sdmc"] / "ultimate/ssbu_online_deluxe_rewrite/config.ini"
        config.write_text("changed after staging")
        with patch.object(staging.subprocess, "Popen") as spawn:
            with self.assertRaisesRegex(RuntimeError, "Deluxe configuration changed"):
                staging.start(self.stage, receipt, "live-coexist-test", "fixture")
            spawn.assert_not_called()


class NormalStopTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="live-stop-test-", dir="/tmp/opencode")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.phase = "live-stopped-fixture"
        self.manifest = self.root / "runs" / self.phase / "run.json"
        self.manifest.parent.mkdir(parents=True)
        self.run = {"state": "running", "runtime_seconds": 900, "started_at": 1000}
        root = patch.object(staging, "ROOT", self.root)
        root.start()
        self.addCleanup(root.stop)

    def test_completed_run_never_issues_late_control_requests(self):
        self.run["state"] = "stopped"
        self.manifest.write_text(json.dumps(self.run))
        with patch.object(staging.subprocess, "run") as stop, \
                patch.object(staging.time, "time", return_value=5000):
            staging.normal_stop(self.phase, os.getpid())
            stop.assert_not_called()

    def test_delayed_parent_still_requests_bounded_normal_shutdown(self):
        self.manifest.write_text(json.dumps(self.run))
        with patch.object(staging.subprocess, "run", return_value=staging.subprocess.CompletedProcess([], 0, "", "")) as stop, \
                patch.object(staging.time, "time", return_value=1661):
            staging.normal_stop(self.phase, os.getpid())
            stop.assert_called_once_with(staging.PS + [
                f"& /usr/bin/python3 '{self.root}/acceptance.py' stop '{self.phase}'"],
                check=True, timeout=179, capture_output=True, text=True)

    def test_keyboard_overlap_retries_with_remaining_absolute_budget(self):
        self.manifest.write_text(json.dumps(self.run))
        rejected = staging.subprocess.CalledProcessError(1, [], stderr='{"code": "modal_active"}')
        stopped = staging.subprocess.CompletedProcess([], 0, "", "")
        with patch.object(staging.subprocess, "run", side_effect=[rejected, stopped]) as stop, \
                patch.object(staging.time, "time", side_effect=[1660,1660,1660,1662,1662,1662]), \
                patch.object(staging.time, "sleep"):
            staging.normal_stop(self.phase, os.getpid())
            self.assertEqual(stop.call_count, 2)
            self.assertEqual([c.kwargs['timeout'] for c in stop.call_args_list], [180,178])

    def test_benchmark_and_slowdown_stop_before_parent_target(self):
        for category in ('bench', 'slow'):
            phase = f'live-r27-{category}-fixture'
            manifest = self.root / 'runs' / phase / 'run.json'
            manifest.parent.mkdir(parents=True)
            manifest.write_text(json.dumps(self.run))
            with self.subTest(category=category), \
                    patch.object(staging.subprocess, 'run', return_value=staging.subprocess.CompletedProcess([], 0, '', '')) as stop, \
                    patch.object(staging.time, 'time', return_value=1466):
                staging.normal_stop(phase, os.getpid())
                self.assertEqual(stop.call_count, 1)
                self.assertEqual(stop.call_args.kwargs['timeout'], 179)
        self.assertEqual(staging.normal_stop_after('live-r27-reopen'), 660)

    def test_persistent_modal_cannot_extend_shutdown_window(self):
        self.manifest.write_text(json.dumps(self.run))
        with patch.object(staging.subprocess, "run") as stop, \
                patch.object(staging.time, "time", return_value=1841):
            with self.assertRaisesRegex(RuntimeError, "exhausted its 180s window"):
                staging.normal_stop(self.phase, os.getpid())
            stop.assert_not_called()

    def test_monitor_rejects_changed_runtime_contract_before_control(self):
        self.run["runtime_seconds"] = 1800
        self.manifest.write_text(json.dumps(self.run))
        with patch.object(staging.subprocess, "run") as stop:
            with self.assertRaisesRegex(RuntimeError, "Unexpected owned runtime bound"):
                staging.normal_stop(self.phase, os.getpid())
            stop.assert_not_called()


class RestoreTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="live-stage-test-", dir="/tmp/opencode")
        self.root = Path(self.temp.name)
        self.stage = self.root / "receipt"
        self.stage.mkdir()
        self.trees = {name: self.root / "profile" / name for name in ("config", "saves", "sdmc")}
        self.protected = {name: self.root / name for name in ("keys", "load")}
        for path in (*self.trees.values(), *self.protected.values()):
            path.mkdir(parents=True)
            (path / "original").write_bytes(b"original fixture")
        self.plugins = self.trees["sdmc"] / "plugins"
        self.plugins.mkdir()
        (self.plugins / "libarcropolis.nro").write_bytes(b"ARC fixture")
        self.patches = [patch.object(staging, "TREES", self.trees),
                        patch.object(staging, "PROTECTED", self.protected),
                        patch.object(staging, "PLUGINS", self.plugins),
                        patch.object(staging, "ARC", staging.sha(self.plugins / "libarcropolis.nro")),
                        patch.object(staging, "stopped", lambda: None)]
        for item in self.patches:
            item.start()
        self.receipt = {"state": "installed", "runs": [], "after": [], "restored_trees": [],
                        "before": {name: staging.tree(path) for name, path in self.trees.items()},
                        "protected": {name: staging.tree(path) for name, path in self.protected.items()}}
        for name, path in self.trees.items():
            shutil.copytree(path, self.stage / "before" / name)
            (path / "native-output").write_bytes(name.encode())

    def tearDown(self):
        for item in reversed(self.patches):
            item.stop()
        self.temp.cleanup()

    def test_interrupted_copy_preserves_both_game_outputs_and_partial_restore(self):
        original_copytree = shutil.copytree

        def interrupted(source, destination, *args, **kwargs):
            if Path(source) == self.stage / "before/config":
                Path(destination).mkdir()
                (Path(destination) / "partial").write_bytes(b"interrupted")
                raise OSError("injected interruption after first restore write")
            return original_copytree(source, destination, *args, **kwargs)

        with patch.object(staging.shutil, "copytree", interrupted):
            with self.assertRaisesRegex(OSError, "injected interruption"):
                staging.restore(self.stage, self.receipt)
        retained = json.loads((self.stage / "receipt.json").read_text())
        self.assertEqual(retained["state"], "restoring")
        final = staging.restore(self.stage, retained)
        self.assertEqual(final["state"], "restored")
        self.assertEqual({name: staging.tree(path) for name, path in self.trees.items()}, final["before"])
        self.assertEqual(len(final["after"]), 4)
        for saved in final["after"]:
            self.assertEqual(staging.tree(Path(saved["path"])), saved["manifest"])
        self.assertTrue(any((Path(saved["path"]) / "partial").exists() for saved in final["after"]))
        for name in self.trees:
            self.assertTrue(any((Path(saved["path"]) / "native-output").read_bytes() == name.encode()
                                for saved in final["after"]
                                if (Path(saved["path"]) / "native-output").exists()))
        self.assertEqual(staging.restore(self.stage, final), final)

    def test_corrupt_backup_fails_before_moving_any_profile_tree(self):
        (self.stage / "before/saves/original").write_bytes(b"corrupt backup")
        before_attempt = {name: staging.tree(path) for name, path in self.trees.items()}
        with self.assertRaisesRegex(RuntimeError, "Backup drift"):
            staging.restore(self.stage, self.receipt)
        self.assertEqual({name: staging.tree(path) for name, path in self.trees.items()}, before_attempt)
        self.assertFalse((self.stage / "after").exists())

    def test_changed_protected_data_prevents_restore_without_discarding_outputs(self):
        (self.protected["load"] / "new-user-file").write_bytes(b"new")
        before_attempt = {name: staging.tree(path) for name, path in self.trees.items()}
        with self.assertRaisesRegex(RuntimeError, "Protected keys/load changed"):
            staging.restore(self.stage, self.receipt)
        self.assertEqual({name: staging.tree(path) for name, path in self.trees.items()}, before_attempt)
        self.assertEqual((self.protected["load"] / "new-user-file").read_bytes(), b"new")


if __name__ == "__main__":
    unittest.main()
