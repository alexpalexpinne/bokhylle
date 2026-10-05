-- Keep catalogue display eligibility separate from acquisition automation.
ALTER TABLE author_discoveries ADD COLUMN languages TEXT NOT NULL DEFAULT '[]';
ALTER TABLE author_discoveries ADD COLUMN subjects TEXT NOT NULL DEFAULT '[]';

CREATE INDEX acquisition_requests_user_idx ON acquisition_requests(user_id, acquisition_id);

-- Existing discoveries gain the new metadata on the next background tick.
UPDATE author_follow_refreshes SET checked_at = 0;
