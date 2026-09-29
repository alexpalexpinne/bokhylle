#!/usr/bin/env python3
"""Copy the current source tree into a new directory without Git history or local data."""

import argparse
from pathlib import Path
import shutil
import subprocess


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    source = Path(__file__).resolve().parent.parent
    destination = args.destination.resolve()
    if destination.exists() or destination.is_relative_to(source):
        parser.error("destination must be a new directory outside this repository")
    names = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=source,
    ).decode().split("\0")
    files = []
    private_roots = {".git", "target", "data", "config", "library", "downloads"}
    for name in sorted(set(names) - {""}):
        relative = Path(name)
        path = source / relative
        if not path.exists():
            continue
        if (relative.parts[0] in private_roots
                or relative.parts[:2] in {("demo", "data"), ("frontend", "node_modules"),
                                         ("frontend", "dist")}
                or relative.name == ".env"
                or (relative.name.startswith(".env.") and relative.name != ".env.example")
                or relative.suffix in {".db", ".sqlite", ".sqlite3", ".log"}):
            parser.error(f"private or generated file is visible to Git: {relative}")
        if path.is_symlink() or not path.is_file():
            parser.error(f"export requires regular source files: {relative}")
        files.append(relative)

    destination.mkdir(parents=True)
    for relative in files:
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source / relative, target)
    print(f"Exported {len(files)} source files to {destination}; no Git history copied")


if __name__ == "__main__":
    main()
