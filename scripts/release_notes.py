#!/usr/bin/env python3
"""Extract one release section from CHANGELOG.md."""

import argparse
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()

    text = (ROOT / "CHANGELOG.md").read_text(encoding="utf-8")
    marker = f"## [{args.version}]"
    start = text.find(marker)
    if start < 0:
        raise SystemExit(f"missing changelog section for {args.version}")
    end = text.find("\n## [", start + len(marker))
    section = text[start:end if end >= 0 else None].strip()
    body = section.split("\n", 1)[1].strip() if "\n" in section else section
    args.output.write_text(body + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
