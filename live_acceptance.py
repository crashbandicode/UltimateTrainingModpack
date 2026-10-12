"""Parent-only live capture staging. A separately recorded runtime lease is required.

prepare REV[-CAMPAIGN] normal|trace CLEANUP_BY_ISO [deluxe6c39]
configure REV[-CAMPAIGN] on|off
start REV[-CAMPAIGN] UNIQUE_PHASE CLEANUP_BY_ISO
restore REV[-CAMPAIGN]

Never boots a game. Use acceptance.py ready/boot/shot/stop after start. No old
revision-specific installer is invoked. Receipts and after-state are retained.
"""
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1] / "UltimateTrainingModpack-single-file"
ROOT = Path("/home/intpa/eden-replay/acceptance-fable")
PROFILE = ROOT / "profile"
TREES = {"config": PROFILE / "config", "saves": PROFILE / "data/eden/nand/user/save",
         "sdmc": PROFILE / "data/eden/sdmc"}
PROTECTED = {"keys": PROFILE / "data/eden/keys", "load": PROFILE / "data/eden/load"}
SETTINGS = TREES["sdmc"] / "ultimate/TrainingModpack"
PLUGINS = TREES["sdmc"] / "atmosphere/contents/01006A800016E000/romfs/skyline/plugins"
ARC = "92171b2f4dbd7afe9361a55922f01de7f46ffae09f05185c1cea4ab79dba9a1c"
HOOKS = {"libnro_hook.nro": "ca7aec03aee49fea539dcb92569fea5bc5389f77377ba2a30e4d0b7ebd0255e6",
         "libparam_hook.nro": "3ed7a2945084176cbb7742d99878f1e8d3a0992feb0fef74be2cfbc68ac8fb88"}
EXTERNAL_ROOTS = {
    "smash-match-data": Path("/home/intpa/smash-match-data-design"),
    "smash-replay-core-manifest": Path("/home/intpa/smash-replay-export/replay-core"),
}
PS = ["/usr/bin/pwsh", "-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]
RUNTIME = 900
NORMAL_STOP_AFTER = 660
DELUXE_NAME = "libssbu_online_deluxe_rewrite.nro"
DELUXE_SHA = "6c396f10e8c4c3bb5bce41de7922f63f77500de37c39c7e477ab54f2f527f8a9"
DELUXE_FIXTURE = HERE / "deluxe-fixture-6c396f10"
DELUXE_CONFIG = ("version=1\neffects=on\neffects_scope=all\ninput_buffer=auto\n"
                 "render_mode=lesslag\nauto_profile=false\nplatform=emulator\n"
                 "peer_observer=false\nstealth=true\nstreamer=false\nfps_boost=false\n")


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def tree(path):
    require(path.is_dir() and not path.is_symlink(), f"Not an owned directory: {path}")
    files, directories = {}, []
    for item in sorted(path.rglob("*")):
        require(not item.is_symlink(), f"Unexpected symlink: {item}")
        name = str(item.relative_to(path))
        if item.is_dir():
            directories.append(name)
        else:
            require(item.is_file(), f"Unexpected non-regular file: {item}")
            files[name] = sha(item)
    return {"files": files, "directories": directories}


def inventory():
    require(PLUGINS.is_dir() and not PLUGINS.is_symlink(), "Missing owned plugins directory")
    result = {}
    for path in PLUGINS.iterdir():
        require(path.is_file() and not path.is_symlink(), f"Unexpected plugin entry: {path.name}")
        result[path.name] = sha(path)
    return result


def gate():
    available = int(next(line.split()[1] for line in Path("/proc/meminfo").read_text().splitlines()
                         if line.startswith("MemAvailable:")))
    conflicts = []
    for proc in Path("/proc").iterdir():
        if not proc.name.isdigit():
            continue
        try:
            comm = (proc / "comm").read_text().strip()
            if comm in ("eden", "cargo", "cargo-skyline", "rustc", "ninja", "cc1", "cc1plus",
                        "clang", "cmake", "gdb", "ffmpeg", "render-core"):
                conflicts.append({"pid": int(proc.name), "comm": comm})
        except FileNotFoundError:
            pass
    result = {"at": time.time(), "available_kib": available, "conflicts": conflicts}
    require(available > 10 * 1024 * 1024 and not conflicts, f"Resource gate failed: {result}")
    return result


def stopped():
    for path in (ROOT / "runs").glob("*/run.json"):
        run = json.loads(path.read_text())
        require(run.get("state") == "stopped", f"Clone run not stopped: {path}")
        unit = run["unit"]
        require(re.fullmatch(r"eden-control-[a-f0-9]+(?:\.service)?", unit), "Unknown run unit")
        result = subprocess.run(PS + [f"systemctl --user show '{unit}' -p MainPID -p ActiveState"],
                                capture_output=True, text=True, check=True, timeout=15)
        require("MainPID=0" in result.stdout and any(f"ActiveState={state}" in result.stdout
                for state in ("inactive", "failed")), f"Clone unit still active: {unit}")


def deadline(text):
    value = datetime.fromisoformat(text)
    require(value.tzinfo is not None, "Cleanup deadline needs an explicit timezone")
    stamp = value.astimezone(timezone.utc).timestamp()
    require(time.time() + RUNTIME + 120 < stamp, "Insufficient runtime and cleanup margin")
    require(stamp < time.time() + 3 * 3600, "Use a finite near-term lease")
    return stamp


def relative(name):
    path = Path(name)
    require(not path.is_absolute() and path.parts and all(part not in ("..", ".") for part in path.parts),
            "Invalid relative source path")
    return path


def frozen(revision, mode):
    folder = HERE / f"revision{revision}-builds"
    pin = json.loads((folder / f"{mode}.json").read_text())
    require(pin["revision"] == revision and pin["mode"] == mode, "Wrong frozen candidate")
    require(pin["status"] == "checked_candidate_runtime_pending", "Unexpected candidate status")
    require(sha(folder / f"{mode}.nro") == pin["sha256"], "Frozen NRO hash mismatch")
    build = json.loads((HERE / relative(pin["build_receipt"])).read_text())
    require(build["exit_code"] == 0, "Failed native build receipt")
    require(pin["source_hashes"], "Missing product source pins")
    for name, digest in pin["source_hashes"].items():
        path = relative(name)
        require(sha(REPO / path) == sha(folder / "source" / path) == digest, f"Source drift: {name}")
    external = pin.get("external_sources", {})
    if revision >= 24:
        require(set(external) == set(EXTERNAL_ROOTS), "Missing shared dependency pins")
    for name, dependency in external.items():
        require(name in EXTERNAL_ROOTS and Path(dependency["root"]) == EXTERNAL_ROOTS[name],
                "Unexpected external source root")
        snapshot = relative(dependency["snapshot"])
        require(str(snapshot) == f"external/{name}" and dependency["files"], "Invalid external snapshot")
        for source, digest in dependency["files"].items():
            path = relative(source)
            require(sha(EXTERNAL_ROOTS[name] / path) == sha(folder / snapshot / path) == digest,
                    f"External source drift: {name}/{source}")
    normal = json.loads((folder / "normal.json").read_text())
    require(pin["source_hashes"] == normal["source_hashes"]
            and external == normal.get("external_sources", {}), "Normal/trace source mismatch")
    payloads = {"libtraining_modpack.nro": pin["sha256"], **HOOKS}
    paths = {"libtraining_modpack.nro": folder / f"{mode}.nro",
             **{name: HERE / "macro-profile-17" / name for name in HOOKS}}
    require(all(sha(paths[name]) == digest for name, digest in payloads.items()), "Payload pin mismatch")
    return pin, payloads, paths


def write_receipt(stage, receipt):
    receipt["updated_at"] = time.time()
    temporary = stage / "receipt.next.json"
    with temporary.open("w") as stream:
        json.dump(receipt, stream, indent=2)
        stream.write("\n")
        stream.flush()
    temporary.replace(stage / "receipt.json")


def verify_protected(receipt):
    require({name: tree(path) for name, path in PROTECTED.items()} == receipt["protected"],
            "Protected keys/load changed; preserve and investigate")


def owned_profile():
    for path in (ROOT, PROFILE, *TREES.values(), *PROTECTED.values(), SETTINGS, PLUGINS):
        require(path.resolve() == path, f"Owned profile path redirects elsewhere: {path}")


def campaign_stage(token):
    match = re.fullmatch(r"([1-9][0-9]*)(?:-([a-z0-9][a-z0-9-]{0,23}))?", token)
    require(match is not None and int(match[1]) >= 23, "Expected REV or REV-short-campaign, revision23 or later")
    return int(match[1]), HERE / f"live-acceptance-{token}"


def deluxe_payload():
    source = DELUXE_FIXTURE / DELUXE_NAME
    manifest = json.loads((DELUXE_FIXTURE / "manifest.json").read_text())
    require(source.resolve() == source and manifest["nro"] == DELUXE_NAME
            and manifest["nro_sha256"] == DELUXE_SHA and manifest["nro_size"] == 2428928
            and source.stat().st_size == 2428928 and sha(source) == DELUXE_SHA,
            "Deluxe fixture pin mismatch")
    return source


def prepare(stage, revision, mode, cleanup, coexistence=None):
    owned_profile()
    pin, payloads, paths = frozen(revision, mode)
    require(coexistence in (None, "deluxe6c39"), "Unknown coexistence fixture")
    if coexistence:
        paths[DELUXE_NAME] = deluxe_payload()
        payloads[DELUXE_NAME] = DELUXE_SHA
    stopped()
    cleanup = deadline(cleanup)
    preflight = gate()
    require(not stage.exists(), "Existing receipt; restore or inspect instead of overwriting")
    require(inventory() == {"libarcropolis.nro": ARC}, "Baseline must be ARC-only")
    require(not (SETTINGS / "replay_capture.ini").exists(), "Unexpected existing capture INI")
    require(not (SETTINGS / "replay_captures").exists(), "Unexpected existing capture outputs")
    before = {name: tree(path) for name, path in TREES.items()}
    protected = {name: tree(path) for name, path in PROTECTED.items()}
    stage.mkdir()
    receipt = {"revision": revision, "campaign": stage.name, "mode": mode, "profile": str(PROFILE), "state": "backing_up",
               "created_at": time.time(), "cleanup_by": cleanup, "runtime_seconds": RUNTIME,
               "pin": pin, "payloads": payloads, "before": before, "protected": protected,
               "preflight": preflight, "runs": [], "restored_trees": [], "after": []}
    write_receipt(stage, receipt)
    for name, path in TREES.items():
        shutil.copytree(path, stage / "before" / name)
        require(tree(stage / "before" / name) == before[name], f"Backup mismatch: {name}")
    require({name: tree(path) for name, path in TREES.items()} == before, "Profile changed during backup")
    stopped()
    gate()
    require(time.time() + RUNTIME + 120 < cleanup, "Backup used the reserved runtime margin")
    receipt["state"] = "installing"
    write_receipt(stage, receipt)
    for name, source in paths.items():
        require(not (PLUGINS / name).exists(), f"Unexpected staged plugin: {name}")
        shutil.copy2(source, PLUGINS / name)
    config = SETTINGS / "training_modpack_menu.conf"
    menu = json.loads(config.read_text())
    menu["menu"]["macro_source"] = [0, 1]
    menu["menu"]["recording_crop"] = [0, 1]
    config.write_text(json.dumps(menu, indent=2) + "\n")
    if coexistence:
        deluxe_config = TREES["sdmc"] / "ultimate/ssbu_online_deluxe_rewrite/config.ini"
        deluxe_config.parent.mkdir(parents=True, exist_ok=True)
        deluxe_config.write_text(DELUXE_CONFIG)
        receipt["deluxe"] = {"fixture": coexistence, "sha256": DELUXE_SHA,
                             "config_sha256": sha(deluxe_config)}
    require(inventory() == {"libarcropolis.nro": ARC, **payloads}, "Installed plugin mismatch")
    verify_protected(receipt)
    receipt.update(state="installed", capture_ini_sha256=None)
    write_receipt(stage, receipt)
    return receipt


def normal_stop_after(phase):
    # Benchmark parent navigation can be delayed by model/tool turns. Enforce
    # its shorter target independently; historical acceptance keeps its budget.
    return 465 if re.fullmatch(r"live-r27-(?:bench|slow)-[a-z0-9-]{1,20}", phase) else NORMAL_STOP_AFTER


def start(stage, receipt, phase, cleanup):
    require(receipt["state"] == "installed", "Candidate is not installed")
    # This fixed clone root leaves 41 phase bytes within eden-run's 100-byte
    # Unix socket limit, also below control.run_path's 49-character limit.
    require(re.fullmatch(r"live-[a-z0-9-]{1,36}", phase),
            "Use a unique live-* phase of at most 41 characters")
    require(not (ROOT / "runs" / phase).exists() and not (ROOT / f"owned-{phase}.json").exists(),
            "Phase already exists")
    frozen(receipt["revision"], receipt["mode"])
    stopped()
    cleanup = deadline(cleanup)
    preflight = gate()
    require(inventory() == {"libarcropolis.nro": ARC, **receipt["payloads"]}, "Staged inventory changed")
    if receipt.get("deluxe"):
        require(sha(TREES["sdmc"] / "ultimate/ssbu_online_deluxe_rewrite/config.ini")
                == receipt["deluxe"]["config_sha256"], "Deluxe configuration changed")
    verify_protected(receipt)
    ini = SETTINGS / "replay_capture.ini"
    expected_ini = receipt.get("capture_ini_sha256")
    require((not ini.exists()) if expected_ini is None else (ini.is_file() and sha(ini) == expected_ini),
            "Capture INI changed; use stopped-only configure before a later run")
    run = {"phase": phase, "state": "launching", "at": time.time(), "cleanup_by": cleanup,
           "preflight": preflight}
    receipt["runs"].append(run)
    write_receipt(stage, receipt)
    # Reserve all logs before launching anything; a reused evidence name must
    # not leave an unguarded frontend behind.
    with (ROOT / f"launch-{phase}.log").open("xb") as log, \
            (ROOT / f"guard-{phase}.log").open("xb") as guard_log, \
            (ROOT / f"normal-stop-{phase}.log").open("xb") as stop_log:
        launcher = subprocess.Popen(PS + [f"& '{ROOT}/launch.ps1' -Phase '{phase}' -Runtime {RUNTIME}"],
            cwd=ROOT, stdin=subprocess.DEVNULL, stdout=log, stderr=log, start_new_session=True)
        run["launcher_pid"] = launcher.pid
        write_receipt(stage, receipt)
        try:
            guard = subprocess.Popen(PS + [f"& /usr/bin/python3 '{ROOT}/acceptance.py' guard '{phase}' {launcher.pid}"],
                cwd=ROOT, stdin=subprocess.DEVNULL, stdout=guard_log, stderr=guard_log, start_new_session=True)
            run["guard_pid"] = guard.pid
            write_receipt(stage, receipt)
            normal_guard = subprocess.Popen(PS + [f"& /usr/bin/python3 '{Path(__file__).resolve()}' normal-stop '{phase}' {launcher.pid}"],
                cwd=ROOT, stdin=subprocess.DEVNULL, stdout=stop_log, stderr=stop_log, start_new_session=True)
        except OSError:
            # Only our just-created run may be stopped. Preserve the partial
            # receipt so the parent can independently verify every owned PID.
            manifest = ROOT / "runs" / phase / "run.json"
            for _ in range(20):
                if manifest.exists() or launcher.poll() is not None:
                    break
                time.sleep(0.5)
            if manifest.exists():
                unit = json.loads(manifest.read_text())["unit"]
                require(re.fullmatch(r"eden-control-[a-f0-9]+(?:\.service)?", unit), "Unknown owned unit")
                subprocess.run(PS + [f"systemctl --user stop '{unit}'"], check=True, timeout=30)
            if launcher.poll() is None:
                launcher.terminate()
                launcher.wait(timeout=30)
            run["state"] = "guard_launch_failed"
            write_receipt(stage, receipt)
            raise
    run.update(normal_stop_pid=normal_guard.pid, normal_stop_after=normal_stop_after(phase), state="launched")
    with (ROOT / f"owned-{phase}.json").open("x") as stream:
        json.dump({**run, "runtime_seconds": RUNTIME,
                   "host_available_kib_before": preflight["available_kib"]}, stream, indent=2)
    receipt["cleanup_by"] = cleanup
    write_receipt(stage, receipt)
    return receipt


def normal_stop(phase, launcher):
    """Independent normal-shutdown deadline; the existing host guard stays separate."""
    require(re.fullmatch(r"live-[a-z0-9-]{1,36}", phase), "Invalid owned phase")
    manifest = ROOT / "runs" / phase / "run.json"
    bound = time.monotonic() + RUNTIME + 30
    stop_after = normal_stop_after(phase)
    print(json.dumps({"phase": phase, "pid": os.getpid(), "launcher_pid": launcher,
                      "normal_stop_after": stop_after}), flush=True)
    while time.monotonic() < bound:
        try:
            run = json.loads(manifest.read_text())
        except (FileNotFoundError, json.JSONDecodeError):
            run = None
        if run is not None:
            if run.get("state") == "stopped":
                return
            require(run.get("runtime_seconds") == RUNTIME, "Unexpected owned runtime bound")
            if time.time() >= run["started_at"] + stop_after:
                remaining = run["started_at"] + stop_after + 180 - time.time()
                require(remaining > 0, "Normal shutdown exhausted its 180s window")
                print(json.dumps({"normal_stop_requested_at": time.time(), "phase": phase}), flush=True)
                # Bound the existing CLI stop, leaving the independent 900s cap
                # and host-memory guard intact even when frontend shutdown fails.
                try:
                    result = subprocess.run(PS + [f"& /usr/bin/python3 '{ROOT}/acceptance.py' stop '{phase}'"],
                                            check=True, timeout=remaining, capture_output=True, text=True)
                except subprocess.CalledProcessError as error:
                    print(error.stdout or "", end="", flush=True)
                    print(error.stderr or "", end="", flush=True)
                    # A native naming dialog can overlap the deadline. Retain
                    # the rejection and retry after it closes, within the SAME
                    # absolute shutdown window; never reset the hard-cap margin.
                    if '"code": "modal_active"' not in (error.stderr or ""):
                        raise
                else:
                    print(result.stdout or "", end="", flush=True)
                    print(result.stderr or "", end="", flush=True)
                    return
        if not Path(f"/proc/{launcher}").exists():
            return
        time.sleep(1)
    raise RuntimeError("Normal-stop monitor exceeded its independent lifetime bound")


def restore(stage, receipt):
    stopped()
    # Also cover interrupted starts that never produced a run.json.
    for run in receipt["runs"]:
        for name in ("launcher_pid", "guard_pid", "normal_stop_pid"):
            pid = run.get(name)
            require(pid is None or not Path(f"/proc/{pid}").exists(), f"Owned {name} still present: {pid}")
    verify_protected(receipt)
    if receipt["state"] == "restored":
        require({name: tree(path) for name, path in TREES.items()} == receipt["before"],
                "Previously restored profile changed")
        return receipt
    if receipt["state"] == "backing_up":
        require({name: tree(path) for name, path in TREES.items()} == receipt["before"],
                "Incomplete backup and changed profile; manual preservation needed")
        receipt["state"] = "restored"
        write_receipt(stage, receipt)
        return receipt
    require(receipt["state"] in ("installing", "installed", "restoring", "restored"), "Unknown receipt state")
    for name in TREES:
        require(tree(stage / "before" / name) == receipt["before"][name], f"Backup drift: {name}")
    receipt["state"] = "restoring"
    write_receipt(stage, receipt)
    (stage / "after").mkdir(exist_ok=True)
    for name, path in TREES.items():
        if name in receipt["restored_trees"]:
            require(tree(path) == receipt["before"][name], f"Restored tree changed: {name}")
            continue
        if path.exists():
            destination = stage / "after" / f"{name}-{time.time_ns()}"
            receipt["after"].append({"tree": name, "path": str(destination), "manifest": tree(path)})
            write_receipt(stage, receipt)
            path.rename(destination)  # Preserve every after-state, including interrupted restores.
        shutil.copytree(stage / "before" / name, path)
        require(tree(path) == receipt["before"][name], f"Restore mismatch: {name}")
        receipt["restored_trees"].append(name)
        write_receipt(stage, receipt)
    verify_protected(receipt)
    require(inventory() == {"libarcropolis.nro": ARC}, "Original plugin restoration failed")
    receipt.update(state="restored", restored_at=time.time())
    write_receipt(stage, receipt)
    return receipt


def main():
    mode, token, *args = sys.argv[1:]
    if mode == "normal-stop":
        require(len(args) == 1, "normal-stop PHASE LAUNCHER_PID")
        normal_stop(token, int(args[0]))
        return
    revision, stage = campaign_stage(token)
    if mode == "prepare":
        require(len(args) in (2, 3) and args[0] in ("normal", "trace"),
                "prepare REV[-CAMPAIGN] normal|trace CLEANUP_BY_ISO [deluxe6c39]")
        receipt = prepare(stage, revision, *args)
    else:
        owned_profile()
        receipt = json.loads((stage / "receipt.json").read_text())
        require(receipt["profile"] == str(PROFILE) and receipt["revision"] == revision, "Wrong owned receipt")
        require(receipt.get("campaign", f"live-acceptance-{revision}") == stage.name, "Wrong campaign receipt")
        if mode == "configure":
            require(len(args) == 1 and args[0] in ("on", "off"), "configure REV on|off")
            stopped()
            require(receipt["state"] == "installed", "Candidate is not installed")
            ini = SETTINGS / "replay_capture.ini"
            require(not ini.is_symlink() and (not ini.exists() or ini.is_file()), "Invalid capture INI path")
            verify_protected(receipt)
            ini.write_text(f"[capture]\nenabled={args[0]}\n")
            receipt["capture_ini_sha256"] = sha(ini)
            write_receipt(stage, receipt)
        elif mode == "start":
            require(len(args) == 2, "start REV PHASE CLEANUP_BY_ISO")
            receipt = start(stage, receipt, *args)
        elif mode == "restore":
            require(not args, "restore REV")
            receipt = restore(stage, receipt)
        else:
            raise RuntimeError("Expected prepare, configure, start or restore")
    print(json.dumps({key: receipt.get(key) for key in ("revision", "mode", "state", "profile", "runs")}, indent=2))


if __name__ == "__main__":
    main()
