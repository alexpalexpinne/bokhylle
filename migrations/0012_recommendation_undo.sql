-- Only the latest feedback on an offered suggestion can be undone, for ten
-- minutes. Undo restores taste/dismissal without touching shelf membership.
CREATE TABLE recommendation_feedback_undo (
 user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 identity_key TEXT NOT NULL,
 token TEXT NOT NULL,
 book_id INTEGER REFERENCES books(id) ON DELETE CASCADE,
 previous_preference TEXT,
 applied_preference TEXT,
 previous_dismissed_until INTEGER,
 applied_dismissed_until INTEGER,
 expires_at INTEGER NOT NULL,
 PRIMARY KEY(user_id, identity_key),
 FOREIGN KEY(user_id, identity_key) REFERENCES recommendation_candidates(user_id, identity_key) ON DELETE CASCADE
);
