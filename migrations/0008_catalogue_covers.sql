-- Catalogue artwork remains a fallback after a book gets a local identity.
-- Store the provider identity, never a path derived from an external cover id.
CREATE TABLE book_cover_sources (
    book_id INTEGER PRIMARY KEY REFERENCES books(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    cover_id TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Restore exact artwork identities already present in the catalogue cache.
INSERT OR IGNORE INTO book_cover_sources (book_id, provider, cover_id)
SELECT ids.book_id, ids.provider, json_extract(cache.value, '$.coverId')
FROM book_external_ids ids
JOIN metadata_cache cache ON cache.key = 'book:' || ids.provider || ':' || ids.provider_key
WHERE CASE WHEN json_valid(cache.value) THEN json_type(cache.value, '$.coverId') END = 'text'
  AND trim(CASE WHEN json_valid(cache.value) THEN json_extract(cache.value, '$.coverId') END) <> '';
