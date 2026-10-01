import {
  type ConnectionTest,
  testProwlarr,
  testTorznab,
  testNewznab,
  testSabnzbd,
  testQbittorrent,
  testSmtp,
} from '../../api/settings'

type FieldKind = 'text' | 'number' | 'boolean' | 'select' | 'secret'

export type Field = {
  key: string
  label: string
  kind: FieldKind
  placeholder?: string
  options?: { value: string; label: string }[]
  hint?: string
  defaultValue?: string | number | boolean
  min?: number
  step?: number
}

export type Group = {
  section: SettingsSection
  title: string
  description?: string
  fields: Field[]
  test?: {
    label: string
    run: () => Promise<ConnectionTest>
  }
}

export type SettingsSection =
  | 'overview'
  | 'household'
  | 'library'
  | 'getting-books'
  | 'metadata'
  | 'delivery'
  | 'server'

export const groups: Group[] = [
  {
    section: 'library',
    title: 'Library',
    description: 'Path changes require a restart.',
    fields: [
      { key: 'library.root', label: 'Library root', kind: 'text', hint: 'Restart required' },
      {
        key: 'downloads.dir',
        label: 'Downloads directory',
        kind: 'text',
        hint: 'Restart required',
      },
      { key: 'library.scan_on_startup', label: 'Scan on startup', kind: 'boolean' },
      {
        key: 'library.scan_interval_hours',
        label: 'Scheduled scan (hours)',
        kind: 'number',
        placeholder: '0',
        hint: '0 disables automatic scans',
      },
      { key: 'library.preferred_format', label: 'Preferred format', kind: 'text' },
      { key: 'library.preferred_language', label: 'Preferred language', kind: 'text' },
    ],
  },
  {
    section: 'server',
    title: 'Backups',
    fields: [
      { key: 'backups.interval_hours', label: 'Automatic backup interval (hours)', kind: 'number', defaultValue: 24, min: 0, step: 0.25, hint: '0 disables scheduled database backups.' },
      { key: 'backups.keep', label: 'Backups to keep', kind: 'number', defaultValue: 7, min: 1, step: 1, hint: 'Retention is applied during the next backup attempt.' },
    ],
  },
  {
    section: 'server',
    title: 'Updates',
    fields: [
      { key: 'updates.check_enabled', label: 'Check GitHub releases automatically', kind: 'boolean', defaultValue: true, hint: 'Checks once a day; retries after an hour if unavailable. Bokhylle continues working offline.' },
    ],
  },
  {
    section: 'server',
    title: 'Security',
    fields: [
      {
        key: 'security.secure_cookies',
        label: 'Secure cookies',
        kind: 'boolean',
        hint: 'Enable when serving over HTTPS',
      },
      {
        key: 'security.trusted_proxy',
        label: 'Trust proxy headers',
        kind: 'boolean',
        hint: 'Only enable behind a reverse proxy: uses X-Forwarded-For for rate limiting',
      },
    ],
  },
  {
    section: 'getting-books',
    title: 'Search source',
    description: 'Automatic prefers an existing Prowlarr connection, then Torznab, then Newznab.',
    fields: [{
      key: 'integrations.indexer.provider',
      label: 'Active source',
      kind: 'select',
      options: [
        { value: 'auto', label: 'Automatic' },
        { value: 'prowlarr', label: 'Prowlarr' },
        { value: 'torznab', label: 'Torznab' },
        { value: 'newznab', label: 'Newznab' },
      ],
    }],
  },
  {
    section: 'getting-books',
    title: 'Prowlarr',
    fields: [
      { key: 'integrations.prowlarr.url', label: 'URL', kind: 'text' },
      { key: 'integrations.prowlarr.api_key', label: 'API key', kind: 'secret' },
    ],
    test: { label: 'Test Prowlarr', run: testProwlarr },
  },
  {
    section: 'getting-books',
    title: 'Torznab',
    description: 'Connect Jackett or a Torznab-compatible torrent indexer. Use its API endpoint, without the API key in the URL.',
    fields: [
      { key: 'integrations.torznab.url', label: 'Torznab API URL', kind: 'text', placeholder: 'http://jackett:9117/api/v2.0/indexers/all/results/torznab/api' },
      { key: 'integrations.torznab.api_key', label: 'API key', kind: 'secret' },
      { key: 'integrations.torznab.categories', label: 'Categories', kind: 'text', placeholder: '7000', hint: 'Comma-separated category IDs. Leave empty to search all categories.' },
    ],
    test: { label: 'Test Torznab', run: testTorznab },
  },
  {
    section: 'getting-books',
    title: 'Newznab',
    description: 'Search a Newznab-compatible Usenet indexer. SABnzbd must also be configured to retrieve matches.',
    fields: [
      { key: 'integrations.newznab.url', label: 'Newznab API URL', kind: 'text', placeholder: 'http://indexer:5076/api' },
      { key: 'integrations.newznab.api_key', label: 'API key', kind: 'secret' },
      { key: 'integrations.newznab.categories', label: 'Categories', kind: 'text', placeholder: '7000', hint: 'Comma-separated category IDs; empty searches all categories.' },
    ],
    test: { label: 'Test Newznab', run: testNewznab },
  },
  {
    section: 'getting-books',
    title: 'SABnzbd',
    description: 'Completed jobs must appear inside the configured downloads directory in Bokhylle.',
    fields: [
      { key: 'integrations.sabnzbd.url', label: 'SABnzbd API URL', kind: 'text', placeholder: 'http://sabnzbd:8080/api' },
      { key: 'integrations.sabnzbd.api_key', label: 'API key', kind: 'secret' },
      { key: 'integrations.sabnzbd.category', label: 'Category', kind: 'text', placeholder: 'books-app' },
    ],
    test: { label: 'Test SABnzbd', run: testSabnzbd },
  },
  {
    section: 'metadata',
    title: 'Metadata',
    description:
      'Metadata and ratings are chosen separately: Google-only metadata still uses Open Library for ratings unless you change the ratings source. Restart required.',
    fields: [
      {
        key: 'metadata.provider',
        label: 'Metadata provider',
        kind: 'select',
        hint: 'Restart required · Automatic uses Open Library primarily and Google Books to fill gaps',
        options: [
          { value: 'automatic', label: 'Automatic — recommended' },
          { value: 'openlibrary', label: 'Open Library only' },
          { value: 'google_books', label: 'Google Books only' },
        ],
      },
      {
        key: 'ratings.provider',
        label: 'Ratings provider',
        kind: 'select',
        hint: 'Restart required · one source per score; never averaged',
        options: [
          { value: 'same_as_metadata', label: 'Same as metadata' },
          { value: 'openlibrary', label: 'Open Library' },
          { value: 'google_books', label: 'Google Books' },
          { value: 'disabled', label: 'Disabled' },
        ],
      },
      {
        key: 'integrations.google_books.api_key',
        label: 'Google Books API key',
        kind: 'secret',
        hint: 'Optional; raises the Google quota for this instance',
      },
    ],
  },
  {
    section: 'getting-books',
    title: 'qBittorrent',
    fields: [
      { key: 'integrations.qbittorrent.url', label: 'URL', kind: 'text' },
      {
        key: 'integrations.qbittorrent.api_key',
        label: 'API key',
        kind: 'secret',
        hint: 'Preferred; leave empty to use username/password',
      },
      { key: 'integrations.qbittorrent.username', label: 'Username', kind: 'text' },
      { key: 'integrations.qbittorrent.password', label: 'Password', kind: 'secret' },
      {
        key: 'integrations.qbittorrent.category',
        label: 'Category',
        kind: 'text',
        placeholder: 'books-app',
      },
    ],
    test: { label: 'Test qBittorrent', run: testQbittorrent },
  },
  {
    section: 'getting-books',
    title: 'Imports',
    fields: [
      {
        key: 'imports.watch_enabled',
        label: 'Watch an import folder',
        kind: 'boolean',
        hint: 'Import settled EPUB, PDF, and CBZ files placed in the folder',
      },
      {
        key: 'imports.watch_folder',
        label: 'Watch folder',
        kind: 'text',
        hint: 'Absolute path visible to Bokhylle; defaults to /config/ingest in Docker',
      },
      {
        key: 'imports.strategy',
        label: 'Import strategy',
        kind: 'select',
        hint: 'Hardlink keeps seeding without using extra space (same filesystem required)',
        options: [
          { value: 'hardlink', label: 'Hardlink (default)' },
          { value: 'move', label: 'Move (removes the downloaded file)' },
          { value: 'copy', label: 'Copy (uses extra space)' },
        ],
      },
      {
        key: 'imports.cleanup_downloads',
        label: 'Remove downloads after import',
        kind: 'boolean',
        hint: 'Deletes the torrent and its data after a successful import',
      },
    ],
  },
  {
    section: 'getting-books',
    title: 'Keep looking',
    description: 'Automatically retry requests Bokhylle could not find or fetch yet.',
    fields: [
      { key: 'retries.enabled', label: 'Automatically keep looking', kind: 'boolean' },
      {
        key: 'retries.max_days',
        label: 'Stop after (days)',
        kind: 'number',
        placeholder: '60',
        hint: '0 keeps looking without a time limit',
      },
    ],
  },
  {
    section: 'delivery',
    title: 'E-reader delivery',
    description: 'SMTP server used to email books to Kindle, PocketBook, and other email-capable readers.',
    fields: [
      { key: 'smtp.host', label: 'SMTP host', kind: 'text' },
      { key: 'smtp.port', label: 'SMTP port', kind: 'number', placeholder: '587' },
      { key: 'smtp.username', label: 'SMTP username', kind: 'text' },
      { key: 'smtp.password', label: 'SMTP password', kind: 'secret' },
      { key: 'smtp.from', label: 'Sender address', kind: 'text' },
      {
        key: 'smtp.tls',
        label: 'TLS mode',
        kind: 'select',
        options: [
          { value: 'starttls', label: 'STARTTLS (default)' },
          { value: 'tls', label: 'Implicit TLS' },
          { value: 'none', label: 'None' },
        ],
      },
      {
        key: 'delivery.kindle_address',
        label: 'Default Kindle address',
        kind: 'text',
        hint: 'Used when a user has no personal destination',
      },
      {
        key: 'delivery.max_attachment_mb',
        label: 'Max attachment (MB)',
        kind: 'number',
        placeholder: '25',
      },
      {
        key: 'delivery.amazon_domain',
        label: 'Amazon domain',
        kind: 'text',
        placeholder: 'amazon.com',
        hint: 'Used for the "approved sender" help link (e.g. amazon.de)',
      },
    ],
    test: { label: 'Test SMTP', run: testSmtp },
  },
]
