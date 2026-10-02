#!/usr/bin/env python3
"""Prints the CHANGELOG.md entry for one version.

Markdown goes to --out (the GitHub release body); --plain-out gets a text version for
latest.json, which the Updates page renders unformatted. Exits 1 when the version has no
entry, so the workflow can fall back to commit subjects.
"""

import argparse
import re
import sys
from pathlib import Path


def section(text: str, version: str) -> str:
    """The lines under `## [version] - date`, up to the next release heading."""
    out: list[str] = []
    inside = False
    for line in text.splitlines():
        if line.startswith("## "):
            if inside:
                break
            inside = line.startswith(f"## [{version}]") or line.startswith(f"## {version} ")
            continue
        if inside and not line.startswith("["):  # skip link definitions
            out.append(line)
    return "\n".join(out).strip("\n")


def plain(markdown: str) -> str:
    """Readable without a Markdown renderer: no commit links, headings or emphasis."""
    text = re.sub(r" \(\[`[^`]+`\]\([^)]+\)(?:, \[`[^`]+`\]\([^)]+\))*\)", "", markdown)
    text = re.sub(r"\[(#\d+)\]\([^)]+\)", r"\1", text)
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    lines = []
    for line in text.splitlines():
        line = re.sub(r"^#+ ", "", line)
        line = re.sub(r"^_(.*)_$", r"\1", line)
        line = line.replace("**", "").replace("`", "")
        lines.append(line)
    return "\n".join(lines).strip("\n")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("version")
    parser.add_argument("--changelog", default="CHANGELOG.md")
    parser.add_argument("--out")
    parser.add_argument("--plain-out")
    args = parser.parse_args()

    path = Path(args.changelog)
    if not path.is_file():
        print(f"{path} not found", file=sys.stderr)
        return 1

    entry = section(path.read_text(encoding="utf-8"), args.version)
    if not entry:
        print(f"no entry for {args.version} in {path}", file=sys.stderr)
        return 1

    if args.out:
        Path(args.out).write_text(entry + "\n", encoding="utf-8")
    if args.plain_out:
        Path(args.plain_out).write_text(plain(entry) + "\n", encoding="utf-8")
    if not args.out and not args.plain_out:
        print(entry)
    return 0


if __name__ == "__main__":
    sys.exit(main())
