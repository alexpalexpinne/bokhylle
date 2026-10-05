-- Canonical recommendation topics preserve original catalogue labels.
CREATE TABLE subject_aliases (alias TEXT PRIMARY KEY, concept TEXT NOT NULL);
INSERT INTO subject_aliases VALUES
 ('fantasy fiction','fantasy'), ('detective and mystery stories','mystery'),
 ('detective and mystery fiction','mystery'), ('science fiction stories','science fiction'),
 ('sci fi','science fiction'), ('scifi','science fiction'), ('humour','humor');

CREATE VIEW subject_concepts AS
WITH RECURSIVE terms(normalized_name) AS (
 SELECT normalized_name FROM subjects UNION SELECT normalized_name FROM user_subject_interests
 UNION SELECT normalized_name FROM user_subject_prefs UNION SELECT alias FROM subject_aliases
 UNION SELECT concept FROM subject_aliases
), stripped(normalized_name, base) AS (
 SELECT normalized_name, normalized_name FROM terms
 UNION ALL
 SELECT normalized_name, CASE
   WHEN base LIKE '% in english' THEN substr(base,1,length(base)-11)
   WHEN base LIKE '% american' THEN substr(base,1,length(base)-9)
   WHEN base LIKE '% english' THEN substr(base,1,length(base)-8)
   WHEN base LIKE '% general' THEN substr(base,1,length(base)-8) END
 FROM stripped WHERE base LIKE '% in english' OR base LIKE '% american'
   OR base LIKE '% english' OR base LIKE '% general'
)
SELECT normalized_name, COALESCE(a.concept, base) AS concept FROM stripped
LEFT JOIN subject_aliases a ON a.alias = base
WHERE base NOT LIKE '% in english' AND base NOT LIKE '% american'
  AND base NOT LIKE '% english' AND base NOT LIKE '% general';

-- Offered candidates authorize feedback without exposing another profile's
-- private book identities. Only visible impressions establish last_seen_at.
CREATE TABLE recommendation_candidates (
 user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
 identity_key TEXT NOT NULL CHECK(length(identity_key) = 64),
 book_id INTEGER REFERENCES books(id) ON DELETE CASCADE,
 provider TEXT, provider_key TEXT,
 offered_at INTEGER NOT NULL DEFAULT (unixepoch()),
 last_seen_at INTEGER, dismissed_until INTEGER,
 PRIMARY KEY(user_id, identity_key)
);
CREATE INDEX recommendation_candidates_age_idx ON recommendation_candidates(offered_at);
