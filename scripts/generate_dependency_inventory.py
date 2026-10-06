#!/usr/bin/env python3
"""Generate a deterministic transitive Rust dependency/license inventory for AMRI VPN."""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT_DIR = ROOT / "dist" / "compliance"


def cargo_metadata() -> dict[str, Any]:
    completed = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


def lock_sha256() -> str:
    lock_path = ROOT / "Cargo.lock"
    return hashlib.sha256(lock_path.read_bytes()).hexdigest()


def clean_license_file(value: str | None) -> str | None:
    if not value:
        return None
    return Path(value).name


def collect_packages(metadata: dict[str, Any]) -> list[dict[str, Any]]:
    workspace_members = set(metadata.get("workspace_members", []))
    resolve = metadata.get("resolve") or {}
    resolved_ids = {node["id"] for node in resolve.get("nodes", [])}
    packages_by_id = {package["id"]: package for package in metadata.get("packages", [])}

    packages: list[dict[str, Any]] = []
    for package_id in sorted(resolved_ids):
        if package_id in workspace_members:
            continue
        package = packages_by_id.get(package_id)
        if package is None:
            raise RuntimeError(f"resolved package is missing from cargo metadata: {package_id}")

        packages.append(
            {
                "name": package["name"],
                "version": package["version"],
                "license": package.get("license"),
                "license_file": clean_license_file(package.get("license_file")),
                "repository": package.get("repository"),
                "source": package.get("source"),
            }
        )

    packages.sort(key=lambda item: (item["name"].lower(), item["version"], item["source"] or ""))
    return packages


def validate(packages: list[dict[str, Any]]) -> list[str]:
    errors: list[str] = []
    for package in packages:
        if not package.get("license") and not package.get("license_file"):
            errors.append(
                f'{package["name"]} {package["version"]}: missing Cargo license/license_file metadata'
            )
    return errors


def markdown(packages: list[dict[str, Any]], cargo_lock_sha256: str) -> str:
    lines = [
        "# AMRI VPN transitive Rust dependency inventory",
        "",
        "Generated from the locked Cargo dependency graph. This inventory is a release-compliance",
        "input, not legal advice and not a substitute for reviewing each applicable license.",
        "",
        f"- Cargo.lock SHA-256: `{cargo_lock_sha256}`",
        f"- Third-party resolved packages: **{len(packages)}**",
        "",
        "| Package | Version | License | License file | Repository | Source |",
        "| --- | --- | --- | --- | --- | --- |",
    ]

    def cell(value: Any) -> str:
        if value is None or value == "":
            return "—"
        return str(value).replace("|", "\\|").replace("\n", " ")

    for package in packages:
        lines.append(
            "| "
            + " | ".join(
                [
                    cell(package["name"]),
                    cell(package["version"]),
                    cell(package["license"]),
                    cell(package["license_file"]),
                    cell(package["repository"]),
                    cell(package["source"]),
                ]
            )
            + " |"
        )

    lines.append("")
    return "\n".join(lines)


def write_inventory(output_dir: Path) -> tuple[Path, Path, list[str]]:
    metadata = cargo_metadata()
    packages = collect_packages(metadata)
    errors = validate(packages)
    lock_hash = lock_sha256()

    output_dir.mkdir(parents=True, exist_ok=True)
    json_path = output_dir / "AMRI-VPN-Rust-Dependency-Inventory.json"
    md_path = output_dir / "AMRI-VPN-Rust-Dependency-Inventory.md"

    payload = {
        "schema_version": 1,
        "cargo_lock_sha256": lock_hash,
        "dependency_count": len(packages),
        "packages": packages,
    }
    json_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    md_path.write_text(markdown(packages, lock_hash), encoding="utf-8")
    return json_path, md_path, errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--output-dir",
        type=Path,
        default=DEFAULT_OUTPUT_DIR,
        help="directory for deterministic JSON and Markdown inventory files",
    )
    args = parser.parse_args()

    try:
        json_path, md_path, errors = write_inventory(args.output_dir)
    except (OSError, subprocess.CalledProcessError, json.JSONDecodeError, RuntimeError) as error:
        print(f"dependency inventory generation failed: {error}", file=sys.stderr)
        return 1

    print(f"generated {json_path}")
    print(f"generated {md_path}")

    if errors:
        print("dependency license metadata validation failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 2

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
