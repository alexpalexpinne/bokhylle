use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

pub async fn init(database_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(parent) = database_path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).map_err(sqlx::Error::Io)?;
    }

    let options = SqliteConnectOptions::new()
        .filename(database_path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);

    // SQLite requires a table rebuild to widen a CHECK constraint. Run
    // migrations on one connection with FK actions disabled, then verify all
    // references before opening normal connections with enforcement enabled.
    let migration_pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone().foreign_keys(false))
        .await?;
    sqlx::migrate!("../../migrations")
        .run(&migration_pool)
        .await?;
    let violations: Vec<(String, i64, String, i64)> = sqlx::query_as("PRAGMA foreign_key_check")
        .fetch_all(&migration_pool)
        .await?;
    if !violations.is_empty() {
        return Err(sqlx::Error::Protocol(
            "migration left broken foreign keys".into(),
        ));
    }
    migration_pool.close().await;

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await?;

    Ok(pool)
}

/// Drop cache rows that expired long ago. Recently expired rows stay so a
/// provider outage can still serve stale data; this only bounds growth.
pub async fn prune_metadata_cache(pool: &SqlitePool) -> Result<u64, sqlx::Error> {
    let result =
        sqlx::query("DELETE FROM metadata_cache WHERE expires_at < unixepoch() - 30 * 24 * 3600")
            .execute(pool)
            .await?;
    Ok(result.rows_affected())
}
