-- Book access outlives shelf membership; a shelf is a private reading list.
ALTER TABLE users ADD COLUMN default_book_sharing TEXT NOT NULL DEFAULT 'shared'
    CHECK (default_book_sharing IN ('private', 'shared'));
ALTER TABLE books ADD COLUMN sharing_managed INTEGER NOT NULL DEFAULT 0
    CHECK (sharing_managed IN (0, 1));
ALTER TABLE book_requests ADD COLUMN sharing TEXT NOT NULL DEFAULT 'shared'
    CHECK (sharing IN ('private', 'shared'));
UPDATE book_requests SET sharing = 'private'
WHERE user_id IN (SELECT id FROM users WHERE profile_type = 'child');

CREATE TABLE book_access (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    book_id INTEGER NOT NULL REFERENCES books(id) ON DELETE CASCADE,
    sharing TEXT NOT NULL CHECK (sharing IN ('private', 'shared')),
    PRIMARY KEY (user_id, book_id)
);
CREATE INDEX book_access_book_idx ON book_access(book_id, sharing, user_id);

-- Preserve every existing reader's access. Unclaimed library books remain
-- shared until a reader deliberately sets their sharing.
INSERT INTO book_access (user_id, book_id, sharing)
SELECT ub.user_id, ub.book_id,
       CASE WHEN u.profile_type = 'child' THEN 'private' ELSE 'shared' END
FROM user_books ub JOIN users u ON u.id = ub.user_id WHERE ub.on_shelf = 1;
