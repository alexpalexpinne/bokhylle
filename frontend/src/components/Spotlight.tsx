import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { Link, useLocation } from 'react-router-dom'
import { ArrowRight, ChevronLeft, ChevronRight } from 'lucide-react'
import { type SpotlightItem, authorList, coverUrl } from '../api/library'
import { discoverCoverUrl } from '../api/discover'
import { useRecommendationImpression } from '../lib/useRecommendationImpressions'
import { heroBlurb } from '../lib/blurb'
import { BookCover } from './BookCover'
import { BrandMark } from './BrandMark'
import { ButtonLink } from './ui/Button'
import { MetaLine } from './ui/MetaLine'
import { useAuth } from '../auth/useAuth'
import { ShelfDecoration, ShelfSurface } from './ShelfStructure'

const ROTATION_MS = 10_000

function ownershipLabel(item: SpotlightItem): string | null {
  if (item.ownership === 'shelf') {
    return null
  }
  if (item.ownership === 'household') {
    return 'In household library'
  }
  return null
}

const GENERIC_SUBJECTS = new Set([
  'fiction',
  'general',
  'literary collections',
  'juvenile fiction',
  'american fiction',
  'english fiction',
])

const SUBJECT_ALIASES: Record<string, string> = {
  'fantasy fiction': 'Fantasy',
  'science fiction': 'Science Fiction',
  'historical fiction': 'Historical',
  'detective and mystery stories': 'Mystery',
  'adventure stories': 'Adventure',
  'epic fiction': 'Epic',
}

function presentationSubjects(subjects: string[]): string[] {
  const seen = new Set<string>()
  const result: string[] = []
  for (const raw of subjects) {
    // This is provider collection membership, not a genre or our endorsement.
    if (/^(?:open[\s_-]*library(?:['’]s)?[\s_-]+)?staff[\s_-]+picks$/i.test(raw.trim())) continue
    const subject = homeLabel(raw)
    const key = subject.toLowerCase()
    if (!key || GENERIC_SUBJECTS.has(key)) {
      continue
    }
    const label = SUBJECT_ALIASES[key] ?? subject.replace(/ fiction$/i, '')
    const normalized = label.toLowerCase()
    if (seen.has(normalized)) {
      continue
    }
    seen.add(normalized)
    result.push(label)
    if (result.length === 2) {
      break
    }
  }
  return result
}

function homeLabel(value: string): string {
  return value
    .replace(/\bopen[\s_-]*library(?:['’]s)?[\s_-]*/gi, '')
    .replace(/\bstaff[\s_-]+picks\b/gi, 'Staff picks')
    .trim()
}

function displayTitle(title: string): string {
  const beforeSubtitle = title.split(':')[0].split(' (')[0].trim()
  return beforeSubtitle.length >= 8 ? beforeSubtitle : title
}

export function Spotlight({ items, preferredLanguages }: { items: SpotlightItem[]; preferredLanguages: string[] }) {
  const location = useLocation()
  const { user, demo } = useAuth()
  const isChild = user?.profileType === 'child'
  // Child and shared demo profiles cannot edit appearance; retain manual browsing.
  const canConfigure = !isChild && !demo
  const sectionRef = useRef<HTMLElement>(null)
  const coverRef = useRef<HTMLAnchorElement>(null)
  const copyRef = useRef<HTMLDivElement>(null)
  const [index, setIndex] = useState(0)
  const [navigation, setNavigation] = useState(0)
  const [focused, setFocused] = useState(false)
  const [touching, setTouching] = useState(false)
  const [hovered, setHovered] = useState(false)
  const [wideCover, setWideCover] = useState<string | null>(null)
  const [visible, setVisible] = useState(false)
  const [pageVisible, setPageVisible] = useState(() => !document.hidden)
  const [dialogOpen, setDialogOpen] = useState(false)
  const [reducedMotion, setReducedMotion] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  const rotating = canConfigure && items.length > 1 && (user?.spotlightRotation ?? true) && !focused && !touching && !hovered && !reducedMotion && visible && pageVisible && !dialogOpen

  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)')
    const motionChanged = () => setReducedMotion(media.matches)
    const visibilityChanged = () => setPageVisible(!document.hidden)
    media.addEventListener('change', motionChanged)
    document.addEventListener('visibilitychange', visibilityChanged)
    const observer = new IntersectionObserver(([entry]) => setVisible(entry.isIntersecting && entry.intersectionRatio >= 0.25), { threshold: 0.25 })
    if (sectionRef.current) observer.observe(sectionRef.current)
    // Home stays mounted behind discovery sheets and other dialogs.
    const dialogsChanged = () => setDialogOpen(Boolean(document.querySelector('[role="dialog"], dialog[open]')))
    const dialogs = new MutationObserver((records) => {
      // Ignore cover, text and shelf updates; only inspect added/removed dialogs.
      if (records.some((record) => [...record.addedNodes, ...record.removedNodes].some((node) => node instanceof Element && (node.matches('[role="dialog"], dialog') || node.querySelector('[role="dialog"], dialog'))))) dialogsChanged()
    })
    dialogs.observe(document.body, { childList: true, subtree: true })
    dialogsChanged()
    return () => {
      media.removeEventListener('change', motionChanged)
      document.removeEventListener('visibilitychange', visibilityChanged)
      observer.disconnect()
      dialogs.disconnect()
    }
  }, [])

  useEffect(() => {
    if (!rotating) return
    const timer = window.setInterval(() => setIndex((current) => (current + 1) % items.length), ROTATION_MS)
    return () => window.clearInterval(timer)
  }, [rotating, items.length, navigation])

  useLayoutEffect(() => {
    const copy = copyRef.current
    const main = copy?.querySelector<HTMLElement>('.spotlight-copy-main')
    const heading = copy?.querySelector<HTMLElement>('.spotlight-heading')
    const summary = copy?.querySelector<HTMLElement>('.spotlight-summary')
    const description = copy?.querySelector<HTMLElement>('.spotlight-description')
    if (!copy || !main || !heading || !summary || !description) return
    // Fit copy above the fixed action row; only budget for existing metadata.
    const fitExcerpt = () => {
      if (!window.matchMedia('(min-width: 375px) and (max-width: 899px)').matches) {
        copy.style.removeProperty('--spotlight-description-lines')
        return
      }
      const lineHeight = Number.parseFloat(getComputedStyle(description).lineHeight)
      const gap = Number.parseFloat(getComputedStyle(summary).marginTop)
      const metadata = copy.querySelector<HTMLElement>('.spotlight-metadata')
      const metadataSpace = metadata ? metadata.getBoundingClientRect().height + Number.parseFloat(getComputedStyle(metadata).marginTop) : 0
      const available = main.clientHeight - heading.getBoundingClientRect().height - gap - metadataSpace
      const maxLines = window.matchMedia('(max-width: 639px)').matches ? 5 : 4
      const lines = Math.max(2, Math.min(maxLines, Math.floor(available / lineHeight)))
      copy.style.setProperty('--spotlight-description-lines', String(lines))
    }
    const observer = new ResizeObserver(fitExcerpt)
    observer.observe(main)
    observer.observe(heading)
    fitExcerpt()
    return () => observer.disconnect()
  }, [index, items])

  const impressionRef = useRecommendationImpression(items[index % items.length]?.recommendationKey)
  if (items.length === 0) return null
  const activeIndex = index % items.length
  const item = items[activeIndex]
  const cover = item.bookId
    ? coverUrl(item.bookId)
    : item.coverId
      ? `${discoverCoverUrl(item.coverId, item.title, item.provider ?? undefined)}&size=large`
      : null
  const to = item.cta === 'explore' && item.bookId
    ? `/library/${item.bookId}`
    : `/discover?provider=${encodeURIComponent(item.provider ?? 'openlibrary')}&providerKey=${encodeURIComponent(item.providerKey ?? '')}`
  const linkState = item.cta === 'explore' || isChild ? undefined : { backgroundLocation: location }
  const language = item.languages?.find((value) => preferredLanguages.includes(value))
    ?? (item.languages?.length === 1 ? item.languages[0] : item.language)
  const rating = item.rating && item.ratingCount && item.ratingCount >= 10
    ? `★ ${item.rating.toFixed(1)} · ${item.ratingCount >= 1000 ? `${(item.ratingCount / 1000).toFixed(1)}k` : item.ratingCount} ratings`
    : null
  const metadata = [rating, presentationSubjects(item.subjects).join(' · ') || null, language && language !== 'en' ? language.toUpperCase() : null]
  const ownership = isChild ? null : ownershipLabel(item)
  const decorated = (user?.shelfDecorations ?? true) && !(cover && wideCover === cover)

  function go(offset: number) {
    setIndex((activeIndex + offset + items.length) % items.length)
    setNavigation((current) => current + 1)
  }

  return (
    <section
      ref={sectionRef}
      aria-label="Spotlight"
      aria-roledescription="carousel"
      className="spotlight-layout"
      onFocusCapture={(event) => setFocused(event.target.matches(':focus-visible'))}
      onBlurCapture={(event) => { if (!event.currentTarget.contains(event.relatedTarget)) setFocused(false) }}
      onKeyDownCapture={() => setFocused(true)}
      onPointerEnter={(event) => { if (event.pointerType !== 'touch') setHovered(true) }}
      onPointerLeave={() => setHovered(false)}
      onPointerDownCapture={(event) => { setFocused(false); if (event.pointerType === 'touch') setTouching(true) }}
      onPointerUpCapture={() => setTouching(false)}
      onPointerCancel={() => setTouching(false)}
    >
      <div ref={impressionRef} className="spotlight-display" data-decorated={decorated}>
        <Link ref={coverRef} to={to} state={linkState} aria-label={`View ${item.title}`} viewTransition className="spotlight-cover">
          {cover ? (
            <BookCover key={cover} src={cover} loading="eager" fetchPriority="high" style={{ viewTransitionName: 'book-cover' }} onReady={() => {
              const image = coverRef.current?.querySelector('img')
              setWideCover(image && image.naturalWidth / image.naturalHeight > 1.2 ? cover : null)
            }} />
          ) : (
            <span aria-hidden className="flex items-center justify-center rounded-[3px] bg-surface-2">
              <BrandMark className="h-16 w-16 text-ink-faint" />
            </span>
          )}
        </Link>
        <span className="spotlight-upright"><ShelfSurface upright /></span>
        {decorated && <ShelfDecoration />}
        <ShelfSurface />
      </div>
      <div ref={copyRef} className="spotlight-copy min-w-0">
        <div className="spotlight-copy-main">
          <div className="spotlight-heading">
            <p className="spotlight-reason line-clamp-2 text-xs font-medium text-accent">{homeLabel(item.reasonLabel)}</p>
            <h2 className="spotlight-title mt-3 line-clamp-3 font-display text-ink [text-wrap:balance]">{displayTitle(item.title)}</h2>
            <p className="spotlight-author mt-2 line-clamp-2 text-sm leading-5 text-ink-soft sm:text-base sm:leading-6">{authorList(item.authors)}</p>
          </div>
          <div className="spotlight-summary min-w-0">
            <p className="spotlight-description line-clamp-3 max-w-prose text-sm text-ink-soft sm:text-base">{item.blurb ? heroBlurb(item.blurb) : ''}</p>
            {metadata.some(Boolean) && <div className="spotlight-metadata mt-3">
              <div className="h-4 overflow-hidden"><MetaLine items={metadata} tone="soft" /></div>
            </div>}
          </div>
        </div>
        <div className="spotlight-footer min-w-0">
          <div className="spotlight-actions flex items-center gap-3 sm:gap-4">
            <ButtonLink to={to} state={linkState} variant="primary" size="md">
              {item.cta === 'explore' || isChild ? 'Explore book' : 'Check availability'}
              <ArrowRight size={15} aria-hidden />
            </ButtonLink>
            {ownership && <span className="h-4 truncate text-xs text-ink-muted">{ownership}</span>}
          </div>
        </div>
      </div>
      <div className="spotlight-controls h-11">
        {items.length > 1 && (
          <nav aria-label="Spotlight books" className="flex h-full items-center gap-1 text-ink-muted">
            <button type="button" aria-label="Previous spotlight book" onClick={() => go(-1)} className="flex h-11 w-11 items-center justify-center hover:text-ink">
              <ChevronLeft size={18} aria-hidden />
            </button>
            <span aria-live={rotating ? 'off' : 'polite'} aria-atomic="true" className="min-w-14 text-center text-xs tabular-nums">
              {String(activeIndex + 1).padStart(2, '0')} / {String(items.length).padStart(2, '0')}
              <span className="sr-only">, {item.title}</span>
            </span>
            <button type="button" aria-label="Next spotlight book" onClick={() => go(1)} className="flex h-11 w-11 items-center justify-center hover:text-ink">
              <ChevronRight size={18} aria-hidden />
            </button>
          </nav>
        )}
      </div>
    </section>
  )
}

export function SpotlightSkeleton() {
  return (
    <div className="spotlight-layout animate-pulse" aria-hidden>
      <div className="spotlight-display" data-decorated="true">
        <div className="spotlight-cover"><span className="rounded-[3px] bg-surface-2" /></div>
        <span className="spotlight-upright"><ShelfSurface upright /></span>
        <ShelfDecoration />
        <ShelfSurface />
      </div>
      <div className="spotlight-copy min-w-0">
        <div className="spotlight-copy-main">
          <div className="spotlight-heading">
            <div className="spotlight-reason h-4 w-4/5 max-w-32 rounded bg-surface-2" />
            <div className="spotlight-title mt-3 h-[2.16em]">
              <div className="h-[0.85em] w-4/5 rounded bg-surface-2" />
              <div className="mt-[0.2em] h-[0.85em] w-3/5 rounded bg-surface-2" />
            </div>
            <div className="spotlight-author mt-2 h-5 w-4/5 max-w-40 rounded bg-surface-2 sm:h-6" />
          </div>
          <div className="spotlight-summary min-w-0">
            <div className="spotlight-description space-y-2 text-sm sm:text-base">
              <div className="h-3 w-full rounded bg-surface-2" />
              <div className="h-3 w-4/5 rounded bg-surface-2" />
              <div className="h-3 w-3/5 rounded bg-surface-2" />
            </div>
            <div className="spotlight-metadata mt-3"><div className="h-4 w-28 rounded bg-surface-2" /></div>
          </div>
        </div>
        <div className="spotlight-footer min-w-0">
          <div className="spotlight-actions flex items-center gap-3 sm:gap-4">
            <div className="h-10 w-44 rounded-[3px] bg-surface-2" />
            <div className="h-4 w-28 rounded bg-surface-2" />
          </div>
        </div>
      </div>
      <div className="spotlight-controls h-11" />
    </div>
  )
}
