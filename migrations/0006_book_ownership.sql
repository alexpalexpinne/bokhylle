-- Reading a shared book and acquiring it are different relationships.
ALTER TABLE book_access ADD COLUMN is_owner INTEGER NOT NULL DEFAULT 0
    CHECK (is_owner IN (0, 1));

-- Actual acquisition participants keep independent ownership. Shelf-only
-- readers do not gain the ability to hide or continue sharing someone else's book.
INSERT INTO book_access (user_id, book_id, sharing, is_owner)
SELECT ar.user_id, a.book_id,
       CASE WHEN u.profile_type = 'child' THEN 'private' ELSE u.default_book_sharing END, 1
FROM acquisition_requests ar
JOIN acquisitions a ON a.id = ar.acquisition_id
JOIN users u ON u.id = ar.user_id
WHERE 1
ON CONFLICT(user_id, book_id) DO UPDATE SET is_owner = 1;

INSERT INTO book_access (user_id, book_id, sharing, is_owner)
SELECT a.user_id, a.book_id,
       CASE WHEN u.profile_type = 'child' THEN 'private' ELSE u.default_book_sharing END, 1
FROM acquisitions a JOIN users u ON u.id = a.user_id
WHERE a.user_id IS NOT NULL
ON CONFLICT(user_id, book_id) DO UPDATE SET is_owner = 1;

-- Some managed books predate acquisition history (for example, an imported
-- file later made private). Preserve the earliest known adult's decision.
-- Unmanaged imports remain household books until explicitly acquired.
UPDATE book_access SET is_owner = 1
WHERE user_id = (
    SELECT candidate.user_id FROM book_access candidate
    JOIN users u ON u.id = candidate.user_id AND u.profile_type = 'adult'
    LEFT JOIN user_books ub ON ub.user_id = candidate.user_id AND ub.book_id = candidate.book_id
    WHERE candidate.book_id = book_access.book_id
    ORDER BY COALESCE(ub.added_at, 9223372036854775807), candidate.user_id LIMIT 1
)
AND EXISTS (SELECT 1 FROM books b WHERE b.id = book_access.book_id AND b.sharing_managed = 1)
AND NOT EXISTS (SELECT 1 FROM book_access owner WHERE owner.book_id = book_access.book_id AND owner.is_owner = 1);

CREATE INDEX book_access_owner_idx ON book_access(user_id, is_owner, book_id);
