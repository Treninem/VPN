#!/usr/bin/env python3
"""Build a developer-only AMRI visual source bundle.

The resulting ZIP is for the project owner/developers. It is never embedded in the Windows or
Android application artifacts.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = ROOT / "dist" / "AMRI-VPN-Visual-Source.zip"

FILES = [
    "design/amri-ui-theme.json",
    "tools/amri-ui-studio/index.html",
    "tools/amri-ui-studio/README.md",
    "scripts/generate_ui_theme.py",
    "scripts/verify_visual_assets.py",
    "docs/VISUAL_EDITING_GUIDE_RU.md",
    "apps/windows/src/main.rs",
    "apps/windows/src/theme.rs",
    "apps/windows/src/theme_generated.rs",
    "apps/android/app/src/main/java/ru/amri/vpn/MainActivity.kt",
    "apps/android/app/src/main/java/ru/amri/vpn/AmriTheme.kt",
    "apps/android/app/src/main/java/ru/amri/vpn/GeneratedAmriTheme.kt",
    "assets/brand/asset-blobs.lock",
]

BRAND_DIR = ROOT / "assets" / "brand"


def collect() -> list[Path]:
    paths = [ROOT / relative for relative in FILES]
    paths.extend(sorted(path for path in BRAND_DIR.rglob("*") if path.is_file()))
    missing = [path for path in paths if not path.is_file()]
    if missing:
        relative = ", ".join(str(path.relative_to(ROOT)) for path in missing)
        raise FileNotFoundError(f"visual source inputs are missing: {relative}")
    return paths


def build(output: Path) -> None:
    output.parent.mkdir(parents=True, exist_ok=True)
    paths = collect()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for path in paths:
            archive.write(path, path.relative_to(ROOT).as_posix())

    with zipfile.ZipFile(output, "r") as archive:
        names = set(archive.namelist())
        expected = {path.relative_to(ROOT).as_posix() for path in paths}
        if names != expected:
            raise RuntimeError("visual source ZIP verification failed")
        if archive.testzip() is not None:
            raise RuntimeError("visual source ZIP contains a corrupt member")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    args = parser.parse_args()
    build(args.output.resolve())
    print(args.output.resolve())
