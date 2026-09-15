#!/usr/bin/env python3
"""Fail CI when AMRI UI artwork drifts away from assets/brand.

This guard exists because platform-specific copies previously made it possible for
an app build to use artwork different from the reviewed canonical file.
"""

from __future__ import annotations

import subprocess
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "assets" / "brand"
ASSET_LOCK = BRAND / "asset-blobs.lock"

CANONICAL_GRAPHICS = {
    "amri-icon.png",
    "background-desktop.svg",
    "background-mobile.svg",
    "vpn-power-on.svg",
    "vpn-power-off.svg",
    "settings-button.svg",
    "edit-button.svg",
    "language-button.svg",
    "add-button.svg",
    "delete-button.svg",
    "back-button.svg",
    "close-button.svg",
    "refresh-button.svg",
    "copy-button.svg",
    "info-button.svg",
    "more-button.svg",
}

GRAPHIC_SUFFIXES = {".png", ".svg", ".webp", ".jpg", ".jpeg", ".ico"}

WINDOWS_USED = {
    "amri-icon.png",
    "background-desktop.svg",
    "vpn-power-on.svg",
    "vpn-power-off.svg",
    "settings-button.svg",
    "language-button.svg",
    "add-button.svg",
    "edit-button.svg",
    "delete-button.svg",
    "copy-button.svg",
    "info-button.svg",
    "more-button.svg",
    "close-button.svg",
    "back-button.svg",
}

ANDROID_CANONICAL_INPUTS = {
    "amri-icon.png",
    "background-mobile.svg",
    "vpn-power-on.svg",
    "vpn-power-off.svg",
    "settings-button.svg",
    "language-button.svg",
}


def fail(messages: list[str]) -> None:
    if not messages:
        return
    print("AMRI visual asset integrity check failed:")
    for message in messages:
        print(f"  - {message}")
    raise SystemExit(1)


def tracked_files() -> list[PurePosixPath]:
    output = subprocess.check_output(
        ["git", "ls-files", "-z"], cwd=ROOT, text=False
    )
    return [
        PurePosixPath(item.decode("utf-8"))
        for item in output.split(b"\0")
        if item
    ]


def read(relative: str) -> str:
    return (ROOT / relative).read_text(encoding="utf-8")


def locked_blobs(errors: list[str]) -> dict[str, str]:
    if not ASSET_LOCK.is_file():
        errors.append("missing assets/brand/asset-blobs.lock")
        return {}

    entries: dict[str, str] = {}
    for line_number, raw_line in enumerate(
        ASSET_LOCK.read_text(encoding="utf-8").splitlines(), start=1
    ):
        line = raw_line.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(maxsplit=1)
        if len(parts) != 2:
            errors.append(f"malformed asset lock line {line_number}")
            continue
        blob, name = parts
        if name in entries:
            errors.append(f"duplicate asset lock entry: {name}")
            continue
        entries[name] = blob

    locked_names = set(entries)
    if locked_names != CANONICAL_GRAPHICS:
        missing = sorted(CANONICAL_GRAPHICS - locked_names)
        extra = sorted(locked_names - CANONICAL_GRAPHICS)
        if missing:
            errors.append(f"asset lock is missing entries: {', '.join(missing)}")
        if extra:
            errors.append(f"asset lock contains unknown entries: {', '.join(extra)}")
    return entries


def verify_locked_bytes(errors: list[str]) -> None:
    entries = locked_blobs(errors)
    for name, expected_blob in sorted(entries.items()):
        asset = BRAND / name
        if not asset.is_file():
            continue
        actual_blob = subprocess.check_output(
            ["git", "hash-object", str(asset)], cwd=ROOT, text=True
        ).strip()
        if actual_blob != expected_blob:
            errors.append(
                f"canonical asset bytes changed without review: {name} "
                f"(locked {expected_blob}, actual {actual_blob})"
            )


def main() -> None:
    errors: list[str] = []
    tracked = tracked_files()

    for name in sorted(CANONICAL_GRAPHICS):
        path = BRAND / name
        if not path.is_file():
            errors.append(f"missing canonical asset: assets/brand/{name}")

    verify_locked_bytes(errors)

    # A canonical basename must never be maintained somewhere else in the repo.
    for path in tracked:
        if path.name in CANONICAL_GRAPHICS and path.parent != PurePosixPath("assets/brand"):
            errors.append(
                f"duplicate canonical asset basename outside assets/brand: {path}"
            )

    # Platform source trees must not grow independent raster/SVG artwork copies.
    for path in tracked:
        if path.suffix.lower() not in GRAPHIC_SUFFIXES:
            continue
        if path.parts[:2] == ("assets", "brand"):
            continue
        if path.parts[:2] == ("apps", "windows"):
            errors.append(f"tracked Windows graphic bypasses assets/brand: {path}")
        if path.parts[:4] == ("apps", "android", "app", "src"):
            errors.append(f"tracked Android graphic bypasses build-time canonical copy: {path}")

    windows = read("apps/windows/src/main.rs")
    for name in sorted(WINDOWS_USED):
        expected = f'../../../assets/brand/{name}'
        if expected not in windows:
            errors.append(f"Windows no longer references canonical {name}")
    if "../../../assets/brand/refresh-button.svg" in windows:
        errors.append(
            "Windows exposes refresh-button.svg before a real subscription/source refresh backend exists"
        )

    gradle = read("apps/android/app/build.gradle.kts")
    for name in sorted(ANDROID_CANONICAL_INPUTS):
        if f'amriBrandDir.file("{name}")' not in gradle:
            errors.append(f"Android build no longer sources canonical {name}")

    activity = read("apps/android/app/src/main/java/ru/amri/vpn/MainActivity.kt")
    android_runtime_refs = {
        "R.raw.amri_background_mobile",
        "R.raw.amri_vpn_power_on",
        "R.raw.amri_vpn_power_off",
        "R.raw.amri_settings_button",
        "R.raw.amri_language_button",
    }
    for reference in sorted(android_runtime_refs):
        if reference not in activity:
            errors.append(f"Android UI no longer uses generated canonical resource {reference}")
    if "ImageView.ScaleType.CENTER_CROP" not in activity:
        errors.append("Android mobile background is no longer rendered with cover-style CENTER_CROP")

    manifest = read("apps/android/app/src/main/AndroidManifest.xml")
    for attribute in (
        'android:icon="@drawable/amri_app_icon"',
        'android:roundIcon="@drawable/amri_app_icon"',
    ):
        if attribute not in manifest:
            errors.append(f"Android launcher icon drifted from generated canonical icon: {attribute}")

    fail(errors)
    print(
        "AMRI visual assets OK: exact approved bytes, canonical single-source artwork, "
        "platform references, and duplicate guards verified."
    )


if __name__ == "__main__":
    main()
