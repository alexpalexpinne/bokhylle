-- Local rails look up impression/dismissal history by book without scanning
-- all offered catalogue candidates for the profile.
CREATE INDEX recommendation_candidates_book_idx
 ON recommendation_candidates(user_id, book_id) WHERE book_id IS NOT NULL;
