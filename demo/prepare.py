#!/usr/bin/env python3
"""Prepare isolated, checksum-verified public-domain books for the demo."""

import argparse
import hashlib
import io
import os
from pathlib import Path
import re
import shutil
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent
DATA = ROOT / "data"
CONFIG = DATA / "config"
LIBRARY = DATA / "library"
MARKER = CONFIG / ".bokhylle-demo"
# The illustrated Alice edition is 18.4 MB; cap downloads just above it.
MAX_EPUB_BYTES = 20 * 1024 * 1024


def verified(data: bytes, expected: str) -> None:
    if len(data) > MAX_EPUB_BYTES or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("EPUB size or checksum did not match the pinned edition")
    with zipfile.ZipFile(io.BytesIO(data)) as archive:
        if archive.read("mimetype") != b"application/epub+zip":
            raise ValueError("download is not an EPUB")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--reset", action="store_true", help="delete visitor state after stopping the demo")
    mode.add_argument("--verify", action="store_true", help="check the marker and every downloaded sample without changing files")
    args = parser.parse_args()
    if any(path.is_symlink() for path in (DATA, CONFIG, LIBRARY)):
        raise SystemExit("refusing symlinked demo storage")
    if args.reset or args.verify:
        if not MARKER.is_file() or MARKER.read_text() != "bokhylle-demo-v1\n":
            raise SystemExit("demo marker missing or invalid")
    if not args.reset and not args.verify and (CONFIG / "bokhylle.db").exists() and not MARKER.exists():
        raise SystemExit("refusing to mark an existing database as a demo")

    if args.verify:
        if not LIBRARY.is_dir():
            raise SystemExit("demo library is missing")
    else:
        CONFIG.mkdir(parents=True, exist_ok=True)
        LIBRARY.mkdir(parents=True, exist_ok=True)
        (DATA / "downloads").mkdir(exist_ok=True)
    books = []
    expected_files = set()
    for line in (ROOT / "books.tsv").read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        name, url, digest = line.split("\t")
        if not re.fullmatch(r"[a-z0-9]+(?:-[a-z0-9]+)*", name):
            raise ValueError(f"invalid sample book name: {name}")
        if not re.fullmatch(r"[a-f0-9]{64}", digest):
            raise ValueError(f"invalid checksum for {name}")
        if not url.startswith("https://standardebooks.org/ebooks/"):
            raise ValueError("unexpected ebook source")
        filename = f"{name}.epub"
        if filename in expected_files:
            raise ValueError(f"duplicate sample book: {name}")
        expected_files.add(filename)
        books.append((name, url, digest))

    for path in LIBRARY.iterdir():
        if (not args.verify and not path.is_symlink() and path.is_file()
                and path.name.endswith(".epub.part") and path.name[:-5] in expected_files):
            path.unlink()
            continue
        if path.name not in expected_files or path.is_symlink() or not path.is_file():
            raise ValueError(f"unexpected file in demo library: {path.name}")

    for name, url, digest in books:
        destination = LIBRARY / f"{name}.epub"
        if destination.exists():
            verified(destination.read_bytes(), digest)
            continue
        if args.verify or args.reset:
            raise ValueError(f"missing sample EPUB: {name}")
        request = urllib.request.Request(url, headers={"User-Agent": "Bokhylle-demo-seed/1.0"})
        with urllib.request.urlopen(request, timeout=30) as response:
            data = response.read(MAX_EPUB_BYTES + 1)
        verified(data, digest)
        temporary = destination.with_suffix(".epub.part")
        temporary.write_bytes(data)
        os.replace(temporary, destination)
        print(f"Downloaded {name}")
    if args.reset:
        shutil.rmtree(CONFIG)
        CONFIG.mkdir(parents=True)
    if not args.verify:
        MARKER.write_text("bokhylle-demo-v1\n")
    print(f"Demo {'verified' if args.verify else 'prepared'} at {DATA}")


if __name__ == "__main__":
    main()
