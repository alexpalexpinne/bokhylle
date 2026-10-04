# Changelog

## 0.3.2 — 2026-10-04

- Replace the inline version list with one proposed version and a Change version picker. Desktop uses a dialog and mobile uses a scrolling sheet; reviewing or choosing files does not start a download.
- Require title and author evidence, or an exact ISBN in the release name, before recommending or automatically choosing a file. Exclude clear author, title and volume conflicts from normal choices; label incomplete matches for deliberate review. Recheck older pending choices using their saved request preferences.

## 0.3.1 — 2026-10-04

- Show available versions when opening an undownloaded book for adults using that preference. Keep the picker in the details column on desktop and full width on mobile; opening it does not start a download.
- Use the selected release when getting a book, rechecking it against the saved language and format preferences. Ask for another choice if that release becomes unavailable or unsuitable instead of silently replacing it.
- Make sharing icons visibly interactive for owners and explain read-only sharing states. Hide the default sharing marker until a catalogue book is added.
- Preserve covers after acquisition and sending. Extract embedded artwork during import and recovery, retain catalogue artwork identities, and repair older missing automatic covers without overriding manual choices.
- Open reader setup from Get & send when no reader is saved, using the same reader records as Profile. Save the chosen destination with the acquisition request so later default-reader changes do not redirect the send.
- Replace repeated reader-setup hints with the optional send action. Keep setup in the send dialog and handle stacked dialogs consistently.
- Show role-appropriate guidance when book downloading is not configured, and keep Try again beside temporary version-search errors.
- Label catalogue language availability as English editions or the matching language. Use the actual file language for downloaded books; catalogue descriptions keep their original text.

Upgrade 0.1.0, 0.2.0, or 0.3.0 in place after backing up config and library.
A forward-only migration retains catalogue cover identities. See
[upgrade details](docs/operations.md#database-compatibility). Use
`ghcr.io/alexpalexpinne/bokhylle:v0.3.1` for the amd64/arm64 image.

## 0.3.0 — 2026-10-03

- Make notifications open their book, request, or matching Activity item, including the version chooser when a selection is needed. Keep child links within their requests and assigned books.
- Load Home from local books and saved profile suggestions without waiting for external catalogues. Refresh recommendations in the background for the next visit, keep visible shelves stable, and run independent Spotlight searches with bounded concurrency.
- Simplify adult download selection: Get follows the account preference, with one selectable list in the book dialog and the same chooser in Activity.
- Remove repeated download-preference copy from Discover book dialogs and show the saved sharing default as a compact icon in the header.
- Use saved sharing defaults during acquisition. Show a visible Private or Shared marker that owners can open to change sharing; put bulk sharing under the shelf's More menu.
- Separate acquired ownership from shelf membership. Borrowers cannot change or continue another owner's sharing; independent owners and child assignments keep their access.
- Give Read and Send equal emphasis on downloaded book pages, with a compact mobile cover and icons for Shelf, Like and More. Move Download, additional versions, URL imports, collections and maintenance into a desktop popover or mobile sheet.
- Balance the mobile toolbar across three equal-width cells, keep desktop controls together, and place subjects in the header. Place the sharing icon at the header’s top right with a tooltip. Restore the small uppercase subject styling with a compact expansion control and a full-width mobile row; simplify retry notices to plain text and actions.
- Show download state and progress on book pages. Requesters can choose a reader for Send when ready, change or cancel their scheduled send, and keep reading an existing file while another version is added.
- Convert catalogue Markdown descriptions to readable text and omit contents lists from Spotlight blurbs.

Upgrade 0.1.0 or 0.2.0 in place after backing up config and library. New migrations
separate ownership from borrowing and preserve scheduled reader destinations.
See [upgrade details](docs/operations.md#database-compatibility). Use
`ghcr.io/alexpalexpinne/bokhylle:v0.3.0` for the amd64/arm64 image.

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
