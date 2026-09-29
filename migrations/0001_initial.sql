-- Initial Bokhylle 0.1.0 schema. Released migrations are immutable.
-- Application data and migration history are created at runtime.

CREATE TABLE settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    display_name TEXT,
    preferred_format TEXT,
    preferred_language TEXT,
    acquisition_mode TEXT NOT NULL DEFAULT 'automatic',
    disabled INTEGER NOT NULL DEFAULT 0,
    credential_type TEXT NOT NULL DEFAULT 'legacy',
    credential_version INTEGER NOT NULL DEFAULT 1,
    notification_email TEXT,
    email_notifications INTEGER NOT NULL DEFAULT 0,
    profile_type TEXT NOT NULL DEFAULT 'adult',
    preferred_languages TEXT,
    onboarded_at INTEGER,
    can_request INTEGER NOT NULL DEFAULT 1,
    shelf_finish TEXT NOT NULL DEFAULT 'oak' CHECK (shelf_finish IN ('oak', 'black', 'metal')),
    shelf_decorations INTEGER NOT NULL DEFAULT 1 CHECK (shelf_decorations IN (0, 1)),
    spotlight_rotation INTEGER NOT NULL DEFAULT 1 CHECK (spotlight_rotation IN (0, 1)),
    can_discover INTEGER NOT NULL DEFAULT 0,
    can_acquire INTEGER NOT NULL DEFAULT 1 CHECK (can_acquire IN (0, 1)),
    avatar_version INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE sessions (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    expires_at INTEGER NOT NULL,
    remembered INTEGER NOT NULL DEFAULT 0,
    last_seen_at INTEGER NOT NULL DEFAULT 0,
    absolute_expires_at INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX sessions_expires_at_idx ON sessions (expires_at);

CREATE INDEX sessions_token_hash_idx ON sessions (token_hash);

CREATE TABLE login_attempts (
    scope TEXT NOT NULL,
    key TEXT NOT NULL,
    failures INTEGER NOT NULL DEFAULT 0,
    window_start INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (scope, key)
);

CREATE TABLE authors (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    olid TEXT,
    photo_checked_at INTEGER
);

CREATE UNIQUE INDEX authors_normalized_name_idx ON authors (normalized_name);

CREATE TABLE books (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    normalized_title TEXT NOT NULL,
    description TEXT,
    language TEXT,
    series TEXT,
    series_number TEXT,
    cover_path TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    rating REAL,
    rating_count INTEGER,
    rating_source TEXT,
    metadata_checked_at INTEGER,
    rating_source_key TEXT,
    rating_checked_at INTEGER,
    publication_kind TEXT NOT NULL DEFAULT 'unknown' CHECK (publication_kind IN ('unknown', 'book', 'comic', 'manga', 'magazine', 'catalogue')),
    series_id INTEGER REFERENCES series(id) ON DELETE SET NULL,
    series_link_locked INTEGER NOT NULL DEFAULT 0 CHECK (series_link_locked IN (0, 1)),
    series_sort_order REAL,
    reading_direction TEXT CHECK (reading_direction IN ('ltr', 'rtl')),
    classification_reviewed_at INTEGER
);

CREATE INDEX books_created_at_idx ON books (created_at);

CREATE INDEX books_normalized_title_idx ON books (normalized_title);

CREATE TABLE book_authors (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (book_id, author_id)
);

CREATE INDEX book_authors_author_idx ON book_authors (author_id);

CREATE TABLE editions (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    language TEXT,
    publication_year INTEGER,
    isbn10 TEXT,
    isbn13 TEXT,
    publisher TEXT,
    provider TEXT,
    provider_key TEXT,
    is_unknown INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX editions_book_idx ON editions (book_id);

CREATE UNIQUE INDEX editions_isbn10_idx ON editions (isbn10) WHERE isbn10 IS NOT NULL;

CREATE UNIQUE INDEX editions_isbn13_idx ON editions (isbn13) WHERE isbn13 IS NOT NULL;

CREATE UNIQUE INDEX editions_provider_idx ON editions (provider, provider_key)
    WHERE provider_key IS NOT NULL;

CREATE VIRTUAL TABLE books_fts USING fts5(
    title,
    author,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE subjects (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    normalized_name TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE book_subjects (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    subject_id INTEGER NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    position INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (book_id, subject_id)
);

CREATE INDEX book_subjects_subject_idx ON book_subjects(subject_id);

CREATE TABLE author_external_ids (
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (provider, provider_key)
);

CREATE INDEX author_external_ids_author_idx ON author_external_ids(author_id);

CREATE TABLE book_external_ids (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (provider, provider_key)
);

CREATE INDEX book_external_ids_book_idx ON book_external_ids(book_id);

CREATE TABLE metadata_cache (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    fetched_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);

CREATE INDEX metadata_cache_expires_at_idx ON metadata_cache (expires_at);

CREATE TABLE user_books (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL DEFAULT 'manual',
    preference TEXT,
    hidden INTEGER NOT NULL DEFAULT 0,
    on_shelf INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (user_id, book_id)
);

CREATE INDEX user_books_book_idx ON user_books (book_id);

CREATE TABLE user_subject_prefs (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    normalized_name TEXT NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, normalized_name)
);

CREATE TABLE user_subject_interests (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    normalized_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, normalized_name)
);

CREATE TABLE collections (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX collections_name_idx ON collections (name COLLATE NOCASE);

CREATE TABLE collection_books (
    collection_id INTEGER NOT NULL REFERENCES collections(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (collection_id, book_id)
);

CREATE INDEX collection_books_book_idx ON collection_books (book_id);

CREATE TABLE notifications (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    title TEXT NOT NULL,
    body TEXT,
    acquisition_id TEXT,
    book_id INTEGER,
    read INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX notifications_user_idx ON notifications (user_id, created_at DESC);

CREATE TABLE acquisitions (
    id TEXT PRIMARY KEY NOT NULL,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    preferred_format TEXT,
    preferred_language TEXT,
    status TEXT NOT NULL,
    selected_release_name TEXT,
    selected_release_indexer TEXT,
    selected_release_score INTEGER,
    selected_release_confidence REAL,
    download_provider TEXT,
    provider_download_id TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    progress REAL NOT NULL DEFAULT 0,
    content_path TEXT,
    selected_release_size INTEGER,
    selected_release_format TEXT,
    selected_release_seeders INTEGER,
    ask_before_download INTEGER NOT NULL DEFAULT 0,
    download_speed INTEGER,
    selected_release_key TEXT,
    retry_attempts INTEGER NOT NULL DEFAULT 0,
    retry_started_at INTEGER,
    next_retry_at INTEGER,
    retry_stopped INTEGER NOT NULL DEFAULT 0,
    acquisition_languages TEXT,
    language_key TEXT NOT NULL DEFAULT '',
    cancel_pending INTEGER NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX acquisitions_active_variant_idx
ON acquisitions (book_id, language_key)
WHERE status IN (
    'REQUESTED', 'SEARCHING', 'EVALUATING', 'QUEUED', 'DOWNLOADING', 'DOWNLOADED',
    'INSPECTING', 'IDENTIFIED', 'IMPORTING', 'NEEDS_SELECTION', 'NEEDS_REVIEW'
);

CREATE INDEX acquisitions_book_idx ON acquisitions (book_id);

CREATE INDEX acquisitions_created_idx ON acquisitions (created_at DESC);

CREATE INDEX acquisitions_status_idx ON acquisitions (status);

CREATE TABLE acquisition_events (
    id INTEGER PRIMARY KEY,
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id) ON DELETE CASCADE,
    event TEXT NOT NULL,
    detail TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX acquisition_events_acquisition_idx ON acquisition_events (acquisition_id);

CREATE TABLE acquisition_requests (
    acquisition_id TEXT NOT NULL REFERENCES acquisitions(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    deliver_on_ready INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL DEFAULT 'manual',
    source_author_id INTEGER REFERENCES authors(id) ON DELETE SET NULL,
    PRIMARY KEY (acquisition_id, user_id)
);

CREATE TABLE pending_imports (
    acquisition_id TEXT PRIMARY KEY NOT NULL REFERENCES acquisitions(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL,
    target_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL,
    format TEXT NOT NULL,
    source_path TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE release_blocklist (
    release_key TEXT PRIMARY KEY NOT NULL,
    release_name TEXT NOT NULL,
    indexer TEXT,
    reason TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE book_requests (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'requested',
    decided_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
    decided_at INTEGER,
    acquisition_id TEXT REFERENCES acquisitions(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX book_requests_active_idx
    ON book_requests(book_id, user_id)
    WHERE status = 'requested';

CREATE INDEX book_requests_pending_idx ON book_requests(status, created_at);

CREATE INDEX book_requests_user_idx ON book_requests(user_id, created_at DESC);

CREATE TABLE "delivery_targets" (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    type TEXT NOT NULL CHECK (type IN ('kindle', 'pocketbook', 'kobo', 'boox', 'other')),
    name TEXT NOT NULL,
    address TEXT NOT NULL,
    connector TEXT NOT NULL DEFAULT 'email',
    is_default INTEGER NOT NULL DEFAULT 0,
    enabled INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE UNIQUE INDEX delivery_targets_default_per_user
    ON delivery_targets (user_id)
    WHERE is_default = 1;

CREATE INDEX delivery_targets_user_idx ON delivery_targets (user_id);

CREATE TABLE deliveries (
    id INTEGER PRIMARY KEY,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES book_files(id) ON DELETE CASCADE,
    target_id INTEGER REFERENCES delivery_targets(id) ON DELETE SET NULL,
    user_id INTEGER REFERENCES users(id) ON DELETE SET NULL,
    address TEXT NOT NULL,
    status TEXT NOT NULL,
    error_message TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL DEFAULT 'manual',
    source_author_id INTEGER REFERENCES authors(id) ON DELETE SET NULL
);

CREATE INDEX deliveries_book_idx ON deliveries (book_id);

CREATE INDEX deliveries_user_idx ON deliveries (user_id);

CREATE TABLE reader_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at INTEGER,
    sync_hash TEXT,
    sync_user TEXT
);

CREATE UNIQUE INDEX reader_tokens_sync_hash ON reader_tokens(sync_hash);

CREATE INDEX reader_tokens_user_idx ON reader_tokens (user_id);

CREATE TABLE reading_progress (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    document TEXT NOT NULL,
    book_id INTEGER REFERENCES books(id) ON DELETE CASCADE,
    book_file_id INTEGER REFERENCES book_files(id) ON DELETE SET NULL,
    percentage REAL NOT NULL DEFAULT 0,
    locator TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL DEFAULT 'koreader',
    source_device TEXT,
    device_id TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    revision INTEGER NOT NULL DEFAULT 1,
    UNIQUE(user_id, document)
);

CREATE INDEX reading_progress_user_idx ON reading_progress(user_id, updated_at DESC);

CREATE TABLE kosync_documents (
    document TEXT PRIMARY KEY,
    book_id INTEGER REFERENCES books(id) ON DELETE CASCADE,
    book_file_id INTEGER REFERENCES book_files(id) ON DELETE CASCADE,
    checked_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE agent_tokens (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    scope TEXT NOT NULL DEFAULT 'read',
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    last_used_at INTEGER,
    revoked_at INTEGER
);

CREATE TABLE author_follows (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    auto_acquire INTEGER NOT NULL DEFAULT 0,
    delivery_target_id INTEGER REFERENCES delivery_targets(id) ON DELETE SET NULL,
    baseline_at INTEGER,
    PRIMARY KEY (user_id, author_id)
);

CREATE INDEX author_follows_author_idx ON author_follows (author_id);

CREATE TABLE author_follow_refreshes (
    author_id INTEGER PRIMARY KEY REFERENCES authors(id) ON DELETE CASCADE,
    checked_at INTEGER NOT NULL
);

CREATE TABLE "author_discoveries" (
    id INTEGER PRIMARY KEY,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    title TEXT NOT NULL,
    authors TEXT NOT NULL,
    year INTEGER,
    cover_id TEXT,
    discovered_at INTEGER NOT NULL DEFAULT (unixepoch()),
    language TEXT,
    UNIQUE (author_id, provider, provider_key)
);

CREATE INDEX author_discoveries_author_idx ON author_discoveries (author_id);

CREATE TABLE author_automation_attempts (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    author_id INTEGER NOT NULL REFERENCES authors(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    provider_key TEXT NOT NULL,
    attempted_at INTEGER NOT NULL DEFAULT (unixepoch()),
    result TEXT NOT NULL,
    book_id INTEGER REFERENCES books(id) ON DELETE SET NULL,
    delivery_id INTEGER REFERENCES deliveries(id) ON DELETE SET NULL,
    PRIMARY KEY (user_id, author_id, provider, provider_key)
);

CREATE TABLE demo_gets (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    started_at INTEGER NOT NULL,
    ready_at INTEGER NOT NULL,
    completed_at INTEGER,
    send_when_ready INTEGER NOT NULL DEFAULT 0 CHECK (send_when_ready IN (0, 1)),
    UNIQUE (user_id, book_id)
);

CREATE INDEX demo_gets_due_idx ON demo_gets (ready_at)
    WHERE completed_at IS NULL;

CREATE TABLE demo_sends (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES book_files(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX demo_sends_user_idx ON demo_sends (user_id, created_at DESC);

CREATE TABLE book_available_languages (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    PRIMARY KEY (book_id, language)
);

CREATE TABLE user_avatars (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    data BLOB NOT NULL,
    mime TEXT NOT NULL CHECK (mime IN ('image/png', 'image/jpeg', 'image/webp'))
);

CREATE TABLE "book_files" (
    id INTEGER PRIMARY KEY,
    edition_id INTEGER NOT NULL REFERENCES editions(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    format TEXT NOT NULL CHECK (format IN ('epub', 'pdf', 'cbz')),
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    mtime INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source_path TEXT,
    imported_at INTEGER
);

CREATE INDEX book_files_edition_idx ON book_files (edition_id);

CREATE UNIQUE INDEX book_files_sha256_idx ON book_files (sha256);

CREATE TABLE "browser_reading_positions" (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_file_id INTEGER NOT NULL REFERENCES book_files(id) ON DELETE CASCADE,
    sha256 TEXT NOT NULL,
    format TEXT NOT NULL CHECK (format IN ('epub', 'pdf', 'cbz')),
    locator TEXT NOT NULL CHECK (length(locator) BETWEEN 1 AND 4096),
    percentage REAL NOT NULL CHECK (percentage >= 0 AND percentage <= 1),
    completed INTEGER NOT NULL DEFAULT 0 CHECK (completed IN (0, 1)),
    revision INTEGER NOT NULL CHECK (revision > 0),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, book_file_id)
);

CREATE INDEX browser_reading_positions_user_idx
    ON browser_reading_positions(user_id, updated_at DESC);

CREATE TABLE reader_direction_overrides (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    direction TEXT NOT NULL CHECK (direction IN ('ltr', 'rtl')),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, book_id)
);

CREATE TABLE series (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    sort_name TEXT,
    default_reading_direction TEXT CHECK (default_reading_direction IN ('ltr', 'rtl')),
    auto_key TEXT UNIQUE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX series_sort_name_idx ON series (sort_name, name);

CREATE INDEX books_series_order_idx ON books (series_id, series_sort_order, id);

CREATE INDEX books_publication_kind_idx ON books (publication_kind, id);

CREATE TRIGGER books_auto_series_insert AFTER INSERT ON books
WHEN NEW.series_id IS NULL AND NEW.series_link_locked = 0
  AND NEW.series IS NOT NULL AND trim(NEW.series) <> ''
BEGIN
    INSERT INTO series (name, auto_key)
    SELECT trim(NEW.series), trim(NEW.series)
    WHERE NOT EXISTS (SELECT 1 FROM series
                      WHERE name = trim(NEW.series) OR auto_key = trim(NEW.series));
    UPDATE books SET series_id = (SELECT id FROM series
                                  WHERE name = trim(NEW.series) OR auto_key = trim(NEW.series)
                                  ORDER BY CASE WHEN name = trim(NEW.series) THEN 0 ELSE 1 END, id
                                  LIMIT 1)
    WHERE id = NEW.id AND series_id IS NULL;
END;

CREATE TRIGGER books_auto_series_update AFTER UPDATE OF series ON books
WHEN NEW.series_id IS NULL AND NEW.series_link_locked = 0
  AND NEW.series IS NOT NULL AND trim(NEW.series) <> ''
BEGIN
    INSERT INTO series (name, auto_key)
    SELECT trim(NEW.series), trim(NEW.series)
    WHERE NOT EXISTS (SELECT 1 FROM series
                      WHERE name = trim(NEW.series) OR auto_key = trim(NEW.series));
    UPDATE books SET series_id = (SELECT id FROM series
                                  WHERE name = trim(NEW.series) OR auto_key = trim(NEW.series)
                                  ORDER BY CASE WHEN name = trim(NEW.series) THEN 0 ELSE 1 END, id
                                  LIMIT 1)
    WHERE id = NEW.id AND series_id IS NULL;
END;

CREATE TRIGGER books_series_order_insert AFTER INSERT ON books
WHEN NEW.series_sort_order IS NULL AND NEW.series_number IS NOT NULL
  AND trim(NEW.series_number) <> '' AND trim(NEW.series_number) <> '.'
  AND trim(NEW.series_number) NOT GLOB '*[^0-9.]*'
  AND length(trim(NEW.series_number)) - length(replace(trim(NEW.series_number), '.', '')) <= 1
BEGIN
    UPDATE books SET series_sort_order = CAST(trim(NEW.series_number) AS REAL) WHERE id = NEW.id;
END;

CREATE TRIGGER books_series_order_update AFTER UPDATE OF series_number ON books
WHEN NEW.series_sort_order IS NULL AND NEW.series_number IS NOT NULL
  AND trim(NEW.series_number) <> '' AND trim(NEW.series_number) <> '.'
  AND trim(NEW.series_number) NOT GLOB '*[^0-9.]*'
  AND length(trim(NEW.series_number)) - length(replace(trim(NEW.series_number), '.', '')) <= 1
BEGIN
    UPDATE books SET series_sort_order = CAST(trim(NEW.series_number) AS REAL) WHERE id = NEW.id;
END;

CREATE INDEX books_classification_review_idx
    ON books (classification_reviewed_at, created_at DESC, id DESC);

CREATE TABLE user_book_completions (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    completed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, book_id)
);

CREATE INDEX user_book_completions_book_idx ON user_book_completions (book_id);

CREATE TABLE acquisition_inputs (
    acquisition_id TEXT PRIMARY KEY NOT NULL REFERENCES acquisitions(id) ON DELETE CASCADE,
    method TEXT NOT NULL CHECK (method IN ('http')),
    url TEXT NOT NULL,
    expected_format TEXT NOT NULL CHECK (expected_format IN ('epub', 'pdf', 'cbz')),
    source_kind TEXT NOT NULL,
    source_name TEXT NOT NULL,
    source_key TEXT NOT NULL,
    trusted_origin TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE opds_sources (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    url TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE TABLE watch_imports (
    id TEXT PRIMARY KEY NOT NULL,
    source_path TEXT NOT NULL,
    staged_path TEXT NOT NULL,
    target_path TEXT NOT NULL,
    sha256 TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('placing', 'imported')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    cleanup_pending INTEGER NOT NULL DEFAULT 1,
    error_message TEXT
);

CREATE INDEX watch_imports_status_idx ON watch_imports(status);

CREATE INDEX watch_imports_pending_source_idx ON watch_imports(source_path) WHERE status = 'placing';

CREATE TABLE nzb_inputs (
    acquisition_id TEXT PRIMARY KEY NOT NULL REFERENCES acquisitions(id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    job_name TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    submitted_at INTEGER
);
