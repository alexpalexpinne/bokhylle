import { descriptionText } from './descriptionText'

const BLURB_LIMIT = 220

const BOILERPLATE_MARKERS = [
  'this article is about',
  'for other uses',
  '(disambiguation)',
  'no description available',
]

const LEADING_LABELS =
  /^(from (publishers weekly|library journal|kirkus reviews|booklist|wikipedia|amazon\.com|goodreads)|editorial review|amazon\.com review|product description|publisher'?s description|about the book|summary|review|description|from the publisher)\s*[:\-–—]?\s*/i

const REVIEW_SOURCES = [
  'publishers weekly',
  'library journal',
  'kirkus',
  'booklist',
  'amazon.com review',
  'the new york times',
  'washington post',
  'los angeles times',
  'san francisco chronicle',
  'people magazine',
  'entertainment weekly',
]

function stripLeadingPraise(text: string): string {
  let current = text
  for (let round = 0; round < 4; round += 1) {
    const trimmed = current.trim()
    const first = trimmed[0]
    if (first !== '"' && first !== '\u201c' && first !== "'") {
      return trimmed
    }
    const closing = trimmed.slice(1).search(first === "'" ? /['\u2019]/ : /["\u201d]/)
    if (closing === -1) {
      return trimmed
    }
    const quoted = trimmed.slice(1, closing + 1)
    const rest = trimmed.slice(closing + 2)
    const attributed = /^\s*(--|—|–|-)\s*[A-Z]/.test(rest)
    const quotedIsPraise =
      attributed ||
      (quoted.length < 240 &&
        /[A-Z]/.test(quoted) &&
        (quoted === quoted.toUpperCase() || /!/.test(quoted)))
    if (!quotedIsPraise) {
      return trimmed
    }
    const attribution = rest.match(/^\s*(--|—|–|-)\s*[^"“\n]{0,60}/)
    if (attributed && attribution) {
      current = rest.slice(attribution[0].length)
    } else {
      current = rest
    }
  }
  return current.trim()
}

export function heroBlurb(description: string): string | null {
  const readable = descriptionText(description)
  // Contents lists remain readable on the book page; they are not synopses.
  if (/^\s*(?:table of contents|contents)(?:\s*:|\s*\n|\s*$)/im.test(readable)
    || (readable.match(/\s\/\s+[A-Z]/g) ?? []).length >= 3
    || (readable.match(/^\s*(?:[-*]|\d+[.)])\s+.+$/gm) ?? []).length >= 4) return null
  let text = readable
    .replace(/\s+/g, ' ')
    .trim()

  text = text.replace(LEADING_LABELS, '').trim()
  text = text.replace(/_/g, '').replace(/\s+/g, ' ').trim()
  text = stripLeadingPraise(text)
  if (/https?:\/\/|\[[^\]]*\]\(/i.test(text)) return null

  const head = text.slice(0, 120).toLowerCase()
  if (!text || BOILERPLATE_MARKERS.some((marker) => head.includes(marker))) {
    return null
  }

  // Review/testimonial copy is worse than no blurb in a hero: reject pages
  // that are mostly endorsements rather than description.
  const quotes = (text.match(/["“]/g) ?? []).length
  const attributions = (text.match(/(?:--|—|–)\s*[A-Z][a-zA-Z.]+/g) ?? []).length
  const sources = REVIEW_SOURCES.filter((name) => text.toLowerCase().includes(name)).length
  if (text.startsWith('"') || text.startsWith('\u201c')) {
    return null
  }
  if (sources >= 2 || (quotes >= 2 && attributions >= 1) || attributions >= 3) {
    return null
  }

  const sentences = text.match(/[^.!?]+[.!?]+|[^.!?]+$/g) ?? []
  let blurb = ''
  for (const sentence of sentences) {
    const trimmed = sentence.trim()
    if (blurb.length === 0) {
      blurb = trimmed
    } else if (blurb.length + 1 + trimmed.length <= BLURB_LIMIT) {
      blurb = (blurb + ' ' + trimmed).trim()
    } else {
      // Abbreviations like "Walter M." can leave a tiny first sentence;
      // fill up to the limit from the next one instead of giving up.
      if (blurb.length < 60) {
        const room = Math.max(BLURB_LIMIT - blurb.length - 1, 40)
        blurb = (blurb + ' ' + trimmed.slice(0, room)).trim()
      }
      break
    }
    if (blurb.length >= BLURB_LIMIT) {
      break
    }
  }
  if (blurb.length > BLURB_LIMIT) {
    blurb = blurb.slice(0, BLURB_LIMIT).replace(/\s+\S*$/, '') + '…'
  }
  return blurb.length >= 60 ? blurb : null
}
