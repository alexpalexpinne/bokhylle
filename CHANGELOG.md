# Changelog

## 0.2.0 — 2026-10-02

- Private or shared books for adults, with an account default, a choice when getting a book, and individual or bulk sharing controls. Personal shelves and reading progress stay private. Existing books remain shared after upgrading; other owners retain their access.
- Consistent book access across library browsing, discovery, file downloads, browser readers, OPDS, KOReader sync, and MCP.
- The same release chooser for administrators and adults allowed to get books. Use Automatic, Ask me, or Choose a version for one book; see release names, format, language, size, source, and torrent availability.
- Get another version for downloaded books. Distinct files are kept alongside existing copies and reading positions; active shared downloads retain their original selection.
- Server administration with build and storage health, verified backups, release awareness, and diagnostics.
- Household setup, child starting books, profile marks and pictures, personalised Home shelves, and refreshed public previews.

Upgrade an existing 0.1.0 installation in place after backing up config and
library. New forward-only migrations preserve accounts, shelves, and settings.
Use `ghcr.io/alexpalexpinne/bokhylle:v0.2.0` for the published amd64/arm64 image.
The project remains in public beta; see [current limits](docs/getting-started.md#current-limits).

## 0.1.0

First release for self-hosted household EPUB, PDF, and CBZ libraries.

- Shared library, personal shelves, child profiles, discovery, and requests.
- Browser readers, reader delivery, OPDS, KOReader synchronization, and profile-scoped MCP.
- Optional Prowlarr/Torznab and qBittorrent, Newznab and SABnzbd, direct HTTP, OPDS catalogue acquisition, and watch-folder import.
- Verified import placement, recoverable acquisition and cleanup journals, bounded publication extraction, and scheduled SQLite backups.
- One initial database schema and a container check for fresh install, import, restart, and database/library restore.

Start with a fresh config directory. Development databases and backups are
incompatible with this initial release baseline. Library files can be scanned
into a new installation.

This is an early release. Live Usenet service compatibility, physical reader
delivery, Safari/iPhone behavior, screen-reader testing, and additional NAS
setups remain hands-on validation tasks. See [current limits](docs/getting-started.md#current-limits).
