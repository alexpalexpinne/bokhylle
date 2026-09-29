# Frontend standards

These rules guide new and changed screens. [Design principles](design-principles.md)
define Bokhylle's visual identity; this document defines navigation, forms,
states, and review criteria. Prefer the simplest existing component that meets
the task over a new abstraction.

## Navigation and content

- Organize pages around the person's task, not database keys or service names.
  Keep the main navigation for reading and library tasks; put administrator
  sections inside Settings.
- Give a substantial Settings section its own URL. Use ordinary links with a
  visible current page and browser back behavior. Reserve tabs and segmented
  controls for nearby views of the same task.
- Use a text-based section navigation beside the content on wide screens and
  a labeled section picker on narrow screens. Do not make a long row of
  horizontally scrolling tabs.
- Give each page one clear heading and short purpose. Put actions and feedback
  next to the section they affect. An overview shows status and a route to fix
  it; detailed diagnostics live with the relevant setting.
- Author pages return to the browse view that opened them, preserving its
  search and filters. Use a stable library link when opened directly.
- In book detail sheets, make the available acquisition choice the primary
  button. Show other actions as visible secondary buttons, and expose toggles
  such as Like with a pressed state.
- Use typography, whitespace, and hairline rules for hierarchy. A surface
  should signal interaction or state, following the design principles.
- Use the shared editorial SearchField for page searches, including Books and
  Authors. Keep compact search fields for pickers and dialogs.
- Reserve space between search controls and empty states. Loading controls
  that disappear on an empty result should appear only after the first result
  arrives, so a blank library does not flash irrelevant filters.
- Keep Profile task sections separate: reading preferences, reader delivery,
  taste, integrations, and account security. The overview links to each task.
  Show Format and Language as quick library filters when those facets exist;
  keep Collection, Series, and Subject under More filters.

## Forms and feedback

- Use visible labels and nearby hints. State whether a change takes effect
  immediately, after a restart, or only when an environment override is
  removed. Never imply that an overridden value is active.
- Keep secret values write-only. Show whether a secret is configured and make
  replacement explicit; never display or log the saved value.
- Save within the current section and show pending changes, success, and
  failure there. Warn before navigation discards edits. The current API saves
  one key at a time, so a failed multi-key save must reload persisted values
  and explain that earlier keys may have been saved.
- Keep connection testing distinct from saving. Put its result beside that
  connection, with enough detail to act on a failure.
- Use consistent states: **Not configured**, **Checking**, **Connected**, and
  **Needs attention**. Configuration alone does not mean a service is healthy.
  Use text as well as color to convey status.

## Connections and future providers

- Separate services that **find** releases from clients that **download** them.
  Show each supported connection as a concise row with status and Configure;
  offer Test when it is configured. Show its fields in an editor only when
  needed.
- Initially allow one active download client per protocol. A torrent client
  and a Usenet client may both be active when Usenet support exists. Do not
  add setup controls for unsupported providers.
- Keep provider forms specific to their real settings. Introduce a shared
  provider configuration model when a second implementation demonstrates the
  common needs; the current flat settings API can serve the first UI change.

## Accessibility and responsive behavior

- Use [WCAG 2.2 Level AA](https://www.w3.org/TR/WCAG22/) as the target for
  changed UI. Prefer native links, buttons, inputs, and headings. Section
  navigation is a set of links, not an ARIA tab or menu widget; follow the
  [WAI-ARIA Authoring Practices](https://www.w3.org/WAI/ARIA/apg/patterns/)
  when a true widget is needed.
- Ensure keyboard access, visible focus, explicit field labels, and useful
  status announcements. Preserve reduced-motion behavior.
- Check the changed flow at a narrow mobile width and a desktop width, in
  Paper and Ink. Avoid horizontal page scrolling and keep controls easy to
  tap. Automated accessibility checks supplement keyboard review.

## Settings structure

| Section | Content |
| --- | --- |
| Overview | Concise library and connection status; links to the relevant section |
| Household | Members, roles, and child profiles |
| Library | Storage paths, scanning, file health |
| Getting books | Search sources, download clients, import policy, retries, download path checks |
| Metadata | Metadata and ratings providers, enrichment work |
| Delivery | Email and reader delivery |
| Server | Security, backups, logs, remaining maintenance |

Settings uses section routes and local save behavior with the existing API.
Getting books shows supported services as connection rows, with health checks
beside the settings they describe. The overview links to the relevant section
for details. Design each new client type against this layout when its behavior
is implemented.

## Review of a changed flow

- Run frontend lint and build.
- Exercise the primary action, save and failure states, and navigation with
  unsaved edits in a browser test when those behaviors change.
- Run the existing accessibility scan and manually check keyboard focus,
  narrow layout, and both themes on the changed pages.
