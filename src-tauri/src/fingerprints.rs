//! Fingerprint persistence helpers (oshash / md5 / phash).

use sqlx::SqlitePool;

use crate::models::{new_id, now};

/// Insert or replace a fingerprint row for `(file_id, hash_type)`.
pub async fn upsert(
    pool: &SqlitePool,
    file_id: &str,
    hash_type: &str,
    value: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO fingerprints (id, file_id, hash_type, value, created_at)
        VALUES (?, ?, ?, ?, ?)
        ON CONFLICT(file_id, hash_type) DO UPDATE SET value = excluded.value
        "#,
    )
    .bind(new_id())
    .bind(file_id)
    .bind(hash_type)
    .bind(value)
    .bind(now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove one fingerprint type for a file (e.g. invalidate stale md5 after re-hash).
pub async fn delete_type(
    pool: &SqlitePool,
    file_id: &str,
    hash_type: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM fingerprints WHERE file_id = ? AND hash_type = ?")
        .bind(file_id)
        .bind(hash_type)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get(
    pool: &SqlitePool,
    file_id: &str,
    hash_type: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT value FROM fingerprints WHERE file_id = ? AND hash_type = ?")
            .bind(file_id)
            .bind(hash_type)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(v,)| v))
}

/// Reuse a pHash already stored for any file with the same oshash (move / re-index).
pub async fn find_phash_by_oshash(
    pool: &SqlitePool,
    oshash: &str,
) -> Result<Option<String>, sqlx::Error> {
    let row: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT ph.value
        FROM fingerprints o1
        JOIN fingerprints o2 ON o2.hash_type = 'oshash' AND o2.value = o1.value
        JOIN fingerprints ph ON ph.file_id = o2.file_id AND ph.hash_type = 'phash'
        WHERE o1.hash_type = 'oshash' AND o1.value = ?
        LIMIT 1
        "#,
    )
    .bind(oshash)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(v,)| v))
}
