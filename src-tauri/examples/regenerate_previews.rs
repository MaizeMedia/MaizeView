//! One-off: regenerate missing thumbnails for the local dev DB.
//! Usage: cargo run --example regenerate_previews --release

use maizeview_lib::{db, previews_job};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,maizeview_lib=info")
        .init();

    let pool = db::init_pool().await?;
    let before: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE duration IS NOT NULL AND thumb_path IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    eprintln!("Generating thumbnails for {} files…", before.0);

    previews_job::run_silent(&pool).await?;

    let after: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM files WHERE duration IS NOT NULL AND thumb_path IS NULL",
    )
    .fetch_one(&pool)
    .await?;
    let with_thumb: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM files WHERE thumb_path IS NOT NULL")
            .fetch_one(&pool)
            .await?;
    eprintln!(
        "Done. {} thumbs on disk, {} still missing.",
        with_thumb.0, after.0
    );
    Ok(())
}
