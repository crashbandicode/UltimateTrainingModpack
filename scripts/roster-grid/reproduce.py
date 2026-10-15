"""Rebuild both roster archives and the waiting copy/literal patch from private baselines."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import sys

sys.dont_write_bytecode = True

HERE = Path(__file__).resolve().parent


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("builder", "original_donor", "original_waiting", "stock", "output_dir"):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    baselines = [args.original_donor, args.original_waiting, args.stock]
    expected = ["8dd9523eac8716199fbea50f9a5baa9d8146568b41c8a0ae9bf7f08da3d06f96",
                "3504ddb64e0299cd72c962c6657ecca339a23a64dca0e76761cc58c28cc7a668",
                "7dcc45360bd84dadffcfe8b47771f5d6b40e44468c6a54cbd3dbb6b4142eea64"]
    for path, digest in zip(baselines, expected):
        assert sha(path.read_bytes()) == digest, f"Wrong private baseline: {path}"
    icons = json.loads((HERE / "assets/sources.json").read_text())["icons"]
    assert len(icons) == 89
    for icon in icons:
        assert sha((HERE / "assets" / icon["filename"]).read_bytes()) == icon["sha256"]
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for source, name in zip(baselines[:2], ("layout", "waiting")):
        subprocess.run([str(args.builder.resolve()), str(source.resolve()), str(out / f"{name}-roster.arc"),
                        str(HERE / "assets"), str(out / f"roster-{name}-report.json")], check=True)
    spec = importlib.util.spec_from_file_location("waiting_patch", HERE.parent / "make_waiting_patch.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    sources = (args.stock.read_bytes(), (out / "layout-roster.arc").read_bytes())
    target = (out / "waiting-roster.arc").read_bytes()
    patch, report = module.make(*sources, target)
    # Independently exercise the runtime recipe format, including donor fingerprint.
    assert patch[:4] == b"TMW1"
    for i, data in enumerate((*sources, target)):
        assert struct.unpack_from("<IQ", patch, 4 + i * 12) == (len(data), module.fingerprint(data))
    pos, decoded = 40, bytearray()
    while pos < len(patch):
        tag, count = struct.unpack_from("<BI", patch, pos)
        pos += 5
        if tag == 2:
            decoded += patch[pos:pos + count]
            pos += count
        else:
            assert tag in (0, 1)
            offset, = struct.unpack_from("<I", patch, pos)
            pos += 4
            decoded += sources[tag][offset:offset + count]
    assert decoded == target
    report["sha256"] = {name: sha(data) for name, data in zip(("stock", "donor", "target", "patch"), (*sources, target, patch))}
    (out / "waiting_ui.patch").write_bytes(patch)
    (out / "waiting_patch_manifest.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
