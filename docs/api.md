# HTTP API contract

`openapi.json` describes all registered `/api/*` HTTP operations. A running
server serves the same document at `GET /openapi.json`. The document is generated
from Axum route registrations and Rust request and response types, with explicit
metadata for session authentication, errors, status codes, and file responses.
HTTP route registrations live in `crates/bokhylle-server/src/routes/registry/`;
its domain routers are merged once for the running app and OpenAPI document.

The application authenticates most operations with the `bokhylle_session`
cookie returned by login. The document marks the public health, login, logout,
login user list, and demo entry operations separately. An administrator's role
and a child's access to a book are enforced by the server; possession of a cookie
alone does not grant access. Error responses use `{ code, message, details }`.
Downloads return EPUB, PDF, or CBZ data, backup downloads use an octet stream, and
cover routes return images. The OpenAPI contract covers `/api/*`; OPDS, KOReader
sync, and MCP retain their own protocols.

Direct acquisition uses `POST /api/books/{book_id}/acquisitions/http` with an
HTTP(S) `url` and optional `format` (`epub`, `pdf`, or `cbz`). OPDS sources use
`GET` and `POST /api/catalogues`, `DELETE /api/catalogues/{id}`, and
`GET /api/catalogues/{id}/feed`; a chosen file starts through
`POST /api/catalogues/{id}/acquisitions`. Only administrators manage sources.
The feed response includes file choices, but not their remote download URLs;
the acquisition endpoint refetches the selected page before queuing the file.
Existing acquisition and import status endpoints track both HTTP and torrent
jobs, as well as NZB jobs. Release previews and candidate choices include a
`method` (`torrent`, `nzb`, or `http`) so clients can describe availability
without assuming every result has a seeder count. Adult acquisition permission
and child restrictions apply to these routes.

Administrators can select `integrations.indexer.provider` (`auto`, `prowlarr`,
`torznab`, or `newznab`) and configure each source through the settings API.
Newznab needs `integrations.newznab.url`, `.api_key`, and optional `.categories`;
SABnzbd needs `integrations.sabnzbd.url`, `.api_key`, and `.category`. API keys
remain write-only. `POST /api/admin/integrations/newznab/test` and
`POST /api/admin/integrations/sabnzbd/test` check their respective connections.
The selected Newznab candidate carries an NZB GUID; the private retrieval URL
is stored separately for restart recovery. Completed SABnzbd paths must resolve
inside the configured downloads directory before entering the common importer.
`imports.watch_enabled` and `imports.watch_folder` configure the optional local
watcher; it imports settled top-level EPUB/PDF/CBZ files and journals placement.
`GET /api/admin/maintenance/watch` reports its enabled state, configured path,
unfinished placements, pending cleanup, files in review, and the last journaled
error. The route requires an administrator. Cleanup recovery continues when
new watch-folder imports are disabled.

The browser reader uses these operations for a specific
`book_id` and `file_id`:

- `GET /api/books/{book_id}/files/{file_id}/content` streams authorized EPUB,
  PDF, or CBZ bytes inline with range support and private, no-store caching. Child profiles
  may use it only while the book is assigned to their shelf; download remains
  forbidden to children.
- `GET /api/books/{book_id}/files/{file_id}/position` returns the current file
  SHA-256 identity, format, this profile's browser position, and any exact-file
  KOReader position, plus the nullable book direction, nullable series default,
  this profile's optional override, and book-level `bookCompleted` state.
  `position: null` means no browser locator exists
  for those exact bytes.
- `PUT /api/books/{book_id}/files/{file_id}/position` saves a bounded locator,
  percentage from 0 to 1, completion flag, file SHA-256, expected browser
  revision, and expected KOReader revision. EPUB may include an exact XPointer
  produced from its CFI; PDF and CBZ page locators are copied to the KOReader
  document record. Use revision `0` to create a position for new file bytes.
- `GET` and `PUT /api/books/{book_id}/completion` read and explicitly set
  this profile's finished state for a book (`{"completed": true | false}`).
  The state survives file replacement. Children can update only assigned books.
  A stale revision or file identity returns HTTP 409; fetch the position again.
- `PUT /api/books/{book_id}/files/{file_id}/direction` accepts
  `{ "direction": "ltr" | "rtl" | null }` to set or clear the current profile's
  book override. Child profiles must still have the book on their shelf.
- `GET /api/books/{book_id}/files/{file_id}/pages` returns the page count for a
  CBZ, and `GET /api/books/{book_id}/files/{file_id}/pages/{page}` serves one
  bounded image page. The same child shelf check applies to both operations.

Browser EPUB CFIs remain separate from KOReader XPointers. Percentage is for
display only; it never substitutes for an exact locator during handoff.

`GET /api/library/comics` returns paged series tiles and standalone comic or
manga books. `GET /api/series/{id}` returns ordered volumes and the signed-in
profile's `reading` state (`finishedBookIds`, `current`, `nextBookId`, and
`missingNextVolume`). Both use the
requested shelf scope; child profiles are always restricted to their own shelf,
including when a caller asks for household scope. `GET /api/books/facets`
includes publication kind counts for the same scope. Book list and search
accept `kind=books` or `kind=comics` for the visible Library categories.
Administrators can list, create, or update local series through
`/api/admin/series` and correct a book's `publicationKind`, nullable `seriesId`,
`seriesSortOrder`, and nullable `readingDirection` through
`PUT /api/admin/books/{id}`. Omitted correction fields retain their values;
an explicit JSON null clears an override or link. Book detail displays the
linked local series name in `series` and retains imported text separately in
`legacySeriesText`.

Administrators can page through `GET /api/admin/books/classification-review`
with `status=pending` (the default) or `status=all`, `kind=book|comic|manga`,
`attention=simple|review`, and `pageSize` from 1–500 (default 25). The response
includes suggested-type and attention counts across the selected status, before
the other filters. Each item includes its current classification, a suggestion
based on embedded series text, title, filename, and format, and a `needsReview`
flag for comics, manga, or series clues. `POST` to the same route with 1–1,000
`decisions` applies or dismisses a batch atomically. An `apply` decision
supplies a publication kind, optional local `seriesId` or `newSeriesName`,
volume label, numeric sort order, and reading direction. `dismiss` only marks
the item reviewed. `onlyIfPending` rejects a stale selection when another
review has already processed the file. The server never applies suggestions
during a scan; manual series links and volume corrections are retained during
later metadata refreshes.

Book and edition detail responses include `metadataSources`: each entry has
`field`, `source`, nullable `sourceKey`, and `manual`. Origins include provider
names, `epub`, `pdf`, `cbz`, `filename`, `mixed`, and `unknown`; manual corrections report
`source=manual`. Existing metadata is migrated with unknown origins rather
than inferring them from provider identities. Source keys never contain local
filesystem paths. Ratings retain their separate rating provenance.

`PUT /api/admin/books/{id}` only changes supplied fields. Corrections to title,
authors, description, language, imported series text, volume label, and the first
edition's publication year protect those fields from automatic updates. An
explicit null or empty string clears nullable text while preserving the manual
intent; an empty author list is also protected. `useAutomaticMetadata` accepts
field names (`title`, `authors`, `description`, `language`, `series`,
`seriesNumber`, `publicationYear`, `cover`, `publisher`) and restores each
selected field's saved automatic value, releasing its manual correction.
Cover and publisher have provenance but no manual editor in this release.
A field cannot be corrected and reset in the same request. The update, field
ownership records, and search index commit together. Resetting imported series
metadata does not remove an explicit local series link. Catalogue availability
and a file's actual edition language remain separate from a corrected book
language.

Adult shelves are private: `user` shelf filters accept the caller's own ID,
or an administrator's child profile ID. Only administrators may list child
profiles, assign a child's shelf, and approve or decline another person's book
request. `GET /api/books/{id}/shelf-users` returns child assignments to admins
only. `canAcquire` in the admin user API controls whether a non-admin adult may
start a new acquisition. It does not limit reading existing shared books or
adding one to that adult's own shelf. A reader without it may create a request
for an administrator. Completed acquisitions enter the common library once.

`GET /api/auth/me` and profile updates include `shelfFinish` (`oak`, `black`, or
`metal`), `shelfDecorations`, and `spotlightRotation`. Adult profiles save these
through the existing `PUT /api/profile`; omitted fields retain their values.
Invalid shelf finishes reject the entire update. Existing child and shared demo
restrictions still apply. These appearance choices are independent of the
browser's Paper/Ink theme and do not change book access or reading preferences.

Each signed-in user can set a profile picture with `PUT /api/profile/avatar`
(raw PNG, JPEG, or WebP body, matching `Content-Type`, maximum 1 MB), read it
with `GET /api/profile/avatar`, or remove it with `DELETE /api/profile/avatar`.
The image is stored in `user_avatars` and can only be read or changed by
that user, including child profiles. `GET /api/auth/me` includes
`avatarVersion` (null when absent) so clients can refresh the picture after a change.

## Updating the contract

After changing HTTP routes or their data shapes, run:

```sh
make openapi
make openapi-check
```

This regenerates `openapi.json` and `frontend/src/api/generated.ts`. The frontend
binds its JSON requests to generated operations through `apiRoute`, which checks
the route, method, URL shape, request body, and response type at build time.
Response types used by the frontend come from the generated components. Every JSON success response
has a schema, including responses that use 201 or 202 and endpoints with more
than one success status. Admin settings contain a map of values whose type
depends on the setting key, diagnostic event details follow their event type,
and older review journal entries may have earlier fields; those values remain
open within their typed response objects. Nullable response fields are required
when Rust serializes them as explicit null; fields with `skip_serializing_if`
stay optional. Do not hand-edit either generated
file. CI checks that every registered `/api/*`
operation is present, has a constrained JSON success and error response,
declares authentication, and has current generated files.
