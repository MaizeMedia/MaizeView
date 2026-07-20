//! One-off: compute missing MD5 fingerprints for the local dev DB.
//! Usage: cargo run --example regenerate_fingerprints --release

use maizeview_lib::{db, fingerprints_job};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,maizeview_lib=info")
        .init();

    let pool = db::init_pool().await?;
    let before: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM files f
        LEFT JOIN fingerprints fp ON fp.file_id = f.id AND fp.hash_type = 'md5'
        WHERE fp.id IS NULL
        "#,
    )
    .fetch_one(&pool)
    .await?;
    eprintln!("Computing MD5 for {} files…", before.0);

    fingerprints_job::run_silent(&pool).await?;

    let after: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*)
        FROM files f
        LEFT JOIN fingerprints fp ON fp.file_id = f.id AND fp.hash_type = 'md5'
        WHERE fp.id IS NULL
        "#,
    )
    .fetch_one(&pool)
    .await?;
    let with_md5: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM fingerprints WHERE hash_type = 'md5'")
            .fetch_one(&pool)
            .await?;
    eprintln!(
        "Done. {} md5 fingerprints stored, {} still missing.",
        with_md5.0, after.0
    );
    Ok(())
}
