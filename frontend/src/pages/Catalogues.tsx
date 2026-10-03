import { useEffect, useState, type FormEvent } from 'react'
import { Link, useNavigate } from 'react-router-dom'
import { ArrowLeft, ArrowRight, Search } from 'lucide-react'

import { ApiError } from '../api/client'
import {
  type CatalogFeed,
  type CatalogSource,
  acquireCatalogEntry,
  addCatalogSource,
  fetchCatalogFeed,
  fetchCatalogSources,
  removeCatalogSource,
} from '../api/catalogues'
import { useAuth } from '../auth/useAuth'
import { Button } from '../components/ui/Button'
import { Field, Input } from '../components/ui/Field'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'

export function Catalogues() {
  const navigate = useNavigate()
  const { user } = useAuth()
  const isAdmin = user?.role === 'admin'
  const canAcquire = isAdmin || user?.canAcquire === true
  const [sources, setSources] = useState<CatalogSource[]>([])
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [feed, setFeed] = useState<CatalogFeed | null>(null)
  const [query, setQuery] = useState('')
  const [name, setName] = useState('')
  const [url, setUrl] = useState('')
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [acquiring, setAcquiring] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    let cancelled = false
    fetchCatalogSources()
      .then((items) => {
        if (cancelled) return
        setSources(items)
        if (items.length > 0) setLoading(true)
        setSelectedId((current) => current ?? items[0]?.id ?? null)
      })
      .catch((caught: unknown) => {
        if (!cancelled) setError(caught instanceof ApiError ? caught.message : 'Could not load catalogues')
      })
    return () => { cancelled = true }
  }, [])

  useEffect(() => {
    if (!selectedId) return
    let cancelled = false
    fetchCatalogFeed(selectedId)
      .then((page) => { if (!cancelled) setFeed(page) })
      .catch((caught: unknown) => {
        if (!cancelled) setError(caught instanceof ApiError ? caught.message : 'Could not open catalogue')
      })
      .finally(() => { if (!cancelled) setLoading(false) })
    return () => { cancelled = true }
  }, [selectedId])

  function selectSource(id: string | null) {
    if (id === selectedId) return
    setFeed(null)
    setLoading(id !== null)
    setError(null)
    setSelectedId(id)
  }

  async function openPage(options: { url?: string; q?: string }) {
    if (!selectedId) return
    setLoading(true)
    setError(null)
    try {
      setFeed(await fetchCatalogFeed(selectedId, options))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not open catalogue page')
    } finally {
      setLoading(false)
    }
  }

  async function search(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!query.trim()) return
    await openPage({ q: query.trim() })
  }

  async function addSource(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setSaving(true)
    setError(null)
    try {
      const added = await addCatalogSource(name.trim(), url.trim())
      setSources((current) => [...current, added].sort((a, b) => a.name.localeCompare(b.name)))
      setName('')
      setUrl('')
      selectSource(added.id)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not add catalogue')
    } finally {
      setSaving(false)
    }
  }

  async function removeSource(source: CatalogSource) {
    setSaving(true)
    setError(null)
    try {
      await removeCatalogSource(source.id)
      const remaining = sources.filter((item) => item.id !== source.id)
      setSources(remaining)
      if (selectedId === source.id) selectSource(remaining[0]?.id ?? null)
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not remove catalogue')
    } finally {
      setSaving(false)
    }
  }

  async function acquire(entryId: string, fileIndex: number) {
    if (!selectedId || !feed) return
    const key = `${entryId}:${fileIndex}`
    setAcquiring(key)
    setError(null)
    try {
      await acquireCatalogEntry(selectedId, feed.pageUrl, entryId, fileIndex)
      navigate('/activity')
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not get this book')
    } finally {
      setAcquiring(null)
    }
  }

  const selected = sources.find((source) => source.id === selectedId)

  return (
    <section className="space-y-10">
      <PageHeader
        eyebrow="Discover · Catalogues"
        title="Browse book catalogues"
        description="Choose a catalogue, explore its shelves, and add a downloadable edition to Bokhylle."
        actions={<Link to="/discover" className="inline-flex items-center gap-1 text-sm text-accent hover:text-accent-strong"><ArrowLeft size={15} /> Book search</Link>}
      />

      {error && <p role="alert" className="border-l-2 border-danger pl-3 text-sm text-danger">{error}</p>}

      {sources.length > 0 && (
        <div className="flex flex-wrap gap-x-5 gap-y-2 border-b border-line pb-3">
          {sources.map((source) => (
            <button
              key={source.id}
              type="button"
              onClick={() => { selectSource(source.id); setQuery('') }}
              aria-current={selectedId === source.id ? 'page' : undefined}
              className={`border-b-2 pb-2 text-sm font-medium ${selectedId === source.id ? 'border-accent text-ink' : 'border-transparent text-ink-muted hover:text-ink'}`}
            >
              {source.name}
            </button>
          ))}
        </div>
      )}

      {selected && (
        <div className="space-y-8">
          <div className="flex flex-wrap items-end justify-between gap-4">
            <div>
              <p className="text-xs uppercase tracking-[0.18em] text-ink-faint">{selected.name}</p>
              <h2 className="mt-1 font-display text-3xl text-ink">{feed?.title ?? 'Opening catalogue…'}</h2>
            </div>
            <Button size="sm" variant="ghost" disabled={loading} onClick={() => void openPage({})}>Catalogue home</Button>
          </div>

          {feed?.searchAvailable && (
            <form onSubmit={(event) => void search(event)} className="flex max-w-xl gap-2">
              <Input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search this catalogue" aria-label="Search this catalogue" />
              <Button variant="primary" type="submit" disabled={loading || !query.trim()}><Search size={16} /> Search</Button>
            </form>
          )}

          {loading && <p role="status" className="text-sm text-ink-muted">Opening catalogue…</p>}

          {!!feed?.navigation.length && (
            <section>
              <SectionMark number="01" title="Browse" />
              <div className="mt-4 divide-y divide-line">
                {feed.navigation.map((link, index) => (
                  <button key={`${link.url}:${index}`} type="button" disabled={loading} onClick={() => void openPage({ url: link.url })} className="flex w-full items-center justify-between gap-3 py-3 text-left text-sm text-ink hover:text-accent disabled:opacity-50">
                    {link.title}<ArrowRight size={16} aria-hidden />
                  </button>
                ))}
              </div>
            </section>
          )}

          {!!feed?.entries.length && (
            <section>
              <SectionMark number={feed.navigation.length ? '02' : '01'} title="Books" />
              {canAcquire && <p className="mt-4 text-xs text-ink-muted">New additions will be {user?.defaultBookSharing === 'private' ? 'private' : 'shared with the household'}.</p>}
              <div className="mt-4 divide-y divide-line">
                {feed.entries.map((entry, index) => (
                  <article key={`${entry.id}:${index}`} className="grid gap-3 py-5 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-center">
                    <div>
                      <h3 className="font-display text-xl text-ink">{entry.title}</h3>
                      <p className="mt-1 text-sm text-ink-muted">{entry.authors.join(', ') || 'Author unknown'}{entry.language ? ` · ${entry.language.toUpperCase()}` : ''}</p>
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {entry.files.map((file) => (
                        <Button key={file.index} size="sm" variant="secondary" disabled={!canAcquire || acquiring !== null} onClick={() => void acquire(entry.id, file.index)} title={file.label}>
                          {acquiring === `${entry.id}:${file.index}` ? 'Getting…' : `Get ${file.format.toUpperCase()}`}
                        </Button>
                      ))}
                    </div>
                  </article>
                ))}
              </div>
            </section>
          )}

          {feed?.next && <Button variant="ghost" disabled={loading} onClick={() => void openPage({ url: feed.next! })}>Next page <ArrowRight size={16} /></Button>}
          {feed && !feed.navigation.length && !feed.entries.length && <p className="text-sm text-ink-muted">This page has no downloadable books or sections.</p>}
        </div>
      )}

      {sources.length === 0 && <p className="text-sm text-ink-muted">No catalogues have been added yet.</p>}

      {isAdmin && (
        <section className="border-t border-line pt-8">
          <SectionMark number="03" title="Manage catalogues" />
          <p className="mt-3 max-w-2xl text-sm text-ink-muted">Add an OPDS 1.x or 2.0 feed. Public feeds and catalogues on your private network are supported; Bokhylle imports direct EPUB, PDF, and CBZ links.</p>
          <form onSubmit={(event) => void addSource(event)} className="mt-5 grid max-w-2xl gap-3 sm:grid-cols-[1fr_2fr_auto] sm:items-end">
            <Field label="Name"><Input value={name} onChange={(event) => setName(event.target.value)} required maxLength={80} placeholder="Project Gutenberg" /></Field>
            <Field label="OPDS feed URL"><Input value={url} onChange={(event) => setUrl(event.target.value)} required type="url" placeholder="https://www.gutenberg.org/ebooks/search.opds/" /></Field>
            <Button variant="primary" type="submit" disabled={saving || !name.trim() || !url.trim()}>Add catalogue</Button>
          </form>
          {sources.length > 0 && <div className="mt-5 flex flex-wrap gap-x-5 gap-y-2">{sources.map((source) => <button key={source.id} type="button" disabled={saving} onClick={() => void removeSource(source)} className="text-xs text-ink-muted underline hover:text-danger">Remove {source.name}</button>)}</div>}
        </section>
      )}
    </section>
  )
}
