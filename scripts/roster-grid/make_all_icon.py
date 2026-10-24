"""Deterministically draw the All tile's Smash Ball symbol; no external assets."""
from pathlib import Path
import hashlib
import struct
import zlib


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def main():
    size = 96
    scanlines = bytearray()
    for y in range(size):
        scanlines.append(0)
        for x in range(size):
            covered = sum(
                (x + (sx + 0.5) / 4 - 48) ** 2 + (y + (sy + 0.5) / 4 - 48) ** 2 <= 43 ** 2
                and not (30 <= x + (sx + 0.5) / 4 < 38 or 57 <= y + (sy + 0.5) / 4 < 65)
                for sy in range(4) for sx in range(4)
            )
            scanlines.extend((255, 255, 255, round(covered * 255 / 16)))
    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(scanlines, 9)) + chunk(b"IEND", b""))
    path = Path(__file__).resolve().parent / "assets" / "all-smash-ball.png"
    path.write_bytes(png)
    print(f"{len(png)} bytes sha256={hashlib.sha256(png).hexdigest()}")


if __name__ == "__main__":
    main()
