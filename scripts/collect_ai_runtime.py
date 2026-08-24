#!/usr/bin/env python3
"""Assemble an experimental PdfMerger package with runtime AI backends."""

import argparse
from pathlib import Path
import shutil

CORE_LIBRARIES = ("ggml-base.dll", "ggml.dll", "llama-common.dll", "llama.dll")


def copy_required(source: Path, destination: Path, names: tuple[str, ...]) -> None:
    for name in names:
        path = source / name
        if not path.is_file():
            raise SystemExit(f"required runtime file does not exist: {path}")
        shutil.copy2(path, destination / name)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True, type=Path)
    parser.add_argument("--runtime-dir", required=True, type=Path)
    parser.add_argument("--output-dir", required=True, type=Path)
    parser.add_argument(
        "--accelerator-runtime",
        action="append",
        default=[],
        type=Path,
        help="Optional accelerator dependency to place beside the executable (repeatable)",
    )
    args = parser.parse_args()

    if not args.binary.is_file():
        raise SystemExit(f"application binary does not exist: {args.binary}")

    backend_source = args.runtime_dir / "backends"
    backends = sorted(backend_source.glob("*.dll"))
    if not backends:
        raise SystemExit(f"no runtime backends found in: {backend_source}")

    args.output_dir.mkdir(parents=True, exist_ok=True)
    backend_output = args.output_dir / "backends"
    backend_output.mkdir(exist_ok=True)
    shutil.copy2(args.binary, args.output_dir / "PdfMerger.exe")
    copy_required(args.runtime_dir / "bin", args.output_dir, CORE_LIBRARIES)

    for backend in backends:
        shutil.copy2(backend, backend_output / backend.name)
    for dependency in args.accelerator_runtime:
        if not dependency.is_file():
            raise SystemExit(f"accelerator runtime file does not exist: {dependency}")
        shutil.copy2(dependency, args.output_dir / dependency.name)

    print(f"assembled {args.output_dir} with {len(backends)} runtime backend(s)")


if __name__ == "__main__":
    main()
