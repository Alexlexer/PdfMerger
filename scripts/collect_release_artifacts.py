#!/usr/bin/env python3
"""Normalize native packages and create portable release archives."""

import argparse
from pathlib import Path
import shutil
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
DOCS = ("README.md", "LICENSE", "THIRD_PARTY_LICENSES.html")


def compound_suffix(path: Path) -> str:
    name = path.name.lower()
    for suffix in (".tar.gz", ".appimage", ".dmg", ".exe", ".msi", ".deb"):
        if name.endswith(suffix):
            return path.name[-len(suffix):]
    return path.suffix


def add_portable_archive(output: Path, binary: Path, target: str, version: str) -> None:
    bundle = f"PdfMerger-{version}-{target}"
    if target.startswith("windows-"):
        archive = output / f"{bundle}-portable.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as zipped:
            zipped.write(binary, f"{bundle}/PdfMerger.exe")
            for document in DOCS:
                zipped.write(ROOT / document, f"{bundle}/{document}")
    elif target.startswith("linux-"):
        archive = output / f"{bundle}-portable.tar.gz"
        with tarfile.open(archive, "w:gz", compresslevel=9) as tarred:
            tarred.add(binary, f"{bundle}/pdf-merger")
            for document in DOCS:
                tarred.add(ROOT / document, f"{bundle}/{document}")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--target", required=True)
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--package-dir", default=ROOT / "dist", type=Path)
    parser.add_argument("--output-dir", default=ROOT / "release-artifacts", type=Path)
    args = parser.parse_args()

    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    if not args.binary.is_file():
        raise SystemExit(f"release binary does not exist: {args.binary}")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    package_files = [path for path in args.package_dir.iterdir() if path.is_file()]
    if not package_files:
        raise SystemExit(f"no native package was produced in {args.package_dir}")

    for package in package_files:
        suffix = compound_suffix(package)
        destination = args.output_dir / f"PdfMerger-{version}-{args.target}{suffix}"
        shutil.copy2(package, destination)

    add_portable_archive(args.output_dir, args.binary, args.target, version)
    produced = sorted(path.name for path in args.output_dir.iterdir() if path.is_file())
    print("\n".join(produced))


if __name__ == "__main__":
    main()
