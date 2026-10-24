"""Package decoded stock BNTX pixels as PNG; requires a verified stage catalog."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from PIL import Image

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for argument in ("helper", "source", "catalog", "output"):
        parser.add_argument(argument, type=Path)
    parser.add_argument("--group", type=int, choices=(1, 2, 3), default=1)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    rows = json.loads(args.catalog.read_text())
    icons = []
    for row in rows:
        basename = f"stage_{args.group}_{row['name_id'].lower()}.bntx"
        bntx = args.source / basename
        with tempfile.NamedTemporaryFile(prefix="stage-portrait-", suffix=".rgba") as raw:
            result = subprocess.run([str(args.helper.resolve()), str(bntx.resolve()), raw.name],
                                    text=True, capture_output=True, check=True)
            width, height, texture = result.stdout.strip().split()
            pixels = Path(raw.name).read_bytes()
        width, height = int(width), int(height)
        assert len(pixels) == width * height * 4
        filename = f"stage-{row['stage_id']:03}.png"
        target = args.output / filename
        Image.frombytes("RGBA", (width, height), pixels).save(target)
        prefix = "replace_patch" if row["is_patch"] else "replace"
        icons.append({"id": row["stage_id"], "name": row["name"].strip(), "filename": filename,
                      "sha256": hashlib.sha256(target.read_bytes()).hexdigest(),
                      "source_sha256": hashlib.sha256(bntx.read_bytes()).hexdigest(),
                      "source_path": f"ui/{prefix}/stage/stage_{args.group}/{basename}",
                      "source_texture": texture, "width": width, "height": height,
                      "rgba_sha256": hashlib.sha256(pixels).hexdigest()})
    by_id = {icon["id"]: icon for icon in icons}
    # Stage_0 contains series logos; BF and FD must have distinct stage artwork.
    if 0 in by_id and 3 in by_id:
        assert by_id[0]["rgba_sha256"] != by_id[3]["rgba_sha256"], "identical BF/FD imagery"
    (args.output / "sources.json").write_text(json.dumps({"icons": icons}, indent=2) + "\n")
    print(json.dumps({"decoded": len(icons), "distinct_images": len({i['rgba_sha256'] for i in icons}),
                      "dimensions": sorted({(i['width'], i['height']) for i in icons})}))

if __name__ == "__main__":
    main()
