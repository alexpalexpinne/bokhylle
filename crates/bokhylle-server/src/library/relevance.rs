//! Recommendation eligibility. Every query supplies a single `viewer(id)` CTE
//! and uses `b` for the candidate book. Deliberate household browsing does not
//! apply these filters.

pub(crate) const EXCLUSIONS: &str = "
    AND NOT EXISTS (
        SELECT 1 FROM user_books excluded
        WHERE excluded.user_id = (SELECT id FROM viewer) AND excluded.book_id = b.id
          AND excluded.preference = 'not_for_me'
    )
    AND NOT EXISTS (
        SELECT 1 FROM book_subjects excluded_subject
        JOIN subjects s ON s.id = excluded_subject.subject_id
        JOIN user_subject_prefs hidden ON hidden.normalized_name = s.normalized_name
        WHERE excluded_subject.book_id = b.id
          AND hidden.user_id = (SELECT id FROM viewer) AND hidden.hidden = 1
    )";

pub(crate) fn personal() -> String {
    PERSONAL.replace(
        "{informative_subject}",
        &super::subjects::informative_sql("seed_topic.normalized_name"),
    )
}

const PERSONAL: &str = "
    AND (
        EXISTS (
            SELECT 1 FROM user_books own
            WHERE own.book_id = b.id AND own.user_id = (SELECT id FROM viewer)
              AND (own.on_shelf = 1 OR own.preference = 'liked')
        )
        OR EXISTS (
            SELECT 1 FROM book_authors ba JOIN author_follows followed ON followed.author_id = ba.author_id
            WHERE ba.book_id = b.id AND followed.user_id = (SELECT id FROM viewer)
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject JOIN subjects s ON s.id = candidate_subject.subject_id
            JOIN user_subject_interests interest ON interest.normalized_name = s.normalized_name
            WHERE candidate_subject.book_id = b.id AND interest.user_id = (SELECT id FROM viewer)
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN book_subjects seed_subject ON seed_subject.subject_id = candidate_subject.subject_id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN user_books seed ON seed.book_id = seed_subject.book_id
            WHERE candidate_subject.book_id = b.id AND seed.user_id = (SELECT id FROM viewer)
              AND {informative_subject}
              AND (seed.preference IS NULL OR seed.preference = 'liked')
              AND (seed.preference = 'liked' OR (seed.on_shelf = 1
                   AND seed.source IN ('manual', 'requested', 'sent', 'agent', 'book_request')))
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN book_subjects seed_subject ON seed_subject.subject_id = candidate_subject.subject_id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN acquisitions acquisition ON acquisition.book_id = seed_subject.book_id
            JOIN acquisition_requests request ON request.acquisition_id = acquisition.id
            WHERE candidate_subject.book_id = b.id AND request.user_id = (SELECT id FROM viewer)
              AND {informative_subject}
              AND NOT EXISTS (SELECT 1 FROM user_books rejected
                  WHERE rejected.user_id = request.user_id AND rejected.book_id = acquisition.book_id
                    AND rejected.preference = 'not_for_me')
        )
        OR EXISTS (
            SELECT 1 FROM book_subjects candidate_subject
            JOIN book_subjects seed_subject ON seed_subject.subject_id = candidate_subject.subject_id
            JOIN subjects seed_topic ON seed_topic.id = seed_subject.subject_id
            JOIN deliveries delivery ON delivery.book_id = seed_subject.book_id
            WHERE candidate_subject.book_id = b.id AND delivery.user_id = (SELECT id FROM viewer)
              AND {informative_subject}
              AND delivery.status = 'SENT'
              AND NOT EXISTS (SELECT 1 FROM user_books rejected
                  WHERE rejected.user_id = delivery.user_id AND rejected.book_id = delivery.book_id
                    AND rejected.preference = 'not_for_me')
        )
    )";
