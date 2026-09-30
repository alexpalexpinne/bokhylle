-- Keep automatic metadata available underneath explicit manual corrections.
CREATE TABLE book_metadata_fields (
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('title', 'authors', 'description', 'language', 'series', 'seriesNumber', 'cover')),
    automatic_value TEXT NOT NULL CHECK (json_valid(automatic_value)),
    source TEXT NOT NULL DEFAULT 'unknown',
    source_key TEXT,
    manual INTEGER NOT NULL DEFAULT 0 CHECK (manual IN (0, 1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (book_id, field)
);

CREATE TABLE edition_metadata_fields (
    edition_id INTEGER NOT NULL REFERENCES editions(id) ON DELETE CASCADE,
    field TEXT NOT NULL CHECK (field IN ('title', 'language', 'publicationYear', 'publisher')),
    automatic_value TEXT NOT NULL CHECK (json_valid(automatic_value)),
    source TEXT NOT NULL DEFAULT 'unknown',
    source_key TEXT,
    manual INTEGER NOT NULL DEFAULT 0 CHECK (manual IN (0, 1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (edition_id, field)
);

-- Historical field origins cannot be inferred from a book's provider identity.
INSERT INTO book_metadata_fields (book_id, field, automatic_value, manual)
SELECT b.id, j.key,
       CASE WHEN j.type = 'array' THEN j.value ELSE json_quote(j.value) END,
       CASE WHEN j.key = 'seriesNumber' THEN b.series_link_locked ELSE 0 END
FROM books b, json_each(json_object(
    'title', b.title, 'description', b.description, 'language', b.language,
    'series', b.series, 'seriesNumber', b.series_number, 'cover', b.cover_path,
    'authors', json((SELECT json_group_array(name) FROM (
        SELECT a.name FROM book_authors ba JOIN authors a ON a.id = ba.author_id
        WHERE ba.book_id = b.id ORDER BY ba.position, a.name
    )))
)) j;

INSERT INTO edition_metadata_fields (edition_id, field, automatic_value)
SELECT e.id, j.key, json_quote(j.value)
FROM editions e, json_each(json_object(
    'title', e.title, 'language', e.language,
    'publicationYear', e.publication_year, 'publisher', e.publisher
)) j;
