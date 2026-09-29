#!/usr/bin/env python3
"""Reject edits or deletions to migrations already present at a base commit."""

import argparse
import subprocess
from pathlib import Path


def git(*args: str) -> bytes:
    return subprocess.check_output(["git", *args])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", help="trusted main-branch commit to compare against")
    args = parser.parse_args()

    # GitHub uses an all-zero before SHA for the first push of a new branch.
    # There are no previously applied migrations to compare in a new repository.
    if args.base == "0" * 40:
        print("Initial push: no previous migrations to compare")
        return

    existing = git("ls-tree", "-r", "--name-only", args.base, "migrations/")
    changed = []
    for name in existing.decode().splitlines():
        path = Path(name)
        if path.suffix != ".sql":
            continue
        if not path.is_file() or path.read_bytes() != git("show", f"{args.base}:{name}"):
            changed.append(name)

    if changed:
        parser.exit(1, "Applied migrations must remain unchanged; add a new file instead:\n"
                    + "\n".join(f"  {name}" for name in changed) + "\n")
    print("Existing migrations are unchanged")


if __name__ == "__main__":
    main()
