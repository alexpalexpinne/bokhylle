# Spotlight and shelves

This document records the current Home and browsing behavior. See
[design principles](design-principles.md) for the visual rules and
[frontend standards](frontend-standards.md) for interaction and accessibility.

## Shelves and appearance

Home uses continuous shelf rails and a matching featured shelf. The shelf
surface and vase are small inline SVGs with no external assets or new packages.
Profile → Appearance saves Light oak, Faded black or Muted metal, decorations,
and automatic Spotlight rotation per profile. Paper/Ink remains the device's
theme choice.

Discover and Library use responsive shelf grids with one continuous surface
per row, including partial rows. They share Home's cover sizing, materials and
profile finish while retaining filters, pagination, child shelf scope and the
existing book, author and actions. Author portraits keep their layout.
Shelf bands are 6px on mobile and 8px on larger screens, with shared top, front
and side faces and restrained CSS shadows.

Library keeps All and relevant publication types as quiet scope tabs. Comics &
Manga appears only when the current shelf scope has classified volumes. Its
cards represent local series identities, with ungrouped comics shown
individually. A series opens an
ordered volume shelf; each volume still opens its own book detail page. Search
returns individual volumes. Magazine and catalogue kinds remain in All until
their own publication and issue model is ready.

The series page shows the current volume and finished volumes for the signed-in
profile. **Mark finished** is an explicit choice; merely opening a file does
not count. The next-volume link requires a single, consecutive numbered volume
after the highest finished regular volume. A gap is shown as missing, while
duplicate numbers and specials do not produce a guess. Progress remains tied
to the book when its file is replaced.

## Spotlight layout and rotation

Home first loads local books and saved catalogue suggestions. It does not wait
for external catalogue searches before showing Spotlight. On an initial load,
sections appear in their established order as their requests finish; slower
collections and authors do not hold up the featured book or earlier shelves.
Loading slots use the same featured shelf and rail geometry as the content.

A complete Home snapshot is retained for one minute in the browser tab, scoped
to the authenticated profile, role, child permissions and preferred languages.
It survives a page reload and is cleared on sign-out, session expiry or demo
profile switching. A returning visitor sees that snapshot immediately while
requests prepare the next visit. The current visit's books and recommendations
stay in place. Explicit likes, rejections, shelf changes, follows, sharing and
interest changes invalidate the snapshot. Hiding or restoring a subject refreshes
Spotlight, Picked for you, rediscovery and subject rails immediately; older
in-flight responses cannot restore an excluded recommendation.

Catalogue recommendations also have a saved snapshot on the server, separate
from local books. It refreshes after fifteen minutes, with up to two independent
catalogue searches running together across profiles. Repeated refreshes for a
profile share the same work. Matching saved suggestions can be used for up to
one day while a refresh runs or a provider is unavailable. Changing taste seeds,
preferred languages, profile type or metadata providers invalidates that
selection. The strongest subject and author stay in the seed set, with up to
two further seeds of each kind rotating daily through the remaining taste pool.
Current ownership, likes, rejections, hidden subjects and child exclusions are
checked on every response;
local books always use the current sharing and shelf rules.

Refreshed catalogue suggestions are saved for the next Home visit rather than
being inserted above shelves already being browsed. A new profile with no local
books or saved suggestions can show its first catalogue results when they arrive,
since there are no existing shelves to move.

Spotlight advances every ten seconds while visible; hover, keyboard focus,
touch and open dialogs suspend it temporarily. Rotation starts a fresh interval
after interaction ends or an arrow is used. Previous and next controls remain
centered beneath both Spotlight columns at every screen size. Reduced motion,
child and shared demo profiles use manual navigation.

At 375px and above, mobile Spotlight puts the text and action on the left and
the featured cover on the right, with carousel controls below. Narrower phones
keep the description and action below both columns and omit the vase. Mobile
excerpts use two to five complete lines according to the space beside the
title. Actions occupy a fixed row with at least 12–16px clearance after the
excerpt and metadata, keeping the button stationary between slides. Shorter
copy leaves extra whitespace above that row. Unowned books have no ownership
label. The outer frame, shelf and controls also stay stable, with the mobile
shelf centered vertically. Covers keep their proportions and space beside the
upright; landscape artwork also omits the vase.

## Book information

Provider staff-picks collection tags are hidden in Spotlight; genuine subjects,
ratings and language metadata remain available. Spotlight applies each
profile's preferred languages to saved catalogue works and to the actual
editions of downloaded files. Catalogue availability is stored separately from
a file's edition language; an Open Library work with English and Russian
editions is eligible for an English reader, while a Russian-only downloaded
file is not. Unknown-language books remain eligible. The library detail
distinguishes "Available in EN" from an edition's language and publication year;
it does not label a catalogue work with an arbitrary sampled edition's date or
language.

Continue reading appears for unfinished books reported by browser or KOReader progress
sync.

Spotlight displays at most five books, interleaving personal shelf, household
and catalogue candidates. It scores every candidate against the complete taste
profile and prefers varied authors and series, relaxing variety for a small
candidate pool. Only the final featured catalogue books leave Picked for you;
catalogue books rejected by the presentation filter return to that rail.
Catalogue searches retrieve up to fifty candidates before display filtering,
merge suggestions across seeds, rank the complete eligible pool against all
interests and authors, and fill Picked for you with up to eighteen. A book can
match interests that were not used for that day’s bounded provider searches.
Detail lookups for featured candidates retain additional subjects and languages
and recheck eligibility before those books enter either recommendation surface.

Spotlight candidates remain in Recently Added, Rediscover your library, and
subject or child shelf rails. Featuring a book does not remove it from its
shelf; even a small shelf can show its books below Spotlight. Empty sections
remain hidden. Subject rails require at least three eligible books with a
shared informative subject; books without subject metadata still appear in
the shelf's recent and rediscovery rails. Rediscovery shows up to twelve books,
uses a stable profile/day shuffle, and applies feedback and actual file-language
eligibility. Recently Added and Continue reading remain activity-based shelves.

## Personal recommendations

Household ownership alone does not qualify a book for adult Home rails or
Spotlight. Candidates need a personal reason: shelf membership, a like, a
followed author, a selected interest, or informative subjects shared with books
the profile deliberately shelved, requested, or received on its reader. Generic
catalogue tags such as "fiction" do not establish inferred interests. Bulk claiming books
does not make every subject a taste signal. Child assignments and another
profile's likes never establish adult affinity.

Picked for you uses explicit interests and taste derived from likes, requests,
successful reader sends, deliberate shelf additions, and followed authors.
Shared taste rules weight likes at 5, requests at 3, and successful sends or
deliberate shelf additions at 1. Each book contributes its strongest signal once;
subject and author weights accumulate across distinct books. Explicit interests
and author follows each add 5, and follows augment rather than replace inferred
author taste. Catalogue noise is removed before seed selection.
Personal affinity selects candidates; available household
copies determine whether there are enough books for a useful local rail.
Local subject selection multiplies accumulated taste by subject specificity:
1 for broad genres, 3 for ordinary topics and 5 for specific topics. Books inside
each rail rank by complete subject/author affinity, then recency. The liked-books
rail requires two informative shared subjects and has priority when eligible.
Home shows up to five local recommendation rails, suppressing subject aliases
and rails sharing at least three quarters of the smaller rail's books.

Followed-author discoveries apply the same language, ownership and feedback
rules before choosing two books per author, up to twelve total. All available
catalogue languages count; unknown languages remain displayable. This does not
relax language checks for automatic acquisition or delivery. Library search
puts exact title/author matches before partial matches and uses full-text
relevance before recency. Recent library updates respect current sharing.

Marking a book finished adds no recommendation weight. Its existing like,
request, send, or deliberate shelf-addition signals remain. Completion is
engagement, not an explicit preference, and does not exclude the book from
recommendations.

The isolated demo starts adult visitors with two sample likes, two author
follows, and reading interests. Curated subjects on the prepared EPUBs allow
the normal local recommendation rules to produce useful rails. Demo Picked
for you and followed-author discoveries use those sample copies, with local
covers and book links; they exclude books already on the visitor's shelf and
respect feedback, without querying an external catalogue. Child profiles
retain their assigned shelves and receive none of the adult's seeded taste.

Hidden subjects and "not for me" exclude local recommendation candidates,
including backfill candidates. Sparse profiles can choose reading interests
through the setup wizard or deliberately browse Household. Adult household
browsing, search, file access, and acquisition reuse retain their existing scope;
children remain shelf scoped. There is no Audience assignment layer.


**Explore more** opens a personalized shelf with up to 72 suggestions and a
reading-interest filter. It loads one bounded selection per visit and reveals
24 books at a time, so newly recorded impressions cannot reorder the next group.
It shares Home's ranking, permissions, language rules and feedback. Search the
catalogue remains available for deliberate browsing beyond this selection.

Browser Back and Forward restore the expanded Library or recommendations list,
page scroll position, horizontal shelf positions and the book link's keyboard
focus. Profile-scoped book data is retained in memory for one minute; list counts
and positions are retained for up to one hour. Expired Library data is fetched
again for every previously loaded page. Sign-out, session expiry and demo profile
switching clear these browsing records. Explicit taste and sharing changes clear
cached book data while keeping the current browsing position.

Recommendation feedback uses a compact desktop popover or a mobile sheet.
Confirmation and **Undo** stay visible above the mobile navigation until
dismissed, replaced by another action or the page is left. Undo is available for
ten minutes and restores the previous preference or dismissal without adding a
book to the shelf or starting an acquisition. Later feedback, changed access,
or an expired receipt cannot be overwritten. Rejections can still be restored
through Profile → Your taste.

Recommendations and catalogue searches reserve matching shelf placeholders
while the first response is pending. A failed request shows **Try again** beside
its explanation; available books remain visible during refresh or pagination
failures. An empty state appears only after a successful empty response.

The suggestion menu offers **Like**, **Not for me**, and **Show something else**.
Like and Not for me create personal preferences for catalogue books without
adding them to a shelf or starting an acquisition. Profile → Your taste lists
rejected books and lets the reader restore them. Show something else sets a
book aside for seven days without changing its taste weight. Local recommendation rails also honor temporary dismissal and the repeat
penalty. Explicit feedback
refreshes both Home and the personalized shelf; an older request cannot undo it.

Equivalent subject labels share one recommendation concept, including Fantasy
and Fantasy fiction, Mystery and Detective and mystery stories, and Humour and
Humor. Language and catalogue qualifiers are stripped for matching. A book
contributes once per concept even when both tags are present. Taste, hidden
subjects, local rail membership, catalogue scores, and Library subject filters/facets
use the same mapping;
original metadata labels remain available on book pages. View all from adult
recommendation rails opens the matching household scope; child links retain the
assigned shelf. The mixed liked-books rail has no category-hide action.

A suggestion records a visible impression only after at least half of its cover
is visible for one second in an active tab with no open dialog. Offscreen rail
books and hidden Spotlight slides do not count. Browser requests are batched.
A recently seen book receives a 20% score reduction for three days and 10% for
the following four days (integer scores round down); it remains eligible. Ignoring it does not create a
negative preference. Impressions update at most once per day per book/profile.
Offered candidate records authorize feedback for 24 hours and are pruned after
30 days of inactivity. They store profile-scoped identities and timestamps in
the local SQLite database; there is no external analytics service.

**Continue a series** uses the reader's explicit finished marks and shares the
series page's ordering rules. It offers a unique next regular integer volume,
or a search link for a proven gap when a later volume is known. An existing
unfinished volume stays in Continue reading. Fractional issues, specials,
conflicting order labels, and duplicate numbers do not establish a successor.
Adults see currently shared volumes; children see only assigned volumes and
receive no gap/search suggestion. Rejected books, hidden topics, and preferred
languages still govern eligible successors.

### Local quality diagnostics

Administrators can inspect their own most recent selection with
`GET /api/recommendations/diagnostics` using their signed-in session. The result
includes retrieved catalogue candidates (including repeated results across
seeds), unique eligible/selected books, subject and author score contributions,
recent-impression penalties, catalogue-cache use, filter counts, and elapsed
milliseconds for taste, local queries, catalogue work and ranking. Local query
counts start after SQL eligibility filtering. Cached passes retain provider
filter counts from the search that created the snapshot; live filters and
dismissals are still reapplied. Diagnostics expire after one day and are never
an endpoint for inspecting another reader's preferences.

`crates/bokhylle-server/tests/recommendations.rs` uses fictional reader scenarios
for cold starts, likes, follows, aliases, full-profile ranking, large provider
pools, repeated exposure, rejection/restore, series gaps, and permission changes.
A 1,000-book household fixture reports query timings without a machine-dependent
pass/fail time limit. Run `cargo test -p bokhylle-server --test recommendations -- --nocapture` to inspect those timings. Browser coverage verifies visible-only
impressions, stable Show more, rejection and restoration. These checks detect
policy regressions; they do not claim to measure an individual's enjoyment.

## Metadata corrections

Administrators see field origins in **Fix details**. Editing a field protects it
through provider enrichment, imports, rescans, and author repair, including when
the field is intentionally cleared. Saving the form sends only changed fields.
**Use automatic metadata again** selects an explicit reset; Save restores the
saved automatic value and releases the correction, while Cancel leaves it
protected. Historical values without source records are labeled unknown.
