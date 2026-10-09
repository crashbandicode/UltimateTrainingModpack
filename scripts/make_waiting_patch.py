"""Build a copy/literal recipe from private inputs; embed no complete game archive."""

import argparse
import hashlib
import json
from pathlib import Path
import struct

CHUNK = 32
STRIDE = 16


def fingerprint(data):
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


def match_length(source, offset, target, pos):
    limit = min(len(source) - offset, len(target) - pos)
    n = CHUNK
    while n + 4096 <= limit and source[offset+n:offset+n+4096] == target[pos+n:pos+n+4096]:
        n += 4096
    lo, hi = n, min(n + 4096, limit)
    while lo < hi:
        mid = (lo + hi + 1) // 2
        if source[offset+n:offset+mid] == target[pos+n:pos+mid]:
            lo = mid
        else:
            hi = mid - 1
    return lo


def make(stock, donor, target):
    sources = (stock, donor)
    index = {}
    for tag, source in enumerate(sources):
        for offset in range(0, len(source) - CHUNK + 1, STRIDE):
            choices = index.setdefault(source[offset:offset+CHUNK], [])
            # Keep early and recent positions per source for common headers/zero runs.
            same = [choice for choice in choices if choice[0] == tag]
            if len(same) < 4:
                choices.append((tag, offset))
            else:
                choices[choices.index(same[-1])] = (tag, offset)

    patch = bytearray(b"TMW1")
    for data in (*sources, target):
        patch += struct.pack("<IQ", len(data), fingerprint(data))
    literal, decoded = bytearray(), bytearray()
    pos, ops = 0, 0
    counts = [0, 0, 0]

    def flush():
        nonlocal ops
        if literal:
            patch.extend(struct.pack("<BI", 2, len(literal)))
            patch.extend(literal)
            decoded.extend(literal)
            counts[2] += len(literal)
            literal.clear()
            ops += 1

    while pos < len(target):
        best = (0, 0, 0)
        for tag, offset in index.get(target[pos:pos+CHUNK], []):
            length = match_length(sources[tag], offset, target, pos)
            if length > best[0]:
                best = (length, tag, offset)
        length, tag, offset = best
        if length >= CHUNK:
            flush()
            patch += struct.pack("<BII", tag, length, offset)
            decoded += sources[tag][offset:offset+length]
            counts[tag] += length
            pos += length
            ops += 1
        else:
            literal.append(target[pos])
            pos += 1
    flush()
    assert decoded == target
    return patch, {
        "ops": ops, "stock_copied": counts[0], "donor_copied": counts[1],
        "literal_bytes": counts[2], "patch_bytes": len(patch), "target_bytes": len(target),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("stock", "donor", "target", "output"):
        parser.add_argument(name, type=Path)
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()
    data = [path.read_bytes() for path in (args.stock, args.donor, args.target)]
    patch, report = make(*data)
    args.output.write_bytes(patch)
    report["sha256"] = {
        key: hashlib.sha256(value).hexdigest()
        for key, value in zip(("stock", "donor", "target", "patch"), [*data, patch])
    }
    if args.report:
        args.report.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
