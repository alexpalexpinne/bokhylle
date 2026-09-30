import { useState } from 'react'
import { createPortal } from 'react-dom'
import { useNavigate } from 'react-router-dom'
import { MoreHorizontal, Trash2, Wrench } from 'lucide-react'
import { ApiError } from '../../api/client'
import {
  type BookDetail,
  type BookUpdate,
  type MetadataField,
  deleteBookAdmin,
  deleteBookFileAdmin,
  updateBookAdmin,
} from '../../api/library'
import { Button } from '../../components/ui/Button'
import { IconButton } from '../../components/ui/IconButton'
import { Modal } from '../../components/ui/Modal'
import { descriptionText } from '../../lib/descriptionText'
import { useMutation } from '../../lib/useMutation'
import { createAdminSeries, fetchAdminSeries, type SeriesRecord } from '../../api/series'
import { HouseholdAccess } from './HouseholdAccess'

const fields = [
  { key: 'title', label: 'Title', wide: true },
  { key: 'authors', label: 'Authors (comma separated)', wide: true },
  { key: 'language', label: 'Language', wide: false },
  { key: 'publicationYear', label: 'Publication year', wide: false },
] as const

export function BookAdminActions({ book, onUpdated }: { book: BookDetail; onUpdated: () => void }) {
  const navigate = useNavigate()
  const [action, setAction] = useState<'edit' | 'delete' | null>(null)
  const [form, setForm] = useState({
    title: '',
    authors: '',
    description: '',
    language: '',
    seriesChoice: '',
    seriesName: '',
    seriesNumber: '',
    seriesSortOrder: '',
    publicationKind: 'unknown',
    publicationYear: '',
    readingDirection: '' as '' | 'ltr' | 'rtl',
  })
  const [initialForm, setInitialForm] = useState(form)
  const [automaticFields, setAutomaticFields] = useState<MetadataField[]>([])
  const [availableSeries, setAvailableSeries] = useState<SeriesRecord[]>([])
  const [seriesError, setSeriesError] = useState<string | null>(null)
  const mutation = useMutation()

  function openEdit() {
    mutation.clearError()
    const publicationYear = book.editions.length ? book.editions[0].publicationYear : book.publicationYear
    const initial = {
      title: book.title,
      authors: book.authors.join(', '),
      description: book.description ? descriptionText(book.description) : '',
      language: book.language ?? '',
      seriesChoice: book.seriesId ? String(book.seriesId) : '',
      seriesName: '',
      seriesNumber: book.seriesNumber ?? '',
      seriesSortOrder: book.seriesSortOrder == null ? '' : String(book.seriesSortOrder),
      publicationKind: book.publicationKind,
      publicationYear: publicationYear == null ? '' : String(publicationYear),
      readingDirection: (book.readingDirection === 'rtl' || book.readingDirection === 'ltr' ? book.readingDirection : '') as '' | 'ltr' | 'rtl',
    }
    setForm(initial)
    setInitialForm(initial)
    setAutomaticFields([])
    setSeriesError(null)
    void fetchAdminSeries().then(setAvailableSeries).catch((caught: unknown) => {
      setSeriesError(caught instanceof ApiError ? caught.message : 'Could not load series')
    })
    setAction('edit')
  }

  function save() {
    void mutation.run(
      'edit',
      async () => {
        const created = form.seriesChoice === 'new'
          ? await createAdminSeries(form.seriesName.trim())
          : null
        const chosen = created?.id ?? (form.seriesChoice && form.seriesChoice !== 'new'
          ? Number(form.seriesChoice) : null)
        const update: BookUpdate = { useAutomaticMetadata: automaticFields }
        if (form.title !== initialForm.title && !automaticFields.includes('title')) update.title = form.title
        if (form.authors !== initialForm.authors && !automaticFields.includes('authors')) {
          update.authors = form.authors.split(',').map((author) => author.trim()).filter(Boolean)
        }
        if (form.description !== initialForm.description && !automaticFields.includes('description')) update.description = form.description
        if (form.language !== initialForm.language && !automaticFields.includes('language')) update.language = form.language
        if (form.seriesChoice !== initialForm.seriesChoice) update.seriesId = chosen
        if (form.seriesNumber !== initialForm.seriesNumber && !automaticFields.includes('seriesNumber')) update.seriesNumber = form.seriesNumber
        if (form.seriesSortOrder !== initialForm.seriesSortOrder) update.seriesSortOrder = form.seriesSortOrder ? Number(form.seriesSortOrder) : null
        if (form.publicationKind !== initialForm.publicationKind) update.publicationKind = form.publicationKind
        if (form.publicationYear !== initialForm.publicationYear && !automaticFields.includes('publicationYear')) update.publicationYear = form.publicationYear ? Number(form.publicationYear) : null
        if (form.readingDirection !== initialForm.readingDirection) update.readingDirection = form.readingDirection || null
        return updateBookAdmin(book.id, update)
      },
      'Could not save the book',
      () => {
        setAction(null)
        onUpdated()
      },
    )
  }

  function sourceHint(field: MetadataField, label: string) {
    const bookSource = book.metadataSources?.find((entry) => entry.field === field)
    const fileEdition = book.editions.find((edition) => edition.id === book.files[0]?.editionId)
    const sources = field === 'publicationYear' ? book.editions[0]?.metadataSources
      : field === 'language' && !bookSource?.manual && fileEdition ? fileEdition.metadataSources
      : book.metadataSources
    const source = sources?.find((entry) => entry.field === field)
    const labels: Record<string, string> = {
      filename: 'Filename', manual: 'Manual correction', unknown: 'Source unknown', mixed: 'Multiple sources',
      openlibrary: 'Open Library', googlebooks: 'Google Books', epub: 'EPUB', pdf: 'PDF', cbz: 'CBZ',
    }
    const resetting = automaticFields.includes(field)
    return (
      <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs text-ink-faint">
        <span>{resetting ? 'Automatic value will be restored on save.' : labels[source?.source ?? 'unknown'] ?? source?.source}</span>
        {source?.manual && (
          <button
            type="button"
            aria-label={`${resetting ? 'Keep manual correction' : 'Use automatic metadata again'} for ${label}`}
            className="min-h-8 text-accent underline underline-offset-2 focus-visible:outline-2 focus-visible:outline-focus"
            onClick={() => setAutomaticFields((current) => resetting ? current.filter((value) => value !== field) : [...current, field])}
          >
            {resetting ? 'Keep manual correction' : 'Use automatic metadata again'}
          </button>
        )}
      </div>
    )
  }

  return (
    <>
      <details className="relative">
        <summary
          aria-label="Book actions"
          className="flex h-10 w-10 cursor-pointer list-none items-center justify-center rounded-[3px] text-ink-soft transition-colors hover:bg-surface-2 hover:text-ink [&::-webkit-details-marker]:hidden"
        >
          <MoreHorizontal size={18} aria-hidden />
        </summary>
        <div className="absolute left-0 z-20 mt-1 w-44 rounded-[3px] bg-surface-2 p-1 shadow-modal">
          <button
            type="button"
            onClick={openEdit}
            className="flex w-full items-center gap-2 rounded-[3px] px-3 py-2 text-left text-sm text-ink-soft transition-colors hover:bg-surface-3 hover:text-ink"
          >
            <Wrench size={14} aria-hidden />
            Fix details
          </button>
          <HouseholdAccess bookId={book.id} menu />
          <button
            type="button"
            onClick={() => {
              mutation.clearError()
              setAction('delete')
            }}
            className="flex w-full items-center gap-2 rounded-[3px] px-3 py-2 text-left text-sm text-danger transition-colors hover:bg-surface-3"
          >
            <Trash2 size={14} aria-hidden />
            Delete book
          </button>
        </div>
      </details>
      {action === 'edit' && createPortal(
        <Modal
          title="Fix details"
          description="Your corrections stay in place through imports and metadata refreshes. Only changed fields are saved."
          onClose={() => setAction(null)}
          wide
          footer={
            <>
              <Button variant="ghost" onClick={() => setAction(null)}>Cancel</Button>
              <Button variant="primary" disabled={mutation.busyKey !== null} onClick={save}>
                {mutation.busyKey ? 'Saving…' : 'Save'}
              </Button>
            </>
          }
        >
          {mutation.error && <p className="mb-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{mutation.error}</p>}
          {seriesError && <p className="mb-4 text-sm text-danger">{seriesError}</p>}
          <div className="grid gap-4 sm:grid-cols-2">
            {fields.map((field) => (
              <div key={field.key} className={`block${field.wide ? ' sm:col-span-2' : ''}`}>
                <label className="block">
                  <span className="mb-1.5 block text-xs text-ink-muted">{field.label}</span>
                  <input
                    type={field.key === 'publicationYear' ? 'number' : 'text'}
                    value={form[field.key]}
                    disabled={automaticFields.includes(field.key)}
                    onChange={(event) => setForm((current) => ({ ...current, [field.key]: event.target.value }))}
                    placeholder={field.key === 'language' ? 'en' : undefined}
                    className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus"
                  />
                </label>
                {sourceHint(field.key, field.label)}
              </div>
            ))}
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Publication type</span>
              <select value={form.publicationKind} onChange={(event) => setForm((current) => ({ ...current, publicationKind: event.target.value }))} className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus">
                <option value="unknown">Unclassified</option>
                <option value="book">Book</option>
                <option value="comic">Comic</option>
                <option value="manga">Manga</option>
                <option value="magazine">Magazine</option>
                <option value="catalogue">Catalogue</option>
              </select>
            </label>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Series</span>
              <select value={form.seriesChoice} onChange={(event) => setForm((current) => ({ ...current, seriesChoice: event.target.value }))} className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus">
                <option value="">No series</option>
                {availableSeries.map((item) => <option key={item.id} value={item.id}>{item.name}</option>)}
                <option value="new">Create a new series…</option>
              </select>
            </label>
            {form.seriesChoice === 'new' && <label className="block sm:col-span-2">
              <span className="mb-1.5 block text-xs text-ink-muted">New series name</span>
              <input value={form.seriesName} onChange={(event) => setForm((current) => ({ ...current, seriesName: event.target.value }))} className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus" />
            </label>}
            {book.legacySeriesText && <p className="text-xs text-ink-muted sm:col-span-2">Imported series text: {book.legacySeriesText}</p>}
            <div className="block">
              <label className="block">
                <span className="mb-1.5 block text-xs text-ink-muted">Volume label</span>
                <input value={form.seriesNumber} disabled={automaticFields.includes('seriesNumber')} onChange={(event) => setForm((current) => ({ ...current, seriesNumber: event.target.value }))} className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus" />
              </label>
              {sourceHint('seriesNumber', 'Volume label')}
            </div>
            <label className="block">
              <span className="mb-1.5 block text-xs text-ink-muted">Volume sort order</span>
              <input type="number" step="any" value={form.seriesSortOrder} onChange={(event) => setForm((current) => ({ ...current, seriesSortOrder: event.target.value }))} placeholder="Automatic for numbers" className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus" />
            </label>
            <label className="block sm:col-span-2">
              <span className="mb-1.5 block text-xs text-ink-muted">Reading direction</span>
              <select value={form.readingDirection} onChange={(event) => setForm((current) => ({ ...current, readingDirection: event.target.value as '' | 'ltr' | 'rtl' }))} className="w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus">
                <option value="">Inherit</option>
                <option value="ltr">Left to right</option>
                <option value="rtl">Right to left</option>
              </select>
            </label>
            <div className="block sm:col-span-2">
              <label htmlFor={`book-${book.id}-description`} className="mb-1.5 block text-xs text-ink-muted">Description</label>
              <textarea
                id={`book-${book.id}-description`}
                value={form.description}
                disabled={automaticFields.includes('description')}
                onChange={(event) => setForm((current) => ({ ...current, description: event.target.value }))}
                rows={4}
                className="w-full resize-y rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus"
              />
              {sourceHint('description', 'Description')}
            </div>
          </div>
        </Modal>,
        document.body,
      )}
      {action === 'delete' && createPortal(
        <Modal
          title={`Delete "${book.title}"?`}
          description="This removes the book and its files from the library. This cannot be undone."
          onClose={() => setAction(null)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setAction(null)}>Cancel</Button>
              <Button
                variant="danger"
                disabled={mutation.busyKey !== null}
                onClick={() => void mutation.run(
                  'delete',
                  () => deleteBookAdmin(book.id),
                  'Could not delete the book',
                  () => navigate('/library'),
                )}
              >
                Delete book
              </Button>
            </>
          }
        >
          {mutation.error && <p className="text-sm text-danger">{mutation.error}</p>}
        </Modal>,
        document.body,
      )}
    </>
  )
}

export function DeleteBookFileButton({ bookId, file, onDeleted }: {
  bookId: number
  file: BookDetail['files'][number]
  onDeleted: () => void
}) {
  const [open, setOpen] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function remove() {
    setBusy(true)
    setError(null)
    try {
      await deleteBookFileAdmin(bookId, file.id)
      setOpen(false)
      onDeleted()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not delete the file')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <IconButton
        label={`Delete ${file.format.toUpperCase()} file`}
        tone="danger"
        onClick={() => {
          setError(null)
          setOpen(true)
        }}
      >
        <Trash2 size={15} aria-hidden />
      </IconButton>
      {open && createPortal(
        <Modal
          title={`Delete the ${file.format.toUpperCase()} file?`}
          description="The file is removed from disk; the book entry stays."
          onClose={() => setOpen(false)}
          footer={
            <>
              <Button variant="ghost" onClick={() => setOpen(false)}>Cancel</Button>
              <Button variant="danger" disabled={busy} onClick={() => void remove()}>Delete file</Button>
            </>
          }
        >
          {error && <p className="text-sm text-danger">{error}</p>}
        </Modal>,
        document.body,
      )}
    </>
  )
}
