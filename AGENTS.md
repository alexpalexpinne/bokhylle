# AGENTS.md

## Project

Bokhylle is a self-hosted household book library. People browse local books, discover titles, request or acquire missing books, manage personal shelves, and read or send EPUB/PDF/CBZ files. Adults can manage child profiles. OPDS catalogues, Prowlarr/Torznab, Newznab, qBittorrent, SABnzbd, email delivery, and reader integrations are optional infrastructure behind that experience.

One Rust server provides the API, background work, and built React app. SQLite stores metadata and user state; the configured library directory stores the books. Keep imports recoverable and permissions consistent across HTTP, OPDS, kosync, and MCP.

## Working rules

- Do not push, open a pull request, publish an image, or dispatch a GitHub workflow without the maintainer's approval. Verify locally first.
- Keep secrets and local data out of commits. `.env`, `data/`, `config/`, `library/`, `downloads/`, and build outputs are ignored.
- Read current code and tests before relying on old plans. Update public docs when behavior or configuration changes.
- Use fictional books or the isolated demo for screenshots. Refresh the README and website previews when their UI changes; never capture a household library for public assets.
- Add a new SQL migration for schema changes. Applied migrations are immutable and embedded into the server at compile time.
- Match the editorial Paper and Ink design in `docs/design-principles.md`. Reuse shared UI components when practical.

## Repository map

- `crates/bokhylle-core`: book formats, title and ISBN identity helpers.
- `crates/bokhylle-library`: filesystem scans, EPUB/PDF/CBZ extraction, covers.
- `crates/bokhylle-metadata`: provider interface and Open Library/Google Books clients.
- `crates/bokhylle-acquisition`: release evaluation, Prowlarr/Torznab/Newznab and qBittorrent/SABnzbd clients, state machine.
- `crates/bokhylle-importer`: safe archive inspection and candidate scoring.
- `crates/bokhylle-server`: Axum API, auth, settings, SQLite, background jobs, import and delivery.
- `crates/bokhylle-server/src/services`: product rules shared by HTTP and MCP.
- `migrations/`: forward-only SQLite schema changes.
- `frontend/src/pages`, `frontend/src/api`, `frontend/src/components`: React screens, clients, UI.
- `website/` and `demo/`: static public introduction and isolated sample installation.
- `docs/`: installation, operations, API, design, and publishing guides.
- `crates/bokhylle-server/tests`, `frontend/tests/e2e`: integration and browser coverage.

## Commands

- `make check`: Rust formatting, Clippy with warnings denied, workspace tests, frontend lint and build.
- `cargo test -p bokhylle-server --test pipeline`: focused server integration test.
- `pnpm -C frontend build` and `pnpm -C frontend lint`: frontend checks.
- `make dev-server` and `make dev-frontend`: local backend and Vite server.
- Use Node.js 24 and pnpm 10. Build the frontend before serving it from the Rust server.

## Product invariants

- Only EPUB, PDF, and CBZ enter the library. Archives can contain them, but archive and path limits must remain enforced. CBZ image entry and decompression limits also apply.
- Never trust a path returned by a download client. Canonical containment inside the configured downloads directory is required before inspection or import. Direct downloads and watch-folder snapshots use approved staging areas; all final targets and recovery paths must remain inside the library root.
- Imports place a verified partial file and rename it atomically. Acquisitions journal the intended target in `pending_imports` before placement so recovery can finish after interruption.
- Acquisition transitions live in `crates/bokhylle-acquisition/src/state.rs` and use compare-and-swap updates with an event in the same transaction. The active variant unique index in `migrations/0001_initial.sql` follows the protected-state set; changing that set requires a new migration.
- An acquisition is shared per book and accepted-language variant. Store the requester's language intent when creating it; current profile settings must not silently change active work.
- `user_books` is the personal book relation. Household ownership and each profile's shelf, taste, and child visibility are distinct. Child routes must continue to enforce shelf-scoped access.
- HTTP and MCP paths for the same operation call the shared services. MCP tokens are profile-scoped; child rules and user roles still apply.
- Reader tokens and agent tokens are hashed at rest and shown only once. Secret settings are write-only through the admin API and must not be logged.
- Blocking filesystem and attachment reads stay off the async runtime. Provider responses have size caps.

## Testing

Server integration tests use in-process Axum and fake metadata, indexer, and download providers; background loops are ticked explicitly. Browser tests in `frontend/tests/e2e` need a live server and `BOKHYLLE_E2E_PASSWORD`. Set `BOKHYLLE_E2E_STRICT=1` to fail instead of skipping if unavailable. Add tests where a change crosses a permission, recovery, import, or acquisition boundary.

## Deployment notes

The Dockerfile caches Rust dependencies from Cargo manifests with stub sources. The backend stage copies migrations and touches real sources before rebuilding; keep that step or Docker may ship the stub binary. CI verifies install, import, restart, and restore with `scripts/check_image.py` before publishing each image. For internet access, require HTTPS and secure cookies; trust proxy headers only behind a controlled reverse proxy.
