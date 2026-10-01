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

Spotlight candidates remain in Recently Added, Rediscover your library, and
subject or child shelf rails. Featuring a book does not remove it from its
shelf; even a small shelf can show its books below Spotlight. Empty sections
remain hidden. Subject rails require at least three eligible books with a
shared informative subject; books without subject metadata still appear in
the shelf's recent and rediscovery rails.

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
Spotlight's taste seeds weight likes at 5, requested/sent shelf entries at 3,
and manual additions at 1; explicit interests and followed authors also supply
seeds. Local subject rails rank likes at 5, requests at 3, and successful
deliveries at 1. Personal affinity selects candidates; available household
copies determine whether there are enough books for a useful local rail.

Marking a book finished adds no recommendation weight. Its existing like,
request, send, or deliberate shelf-addition signals remain. Completion is
engagement, not an explicit preference, and does not exclude the book from
recommendations.

The isolated demo starts adult visitors with two sample likes, two author
follows, and reading interests. Curated subjects on the prepared EPUBs allow
the normal local recommendation rules to produce useful rails. Demo Picked
for you and followed-author discoveries use those sample copies, with local
covers and book links; they never query an external catalogue. Child profiles
retain their assigned shelves and receive none of the adult's seeded taste.

Hidden subjects and "not for me" exclude local recommendation candidates,
including backfill candidates. Sparse profiles can choose reading interests
through the setup wizard or deliberately browse Household. Adult household
browsing, search, file access, and acquisition reuse retain their existing scope;
children remain shelf scoped. There is no Audience assignment layer.

## Metadata corrections

Administrators see field origins in **Fix details**. Editing a field protects it
through provider enrichment, imports, rescans, and author repair, including when
the field is intentionally cleared. Saving the form sends only changed fields.
**Use automatic metadata again** selects an explicit reset; Save restores the
saved automatic value and releases the correction, while Cancel leaves it
protected. Historical values without source records are labeled unknown.
