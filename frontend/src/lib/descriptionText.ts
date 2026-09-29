/** Turn provider or EPUB description markup into readable, inert text. */
export function descriptionText(raw: string): string {
  const entities: Record<string, string> = {
    amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ',
    lsquo: '‘', rsquo: '’', ldquo: '“', rdquo: '”', ndash: '–', mdash: '—', hellip: '…',
  }
  return raw
    .replace(/<(script|style)\b[^>]*>[\s\S]*?<\/\1\s*>/gi, '')
    .replace(/<\s*br\s*\/?>/gi, '\n')
    .replace(/<\s*\/\s*(?:p|div|li|h[1-6])\s*>/gi, '\n\n')
    .replace(/<[^>]*>/g, '')
    .replace(/&(#(?:x[0-9a-f]+|\d+)|[a-z]+);/gi, (whole, entity: string) => {
      if (entity.startsWith('#')) {
        const hexadecimal = entity[1]?.toLowerCase() === 'x'
        const value = Number.parseInt(entity.slice(hexadecimal ? 2 : 1), hexadecimal ? 16 : 10)
        return Number.isInteger(value) && value > 0 && value <= 0x10ffff
          ? String.fromCodePoint(value)
          : whole
      }
      return entities[entity.toLowerCase()] ?? whole
    })
    .replace(/[\t ]+\n/g, '\n')
    .replace(/\n[\t ]+/g, '\n')
    .replace(/[\t ]{2,}/g, ' ')
    .replace(/\n{3,}/g, '\n\n')
    .trim()
}
