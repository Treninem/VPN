#!/usr/bin/env python3
"""Wrap the approved AMRI PNG in a Windows ICO container without altering image pixels."""

from __future__ import annotations

from pathlib import Path
import struct

ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "assets" / "brand" / "amri-icon.png"
OUTPUT = ROOT / "dist" / "windows" / "amri-vpn.ico"
PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"


def main() -> int:
    png = SOURCE.read_bytes()
    if not png.startswith(PNG_SIGNATURE) or len(png) < 24 or png[12:16] != b"IHDR":
        raise SystemExit("canonical AMRI icon is not a valid PNG")

    width, height = struct.unpack(">II", png[16:24])
    if not 1 <= width <= 256 or not 1 <= height <= 256:
        raise SystemExit("Windows ICO wrapper requires PNG dimensions from 1 to 256 pixels")

    OUTPUT.parent.mkdir(parents=True, exist_ok=True)
    header = struct.pack("<HHH", 0, 1, 1)
    directory = struct.pack(
        "<BBBBHHII",
        0 if width == 256 else width,
        0 if height == 256 else height,
        0,
        0,
        1,
        32,
        len(png),
        6 + 16,
    )
    OUTPUT.write_bytes(header + directory + png)
    print(f"wrote {OUTPUT.relative_to(ROOT)} from approved {SOURCE.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
