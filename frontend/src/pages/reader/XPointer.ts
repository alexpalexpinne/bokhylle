import { EpubCFI } from 'epubjs'
import type Section from 'epubjs/types/section'

// KOReader's CREngine uses one-based DocFragment and sibling indices; text
// offsets count collapsed whitespace. EPUB CFI uses the spine and DOM offsets.
const fragment = /^\/body\/DocFragment(?:\[(\d+)\])?\/body(?:\/(.*))?$/
const whitespace = /[ \t\r\n\f]/
const blocks = new Set('body div p h1 h2 h3 h4 h5 h6 li ul ol blockquote section article table tr td th'.split(' '))

export function xpointerSpineIndex(locator: string): number {
  const match = fragment.exec(locator)
  if (!match) throw new Error('Invalid KOReader locator')
  const index = Number(match[1] ?? 1)
  if (!Number.isSafeInteger(index) || index < 1) throw new Error('Invalid KOReader spine index')
  return index - 1
}

export function cfiSpineIndex(locator: string): number {
  const index = new EpubCFI(locator).spinePos
  if (!Number.isSafeInteger(index) || index < 0) throw new Error('Invalid EPUB spine index')
  return index
}

function textChildren(element: Element): Text[] {
  const nodes = Array.from(element.childNodes)
  return nodes.filter((node, index): node is Text => node.nodeType === Node.TEXT_NODE &&
    (node as Text).data.length > 0 &&
    !(index === 0 && blocks.has(element.localName.toLowerCase()) && !(node as Text).data.trim()))
}

function offsetToRaw(text: string, collapsed: number): number {
  let count = 0
  for (let index = 0; index < text.length; index++) {
    if (index > 0 && whitespace.test(text[index]) && whitespace.test(text[index - 1])) continue
    if (count === collapsed) return index
    count++
  }
  return text.length
}

function offsetToCollapsed(text: string, raw: number): number {
  let count = 0
  for (let index = 0; index < Math.min(raw, text.length); index++) {
    if (index > 0 && whitespace.test(text[index]) && whitespace.test(text[index - 1])) continue
    count++
  }
  return count
}

function preserveWhitespace(node: Node): boolean {
  for (let element = node.parentElement; element; element = element.parentElement) {
    if (['pre', 'code', 'listing', 'textarea'].includes(element.localName.toLowerCase())) return true
  }
  return false
}

function pathFor(element: Element, spineIndex: number, body: Element): string {
  const steps: string[] = []
  for (let current: Element | null = element; current && current !== body; current = current.parentElement) {
    const parent: Element | null = current.parentElement
    if (!parent) throw new Error('Element is outside the EPUB body')
    const siblings = Array.from(parent.children).filter((child) => child.localName.toLowerCase() === current!.localName.toLowerCase())
    const index = siblings.indexOf(current)
    if (index < 0) throw new Error('Element is outside the EPUB body')
    steps.unshift(`${current.localName.toLowerCase()}${siblings.length > 1 ? `[${index + 1}]` : ''}`)
  }
  return `/body/DocFragment[${spineIndex + 1}]/body${steps.length ? `/${steps.join('/')}` : ''}`
}

export function cfiToXPointer(section: Section, locator: string): string {
  if (cfiSpineIndex(locator) !== section.index || !section.document?.body) throw new Error('CFI spine mismatch')
  const range = new EpubCFI(locator).toRange(section.document)
  if (!range) throw new Error('CFI cannot be resolved')
  let node: Node = range.startContainer
  let offset = range.startOffset
  if (node.nodeType === Node.ELEMENT_NODE) {
    const child = node.childNodes[offset]
    if (child) { node = child; offset = 0 }
  }
  if (node.nodeType === Node.TEXT_NODE) {
    const parent = node.parentElement
    if (!parent) throw new Error('Text is outside the EPUB body')
    const children = textChildren(parent)
    const index = children.indexOf(node as Text)
    if (index < 0) return pathFor(parent, section.index, section.document.body)
    const at = preserveWhitespace(node) ? offset : offsetToCollapsed((node as Text).data, offset)
    return `${pathFor(parent, section.index, section.document.body)}/text()${children.length > 1 ? `[${index + 1}]` : ''}.${at}`
  }
  if (node.nodeType !== Node.ELEMENT_NODE) throw new Error('CFI has no element anchor')
  return pathFor(node as Element, section.index, section.document.body)
}

export function xPointerToCfi(section: Section, locator: string): string {
  if (xpointerSpineIndex(locator) !== section.index || !section.document?.body) throw new Error('XPointer spine mismatch')
  const match = fragment.exec(locator)!
  const segments = (match[2] ?? '').split('/').filter(Boolean)
  const last = segments.at(-1) ?? ''
  let textIndex: number | null = null
  let textOffset: number | null = null
  const textMatch = /^text\(\)(?:\[(\d+)\])?\.(\d+)$/.exec(last)
  if (textMatch) {
    segments.pop()
    textIndex = Number(textMatch[1] ?? 1)
    textOffset = Number(textMatch[2])
  } else {
    const elementOffset = /^(.*)\.(\d+)$/.exec(last)
    if (elementOffset) {
      segments[segments.length - 1] = elementOffset[1]
      textIndex = 1
      textOffset = Number(elementOffset[2])
    }
  }
  let element: Element = section.document.body
  for (const segment of segments) {
    const step = /^([a-zA-Z][\w-]*)(?:\[(\d+)\])?$/.exec(segment)
    if (!step) throw new Error('Invalid XPointer element')
    const siblings = Array.from(element.children).filter((child) => child.localName.toLowerCase() === step[1].toLowerCase())
    const next = siblings[Number(step[2] ?? 1) - 1]
    if (!next) throw new Error('XPointer element is absent')
    element = next
  }
  const range = section.document.createRange()
  if (textIndex !== null && textOffset !== null) {
    const node = textChildren(element)[textIndex - 1]
    if (!node) throw new Error('XPointer text is absent')
    range.setStart(node, preserveWhitespace(node) ? Math.min(textOffset, node.data.length) : offsetToRaw(node.data, textOffset))
  } else {
    range.setStart(element, 0)
  }
  range.collapse(true)
  return section.cfiFromRange(range)
}
