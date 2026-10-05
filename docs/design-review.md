# Interaction design review

Reviewed against Bokhylle's [Paper and Ink principles](design-principles.md)
and Emil Kowalski's
[design engineering](https://github.com/emilkowalski/skills/blob/e8a175de22ae1e49370fc144c1f3bb9aeedf988d/skills/emil-design-eng/SKILL.md)
and [mobile guidance](https://github.com/emilkowalski/skills/blob/e8a175de22ae1e49370fc144c1f3bb9aeedf988d/skills/mobile-native/SKILL.md).
The references are pinned to the reviewed revision. They inform interaction
details; Bokhylle's typography, covers, shelves, and palette remain the design
authority.

## Applied changes

| Before | After | Why |
| --- | --- | --- |
| Notification trigger measured 34px high; theme choices measured 28px on phones | 48px targets, wider account menu, scroll containment | Easier tapping and room for short viewports |
| Request information and both decisions competed for one narrow row | Book information above the decisions on phones | Keep the requested book readable |
| Escape dismissed header menus without restoring focus | Focus returns to the relevant trigger | Keep keyboard navigation predictable |
| Small form controls used 12–14px text on touch devices | 16px minimum for small form text; larger search text preserved | Avoid automatic input zoom |
| App shell used the large viewport; safe-area treatment was incomplete | Dynamic viewport, shared safe gutters and header height, padded sheets with or without footers | Keep controls clear of browser chrome and device insets |
| Primary buttons alone provided press feedback; a badge started at 60% scale | Consistent subtle pointer feedback, quieter badge motion, shorter cover transition | Keep motion brief and purposeful |
| Ink browser chrome used a different color from the canvas | Chrome follows the resolved canvas color | Keep the theme continuous |

## Review boundaries

Use fictional fixtures or the isolated demo for visual checks. Check the
account menu, notifications, browsing controls, and dialogs at 320px, 390px,
and desktop widths in Paper and Ink. Include long profile names, long book
titles, short viewports, keyboard use, and reduced motion.

Browser automation verifies geometry, computed styles, focus, theme persistence,
and accessibility. Real iOS and Android devices are still needed to confirm
input zoom, software-keyboard resizing, landscape safe areas, and touch feel.

This pass verified frontend lint and build, 12 browser specs with no skips,
and accessibility checks for the changed menus and dialogs in both themes.
Lint reports eight pre-existing warnings. Short viewports, long names,
immediate reduced-motion cancellation, and sticky Library controls were also
checked. README and website previews were refreshed from fictional fixtures
and a separate demo; phone captures now enable touch-specific styles.

Future changes should keep list navigation immediate, retain native shelf
scrolling, and give any new motion a clear purpose. Introduce an additional
UI library only when an interaction needs behavior the existing shared
components cannot provide.
