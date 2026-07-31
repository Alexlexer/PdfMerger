#!/usr/bin/env python3
"""Validate that release metadata and required assets stay in sync."""

import argparse
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def fail(message: str) -> None:
    raise SystemExit(f"release validation failed: {message}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", help="Release tag to compare with the package version")
    args = parser.parse_args()

    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    packager = tomllib.loads((ROOT / "Packager.toml").read_text(encoding="utf-8"))
    version = cargo["package"]["version"]

    if packager.get("version") != version:
        fail(f"Packager.toml version {packager.get('version')!r} does not match Cargo.toml {version!r}")
    if args.tag and args.tag != f"v{version}":
        fail(f"tag {args.tag!r} does not match v{version}")

    expected = [
        ROOT / "assets" / "icon.svg",
        ROOT / "assets" / "icons" / "icon-32.png",
        ROOT / "assets" / "icons" / "icon-128.png",
        ROOT / "assets" / "icons" / "icon-256.png",
        ROOT / "assets" / "icons" / "icon-512.png",
        ROOT / "assets" / "icons" / "icon.ico",
        ROOT / "assets" / "icons" / "icon.icns",
        ROOT / "LICENSE",
        ROOT / "THIRD_PARTY_LICENSES.html",
    ]
    missing = [path.relative_to(ROOT).as_posix() for path in expected if not path.is_file()]
    if missing:
        fail("missing required files: " + ", ".join(missing))

    changelog = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    if f"## [{version}]" not in changelog:
        fail(f"CHANGELOG.md has no {version} release section")

    print(f"release metadata is consistent for v{version}")


if __name__ == "__main__":
    main()
