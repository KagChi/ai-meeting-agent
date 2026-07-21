//! Database initialization and utilities for SQLite storage

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::{Pool, Sqlite};
use std::path::Path;
use std::str::FromStr;

/// Initialize the SQLite database with schema
pub async fn init_database(db_path: &Path) -> Result<Pool<Sqlite>> {
    let db_url = format!("sqlite:{}", db_path.display());

    let options = SqliteConnectOptions::from_str(&db_url)?
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .with_context(|| format!("Failed to connect to database at {}", db_path.display()))?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run database migrations")?;

    ensure_voice_bank_segment_columns(&pool).await?;

    log::info!("Database initialized at {}", db_path.display());

    Ok(pool)
}

/// Initialize an in-memory SQLite database (for testing)
pub async fn init_memory_database() -> Result<Pool<Sqlite>> {
    let options = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .context("Failed to create in-memory database")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("Failed to run database migrations")?;

    ensure_voice_bank_segment_columns(&pool).await?;

    log::info!("In-memory database initialized");

    Ok(pool)
}

/// Idempotent column adds for voice-bank segment identity fields.
///
/// Kept out of pure SQL migrations because SQLite `ALTER TABLE ADD COLUMN`
/// is not `IF NOT EXISTS` and concurrent migrators / re-runs fail with
/// "duplicate column name".
async fn ensure_voice_bank_segment_columns(pool: &Pool<Sqlite>) -> Result<()> {
    let cols: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM pragma_table_info('transcript_segments')",
    )
    .fetch_all(pool)
    .await
    .context("Failed to read transcript_segments columns")?;

    if !cols.iter().any(|c| c == "person_id") {
        sqlx::query(
            "ALTER TABLE transcript_segments ADD COLUMN person_id TEXT REFERENCES persons(id) ON DELETE SET NULL",
        )
        .execute(pool)
        .await
        .context("Failed to add transcript_segments.person_id")?;
    }

    if !cols.iter().any(|c| c == "identify_confidence") {
        sqlx::query("ALTER TABLE transcript_segments ADD COLUMN identify_confidence REAL")
            .execute(pool)
            .await
            .context("Failed to add transcript_segments.identify_confidence")?;
    }

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_segments_person ON transcript_segments(person_id)",
    )
    .execute(pool)
    .await
    .context("Failed to create idx_segments_person")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_init_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let pool = init_database(&db_path).await.unwrap();

        // Verify tables exist
        let table_count: i32 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN (
                'meetings', 'transcript_segments', 'summaries', 'transcript_versions',
                'persons', 'voiceprints', 'voiceprint_samples'
            )"
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(table_count, 7, "Expected 7 main tables");

        // Verify FTS table exists
        let fts_exists: bool = sqlx::query_scalar(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='transcript_search'"
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert!(fts_exists, "FTS5 table should exist");
    }

    #[tokio::test]
    async fn test_foreign_keys_enabled() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let pool = init_database(&db_path).await.unwrap();

        let fk_enabled: i32 = sqlx::query_scalar("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(fk_enabled, 1, "Foreign keys should be enabled");
    }
}
