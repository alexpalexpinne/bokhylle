#!/usr/bin/env python3
"""Prepare the static website with an explicit HTTPS demo address."""

import argparse
import html
import os
from pathlib import Path
import shutil
from urllib.parse import urlsplit


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--demo-url", default=os.environ.get("BOKHYLLE_DEMO_URL", ""))
    args = parser.parse_args()
    address = urlsplit(args.demo_url)
    if (address.scheme != "https" or not address.hostname or address.username
            or address.password or any(char.isspace() for char in args.demo_url)):
        parser.error("set BOKHYLLE_DEMO_URL to the public HTTPS demo address")
    if args.output.exists():
        parser.error("output directory must not already exist")

    source = Path(__file__).resolve().parent
    page = (source / "index.html").read_text()
    placeholder = '<meta name="bokhylle-demo-url" content="" />'
    if page.count(placeholder) != 1:
        parser.error("index.html must contain one demo URL placeholder")
    page = page.replace(placeholder, '<meta name="bokhylle-demo-url" content="'
                        + html.escape(args.demo_url, quote=True) + '" />')
    args.output.mkdir(parents=True)
    (args.output / "index.html").write_text(page)
    for name in ("site.js", "style.css"):
        shutil.copy2(source / name, args.output / name)
    shutil.copytree(source / "assets", args.output / "assets")
    (args.output / ".nojekyll").touch()
    print(f"Website prepared at {args.output}")


if __name__ == "__main__":
    main()
