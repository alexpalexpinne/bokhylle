import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { Link, useLocation, useNavigate, useParams } from 'react-router-dom'
import { ArrowLeft, BookCheck, BookOpen, BookPlus, ChevronDown, Download, Heart, Search, Send } from 'lucide-react'
import { ApiError } from '../api/client'
import { createAcquisitionForBook, createHttpAcquisitionForBook } from '../api/acquisitions'
import { fetchDemoActivity, startDemoGet } from '../api/demo'
import {
  type BookDetail as BookDetailData,
  type RelatedBooks,
  authorList,
  coverUrl,
  addBookToShelf,
  downloadUrl,
  fetchBook,
  fetchRelatedBooks,
  formatBytes,
  removeBookFromShelf,
  setBookPreference,
} from '../api/library'
import { type DeliveryTarget, fetchTargets } from '../api/delivery'
import { BookCard } from '../components/BookCard'
import { BookCover } from '../components/BookCover'
import { BrandMark } from '../components/BrandMark'
import { BookRail } from '../components/BookRail'
import { CoverField } from '../components/CoverField'
import { SendToReaderDialog } from '../components/SendToReaderDialog'
import { DemoSendDialog } from '../components/DemoSendDialog'
import { MetaLine } from '../components/ui/MetaLine'
import { Field, Input, Select } from '../components/ui/Field'
import { SectionMark } from '../components/ui/SectionMark'
import { Modal } from '../components/ui/Modal'
import { Button, ButtonAnchor, ButtonLink } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'
import { useMutation } from '../lib/useMutation'
import { descriptionText } from '../lib/descriptionText'
import { BookAdminActions } from './book-detail/BookAdminActions'
import { BookActionsMenu } from './book-detail/BookActionsMenu'
import { BookAcquisitionStatus } from './book-detail/BookAcquisitionStatus'
import { CollectionsManager } from './book-detail/CollectionsManager'
import { DeliveryHistory } from './book-detail/DeliveryHistory'
import { HouseholdAccess } from './book-detail/HouseholdAccess'
import { BookSharingSettings } from './book-detail/BookSharingSettings'
import { ReleaseChooserDialog } from '../components/ReleaseChooserDialog'

export function BookDetail({ bookId }: { bookId: string }) {
  const id = Number(bookId)
  const location = useLocation()
  const navigate = useNavigate()
  const { user, demo } = useAuth()
  const isAdmin = user?.role === 'admin'
  const isChild = user?.profileType === 'child'
  const [book, setBook] = useState<BookDetailData | null>(null)
  const [loadedCoverId, setLoadedCoverId] = useState<number | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [related, setRelated] = useState<RelatedBooks | null>(null)
  const [showAllSubjects, setShowAllSubjects] = useState(false)
  const [deliveriesToken, setDeliveriesToken] = useState(0)
  const [targets, setTargets] = useState<DeliveryTarget[]>([])
  const [sendFile, setSendFile] = useState<{ id: number; format: string } | null>(null)
  const [demoNotice, setDemoNotice] = useState<string | null>(null)
  const [demoBusy, setDemoBusy] = useState(false)
  const [demoPending, setDemoPending] = useState<boolean | null>(null)
  const [demoSendOpen, setDemoSendOpen] = useState(false)
  const [linkOpen, setLinkOpen] = useState(false)
  const [linkUrl, setLinkUrl] = useState('')
  const [linkFormat, setLinkFormat] = useState('auto')
  const [selectedFileId, setSelectedFileId] = useState<number | null>(null)
  const [choosingId, setChoosingId] = useState<string | null>(null)
  const [linkBusy, setLinkBusy] = useState(false)
  const [linkError, setLinkError] = useState<string | null>(null)
  const shelfMutation = useMutation()
  const versionMutation = useMutation()

  function getAnotherVersion() {
    void versionMutation.run('version', () => createAcquisitionForBook(id, { askBeforeDownload: true }), 'Could not find other versions', (acquisition) => {
      setChoosingId(acquisition.id)
    })
  }

  async function tryDemoGet(sendWhenReady = false) {
    setDemoBusy(true)
    setDemoNotice(null)
    try {
      await startDemoGet(id, sendWhenReady)
      navigate('/activity')
    } catch (caught) {
      setDemoNotice(caught instanceof ApiError ? caught.message : 'Could not start demo Get')
    } finally {
      setDemoBusy(false)
    }
  }

  async function addFromLink(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setLinkBusy(true)
    setLinkError(null)
    try {
      await createHttpAcquisitionForBook(id, linkUrl.trim(), linkFormat === 'auto' ? undefined : linkFormat)
      navigate('/activity')
    } catch (caught) {
      setLinkError(caught instanceof ApiError ? caught.message : 'Could not add this link')
    } finally {
      setLinkBusy(false)
    }
  }

  const refreshBook = useCallback(() => {
    fetchBook(id)
      .then(setBook)
      .catch((caught: unknown) => {
        setError(caught instanceof ApiError ? caught.message : 'Failed to load book')
      })
  }, [id])

  const refreshAcquisition = useCallback(() => {
    refreshBook()
    setDeliveriesToken((token) => token + 1)
  }, [refreshBook])

  useEffect(() => {
    let cancelled = false

    fetchBook(id)
      .then((data) => {
        if (!cancelled) {
          setBook(data)
        }
      })
      .catch((caught: unknown) => {
        if (!cancelled) {
          setError(caught instanceof ApiError ? caught.message : 'Failed to load book')
        }
      })

    if (!isChild && !demo) {
      fetchTargets()
        .then((items) => {
          if (!cancelled) {
            setTargets(items)
          }
        })
        .catch((caught: unknown) => console.warn('book_detail.read_failed', caught))
    }

    if (demo && !isChild) {
      fetchDemoActivity()
        .then((activity) => {
          if (!cancelled) setDemoPending(activity.gets.some((item) => item.bookId === id && item.status !== 'READY'))
        })
        .catch(() => {
          if (!cancelled) setDemoPending(false)
        })
    }

    if (!isChild) {
      fetchRelatedBooks(id)
        .then((items) => {
          if (!cancelled) {
            setRelated(items)
          }
        })
        .catch((caught: unknown) => console.warn('book_detail.read_failed', caught))
    }

    return () => {
      cancelled = true
    }
  }, [id, isChild, demo])

  if (error) {
    return (
      <div className="space-y-4">
        <Link
          to="/library"
          className="inline-flex items-center gap-1.5 text-sm text-ink-muted transition-colors hover:text-ink"
        >
          <ArrowLeft size={15} />
          Library
        </Link>
        <p className="rounded-card bg-surface px-4 py-3 text-sm text-danger">{error}</p>
      </div>
    )
  }

  if (!book) {
    return (
      <div className="animate-pulse space-y-6">
        <div className="h-72 rounded-panel bg-surface" />
        <div className="h-4 w-2/3 rounded bg-surface" />
        <div className="h-4 w-1/3 rounded bg-surface" />
      </div>
    )
  }

  // The author rail and catalogue availability use the same language lens.
  const preferredLanguages = user?.preferredLanguages?.length
    ? user.preferredLanguages
    : user?.preferredLanguage
      ? [user.preferredLanguage]
      : user?.defaultLanguage
        ? [user.defaultLanguage]
        : []
  const primaryFile = book.files.find((file) => file.id === selectedFileId)
    ?? book.files.find((file) => file.id === book.browserFileId)
    ?? book.files.find((file) => file.format === user?.preferredFormat)
    ?? book.files[0]
  const readerFile = primaryFile
  const availableLanguage = preferredLanguages.find((language) =>
    book.availableLanguages?.includes(language),
  )
  const availabilityLabel = !primaryFile && book.availableLanguages?.length
    ? availableLanguage
      ? `Available in ${availableLanguage.toUpperCase()}`
      : book.availableLanguages.length === 1
        ? `Available in ${book.availableLanguages[0].toUpperCase()}`
        : `Available in ${book.availableLanguages.length} languages`
    : null
  const similarSubject =
    book.subjects.find((subject) => subject.name.includes(' '))?.name ??
    book.subjects[0]?.name ??
    book.title
  const authorRailBooks =
    preferredLanguages.length === 0
      ? (related?.author ?? [])
      : (related?.author ?? []).filter(
          (item) => item.language !== null && preferredLanguages.includes(item.language),
        )

  let railCount = 0
  const nextRail = (present: boolean) =>
    present ? String(++railCount).padStart(2, '0') : undefined
  const seriesNumber = nextRail((related?.series.length ?? 0) > 0)
  const authorNumber = nextRail(authorRailBooks.length > 0)
  const similarNumber = nextRail(book.subjects.length > 0)

  return (
    <article className="space-y-12">
      <section className="relative -mx-4 overflow-hidden sm:-mx-6 lg:-mx-8">
        <CoverField bookId={book.id} />
        <div className="relative mx-auto grid max-w-content grid-cols-[5.5rem_minmax(0,1fr)] items-start gap-x-5 gap-y-3 px-4 py-6 sm:grid-cols-[13rem_minmax(0,1fr)] sm:gap-x-8 sm:px-6 sm:py-8 lg:px-8">
          <div className={`relative aspect-[2/3] w-full ${book.subjects.length > 0 ? 'sm:row-span-3' : 'sm:row-span-2'}`}>
            {loadedCoverId !== book.id && (
              <div aria-hidden className="absolute inset-0 flex items-center justify-center rounded-[3px] bg-surface-2">
                <BrandMark className="h-16 w-16 text-ink-faint" />
              </div>
            )}
            <BookCover
              src={coverUrl(book.id)}
              loading="eager"
              fetchPriority="high"
              onReady={() => setLoadedCoverId(book.id)}
              style={{ viewTransitionName: 'book-cover' }}
              className={`absolute inset-0 h-full w-full rounded-[3px] shadow-lift ${loadedCoverId === book.id ? 'opacity-100' : 'opacity-0'}`}
            />
          </div>

          <div className="min-w-0">
            <div className="flex h-6 items-center justify-between gap-2">
              <Link
                to="/library"
                className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-soft transition-colors hover:text-ink"
              >
                <ArrowLeft size={14} />
                Library
              </Link>
              {!isChild && !demo && <BookSharingSettings book={book} onUpdated={refreshBook} />}
            </div>

            <h1 className="mt-3 font-display text-3xl leading-tight text-ink [overflow-wrap:anywhere] [text-wrap:balance] sm:text-display">
              {book.title}
            </h1>
            {(book.authorRefs ?? []).length > 0 ? (
              <p className="mt-3 text-base text-ink-soft">
                {(book.authorRefs ?? []).map((author, index) => (
                  <span key={author.id}>
                    {index > 0 && ', '}
                    <Link
                      to={`/authors/${author.id}`}
                      state={{ authorReturnTo: `${location.pathname}${location.search}${location.hash}` }}
                      className="transition-colors hover:text-accent"
                    >
                      {author.name}
                    </Link>
                  </span>
                ))}
              </p>
            ) : (
              <p className="mt-3 text-base text-ink-soft">{authorList(book.authors)}</p>
            )}
            {book.seriesId && (book.publicationKind === 'comic' || book.publicationKind === 'manga') && (
              <Link to={`/series/${book.seriesId}${isChild || book.onShelf ? '' : '?scope=household'}`} className="mt-3 inline-block text-sm text-accent hover:text-accent-strong">
                View {book.series ?? 'series'} volumes
              </Link>
            )}

            <MetaLine tone="soft" className="mt-2 break-words" items={[
              book.rating && book.ratingCount && book.ratingCount >= 10
                ? `★ ${book.rating.toFixed(1)} · ${book.ratingCount >= 1000 ? `${(book.ratingCount / 1000).toFixed(1)}k` : String(book.ratingCount)} ratings`
                : null,
              book.publicationYear ? String(book.publicationYear) : null,
              book.language?.toUpperCase() ?? availabilityLabel,
              book.series ? `${book.series}${book.seriesNumber ? ` · ${book.publicationKind === 'comic' || book.publicationKind === 'manga' ? 'Volume' : 'Book'} ${book.seriesNumber}` : ''}` : null,
              primaryFile?.format.toUpperCase() ?? null,
            ]} />
          </div>

          {book.subjects.length > 0 && <div className="col-span-2 flex min-w-0 flex-wrap items-center gap-x-4 gap-y-1.5 sm:col-span-1 sm:col-start-2">
            {book.subjects.slice(0, showAllSubjects ? undefined : 6).map((subject, index) => <Link
              key={subject.normalized} to={`/library?subject=${encodeURIComponent(subject.normalized)}`}
              className={`max-w-full break-words font-sans text-[11px] uppercase tracking-[0.18em] text-ink-soft transition-colors hover:text-ink ${!showAllSubjects && index >= 3 ? 'hidden sm:inline' : ''}`}>
              {subject.name}
            </Link>)}
            {book.subjects.length > 3 && <button type="button" aria-label={showAllSubjects ? 'Show fewer subjects' : 'Show more subjects'} aria-expanded={showAllSubjects} onClick={() => setShowAllSubjects((current) => !current)}
              className={`min-h-11 font-sans text-left text-[11px] font-medium uppercase tracking-[0.18em] text-ink-soft transition-colors hover:text-ink ${book.subjects.length <= 6 ? 'sm:hidden' : ''}`}>
              {showAllSubjects ? 'Fewer subjects' : <>
                <span className="sm:hidden">+{book.subjects.length - 3} more</span>
                <span className="hidden sm:inline">+{book.subjects.length - 6} more</span>
              </>}
            </button>}
          </div>}

          <div className="col-span-2 min-w-0 space-y-4 sm:col-span-1 sm:col-start-2">
            {!isChild && !demo && <BookAcquisitionStatus key={book.id} bookId={book.id} hasFile={!!primaryFile} onReady={refreshAcquisition} onChoose={setChoosingId} />}
            {book.files.length > 1 && !isChild && <Field label="Book file">
              <Select value={primaryFile.id} onChange={(event) => setSelectedFileId(Number(event.target.value))} className="max-w-full sm:max-w-xs">
                {book.files.map((file) => <option key={file.id} value={file.id}>{file.format.toUpperCase()} · {formatBytes(file.size)} · {file.filename}</option>)}
              </Select>
            </Field>}
            <div role="group" aria-label="Reading actions" className="grid gap-2 [&_svg]:shrink-0 sm:flex sm:flex-wrap sm:items-center sm:gap-3">
            {readerFile && (
              <ButtonLink to={`/read/${book.id}/${readerFile.id}`} variant="primary" size="lg" className="sm:min-w-56">
                <BookOpen size={16} aria-hidden />Read in Bokhylle
              </ButtonLink>
            )}
            {primaryFile && !isChild && demo && (
              <>
              <Button variant="primary" size="lg" className="sm:min-w-56" disabled={demoBusy || (!book.onShelf && (demoPending === null || demoPending))} onClick={() => book.onShelf ? setDemoSendOpen(true) : void tryDemoGet(true)}>
                <Send size={16} aria-hidden />
                {demoBusy ? 'Working…' : book.onShelf ? 'Send to Demo Kindle' : demoPending ? 'Getting…' : 'Get & Send to Kindle'}
              </Button>
              {!book.onShelf && !demoPending && <Button variant="secondary" size="lg" disabled={demoBusy || demoPending === null} onClick={() => void tryDemoGet()}>Get for my shelf</Button>}
              </>
            )}
            {primaryFile && !isChild && !demo && (
              <Button
                variant="primary"
                size="lg"
                className="sm:min-w-56"
                onClick={() =>
                  setSendFile({ id: primaryFile.id, format: primaryFile.format })
                }
              >
                <Send size={16} />
                Send to my reader
                {targets.length > 1 && <ChevronDown size={14} />}
              </Button>
            )}
            </div>
            <div role="group" aria-label="Personal book actions" className="flex w-full items-center gap-1 border-y border-line py-1 [&>button]:min-h-12 sm:w-fit sm:gap-2 sm:border-0 sm:py-0 sm:[&>button]:min-h-11">
              {!isChild && (!demo || book.onShelf) && <Button variant="ghost" size="sm"
                aria-label={book.onShelf ? 'Remove from my shelf' : 'Add to my shelf'} aria-pressed={book.onShelf}
                title={book.onShelf ? 'Remove from my shelf' : 'Add to my shelf'}
                className="min-w-0 flex-1 aria-pressed:bg-surface-2 aria-pressed:text-accent sm:flex-none"
                disabled={shelfMutation.busyKey === 'shelf'} onClick={() => void shelfMutation.run(
                'shelf', () => book.onShelf ? removeBookFromShelf(book.id) : addBookToShelf(book.id),
                'Could not update your shelf', refreshBook,
              )}>
                {book.onShelf ? <BookCheck size={20} aria-hidden /> : <BookPlus size={20} aria-hidden />}
                <span className="hidden sm:inline">{book.onShelf ? 'On my shelf' : 'Add to shelf'}</span>
              </Button>}
              <Button variant="ghost" size="sm" aria-label={book.preference === 'liked' ? 'Liked' : 'Like'} aria-pressed={book.preference === 'liked'}
                title={book.preference === 'liked' ? 'Unlike this book' : 'Like this book'}
                className="min-w-0 flex-1 aria-pressed:bg-surface-2 aria-pressed:text-accent sm:flex-none"
                disabled={!!shelfMutation.busyKey} onClick={() => void shelfMutation.run(
                'like', () => setBookPreference(book.id, book.preference === 'liked' ? null : 'liked'),
                'Could not update that like', refreshBook,
              )}><Heart size={20} fill={book.preference === 'liked' ? 'currentColor' : 'none'} aria-hidden /><span className="hidden sm:inline">{book.preference === 'liked' ? 'Liked' : 'Like'}</span></Button>
              {!isChild && <BookActionsMenu>{(close) => <>
                {primaryFile && <ButtonAnchor href={downloadUrl(book.id, primaryFile.id)} variant="ghost" size="sm" className="w-full justify-start" onClick={close}>
                  <Download size={16} aria-hidden />Download {primaryFile.format.toUpperCase()}
                  <span className="ml-auto text-xs font-normal text-ink-muted">{formatBytes(primaryFile.size)}</span>
                </ButtonAnchor>}
                {!demo && <>
                  <CollectionsManager bookId={book.id} onError={setError} onOpen={close} />
                  {(isAdmin || user?.canAcquire) && primaryFile && <Button variant="ghost" size="sm" className="w-full justify-start" disabled={!!versionMutation.busyKey}
                    onClick={() => { close(); getAnotherVersion() }}>Find another version</Button>}
                  <Button variant="ghost" size="sm" className="w-full justify-start" aria-pressed={book.preference === 'not_for_me'} disabled={!!shelfMutation.busyKey} onClick={() => {
                    close()
                    void shelfMutation.run('not-for-me', () => setBookPreference(book.id, book.preference === 'not_for_me' ? null : 'not_for_me'), 'Could not update that preference', refreshBook)
                  }}>{book.preference === 'not_for_me' ? 'Undo not for me' : 'Not for me'}</Button>
                  {(isAdmin || user?.canAcquire) && <div className="border-t border-line pt-1">
                    <Button variant="ghost" size="sm" className="w-full justify-start" onClick={() => { close(); setLinkError(null); setLinkOpen(true) }}>Import from URL</Button>
                  </div>}
                  {isAdmin && <BookAdminActions book={book} onUpdated={refreshBook} onOpen={close} />}
                </>}
              </>}</BookActionsMenu>}
            </div>
            {!isChild && demo && !isAdmin && <HouseholdAccess bookId={book.id} />}
            {versionMutation.error && <p role="alert" className="text-sm text-danger">{versionMutation.error}</p>}
            {demoNotice && <p role="status" className="mt-3 border-l-2 border-accent pl-3 text-sm text-ink-soft">{demoNotice}</p>}
            {demo && demoPending && !book.onShelf && <Link to="/activity" className="mt-3 inline-block text-sm text-accent">Follow in Activity</Link>}
            {shelfMutation.error && (
              <p className="mt-3 border-l-2 border-danger pl-3 text-sm text-danger">
                {shelfMutation.error}
              </p>
            )}
          </div>
        </div>
      </section>

      {book.description && (
        <section className="max-w-3xl">
          <SectionMark title={book.publicationKind === 'comic' || book.publicationKind === 'manga' ? 'About this volume' : 'About this book'} />
          <p className="mt-5 whitespace-pre-line text-[15px] leading-relaxed text-ink-soft">
            {descriptionText(book.description)}
          </p>
        </section>
      )}

      {related && related.series.length > 0 && (
        <BookRail index={seriesNumber} title="More in this series" books={related.series} />
      )}

      {related && authorRailBooks.length > 0 && (
        <BookRail
          index={authorNumber}
          title={`More by ${book.authors[0] ?? 'this author'}`}
          books={authorRailBooks}
        />
      )}

      {!isChild && book.subjects.length > 0 && (
        <section>
          <SectionMark
            number={similarNumber}
            title="Similar in your library"
            action={
              (related?.similar.length ?? 0) < 4 ? (
                <Link
                  to={`/discover?q=${encodeURIComponent(similarSubject)}&type=subject`}
                  className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-muted transition-colors hover:text-ink"
                >
                  <Search size={13} aria-hidden />
                  Find more in Discover
                </Link>
              ) : undefined
            }
          />

          {related && related.similar.length > 0 ? (
            <div className="rail-scroll -mx-4 mt-7 flex snap-x gap-4 overflow-x-auto px-4 pb-2 sm:mx-0 sm:px-0">
              {related.similar.map(({ book: similar, sharedSubjects }) => (
                <div key={similar.id} className="w-32 shrink-0 snap-start sm:w-36 md:w-40">
                  <BookCard book={similar} />
                  <p className="mt-2 line-clamp-2 text-xs leading-snug text-ink-faint">
                    {sharedSubjects.join(' · ')}
                  </p>
                </div>
              ))}
            </div>
          ) : (
            <p className="mt-6 border-l-2 border-line pl-4 text-sm text-ink-muted">
              Nothing similar in your library yet. Use Find more in Discover to browse Open
              Library subjects.
            </p>
          )}
        </section>
      )}

      {(book.editions.length > 1 || book.editions.some((edition) => !edition.unknown)) && (
        <section>
          <SectionMark title="Editions" />
          <div className="mt-2 divide-y divide-line">
            {book.editions.map((edition) => (
              <div key={edition.id} className="grid gap-1.5 py-4 sm:grid-cols-[10rem_1fr]">
                <MetaLine
                  items={[
                    edition.language?.toUpperCase() ?? 'Edition',
                    edition.publicationYear ? String(edition.publicationYear) : null,
                  ]}
                />
                <div className="text-sm text-ink-soft">
                  {edition.isbn13 ??
                    edition.isbn10 ??
                    (edition.unknown ? 'Unknown edition' : '—')}
                  {edition.publisher && (
                    <span className="mt-0.5 block text-xs text-ink-faint">
                      {edition.publisher}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        </section>
      )}

      {!isChild && !demo && <DeliveryHistory bookId={book.id} refreshToken={deliveriesToken} />}

      {choosingId && <ReleaseChooserDialog acquisitionId={choosingId} onClose={() => setChoosingId(null)} onSelected={() => { setChoosingId(null); navigate('/activity') }} />}
      {linkOpen && !isChild && !demo && <Modal title="Import from URL" description="Fetch a direct EPUB, PDF or CBZ file for this book. Use the file URL, rather than a webpage address."
        onClose={() => { if (!linkBusy) setLinkOpen(false) }} footer={<>
          <Button variant="ghost" disabled={linkBusy} onClick={() => setLinkOpen(false)}>Cancel</Button>
          <Button type="submit" form="import-book-url" variant="primary" disabled={linkBusy || !linkUrl.trim()}>{linkBusy ? 'Importing…' : 'Import file'}</Button>
        </>}>
        <form id="import-book-url" onSubmit={(event) => void addFromLink(event)} className="space-y-4">
          <Field label="File URL"><Input type="url" required disabled={linkBusy} value={linkUrl} onChange={(event) => setLinkUrl(event.target.value)} placeholder="https://example.org/book.epub" /></Field>
          <Field label="Format"><Select disabled={linkBusy} value={linkFormat} onChange={(event) => setLinkFormat(event.target.value)}><option value="auto">From URL</option><option value="epub">EPUB</option><option value="pdf">PDF</option><option value="cbz">CBZ</option></Select></Field>
          <p className="text-xs text-ink-muted">{book.sharing ? `Your sharing stays ${book.sharing}.` : `Your addition uses your ${user?.defaultBookSharing ?? 'shared'} sharing preference.`}</p>
          {linkError && <p role="alert" className="text-sm text-danger">{linkError}</p>}
        </form>
      </Modal>}
      {sendFile && (
        <SendToReaderDialog
          bookId={book.id}
          fileId={sendFile.id}
          format={sendFile.format}
          onClose={() => setSendFile(null)}
          onSent={() => setDeliveriesToken((token) => token + 1)}
        />
      )}
      {demoSendOpen && <DemoSendDialog bookId={book.id} title={book.title} onClose={() => setDemoSendOpen(false)} />}
    </article>
  )
}

export function BookDetailRoute() {
  const { bookId } = useParams()
  return <BookDetail key={bookId} bookId={bookId ?? ''} />
}
