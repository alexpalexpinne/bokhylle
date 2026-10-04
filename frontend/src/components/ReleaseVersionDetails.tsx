import { availabilityLabel, formatBytes } from '../api/acquisitions'

type VersionDetails = {
  releaseName?: string | null
  format: string | null
  language: string | null
  sizeBytes: number
  method: string
  seeders?: number | null
  indexer?: string | null
  isCollection?: boolean
  recommended?: boolean
  needsReview?: boolean
}

export function ReleaseVersionDetails({ release, compact = false }: { release: VersionDetails; compact?: boolean }) {
  const availability = availabilityLabel(release.seeders, release.method)
  return <span className="block min-w-0 space-y-1 [overflow-wrap:anywhere]">
    <span className={`block text-sm font-medium text-ink [overflow-wrap:anywhere] ${compact ? 'line-clamp-2' : ''}`}>
      {release.releaseName || 'Unnamed release'}
    </span>
    <span className="block text-xs text-ink-muted">
      {release.recommended && !release.needsReview && <span className="mr-2 font-medium text-accent-strong">Recommended</span>}
      {release.needsReview && <span className="mr-2 font-medium text-warning">Possible match</span>}
      {(release.format ?? 'Unknown format').toUpperCase()} · {release.language?.toUpperCase() ?? 'Language unknown'} · {formatBytes(release.sizeBytes)}
      {release.isCollection && ' · Collection'}
    </span>
    <span className="block text-xs text-ink-muted">
      {release.method === 'nzb' ? 'Usenet' : release.method === 'http' ? 'Direct download' : 'Torrent'}
      {release.indexer && ` · ${release.indexer}`}
      {' · '}<span className={availability.className}>{availability.label}</span>
      {release.method === 'torrent' && typeof release.seeders === 'number' && ` · ${release.seeders} seeders`}
    </span>
  </span>
}
