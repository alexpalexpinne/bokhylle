# Third-party notices

Bokhylle's own source code is licensed under AGPL-3.0-only. Dependencies and
fonts retain their own licenses; the Cargo and pnpm lockfiles identify the
exact dependency versions used by a build.

The development-only OpenAPI type generator includes `argparse` under
Python-2.0 and `type-fest` under MIT or CC0-1.0. These tools do not ship in
the built web app or server image.

RAR archive inspection uses the Rust `rars` crate, which is licensed under
MIT or Apache-2.0. It is a pure Rust implementation with no native extraction
dependency. Bokhylle only extracts RAR archives.

The bundled IBM Plex Sans, Newsreader, and Source Serif 4 fonts use the SIL
Open Font License 1.1. Their copyright notices and license texts ship with
the web app in `frontend/public/licenses/`.

The production reader bundles Epub.js 0.3.93 (BSD 2-Clause) and PDF.js
6.3.289 (Apache 2.0), including the PDF.js worker and its image decoders. Their package license
texts ship at `/licenses/epubjs.txt` and `/licenses/pdfjs.txt` in the built
web app. The installed `pdfjs-dist` package has no separate NOTICE file.
`pnpm -C frontend licenses:check` also checks that these copies match the
installed packages, so dependency updates cannot leave stale notices.
