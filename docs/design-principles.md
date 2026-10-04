# Bokhylle design principles

For navigation, forms, state, and accessibility rules, see
[frontend standards](frontend-standards.md).
The current Home and browsing behavior is recorded in
[Spotlight and shelves](spotlight-and-shelves.md).

Bokhylle is a private library and acquisition service, not a media server.
It should feel like a calm personal library with the clarity of a good
catalogue: warm paper and ink, expressive serif typography, quiet composition,
and real cover art. It remains a useful web application, not a magazine.

## The rules

1. **The book cover is the primary visual object.** Preserve its proportions,
   contain it within a stable space, and make missing artwork fit the same
   composition. The surrounding UI must work with bright, dark, old, and
   unusually shaped covers.
2. **Typography creates hierarchy before containers do.**
3. **Use rules and whitespace before rounded panels.**
4. **Containers communicate interaction or state, not merely layout.**
   Dialogs, menus, inputs, warnings and selected states may have surfaces;
   "Recently Added", "Authors" or "Description" do not need a box.
5. **Pills are only for real filters and tags**, never for buttons or meta.
6. **Primary buttons are rectangular (3px radius), never capsules.**
7. **Paper and Ink are two expressions of one identity.** Paper is the
   canonical Bokhylle look (branding, README, screenshots); Ink is the same
   language after dark. System is the default choice.
8. **Colour appears in deliberate fields, not decorative gradients.**
   Spotlight keeps a neutral background independent of its cover. If another
   view uses a cover-derived field, keep it flat and within Bokhylle's tonal
   range. Terracotta is a restrained accent for primary actions, active
   details, and important links.
9. **Icons are functional, not decorative.** Search, back, download, send,
   bell, overflow, close. A noun does not earn an icon.
10. **Motion should feel like opening, shelving or moving through books**,
    never like generic app animation. Respect reduced motion and keep moving
    content under the reader's control.

## Spotlight

- Pair concise text with one featured cover in a restrained shelf composition.
  Keep the reason, title, author, useful description, and existing actions;
  do not invent ratings, endorsements, or progress. A structural upright and
  shared shelf surface suggest a bookcase without enclosing the whole hero.
- Keep the composition stable as books change. Give the cover a controlled
  space without cropping it; keep the action in a steady position and the
  previous/count/next controls together below the hero.
- Start Home with local books and saved profile suggestions. Reveal initial
  sections in a fixed order with matching loading slots. Prepare refreshed
  recommendations for the next visit so visible shelves stay in place.
- Adult profiles may enable automatic rotation. Pause it during interaction,
  when the hero is out of view or a dialog is open, and when reduced motion is
  requested. Child and shared demo profiles browse manually. Do not add a
  play/pause control to the hero.
- At phone widths, keep the cover beside the heading and text when there is
  room. On the narrowest screens, let longer copy and actions span beneath
  both columns instead of shrinking the cover or text beyond usability.
- Apply preferred languages to Spotlight's candidates. A catalogue work can
  be available in several languages; a downloaded file has an actual edition
  language. Present these as different facts, and never label a whole work
  with the year or language of an arbitrary sampled edition.

## Shelf system

- Use **cover above → continuous shelf line → title and author below** for
  book rails. Keep contextual status or recommendation text quieter than the
  title. Avoid wrapping every book in a conventional card.
- Use the same shelf material and thickness in Spotlight, scrolling rails,
  and responsive book grids. A grid has one line across each row, including
  a partially filled final row. Browsing pages keep their filters, links,
  pagination, and normal page layout; the shelf is their visual treatment.
- Let shelf material provide depth with restrained edge shading and contact
  shadow. Keep wood grain subtle. Profile appearance can choose Light oak,
  Faded black, or Muted metal without changing geometry or readability in
  Paper and Ink.
- A small, scalable vector object may soften the featured shelf. Keep it
  independent of the changing cover, optional per profile, and removable at
  narrow or awkward proportions. Decoration never carries information.
  Prefer shared SVG and CSS to a new asset or package for each variant.
- Show reading progress only when actual reader-synced progress exists.
  Do not add a progress bar merely to fill space beneath a book.

## Motifs

- **Catalogue numbering**: sections carry `01`, `02`, … with a hairline rule.
- **Shelf line**: a shared surface anchors covers without drawing literal
  furniture across the page.
- **Editorial rhythm**: use horizontal scrolling rails for short groups such
  as recommendations and recent additions, and responsive shelf rows for
  catalogue browsing. Keep section names tied to the product, with
  understated "View all" links where useful.
- **Marginalia**: metadata is annotated catalogue information, not pills.
- **Bookshelf mark**: the approved B/H shelf with one terracotta book is the
  logo, favicon and loading mark. Use the supplied
  [Bokhylle assets](../frontend/public/brand/) for standalone brand artwork.
  Keep the logo orange `#D46E3D` distinct from
  interactive text and button colours so those controls retain contrast.
- **Acquisition narrative** (Downloads and request moments): FINDING →
  FOUND → FETCHING → SHELVING → READY → DELIVERED, rendered as numbered
  stages with rules and plain text, not a generic progress bar.

## Book pages

- Keep Read in Bokhylle and Send to my reader equally prominent, with matching
  filled buttons. Place them
  alongside each other on desktop and stack them on mobile. A single file
  selection applies to both actions and to Download inside More.
- Put Shelf, Like and More in one quieter row beneath the reading actions.
  Desktop keeps the three controls together with icons and text; mobile uses
  three equal-width cells across the page, icons with accessible names, clear
  selected states and 48px touch targets.
- Pair a compact cover with the title on mobile so actions remain easy to
  reach. Keep the larger desktop cover beside the title and action area.
- Keep sending discoverable through the book action. Without a saved reader,
  Get & send opens the same reader selection/setup used for available files;
  confirmation starts the download with that destination. Existing readers
  keep their shortcut. Reader setup instructions belong in this dialog and
  Profile, rather than a repeated message in every book's metadata.
- Show actual Private or Shared visibility at the top right of the book header,
  aligned with the Library back link, using a lock
  or household icon with an accessible name and tooltip. Owners open this
  icon to edit sharing in a dialog with an explicit Save action. Editable icons
  have a pointer cursor, a visible hover/focus background, and a tooltip naming
  the action. Borrowers and shared imports have a quieter marker whose tooltip
  explains who controls sharing; tapping it reveals that explanation on mobile.
- Show a sharing marker only for actual library visibility. Keep the default
  for future additions in Profile settings.
- When an adult chooses Show available versions, opening an undownloaded book
  loads a read-only preview and proposes one confidently matched file. Keep its
  compact filename, format, language, size, source and availability in the details
  column beside the cover on desktop, and full width below book information on
  mobile. Change version opens a dedicated dialog on desktop or a scrolling sheet
  on mobile; choosing Use this version returns to the book without downloading.
  Get or Get & send starts the acquisition. Cancelling preserves the previous
  choice. Match identity before ranking formats and availability: reject clear
  author/title/volume conflicts, omit rejected rows from ordinary choices and
  label incomplete identity as Possible match. Possible matches require deliberate
  review and never become automatic recommendations. With no confident result,
  show the review action instead of proposing an arbitrary file; with no matches,
  show a neutral empty state. Missing download setup is a neutral status with Set
  up downloading for administrators and an explanation for other adults.
  Temporary failures offer Try again beside the message. Automatic selection keeps
  the normal Get action and asks for review when identity is uncertain. Existing
  files remain the focus on downloaded books; Find another version stays under More.
- Preserve catalogue artwork while a book is acquired and extract embedded
  artwork before marking its file ready. Discover and Library use the same
  durable cover after a book gets a local identity. Manual cover corrections
  remain authoritative.
- Catalogue language captions name editions (for example, English editions).
  Downloaded books show their file's language. Catalogue descriptions are
  independent provider text; an edition language never describes the language
  of that text. Preserve the supplied description when no verified replacement
  for the same book is available.
- Keep subjects in the header beneath the author and metadata, above download
  status and actions. Preserve the small uppercase catalogue styling. Let the
  subject row span the full header width on mobile. Show three on mobile and six
  on desktop, with a compact +N more control in the row and an accessible label.
- Use plain text and spacing for download failures, followed by Try again and
  the Activity link. The retry section has no decorative vertical rule.
- Pending books show their actual acquisition state and progress, a route to
  Activity and actions appropriate to the requesting profile. Send when ready
  schedules that profile's chosen reader; show the destination on the page.
  An existing file stays readable and sendable while another version is added.
- More opens a desktop popover or mobile sheet. Download, collections,
  additional versions and imports live there; administration is labelled and
  destructive actions retain their confirmation dialogs.

## Profile marks

- Profiles start with initials. A mark is chosen deliberately, never inferred
  from a name, gender, or profile type.
- Adults and children share ten library and nature illustrations: fox, owl, cat,
  bear, whale, book, tree, mountain, moon, and leaf. Use the bundled SVG assets
  in `frontend/public/profile-marks/` with their stable IDs.
- Keep distinct silhouettes, restrained accents, and the same neutral circular
  surface in Paper and Ink. These are household identity marks, not role badges.
- An uploaded personal photo takes precedence. Keep the selected mark so removing
  a photo restores it; no mark means initials.

## Palette

Paper is canonical: cream paper, near-black ink, oxide (burnt orange),
moss and wine as supporting tones. Ink keeps the same hues at dark
lightness with contrast-checked text. Success/warning/danger are semantic
only.

## Type

- Display/editorial: **Newsreader** (variable).
- UI, catalogue numbers and metadata: **IBM Plex Sans** (variable),
  small-caps/tracking for meta.
- Numerals in section marks are tabular.

Use the serif selectively for featured titles and headings. Navigation,
controls, and supporting metadata stay clear and quick to scan.

## Reviewing new work

Any component that violates these principles should be challenged rather
than automatically matching an older generic pattern.
