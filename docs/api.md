# HTTP API contract

`openapi.json` describes all registered `/api/*` HTTP operations. A running
server serves the same document at `GET /openapi.json`. The document is generated
from Axum route registrations and Rust request and response types, with explicit
metadata for session authentication, errors, status codes, and file responses.
HTTP route registrations live in `crates/bokhylle-server/src/routes/registry/`;
its domain routers are merged once for the running app and OpenAPI document.

The application authenticates most operations with the `bokhylle_session`
cookie returned by login. The document marks the public health, login, logout,
login user list and pictures, and demo entry operations separately. An administrator's role
and a child's access to a book are enforced by the server; possession of a cookie
alone does not grant access. Error responses use `{ code, message, details }`.
Downloads return EPUB, PDF, or CBZ data, backup downloads use an octet stream, and
cover routes return images. The OpenAPI contract covers `/api/*`; OPDS, KOReader
sync, and MCP retain their own protocols.

`GET /api/home/spotlight?cachedOnly=true` returns current eligible local books
and matching saved catalogue recommendations without contacting an external
provider. Omit `cachedOnly` or set it to `false` to refresh an expired catalogue
selection before returning. Home uses the cache-only response for its initial
layout and the full response in the background for the next visit. Recommendation
snapshots are profile-scoped and match current taste seeds, language preferences,
profile type and metadata providers; local sharing and child scope are checked
on every response.

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

`POST /api/books/{book_id}/acquisitions` and `POST /api/discover/acquisitions`
accept optional `askBeforeDownload`. Omit it to follow the profile's
`acquisitionMode`; `true` pauses before downloading so the requester can choose
from `GET /api/acquisitions/{id}/candidates`, then submit its candidate `index`
to `POST /api/acquisitions/{id}/select`. The override applies only to a new
acquisition. Joining active work preserves its original requester, selection,
and mode. A completed book can start another acquisition; distinct file bytes
are added alongside existing files, whose reading positions remain intact.

`GET /api/discover/releases` is a read-only preview evaluated against the
profile's accepted languages and requested format. It also accepts
`provider=local&providerKey=local:<book-id>` for an accessible library record.
Selectable versions include an opaque `selectionKey`; disabled versions
include `unavailableReason`. Browsing creates no acquisition or ownership.
Pass a chosen key as `releaseKey` to either acquisition creation endpoint.
The server journals that choice, freezes the request preferences, searches
again, and queues only the same identity if it is still suitable. If the chosen
version disappears or is rejected, the request requires another selection or
reports that no suitable release exists. Joining active work preserves the
original requester's choice.

Release previews return 503 with `code: "indexer_not_configured"` when no
search source is configured. This setup state is distinct from a temporary
search failure (`service_unavailable`), which can be retried.

Both acquisition creation endpoints accept `targetId` with `sendToReader: true`.
The target must be enabled and owned by the caller. Its address commits with
the caller's request, including when joining existing work; later default
changes cannot redirect that send. Omitted targets retain default-reader
behavior. The creation endpoints share this destination logic with the
catalogue acquisition service.

Discovery details for a downloaded book return its file's `language` and the
languages of its local files. Other entries return catalogue edition languages.
The provider's description is independent of those edition languages.

Adults allowed to acquire books see release names, sources, and seeder counts
in discovery previews, matching administrators. Adults needing approval see
availability summaries. Only the original requester or an administrator can
select a candidate. Technical scoring and acquisition
diagnostics remain administrator operations.

Book details include `sharingManaged`: `false` identifies a shared library
import without a personal owner. Only an owner's own sharing grant is editable.
Catalogue cover identities remain attached to local book records. Acquisition
imports extract embedded artwork before becoming ready, including recovery;
cover requests repair missing artwork on older acquisitions using embedded
artwork first and the saved catalogue identity as a fallback. Automatic cover
updates preserve manual corrections.

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

`POST /api/admin/users` accepts optional `startingBookIds` for a child profile
(at most 1,000 IDs). Every selected book must have a file in the household
library; metadata-only or missing books reject the request. Duplicate IDs are
ignored. Credentials, display name, language preferences, permissions and
initial shelf assignments commit in one transaction. A failure rolls back the
account and all assignments. Adults cannot use nonempty `startingBookIds`.

The household UI offers three child access modes using the existing booleans:
assigned books only (`canDiscover=false`, `canRequest=false`), search and ask
(`false`, `true`), and explore and ask (`true`, `true`). The API still supports
existing browse-only profiles (`true`, `false`); unrelated UI edits preserve
their permissions. All child library reads remain restricted to assigned books.

`PUT /api/profile/interests` atomically replaces the interest selection.
If a database write fails, the previous selection remains intact. Restarting
onboarding clears only the completion marker; the wizard preloads saved
interests and likes, and Skip does not replace interests.

`avatarPreset` is the optional bundled profile mark ID: `fox`, `owl`, `cat`,
`bear`, `whale`, `book`, `tree`, `mountain`, `moon`, or `leaf`. It is returned
by `GET /api/auth/me`, `GET /api/auth/users`, and the admin user API. New
profiles default to null (initials). Administrators can send it during account
creation or editing; omission on edit preserves it, while explicit null clears
it. Creation saves it in the same transaction as the account and starting books.

Any signed-in profile, including a child, can set its own mark with
`PUT /api/profile/avatar/preset` and `{ "avatarPreset": "owl" }`, or null to
restore initials. The field is required; unknown IDs and extra fields return
422. This endpoint changes only the current profile's mark. Children still
cannot change general account settings through `PUT /api/profile`.
Bundled artwork is served from `/profile-marks/<id>.svg`. Uploaded photos take
precedence; changing the mark does not delete a photo, and removing a photo
reveals the selected mark or initials.

Each signed-in user can set a profile picture with `PUT /api/profile/avatar`
(raw PNG, JPEG, or WebP body, matching `Content-Type`, maximum 1 MB), read it
with `GET /api/profile/avatar`, or remove it with `DELETE /api/profile/avatar`.
The original image is stored in `user_avatars` and can only be read or changed
through this endpoint by that user, including child profiles. `GET /api/auth/me` includes
`avatarVersion` (null when absent) so clients can refresh the picture after a change.

`GET /api/auth/users` lists enabled sign-in profiles and includes `avatarUrl`
(null when no picture is set). That URL uses the public
`GET /api/auth/users/{id}/avatar` endpoint, which serves only a re-encoded PNG
thumbnail, at most 160 × 160 pixels, with `Cache-Control: no-store`. It applies
image orientation and strips original metadata. Missing, disabled, corrupt, or
over-limit pictures return 404; clients should show the selected bundled mark or initials. Decoding runs off
the async runtime, at most two at a time, with a 1 MB input cap, 4096-pixel
dimension limits, and a 64 MB allocation limit. No additional profile details
or original image bytes are exposed by the thumbnail endpoint.

## Server administration

Server administration is restricted to administrators:

- `GET /api/admin/server` returns build identity, uptime, database responsiveness,
  effective restart notices, and storage grouped by filesystem.
- `GET /api/admin/server/restart` returns only the startup settings that differ
  from the running configuration, without reading storage or exporting values.
- `GET /api/admin/maintenance/backups` includes persisted backup attempts,
  successes, failures, inventory availability, and the next scheduled attempt.
  `backups.interval_hours` and `backups.keep` are changed through the settings API.
- `GET /api/admin/server/diagnostics` generates the allowlisted structured report
  described in [Operations](operations.md#server-administration). Raw logs remain
  separate at `GET /api/admin/logs`.
- `GET /api/admin/server/updates` reads the persisted stable-release cache;
  `POST` checks GitHub subject to a one-minute minimum interval. Offline and
  failed checks retain the last known release and report `unavailable`. Scheduled
  checks follow `updates.check_enabled`, with `BOKHYLLE_UPDATE_CHECKS` taking
  precedence. These routes do not update or restart the installation.

## Regenerating the contract

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

## Book sharing

`UserView.defaultBookSharing` is `private` or `shared`. Set it with `PUT /api/profile` using `defaultBookSharing`; omitted fields retain their current value. Defaults apply when a profile explicitly acquires a book, not when borrowing a shared book onto their shelf. Acquisition bodies for known books, Discover, direct HTTP links, and OPDS catalogue entries accept an optional `sharing` override, saved before starting background work.

Approval requests also accept `sharing` on `POST /api/requests`. The request saves its override or account default when submitted, and approval applies that saved choice when acquisition is needed. A request fulfilled by an existing shared file only adds a borrowed shelf entry. Duplicate pending requests keep their original choice. Children always request private access.

`BookDetail.sharing` is the signed-in owner's choice, or null for a shelf-only borrower. Only non-null sharing denotes ownership; putting a book on a shelf never grants ownership. `sharedInHousehold` reports whether the title is currently available to other adults, including another owner's sharing choice.

Adult owners update their own sharing with `PUT /api/books/{id}/sharing` and `{ "sharing": "private" }` or `shared`. The response is `BookSharingState`. `PUT /api/books/sharing` accepts `{ "bookIds": [1, 2], "sharing": "shared" }` and returns 204. Bulk updates accept 1–1,000 ids and commit atomically; every id must be independently owned by the caller. A shelf-only borrower receives 403, and the entire update is rolled back. Children cannot change sharing. Hidden books return 404 on detail, file, cover, and mutation routes; list counts, facets, authors, series, and collections follow visibility. Adult shelves remain private, including from administrators.

Acquired ownership outlives shelf membership. Coowners retain separate ownership of the same title; changing one sharing choice never changes another. Shelf-only adult borrowers depend on current household sharing and lose app/file access when no owner shares the book. Explicit child assignments remain shelf scoped. Existing unmanaged library imports remain shared. Migration 0006 derives ownership from acquisition participants and creators; for previously managed books without acquisition history, it preserves the earliest known adult as owner. Shelf-only grants cannot keep a book shared. Administrative maintenance, import review, acquisition oversight, backups, and the library filesystem remain operator tools.

The frontend download preference is account specific: `acquisitionMode: "automatic"` selects the best suitable release, while `"ask"` shows candidates in the current book dialog before downloading. Activity uses the same candidate selection operation. The optional `askBeforeDownload` API override remains available for explicit additional-version requests. Joining active work does not change its original requester, mode, language intent or selected release.

## Book download status and scheduled sends

`GET /api/books/{book_id}/acquisitions` returns the book’s acquisitions visible to the signed-in adult: their own requests and joined work, or all work for administrators. Book access is required; child profiles receive 403. The list is independent of Activity pagination.

`AcquisitionView.requestedByMe` identifies participation by the viewing profile. `scheduledDeliveryAddress` is the viewing requester’s frozen address while a send is scheduled, or null. Delivery intent and address are profile scoped. Delivery status follows the existing Activity rules: a participant sees their own delivery; administrative oversight of unjoined work uses the original requester.

`PUT /api/acquisitions/{id}/delivery` accepts `{ "enabled": true, "targetId": 1 }` to schedule a send to an enabled reader owned by the caller. Omit `targetId` to resolve the caller’s default or household fallback at scheduling time. The address is saved with the request; later default changes or target removal do not redirect it. `{ "enabled": false }` cancels only the caller’s scheduled send. Only an adult participating in active work may update it, including when the caller is an administrator. Completed or stopped work returns 409; available files use the normal delivery route. Existing Get & Send intents with no saved address retain default-reader behavior. Migration 0007 adds the saved destination.
