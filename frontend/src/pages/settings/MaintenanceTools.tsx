import { useEffect, useState } from 'react'
import { ApiError } from '../../api/client'
import {
  type ImageJobStatus,
  type ImportJobStatus,
  type MetadataJobStatus,
  cancelMetadataJob,
  fetchImageJob,
  fetchImportJob,
  fetchMetadataJob,
  startImageJob,
  startImportJob,
  startMetadataJob,
} from '../../api/maintenance'
import { type IntegrityReport, fetchIntegrity } from '../../api/users'
import { Button } from '../../components/ui/Button'

export function MaintenanceTools({
  kind,
}: {
  kind: 'library' | 'getting-books' | 'metadata'
}) {
  const [integrity, setIntegrity] = useState<IntegrityReport | null>(null)
  const [imageJob, setImageJob] = useState<ImageJobStatus | null>(null)
  const [metadataJob, setMetadataJob] = useState<MetadataJobStatus | null>(null)
  const [importJob, setImportJob] = useState<ImportJobStatus | null>(null)
  const [busy, setBusy] = useState<'integrity' | 'images' | 'metadata' | 'imports' | null>(
    null,
  )
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (kind === 'metadata') {
      fetchImageJob()
        .then(setImageJob)
        .catch((caught: unknown) => console.warn('settings.image_job_failed', caught))
      fetchMetadataJob()
        .then(setMetadataJob)
        .catch((caught: unknown) => console.warn('settings.metadata_job_failed', caught))
    }
    if (kind === 'getting-books') {
      fetchImportJob()
        .then(setImportJob)
        .catch((caught: unknown) => console.warn('settings.import_job_failed', caught))
    }
  }, [kind])

  useEffect(() => {
    if (!imageJob?.running) {
      return
    }
    const timer = setInterval(() => {
      fetchImageJob()
        .then(setImageJob)
        .catch(() => {})
    }, 2000)
    return () => clearInterval(timer)
  }, [imageJob?.running])

  useEffect(() => {
    if (!importJob?.running) {
      return
    }
    const timer = setInterval(() => {
      fetchImportJob()
        .then(setImportJob)
        .catch(() => {})
    }, 2000)
    return () => clearInterval(timer)
  }, [importJob?.running])

  useEffect(() => {
    if (!metadataJob?.running) {
      return
    }
    const timer = setInterval(() => {
      fetchMetadataJob()
        .then(setMetadataJob)
        .catch(() => {})
    }, 2000)
    return () => clearInterval(timer)
  }, [metadataJob?.running])

  async function runImageRefresh() {
    setBusy('images')
    setError(null)
    try {
      setImageJob(await startImageJob())
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not start the refresh')
    } finally {
      setBusy(null)
    }
  }

  async function runImportDownloads() {
    setBusy('imports')
    setError(null)
    try {
      setImportJob(await startImportJob())
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not start the import')
    } finally {
      setBusy(null)
    }
  }

  async function runMetadataEnrich(force = false) {
    setBusy('metadata')
    setError(null)
    try {
      setMetadataJob(await startMetadataJob(force))
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not start enrichment')
    } finally {
      setBusy(null)
    }
  }

  async function cancelEnrichment() {
    try {
      await cancelMetadataJob()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not cancel enrichment')
    }
  }

  async function runIntegrity() {
    setBusy('integrity')
    setError(null)
    try {
      setIntegrity(await fetchIntegrity())
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Integrity check failed')
    } finally {
      setBusy(null)
    }
  }

  return (
    <section className="border-t border-line pt-6">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold text-ink">
            {kind === 'library'
              ? 'File integrity'
              : kind === 'getting-books'
                ? 'Completed downloads'
                : 'Enrichment'}
          </h2>
          <p className="mt-0.5 text-xs text-ink-faint">
            {kind === 'library'
              ? 'Check that indexed files are still present.'
              : kind === 'getting-books'
                ? 'Import completed downloads that have not entered the library yet.'
                : 'Refresh covers, authors, and missing book information.'}
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          {kind === 'library' && (
            <Button variant="secondary" size="sm" disabled={busy !== null} onClick={() => void runIntegrity()}>
              {busy === 'integrity' ? 'Checking…' : 'Check library'}
            </Button>
          )}
          {kind === 'getting-books' && (
            <Button variant="secondary" size="sm" disabled={busy !== null || importJob?.running === true} onClick={() => void runImportDownloads()}>
              {importJob?.running ? 'Importing…' : 'Import downloads'}
            </Button>
          )}
          {kind === 'metadata' && (
            <>
              <Button variant="secondary" size="sm" disabled={busy !== null || imageJob?.running === true} onClick={() => void runImageRefresh()}>
                {imageJob?.running ? 'Refreshing…' : 'Refresh images'}
              </Button>
              <Button variant="secondary" size="sm" disabled={busy !== null || metadataJob?.running === true} onClick={() => void runMetadataEnrich()}>
                {metadataJob?.running ? 'Enriching…' : 'Enrich metadata'}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                disabled={busy !== null || metadataJob?.running === true}
                onClick={() => {
                  if (window.confirm('Force re-check re-queries every book against the metadata providers. Continue?')) {
                    void runMetadataEnrich(true)
                  }
                }}
                title="Re-check every book, including ones already marked as exhausted"
              >
                Force re-check
              </Button>
              {metadataJob?.running && (
                <Button variant="ghost" size="sm" onClick={() => void cancelEnrichment()}>
                  Cancel
                </Button>
              )}
            </>
          )}
        </div>
      </div>

      {error && (
        <p className="mt-4 rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-danger">{error}</p>
      )}

      {integrity && (
        <div className="mt-4 rounded-card bg-surface-2 px-4 py-3 text-sm">
          <p className="text-ink-soft">
            {integrity.filesChecked} files checked · {integrity.missingCount} missing ·{' '}
            {integrity.booksWithoutFiles} books without files
          </p>
          {integrity.missing.length > 0 && (
            <ul className="mt-2 space-y-1 text-xs text-ink-faint">
              {integrity.missing.slice(0, 10).map((entry) => (
                <li key={entry.fileId} className="truncate">
                  {entry.path}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {imageJob && (imageJob.running || imageJob.finishedAt !== null) && (
        <div role="status" className="mt-4 rounded-card bg-surface-2 px-4 py-3 text-sm text-ink-soft">
          <p>
            {imageJob.running
              ? 'Refreshing images…'
              : `Last refresh: ${imageJob.coversFetched} covers fetched, ${imageJob.coversFailed} failed · ${imageJob.authorsResolved} authors resolved, ${imageJob.authorsFailed} failed · ${imageJob.booksRepaired} books repaired`}
          </p>
          {imageJob.error && <p className="mt-1 text-xs text-danger">{imageJob.error}</p>}
        </div>
      )}

      {importJob && (importJob.running || importJob.finishedAt !== null) && (
        <div role="status" className="mt-4 rounded-card bg-surface-2 px-4 py-3 text-sm text-ink-soft">
          <p>
            {importJob.running
              ? 'Importing completed downloads…'
              : `Last import: ${importJob.imported} imported, ${importJob.already} already in library, ${importJob.skipped} skipped, ${importJob.failed} failed`}
          </p>
          {importJob.files.length > 0 && (
            <p className="mt-1 text-xs text-ink-faint">{importJob.files.join(' · ')}</p>
          )}
          {importJob.failures.length > 0 && (
            <ul className="mt-1 space-y-1 text-xs text-ink-faint">
              {importJob.failures.map((failure) => (
                <li key={failure.name}>
                  <span className="text-ink-soft">{failure.name}</span> · {failure.reason}
                </li>
              ))}
            </ul>
          )}
          {importJob.error && <p className="mt-1 text-xs text-danger">{importJob.error}</p>}
        </div>
      )}

      {metadataJob && (metadataJob.running || metadataJob.finishedAt !== null) && (
        <div role="status" className="mt-4 rounded-card bg-surface-2 px-4 py-3 text-sm text-ink-soft">
          <p>
            {metadataJob.running
              ? 'Enriching metadata…'
              : `${metadataJob.cancelled ? 'Enrichment cancelled' : 'Last enrichment'}: ${metadataJob.booksEnriched} books enriched, ${metadataJob.booksFailed} without subjects`}
          </p>
          {metadataJob.failures.length > 0 && (
            <details className="mt-2">
              <summary className="cursor-pointer text-xs text-ink-muted transition-colors hover:text-ink">
                {metadataJob.failures.length >= 50
                  ? 'Showing the first 50 books without subjects'
                  : `${metadataJob.failures.length} books without subjects`}
              </summary>
              <ul className="mt-2 max-h-48 space-y-1 overflow-y-auto text-xs text-ink-faint">
                {metadataJob.failures.map((failure) => (
                  <li key={failure.bookId}>
                    <span className="text-ink-soft">{failure.title}</span> · {failure.reason}
                  </li>
                ))}
              </ul>
            </details>
          )}
          {metadataJob.error && <p className="mt-1 text-xs text-danger">{metadataJob.error}</p>}
        </div>
      )}

    </section>
  )
}
