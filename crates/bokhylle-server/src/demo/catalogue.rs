//! Curated metadata and disposable taste signals for the prepared demo books.
//! Nothing here runs for a household installation or contacts a provider.

use sqlx::SqlitePool;

use crate::error::AppError;
use crate::library::subjects;

const SUBJECTS: &[(&str, &[&str])] = &[
    ("A Christmas Carol", &["Ghost stories", "Redemption"]),
    (
        "Dracula",
        &["Gothic fiction", "Moral dilemmas", "Supernatural"],
    ),
    (
        "Frankenstein",
        &["Gothic fiction", "Moral dilemmas", "Science fiction"],
    ),
    (
        "Jane Eyre",
        &["Gothic fiction", "Moral dilemmas", "Social class"],
    ),
    (
        "Wuthering Heights",
        &["Gothic fiction", "Moral dilemmas", "Social class"],
    ),
    (
        "The Picture of Dorian Gray",
        &["Gothic fiction", "Moral dilemmas", "Art"],
    ),
    ("Emma", &["Romance", "Social class"]),
    ("Persuasion", &["Romance", "Social class"]),
    ("Sense and Sensibility", &["Romance", "Social class"]),
    ("Great Expectations", &["Social class", "Coming of age"]),
    ("Little Women", &["Family", "Coming of age"]),
    ("The Scarlet Letter", &["Moral dilemmas", "Social class"]),
    ("Moby Dick", &["Adventure", "Seafaring"]),
    ("Treasure Island", &["Adventure", "Seafaring"]),
    ("A Study in Scarlet", &["Mystery", "Detective stories"]),
    (
        "The Hound of the Baskervilles",
        &["Mystery", "Detective stories"],
    ),
    ("The Time Machine", &["Science fiction", "Time travel"]),
    (
        "The War of the Worlds",
        &["Science fiction", "Alien invasion"],
    ),
    ("The Invisible Man", &["Science fiction", "Moral dilemmas"]),
    ("The Wonderful Wizard of Oz", &["Adventure", "Fantasy"]),
    (
        "Alice’s Adventures in Wonderland",
        &["Adventure", "Fantasy"],
    ),
    ("The Secret Garden", &["Friendship", "Coming of age"]),
    ("Pollyanna", &["Friendship", "Coming of age"]),
    ("The Railway Children", &["Family", "Adventure"]),
    ("Five Children and It", &["Family", "Fantasy"]),
    ("Black Beauty", &["Animals", "Kindness"]),
];

pub(super) async fn seed(pool: &SqlitePool) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    for (title, names) in SUBJECTS {
        let book: Option<i64> = sqlx::query_scalar(
            "SELECT b.id FROM books b WHERE b.title = ?
             AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                         WHERE e.book_id = b.id) LIMIT 1",
        )
        .bind(title)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(book) = book else { continue };
        for (position, name) in names.iter().enumerate() {
            let normalized = subjects::normalized(name);
            sqlx::query("INSERT OR IGNORE INTO subjects (name, normalized_name) VALUES (?, ?)")
                .bind(name)
                .bind(&normalized)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "INSERT OR IGNORE INTO book_subjects (book_id, subject_id, position)
                 SELECT ?, id, ? FROM subjects WHERE normalized_name = ?",
            )
            .bind(book)
            .bind(position as i64)
            .bind(&normalized)
            .execute(&mut *tx)
            .await?;
        }
    }
    // The followed-author feed uses the same local catalogue identities as
    // demo Discover. These are sample discoveries, not new-release claims.
    sqlx::query(
        "INSERT INTO author_discoveries
         (author_id, provider, provider_key, title, authors, language, languages, subjects)
         SELECT a.id, 'local', 'local:' || b.id, b.title, a.name, 'en', '[\"en\"]',
                (SELECT json_group_array(s.name) FROM book_subjects bs
                 JOIN subjects s ON s.id = bs.subject_id WHERE bs.book_id = b.id)
         FROM books b JOIN book_authors ba ON ba.book_id = b.id
         JOIN authors a ON a.id = ba.author_id
         WHERE a.normalized_name IN ('charles dickens', 'jane austen')
           AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id)
         ON CONFLICT(author_id, provider, provider_key) DO UPDATE SET
             languages = excluded.languages, subjects = excluded.subjects",
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn seed_profile(pool: &SqlitePool, user_id: i64) -> Result<(), AppError> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "INSERT INTO user_books (user_id, book_id, source, on_shelf, preference)
         SELECT ?, b.id, 'demo', 0, 'liked' FROM books b
         WHERE b.title IN ('Dracula', 'The Picture of Dorian Gray')
           AND EXISTS (SELECT 1 FROM editions e JOIN book_files f ON f.edition_id = e.id
                       WHERE e.book_id = b.id)
         ON CONFLICT(user_id, book_id) DO UPDATE SET preference = excluded.preference",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO author_follows (user_id, author_id)
         SELECT ?, id FROM authors WHERE normalized_name IN ('charles dickens', 'jane austen')",
    )
    .bind(user_id)
    .execute(&mut *tx)
    .await?;
    for interest in ["gothic fiction", "adventure"] {
        sqlx::query(
            "INSERT OR IGNORE INTO user_subject_interests (user_id, normalized_name) VALUES (?, ?)",
        )
        .bind(user_id)
        .bind(interest)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
