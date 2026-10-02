import { useEffect, useState, type FormEvent } from 'react'
import { Link, useLocation, useNavigate, useParams } from 'react-router-dom'
import { ArrowLeft, BookOpen, ChevronDown, Download, Search, Send } from 'lucide-react'
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
import { Button, ButtonAnchor, ButtonLink } from '../components/ui/Button'
import { useAuth } from '../auth/useAuth'
import { useMutation } from '../lib/useMutation'
import { descriptionText } from '../lib/descriptionText'
import { BookAdminActions, DeleteBookFileButton } from './book-detail/BookAdminActions'
import { CollectionsManager } from './book-detail/CollectionsManager'
import { DeliveryHistory } from './book-detail/DeliveryHistory'
import { HouseholdAccess } from './book-detail/HouseholdAccess'
import { BookSharingSettings } from './book-detail/BookSharingSettings'
import { BookSharingChoice, type BookSharing } from '../components/BookSharingChoice'

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
  const [linkSharing, setLinkSharing] = useState<BookSharing | null>(null)
  const [linkBusy, setLinkBusy] = useState(false)
  const [linkError, setLinkError] = useState<string | null>(null)
  const shelfMutation = useMutation()
  const versionMutation = useMutation()

  function getAnotherVersion() {
    void versionMutation.run('version', () => createAcquisitionForBook(id, { askBeforeDownload: true }), 'Could not find other versions', (acquisition) => {
      navigate(`/activity?choose=${encodeURIComponent(acquisition.id)}`)
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
      await createHttpAcquisitionForBook(id, linkUrl.trim(), linkFormat === 'auto' ? undefined : linkFormat, linkSharing ?? book?.sharing ?? user?.defaultBookSharing ?? 'shared')
      navigate('/activity')
    } catch (caught) {
      setLinkError(caught instanceof ApiError ? caught.message : 'Could not add this link')
    } finally {
      setLinkBusy(false)
    }
  }

  function refreshBook() {
    fetchBook(id)
      .then(setBook)
      .catch((caught: unknown) => {
        setError(caught instanceof ApiError ? caught.message : 'Failed to load book')
      })
  }

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
  const primaryFile = book.files[0]
  const readableFiles = book.files.filter((file) => file.format === 'epub' || file.format === 'pdf' || file.format === 'cbz')
  const readerFile = readableFiles.find((file) => file.id === book.browserFileId) ?? readableFiles[0]
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
        <div className="relative mx-auto flex max-w-content flex-col gap-8 px-4 pb-12 pt-8 sm:flex-row sm:items-start sm:px-6 lg:px-8">
          <div className="relative mx-auto aspect-[2/3] w-36 shrink-0 sm:mx-0 sm:w-52">
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

          <div className="min-w-0 flex-1">
            <Link
              to="/library"
              className="inline-flex items-center gap-1.5 text-xs font-medium text-ink-soft transition-colors hover:text-ink"
            >
              <ArrowLeft size={14} />
              Library
            </Link>

            <MetaLine
              className="mt-5"
              tone="soft"
              items={[
                book.rating && book.ratingCount && book.ratingCount >= 10
                  ? `★ ${book.rating.toFixed(1)} · ${
                      book.ratingCount >= 1000
                        ? `${(book.ratingCount / 1000).toFixed(1)}k`
                        : String(book.ratingCount)
                    } ratings`
                  : null,
                book.publicationYear ? String(book.publicationYear) : null,
                book.language?.toUpperCase() ?? availabilityLabel,
                book.series
                  ? `${book.series}${book.seriesNumber ? ` · ${book.publicationKind === 'comic' || book.publicationKind === 'manga' ? 'Volume' : 'Book'} ${book.seriesNumber}` : ''}`
                  : null,
                book.files.length > 0
                  ? book.files.map((file) => file.format.toUpperCase()).join(' + ')
                  : null,
              ]}
            />

            <h1 className="mt-3 font-display text-display text-ink [text-wrap:balance]">
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

            {book.subjects.length > 0 && (
              <div className="mt-6 flex flex-wrap items-center gap-x-4 gap-y-1.5">
                {(showAllSubjects ? book.subjects : book.subjects.slice(0, 8)).map((subject) => (
                  <Link
                    key={subject.normalized}
                    to={`/library?subject=${encodeURIComponent(subject.normalized)}`}
                    className="font-sans text-[11px] uppercase tracking-[0.18em] text-ink-soft transition-colors hover:text-ink"
                  >
                    {subject.name}
                  </Link>
                ))}
                {book.subjects.length > 8 && (
                  <button
                    type="button"
                    onClick={() => setShowAllSubjects((current) => !current)}
                    className="font-sans text-[11px] font-medium uppercase tracking-[0.18em] text-ink-soft transition-colors hover:text-ink"
                  >
                    {showAllSubjects ? 'Fewer subjects' : `+${book.subjects.length - 8} more`}
                  </button>
                )}
              </div>
            )}

            <div className="mt-7 space-y-4">
              <div role="group" aria-label="Reading actions" className="grid gap-2 [&_svg]:shrink-0 sm:flex sm:flex-wrap sm:items-center sm:gap-3">
              {readerFile && (
                <ButtonLink to={`/read/${book.id}/${readerFile.id}`} variant="primary" size="lg">
                  <BookOpen size={16} aria-hidden />Read in Bokhylle
                </ButtonLink>
              )}
              {primaryFile && !isChild && demo && (
                <>
                <Button variant="primary" size="lg" disabled={demoBusy || (!book.onShelf && (demoPending === null || demoPending))} onClick={() => book.onShelf ? setDemoSendOpen(true) : void tryDemoGet(true)}>
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
              <div role="group" aria-label="Book files" className="divide-y divide-ink/15 [&_button]:min-h-11 [&_button]:min-w-11 sm:flex sm:flex-wrap sm:gap-x-4 sm:divide-y-0">
              {book.files.map((file) => (
                <div key={file.id} className="flex min-w-0 items-center gap-2 py-1">
                  {readableFiles.length > 1 && file.id !== readerFile?.id && (
                    <ButtonLink to={`/read/${book.id}/${file.id}`} variant="ghost" size="md" className="shrink-0 px-2" >
                      Read {file.filename || `${file.format.toUpperCase()} ${file.id}`}
                    </ButtonLink>
                  )}
                  {isChild ? (
                    <span className="font-sans text-xs uppercase tracking-[0.12em] text-ink-soft">
                      {file.format.toUpperCase()} · {formatBytes(file.size)}
                    </span>
                  ) : (
                    <>
                      <ButtonAnchor
                        href={downloadUrl(book.id, file.id)}
                        variant="ghost"
                        size="md"
                        className="h-auto min-h-11 flex-1 justify-start px-3 py-2 [&_svg]:shrink-0"
                      >
                        <Download size={16} />
                        <span className="min-w-0 text-left">
                          <span className="block">Download {file.format.toUpperCase()}</span>
                          <span className="block text-xs font-normal text-ink-muted">{formatBytes(file.size)}</span>
                        </span>
                      </ButtonAnchor>
                      {isAdmin && <DeleteBookFileButton bookId={book.id} file={file} onDeleted={refreshBook} />}
                    </>
                  )}
                </div>
              ))}
              </div>
              <div role="group" aria-label="Manage book" className="flex flex-wrap items-center gap-2 border-t border-ink/15 pt-3 [&_button]:min-h-11">
              {!isChild && !demo && <CollectionsManager bookId={book.id} onError={setError} />}
              {!isChild && !demo && <BookSharingSettings book={book} onUpdated={refreshBook} />}
              {!isChild && !demo && (isAdmin || user?.canAcquire) && (
                <>
                {primaryFile && <Button variant="ghost" size="sm" disabled={!!versionMutation.busyKey} onClick={getAnotherVersion}>{versionMutation.busyKey ? 'Finding versions…' : 'Get another version'}</Button>}
                <Button variant="ghost" size="sm" onClick={() => setLinkOpen((open) => !open)} aria-expanded={linkOpen}>Add from link</Button>
                </>
              )}
              {isAdmin && <BookAdminActions book={book} onUpdated={refreshBook} />}
              {!isChild && demo && !isAdmin && <HouseholdAccess bookId={book.id} />}
              </div>
              {versionMutation.error && <p role="alert" className="text-sm text-danger">{versionMutation.error}</p>}
              {linkOpen && !isChild && !demo && (
                <form onSubmit={(event) => void addFromLink(event)} className="max-w-2xl space-y-3 border-t border-line pt-4">
                  <p className="text-sm text-ink-muted">Add a direct EPUB, PDF, or CBZ download for this book.</p>
                  <Field label="Download URL"><Input type="url" required value={linkUrl} onChange={(event) => setLinkUrl(event.target.value)} placeholder="https://example.org/book.epub" /></Field>
                  <BookSharingChoice value={linkSharing ?? book.sharing ?? user?.defaultBookSharing ?? 'shared'} onChange={setLinkSharing} disabled={linkBusy} />
                  <div className="flex flex-wrap items-end gap-3">
                    <Field label="Format"><Select value={linkFormat} onChange={(event) => setLinkFormat(event.target.value)}><option value="auto">From URL</option><option value="epub">EPUB</option><option value="pdf">PDF</option><option value="cbz">CBZ</option></Select></Field>
                    <Button type="submit" variant="primary" disabled={linkBusy || !linkUrl.trim()}>{linkBusy ? 'Adding…' : 'Get this file'}</Button>
                  </div>
                  {linkError && <p role="alert" className="text-sm text-danger">{linkError}</p>}
                </form>
              )}
            </div>

            {demoNotice && <p role="status" className="mt-3 border-l-2 border-accent pl-3 text-sm text-ink-soft">{demoNotice}</p>}
            {demo && demoPending && !book.onShelf && <Link to="/activity" className="mt-3 inline-block text-sm text-accent">Follow in Activity</Link>}

            {primaryFile && !isChild && !demo && (
              <p className="mt-3 text-xs text-ink-soft">
                Delivered by email to your reader.
                {targets.some((target) => target.type === 'kindle') &&
                  ' Kindle addresses must be approved in your Amazon account.'}
              </p>
            )}

            <div className={`mt-6 gap-2 border-t border-line pt-4 [&>button]:min-h-11 ${isChild ? 'flex' : 'grid grid-cols-2 sm:flex sm:flex-wrap sm:items-center'}`}>
              {!isChild &&
                (book.onShelf ? (
                  <button
                    type="button"
                    disabled={shelfMutation.busyKey === 'shelf'}
                    onClick={() =>
                      void shelfMutation.run(
                        'shelf',
                        () => removeBookFromShelf(book.id),
                        'Could not remove it from your shelf',
                        refreshBook,
                      )
                    }
                    className="col-span-2 rounded-[3px] border border-accent bg-accent/10 px-3 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] text-ink transition-colors hover:bg-accent/15 disabled:opacity-50"
                  >
                    Remove from my shelf
                  </button>
                ) : !demo ? (
                  <button
                    type="button"
                    disabled={shelfMutation.busyKey === 'shelf'}
                    onClick={() =>
                      void shelfMutation.run(
                        'shelf',
                        () => addBookToShelf(book.id),
                        'Could not add it to your shelf',
                        refreshBook,
                      )
                    }
                    className="col-span-2 rounded-[3px] border border-line px-3 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] text-ink-soft transition-colors hover:border-ink-faint hover:text-ink disabled:opacity-50"
                  >
                    + Add to my shelf
                  </button>
                ) : null)}
              <button
                type="button"
                aria-pressed={book.preference === 'liked'}
                onClick={() =>
                  void shelfMutation.run(
                    'like',
                    () =>
                      setBookPreference(book.id, book.preference === 'liked' ? null : 'liked'),
                    'Could not update that like',
                    refreshBook,
                  )
                }
                className={`rounded-[3px] border px-3 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors ${
                  book.preference === 'liked'
                    ? 'border-accent bg-accent/10 text-ink'
                    : 'border-line text-ink-soft hover:border-ink-faint hover:text-ink'
                }`}
              >
                ♥ {book.preference === 'liked' ? 'Liked' : 'Like'}
              </button>
              {!isChild && (
                <button
                  type="button"
                  aria-pressed={book.preference === 'not_for_me'}
                  onClick={() =>
                    void shelfMutation.run(
                      'not-for-me',
                      () =>
                        setBookPreference(
                          book.id,
                          book.preference === 'not_for_me' ? null : 'not_for_me',
                        ),
                      'Could not update that preference',
                      refreshBook,
                    )
                  }
                  className={`rounded-[3px] border px-3 py-1.5 font-sans text-[11px] font-medium uppercase tracking-[0.14em] transition-colors ${
                    book.preference === 'not_for_me'
                      ? 'border-accent bg-accent/10 text-ink'
                      : 'border-line text-ink-soft hover:border-ink-faint hover:text-ink'
                  }`}
                >
                  Not for me
                </button>
              )}
            </div>
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
