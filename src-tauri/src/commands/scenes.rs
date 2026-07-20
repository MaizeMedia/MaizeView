//! Scene commands: list (paginated + filtered + sorted), counts, favorite,
//! detail.

use tauri::{Emitter, State};

use crate::{
    commands::{batch_identify::NEEDS_REVIEW_WHERE, err},
    models::{Scene, VideoFile},
    AppState,
};

/// A row in the library grid: the scene plus its primary file's key fields.
/// Joins scenes → one representative file (the first by scanned_at) so the grid
/// can show duration/preview without a second round-trip per row.
#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SceneGridRow {
    pub id: String,
    pub title: Option<String>,
    pub favorite: i64, // 0..5
    pub rating: Option<i64>,
    pub play_count: i64,
    pub created_at: String,
    // from the representative file:
    pub duration: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub thumb_path: Option<String>,
    pub thumb_sprite_path: Option<String>,
    pub file_path: Option<String>,
}

/// Sort order for `list_scenes`. `Favorite` sorts by favorite level desc.
#[derive(Debug, Clone, Copy, serde::Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SortBy {
    #[default]
    Created, // newest first
    Favorite,  // highest favorite level first
    PlayCount, // most-played first
    Title,     // alpha
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct ListScenesArgs {
    /// If true, only scenes with favorite > 0.
    pub favorites_only: Option<bool>,
    /// If set, only scenes whose favorite level is >= this (0..5).
    pub min_favorite: Option<i64>,
    pub search: Option<String>,
    /// When true, scenes must NOT match the include portion of `search`.
    pub search_inverse: Option<bool>,
    /// Each term must NOT match title, details, path, tag, performer, or segment label.
    #[serde(default)]
    pub search_exclude_terms: Vec<String>,
    pub sort: Option<SortBy>,
    /// Only scenes tagged with ALL of these tag IDs (unless tag_match_any).
    #[serde(default)]
    pub tag_ids: Vec<String>,
    /// When true, scene must have ANY of tag_ids instead of ALL.
    pub tag_match_any: Option<bool>,
    /// Scenes must not have any of these tag IDs.
    #[serde(default)]
    pub exclude_tag_ids: Vec<String>,
    /// Only scenes featuring ALL of these performer IDs.
    #[serde(default)]
    pub performer_ids: Vec<String>,
    /// Scenes must not feature any of these performer IDs.
    #[serde(default)]
    pub exclude_performer_ids: Vec<String>,
    /// Minimum primary-file duration in seconds.
    pub min_duration: Option<f64>,
    /// Maximum primary-file duration in seconds.
    pub max_duration: Option<f64>,
    /// When true, only scenes with play_count = 0.
    pub unplayed_only: Option<bool>,
    /// Require at least this many tags (0 / unset = off). Replaces tagged_only.
    pub min_tag_count: Option<i64>,
    /// Legacy: treat as min_tag_count = 1 when min_tag_count unset.
    pub tagged_only: Option<bool>,
    /// Only scenes that had a stash-box identify apply (`stashdb_applied_at`).
    pub identified_only: Option<bool>,
    /// Multiple stash-box matches and not yet applied (needs manual pick).
    pub needs_review_only: Option<bool>,
    /// Only scenes with these studio IDs (ANY).
    #[serde(default)]
    pub studio_ids: Vec<String>,
    /// Scenes must not have these studio IDs.
    #[serde(default)]
    pub exclude_studio_ids: Vec<String>,
    /// Minimum primary-file height in pixels (resolution floor).
    pub min_height: Option<i64>,
    /// Require at least this many performers (0 / unset = off).
    pub min_performer_count: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

enum QueryBind {
    Text(String),
    Float(f64),
    Int(i64),
}

/// Escape a free-text token for FTS5 MATCH (phrase query).
fn fts_phrase(term: &str) -> String {
    format!("\"{}\"", term.replace('"', "\"\""))
}

fn append_text_match_clause(
    sql: &mut String,
    binds: &mut Vec<QueryBind>,
    term: &str,
    negate: bool,
) {
    let trimmed = term.trim();
    if trimmed.is_empty() {
        return;
    }
    let pattern = format!("%{trimmed}%");
    // Prefer `id IN (SELECT scene_id …)` over per-row EXISTS so SQLite can
    // materialize matching tag/performer scene sets once. Critical for invert /
    // exclude terms (NOT (… OR EXISTS …)) which otherwise evaluate both EXISTS
    // for nearly every row when the term is rare.
    //
    // Include (non-negate): title/details via FTS5; path/tags/performers/segments
    // stay LIKE. Negate keeps full LIKE (FTS NOT is awkward for partials).
    if negate {
        sql.push_str(" AND NOT (\n");
        sql.push_str(
            r#"            COALESCE(s.title, '') LIKE ? COLLATE NOCASE
            OR COALESCE(s.details, '') LIKE ? COLLATE NOCASE
            OR COALESCE(f.path, '') LIKE ? COLLATE NOCASE
            OR s.id IN (
                SELECT st.scene_id FROM scene_tags st
                JOIN tags t ON t.id = st.tag_id
                WHERE t.name LIKE ? COLLATE NOCASE
            )
            OR s.id IN (
                SELECT sp.scene_id FROM scene_performers sp
                JOIN performers p ON p.id = sp.performer_id
                WHERE p.name LIKE ? COLLATE NOCASE
            )
            OR s.id IN (
                SELECT sg.scene_id FROM scene_segments sg
                WHERE sg.label LIKE ? COLLATE NOCASE
            )
        )"#,
        );
        for _ in 0..6 {
            binds.push(QueryBind::Text(pattern.clone()));
        }
    } else {
        sql.push_str(" AND (\n");
        sql.push_str(
            r#"            s.rowid IN (SELECT rowid FROM scenes_fts WHERE scenes_fts MATCH ?)
            OR COALESCE(f.path, '') LIKE ? COLLATE NOCASE
            OR s.id IN (
                SELECT st.scene_id FROM scene_tags st
                JOIN tags t ON t.id = st.tag_id
                WHERE t.name LIKE ? COLLATE NOCASE
            )
            OR s.id IN (
                SELECT sp.scene_id FROM scene_performers sp
                JOIN performers p ON p.id = sp.performer_id
                WHERE p.name LIKE ? COLLATE NOCASE
            )
            OR s.id IN (
                SELECT sg.scene_id FROM scene_segments sg
                WHERE sg.label LIKE ? COLLATE NOCASE
            )
        )"#,
        );
        binds.push(QueryBind::Text(fts_phrase(trimmed)));
        for _ in 0..4 {
            binds.push(QueryBind::Text(pattern.clone()));
        }
    }
}

fn order_clause_outer(sort: SortBy) -> &'static str {
    match sort {
        SortBy::Created => "created_at DESC",
        SortBy::Favorite => "favorite DESC, created_at DESC",
        SortBy::PlayCount => "play_count DESC, created_at DESC",
        SortBy::Title => "COALESCE(LOWER(title), 'zzz') ASC",
    }
}

const LIST_SCENES_FROM: &str = r#"
FROM scenes s
INNER JOIN files f ON f.id = (
    SELECT id FROM files
    WHERE scene_id = s.id
    ORDER BY duration DESC NULLS LAST, scanned_at ASC LIMIT 1
)
"#;

struct ListScenesFilter {
    min_fav: i64,
    fav_filter: i64,
    where_sql: String,
    binds: Vec<QueryBind>,
}

fn build_list_scenes_filter(args: &ListScenesArgs) -> ListScenesFilter {
    let min_fav = args.min_favorite.unwrap_or(0).clamp(0, 5);
    let fav_filter = if args.favorites_only.unwrap_or(false) {
        1
    } else {
        0
    };

    let search_raw = args.search.as_deref().unwrap_or("").trim();
    let search_inverse = args.search_inverse.unwrap_or(false);
    let include_terms: Vec<&str> = search_raw
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .collect();

    let mut where_sql = String::from("WHERE s.favorite >= ? AND (? = 0 OR s.favorite > 0)\n");
    let mut binds: Vec<QueryBind> = Vec::new();

    if args.unplayed_only.unwrap_or(false) {
        where_sql.push_str(" AND s.play_count = 0\n");
    }
    if args.identified_only.unwrap_or(false) {
        where_sql.push_str(" AND s.stashdb_applied_at IS NOT NULL\n");
    }
    if args.needs_review_only.unwrap_or(false) {
        where_sql.push_str(&format!(" AND {NEEDS_REVIEW_WHERE}\n"));
    }
    let min_tags = match args.min_tag_count {
        Some(n) if n > 0 => n,
        _ if args.tagged_only.unwrap_or(false) => 1,
        _ => 0,
    };
    if min_tags == 1 {
        where_sql.push_str(" AND EXISTS (SELECT 1 FROM scene_tags st WHERE st.scene_id = s.id)\n");
    } else if min_tags > 1 {
        where_sql
            .push_str(" AND (SELECT COUNT(*) FROM scene_tags st WHERE st.scene_id = s.id) >= ?\n");
        binds.push(QueryBind::Int(min_tags));
    }
    let min_perfs = args.min_performer_count.unwrap_or(0).max(0);
    if min_perfs == 1 {
        where_sql
            .push_str(" AND EXISTS (SELECT 1 FROM scene_performers sp WHERE sp.scene_id = s.id)\n");
    } else if min_perfs > 1 {
        where_sql.push_str(
            " AND (SELECT COUNT(*) FROM scene_performers sp WHERE sp.scene_id = s.id) >= ?\n",
        );
        binds.push(QueryBind::Int(min_perfs));
    }
    if let Some(min_h) = args.min_height {
        if min_h > 0 {
            where_sql.push_str(" AND f.height IS NOT NULL AND f.height >= ?\n");
            binds.push(QueryBind::Int(min_h));
        }
    }
    if let Some(min_d) = args.min_duration {
        if min_d > 0.0 {
            where_sql.push_str(" AND f.duration IS NOT NULL AND f.duration >= ?\n");
            binds.push(QueryBind::Float(min_d));
        }
    }
    if let Some(max_d) = args.max_duration {
        if max_d > 0.0 {
            where_sql.push_str(" AND f.duration IS NOT NULL AND f.duration <= ?\n");
            binds.push(QueryBind::Float(max_d));
        }
    }

    for term in include_terms {
        append_text_match_clause(&mut where_sql, &mut binds, term, search_inverse);
    }
    for term in &args.search_exclude_terms {
        append_text_match_clause(&mut where_sql, &mut binds, term, true);
    }

    if !args.tag_ids.is_empty() {
        if args.tag_match_any.unwrap_or(false) {
            let placeholders = std::iter::repeat("?")
                .take(args.tag_ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            where_sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM scene_tags st WHERE st.scene_id = s.id AND st.tag_id IN ({placeholders}))\n"
            ));
            for tid in &args.tag_ids {
                binds.push(QueryBind::Text(tid.clone()));
            }
        } else {
            for tid in &args.tag_ids {
                where_sql.push_str(
                    " AND EXISTS (SELECT 1 FROM scene_tags st WHERE st.scene_id = s.id AND st.tag_id = ?)\n",
                );
                binds.push(QueryBind::Text(tid.clone()));
            }
        }
    }
    if !args.exclude_tag_ids.is_empty() {
        let placeholders = std::iter::repeat("?")
            .take(args.exclude_tag_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        where_sql.push_str(&format!(
            " AND NOT EXISTS (SELECT 1 FROM scene_tags st WHERE st.scene_id = s.id AND st.tag_id IN ({placeholders}))\n"
        ));
        for tid in &args.exclude_tag_ids {
            binds.push(QueryBind::Text(tid.clone()));
        }
    }

    for pid in &args.performer_ids {
        where_sql.push_str(
            " AND EXISTS (SELECT 1 FROM scene_performers sp WHERE sp.scene_id = s.id AND sp.performer_id = ?)\n",
        );
        binds.push(QueryBind::Text(pid.clone()));
    }
    if !args.exclude_performer_ids.is_empty() {
        let placeholders = std::iter::repeat("?")
            .take(args.exclude_performer_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        where_sql.push_str(&format!(
            " AND NOT EXISTS (SELECT 1 FROM scene_performers sp WHERE sp.scene_id = s.id AND sp.performer_id IN ({placeholders}))\n"
        ));
        for pid in &args.exclude_performer_ids {
            binds.push(QueryBind::Text(pid.clone()));
        }
    }

    if !args.studio_ids.is_empty() {
        let placeholders = std::iter::repeat("?")
            .take(args.studio_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        where_sql.push_str(&format!(
            " AND s.studio_id IS NOT NULL AND s.studio_id IN ({placeholders})\n"
        ));
        for sid in &args.studio_ids {
            binds.push(QueryBind::Text(sid.clone()));
        }
    }
    if !args.exclude_studio_ids.is_empty() {
        let placeholders = std::iter::repeat("?")
            .take(args.exclude_studio_ids.len())
            .collect::<Vec<_>>()
            .join(", ");
        where_sql.push_str(&format!(
            " AND (s.studio_id IS NULL OR s.studio_id NOT IN ({placeholders}))\n"
        ));
        for sid in &args.exclude_studio_ids {
            binds.push(QueryBind::Text(sid.clone()));
        }
    }

    ListScenesFilter {
        min_fav,
        fav_filter,
        where_sql,
        binds,
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ListScenesResult {
    pub scenes: Vec<SceneGridRow>,
    /// Total scenes matching the current filters (before limit/offset).
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone, sqlx::FromRow)]
struct SceneGridRowWithTotal {
    id: String,
    title: Option<String>,
    favorite: i64,
    rating: Option<i64>,
    play_count: i64,
    created_at: String,
    duration: Option<f64>,
    width: Option<i64>,
    height: Option<i64>,
    thumb_path: Option<String>,
    thumb_sprite_path: Option<String>,
    file_path: Option<String>,
    total: i64,
}

#[tauri::command]
pub async fn list_scenes(
    state: State<'_, AppState>,
    args: ListScenesArgs,
) -> Result<ListScenesResult, String> {
    let limit = args.limit.unwrap_or(10_000).clamp(1, 10_000);
    let offset = args.offset.unwrap_or(0).max(0);
    let sort = args.sort.unwrap_or_default();
    let filter = build_list_scenes_filter(&args);
    let order_outer = order_clause_outer(sort);

    // Single pass: materialize matches once, then page + total via window count.
    // Avoids running the (expensive) filter twice for COUNT(*) + SELECT.
    let select_sql = format!(
        r#"WITH matched AS (
        SELECT s.id, s.title, s.favorite, s.rating, s.play_count, s.created_at,
               f.duration, f.width, f.height, f.thumb_path, NULL AS thumb_sprite_path, f.path AS file_path
        {LIST_SCENES_FROM}
        {}
    )
    SELECT id, title, favorite, rating, play_count, created_at,
           duration, width, height, thumb_path, thumb_sprite_path, file_path,
           COUNT(*) OVER() AS total
    FROM matched
    ORDER BY {order_outer}
    LIMIT ? OFFSET ?"#,
        filter.where_sql
    );
    let mut select_q = sqlx::query_as::<_, SceneGridRowWithTotal>(&select_sql)
        .bind(filter.min_fav)
        .bind(filter.fav_filter);
    for b in &filter.binds {
        select_q = match b {
            QueryBind::Text(s) => select_q.bind(s),
            QueryBind::Float(n) => select_q.bind(n),
            QueryBind::Int(n) => select_q.bind(n),
        };
    }
    let rows = select_q
        .bind(limit)
        .bind(offset)
        .fetch_all(&state.pool)
        .await
        .map_err(err)?;

    let total = rows.first().map(|r| r.total).unwrap_or(0);
    let scenes = rows
        .into_iter()
        .map(|r| SceneGridRow {
            id: r.id,
            title: r.title,
            favorite: r.favorite,
            rating: r.rating,
            play_count: r.play_count,
            created_at: r.created_at,
            duration: r.duration,
            width: r.width,
            height: r.height,
            thumb_path: r.thumb_path,
            thumb_sprite_path: r.thumb_sprite_path,
            file_path: r.file_path,
        })
        .collect();

    Ok(ListScenesResult {
        scenes,
        total,
        limit,
        offset,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeleteSceneFailure {
    pub scene_id: String,
    pub error: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DeleteScenesResult {
    pub deleted: u64,
    pub failed: Vec<DeleteSceneFailure>,
}

/// Delete exactly the given scene IDs (deduped).
#[tauri::command]
pub async fn delete_scenes(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scene_ids: Vec<String>,
) -> Result<DeleteScenesResult, String> {
    use std::collections::HashSet;

    let unique: Vec<String> = scene_ids
        .into_iter()
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    if unique.is_empty() {
        return Err("no scenes to delete".into());
    }

    let mut deleted = 0u64;
    let mut failed = Vec::new();

    for id in unique {
        match delete_scene_inner(&app, &state.pool, &id).await {
            Ok(()) => deleted += 1,
            Err(e) => failed.push(DeleteSceneFailure {
                scene_id: id,
                error: e,
            }),
        }
    }

    if deleted == 0 && !failed.is_empty() {
        return Err(failed[0].error.clone());
    }

    Ok(DeleteScenesResult { deleted, failed })
}

#[derive(Debug, serde::Serialize)]
pub struct Counts {
    pub total: i64,
    pub favorites: i64, // favorite > 0
}

#[tauri::command]
pub async fn scene_counts(state: State<'_, AppState>) -> Result<Counts, String> {
    let total: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM scenes s
        WHERE EXISTS (SELECT 1 FROM files f WHERE f.scene_id = s.id)
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(err)?;
    let favorites: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM scenes s
        WHERE s.favorite > 0
          AND EXISTS (SELECT 1 FROM files f WHERE f.scene_id = s.id)
        "#,
    )
    .fetch_one(&state.pool)
    .await
    .map_err(err)?;
    Ok(Counts {
        total: total.0,
        favorites: favorites.0,
    })
}

/// Pool-taking core of backfill_scene_titles. Shared by the Tauri command and
/// the startup hook. Probes cheaply first, then runs all updates in one
/// transaction so the FTS triggers commit once instead of per row.
pub async fn backfill_scene_titles_sqlx(pool: &sqlx::SqlitePool) -> Result<i64, sqlx::Error> {
    // Cheap gate: the backfill only touches title-less scenes that have at
    // least one file (the JOIN below drops file-less scenes, and the scalar
    // subquery returns NULL iff EXISTS is false). When nothing is eligible —
    // the common case after the first run — skip the join scan entirely.
    let eligible = sqlx::query(
        r#"SELECT 1 FROM scenes s
           WHERE (s.title IS NULL OR s.title = '')
             AND EXISTS (SELECT 1 FROM files f WHERE f.scene_id = s.id)
           LIMIT 1"#,
    )
    .fetch_optional(pool)
    .await?;
    if eligible.is_none() {
        return Ok(0);
    }

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT s.id, f.path FROM scenes s
         JOIN files f ON f.id = (SELECT id FROM files WHERE scene_id = s.id ORDER BY duration DESC NULLS LAST, scanned_at ASC LIMIT 1)
         WHERE s.title IS NULL OR s.title = ''",
    )
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;
    let mut count = 0i64;
    for (scene_id, path) in rows {
        let Some(title) = crate::filename_parse::scene_title_from_path(std::path::Path::new(&path))
        else {
            continue;
        };
        sqlx::query("UPDATE scenes SET title = ?, title_source = 'filename' WHERE id = ?")
            .bind(&title)
            .bind(&scene_id)
            .execute(&mut *tx)
            .await?;
        count += 1;
    }
    tx.commit().await?;
    Ok(count)
}

/// One-time backfill: derive titles from filenames for scenes that have none.
/// Useful after upgrading from a version that didn't populate titles at scan.
/// Returns the number of titles set. (Also auto-runs at app startup.)
#[tauri::command]
pub async fn backfill_scene_titles(state: State<'_, AppState>) -> Result<i64, String> {
    backfill_scene_titles_sqlx(&state.pool).await.map_err(err)
}

#[tauri::command]
pub async fn set_favorite(
    state: State<'_, AppState>,
    scene_id: String,
    // New favorite level (0..5). Clamped server-side.
    level: i64,
) -> Result<(), String> {
    let level = level.clamp(0, 5);
    sqlx::query("UPDATE scenes SET favorite = ?, updated_at = ? WHERE id = ?")
        .bind(level)
        .bind(crate::models::now().to_rfc3339())
        .bind(&scene_id)
        .execute(&state.pool)
        .await
        .map_err(err)?;
    Ok(())
}

/// Bump play_count + last_played_at when a scene starts in the player.
#[tauri::command]
pub async fn record_scene_play(state: State<'_, AppState>, scene_id: String) -> Result<(), String> {
    let ts = crate::models::now().to_rfc3339();
    sqlx::query(
        r#"
        UPDATE scenes
        SET play_count = play_count + 1,
            last_played_at = ?,
            updated_at = ?
        WHERE id = ?
        "#,
    )
    .bind(&ts)
    .bind(&ts)
    .bind(&scene_id)
    .execute(&state.pool)
    .await
    .map_err(err)?;
    Ok(())
}

/// Favorite + last_played_at for shuffle weighting (batch).
#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct SceneShuffleMeta {
    pub id: String,
    pub favorite: i64,
    pub last_played_at: Option<String>,
}

#[tauri::command]
pub async fn scenes_shuffle_meta(
    state: State<'_, AppState>,
    scene_ids: Vec<String>,
) -> Result<Vec<SceneShuffleMeta>, String> {
    if scene_ids.is_empty() {
        return Ok(vec![]);
    }
    // Chunk to stay under SQLite variable limits on huge queues.
    let mut out = Vec::with_capacity(scene_ids.len());
    for chunk in scene_ids.chunks(400) {
        let placeholders = std::iter::repeat("?")
            .take(chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql =
            format!("SELECT id, favorite, last_played_at FROM scenes WHERE id IN ({placeholders})");
        let mut q = sqlx::query_as::<_, SceneShuffleMeta>(&sql);
        for id in chunk {
            q = q.bind(id);
        }
        let rows = q.fetch_all(&state.pool).await.map_err(err)?;
        out.extend(rows);
    }
    Ok(out)
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct PerformerRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct StudioRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
pub struct TagRow {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
}

#[derive(Debug, serde::Serialize)]
pub struct SceneDetail {
    pub scene: Scene,
    pub files: Vec<VideoFile>,
    pub performers: Vec<PerformerRow>,
    pub studio: Option<StudioRow>,
    pub tags: Vec<TagRow>,
}

#[tauri::command]
pub async fn scene_detail(
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<SceneDetail, String> {
    let scene: Scene = sqlx::query_as(
        r#"
        SELECT id, title, details, title_source, details_source, studio_id,
               cover_path, cover_source, rating, favorite, play_count,
               last_played_at, last_position,
               stashdb_checked_at, stashdb_match_count, stashdb_applied_at,
               stashdb_ignored_at, stashdb_remote_id,
               created_at, updated_at
        FROM scenes WHERE id = ?
        "#,
    )
    .bind(&scene_id)
    .fetch_one(&state.pool)
    .await
    .map_err(err)?;

    let files: Vec<VideoFile> = sqlx::query_as(
        r#"
        SELECT id, scene_id, path, size_bytes, modified_at, format_name,
               duration, width, height, codec, fps, bitrate,
               thumb_path, thumb_sprite_path, vtt_path, scanned_at
        FROM files WHERE scene_id = ? ORDER BY scanned_at DESC
        "#,
    )
    .bind(&scene_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)?;

    let performers: Vec<PerformerRow> = sqlx::query_as(
        "SELECT p.id, p.name FROM performers p
         JOIN scene_performers sp ON sp.performer_id = p.id
         WHERE sp.scene_id = ?
         ORDER BY p.name",
    )
    .bind(&scene_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)?;

    let studio: Option<StudioRow> = match &scene.studio_id {
        Some(sid) => sqlx::query_as("SELECT id, name FROM studios WHERE id = ?")
            .bind(sid)
            .fetch_optional(&state.pool)
            .await
            .map_err(err)?,
        None => None,
    };

    let tags: Vec<TagRow> = sqlx::query_as(
        "SELECT t.id, t.name, t.color FROM tags t
         JOIN scene_tags st ON st.tag_id = t.id
         WHERE st.scene_id = ?
         ORDER BY t.name",
    )
    .bind(&scene_id)
    .fetch_all(&state.pool)
    .await
    .map_err(err)?;

    Ok(SceneDetail {
        scene,
        files,
        performers,
        studio,
        tags,
    })
}

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
struct FileArtifacts {
    path: String,
    thumb_path: Option<String>,
    thumb_sprite_path: Option<String>,
    vtt_path: Option<String>,
}

fn remove_path_if_exists(path: &str) -> Result<(), String> {
    if path.trim().is_empty() {
        return Ok(());
    }
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("failed to delete {path}: {e}")),
    }
}

/// Windows may keep a file locked briefly after mpv releases it — retry before failing.
async fn remove_file_with_retry(path: &str, attempts: u32) -> Result<(), String> {
    let mut last_err: Option<String> = None;
    for attempt in 0..attempts {
        match remove_path_if_exists(path) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt + 1 < attempts {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| format!("failed to delete {path}")))
}

#[derive(Clone, serde::Serialize)]
struct SceneDeletedPayload {
    scene_id: String,
}

/// Permanently delete a scene: removes video files from disk (with retry), preview
/// artifacts, then the catalog row. Emits `scene://deleted` on success.
/// Caller must release any open player handle before invoking.
pub(crate) async fn delete_scene_inner(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    scene_id: &str,
) -> Result<(), String> {
    let exists: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM scenes WHERE id = ?")
        .bind(scene_id)
        .fetch_one(pool)
        .await
        .map_err(err)?;
    if exists.0 == 0 {
        return Err("scene not found".into());
    }

    let cover: Option<String> = sqlx::query_as("SELECT cover_path FROM scenes WHERE id = ?")
        .bind(scene_id)
        .fetch_optional(pool)
        .await
        .map_err(err)?
        .map(|(p,)| p)
        .flatten();

    let files: Vec<FileArtifacts> = sqlx::query_as(
        "SELECT path, thumb_path, thumb_sprite_path, vtt_path FROM files WHERE scene_id = ?",
    )
    .bind(scene_id)
    .fetch_all(pool)
    .await
    .map_err(err)?;

    for f in &files {
        remove_file_with_retry(&f.path, 8).await?;
    }

    for f in &files {
        if let Some(p) = &f.thumb_path {
            let _ = remove_path_if_exists(p);
        }
        if let Some(p) = &f.thumb_sprite_path {
            let _ = remove_path_if_exists(p);
        }
        if let Some(p) = &f.vtt_path {
            let _ = remove_path_if_exists(p);
        }
    }
    if let Some(p) = cover {
        let _ = remove_path_if_exists(&p);
    }

    let result = sqlx::query("DELETE FROM scenes WHERE id = ?")
        .bind(scene_id)
        .execute(pool)
        .await
        .map_err(err)?;

    if result.rows_affected() == 0 {
        return Err("scene not found".into());
    }

    let _ = app.emit(
        "scene://deleted",
        SceneDeletedPayload {
            scene_id: scene_id.to_string(),
        },
    );

    Ok(())
}

#[tauri::command]
pub async fn delete_scene(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    scene_id: String,
) -> Result<(), String> {
    delete_scene_inner(&app, &state.pool, &scene_id).await
}

#[cfg(test)]
mod tests {
    use super::{append_text_match_clause, QueryBind};

    #[test]
    fn text_match_includes_details_segments_and_binds_six_patterns() {
        let mut sql = String::from("WHERE 1=1");
        let mut binds = Vec::new();
        append_text_match_clause(&mut sql, &mut binds, "redhead", true);
        assert!(sql.contains("COALESCE(s.details, '') LIKE ?"));
        assert!(sql.contains("scene_segments"));
        assert!(sql.contains("NOT ("));
        assert_eq!(binds.len(), 6);
        assert!(matches!(&binds[0], QueryBind::Text(s) if s == "%redhead%"));
    }
}
