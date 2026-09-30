-- Optional bundled profile marks. Existing profiles retain initials or photos.
ALTER TABLE users ADD COLUMN avatar_preset TEXT
    CHECK (avatar_preset IS NULL OR avatar_preset IN (
        'fox', 'owl', 'cat', 'bear', 'whale', 'book', 'tree', 'mountain', 'moon', 'leaf'
    ));
