//! Database setup: pool, migrations, app-data location.

use std::path::PathBuf;

use anyhow::{Context, Result};
use sqlx::{
    migrate::MigrateError,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteSynchronous},
    SqlitePool,
};
use tracing::{info, warn};

/// Embedded migrations — keep a named handle so we can re-sync checksums.
static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Where we store the SQLite file.
///
/// Override with `MAIZEVIEW_DB_PATH` for E2E / isolated test runs.
pub fn db_path() -> Result<PathBuf> {
    if let Ok(raw) = std::env::var("MAIZEVIEW_DB_PATH") {
        let path = PathBuf::from(raw.trim());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }
        return Ok(path);
    }

    let base = dirs::data_dir().context("could not resolve OS data dir")?;
    let dir = base.join("MaizeView");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating data dir {}", dir.display()))?;
    Ok(dir.join("maizeview.db"))
}

/// Open a connection pool and run migrations to the latest version.
///
/// Connect options: WAL + synchronous=NORMAL (safe under WAL — skips the
/// per-commit fsync) + foreign keys on, busy timeout so concurrent scanner
/// jobs and the UI don't immediately error on lock contention.
pub async fn init_pool() -> Result<SqlitePool> {
    let path = db_path()?;
    info!(path = %path.display(), "opening database");

    // `filename` requires an owned/static path; convert the OsString once.
    let filename: std::path::PathBuf = path.clone();
    let options = SqliteConnectOptions::new()
        .filename(filename)
        .create_if_missing(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(std::time::Duration::from_secs(10));

    // Headroom for UI queries while preview/pHash/MD5 workers write (≤16 workers).
    let pool = SqlitePoolOptions::new()
        .max_connections(20)
        .connect_with(options)
        .await
        .context("connecting to sqlite")?;

    run_migrations(&pool).await.context("running migrations")?;

    // Repair `E:foo` → `E:\foo` from bare drive-root scan paths (player/load break).
    if let Err(e) = crate::paths::repair_drive_relative_paths(&pool).await {
        warn!(error = %e, "drive-relative path repair on startup failed");
    }

    // Missing-file reconcile runs in the background from lib.rs setup — the
    // per-file stat checks are too slow to block startup on USB/spinning drives.
    Ok(pool)
}

/// Run embedded migrations. Pre-v1.0 we edit the initial migration in place
/// (see docs/plan.md); sqlx tracks SHA-384 checksums and refuses to start if
/// the file changed after apply. Re-sync checksums once and retry rather than
/// forcing users to wipe their library.
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    match MIGRATOR.run(pool).await {
        Ok(()) => Ok(()),
        Err(MigrateError::VersionMismatch(version)) => {
            warn!(
                version,
                "migration checksum mismatch — re-syncing (pre-v1.0 in-place schema policy)"
            );
            resync_applied_checksums(pool).await?;
            MIGRATOR
                .run(pool)
                .await
                .context("running migrations after checksum re-sync")?;
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

/// Update `_sqlx_migrations.checksum` for every embedded migration that is
/// already recorded. Does not re-run SQL — only fixes the version guard.
async fn resync_applied_checksums(pool: &SqlitePool) -> Result<()> {
    for migration in MIGRATOR.iter() {
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = ?")
            .bind(migration.checksum.as_ref() as &[u8])
            .bind(migration.version)
            .execute(pool)
            .await
            .context("re-syncing migration checksum")?;
    }
    Ok(())
}
