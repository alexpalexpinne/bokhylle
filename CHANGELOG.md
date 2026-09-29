# Changelog

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
