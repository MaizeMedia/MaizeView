//! Duplicate scene groups by pHash Hamming distance.

use std::collections::HashMap;

use tauri::State;

use crate::{commands::err, scanner::phash, AppState};

const DEFAULT_THRESHOLD: u8 = 8;

#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicateSceneEntry {
    pub scene_id: String,
    pub title: Option<String>,
    pub file_path: Option<String>,
    pub phash: String,
    pub thumb_path: Option<String>,
    pub favorite: i64,
    // Representative-file specs (keeper-decision info):
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration: Option<f64>,
    pub fps: Option<f64>,
    pub bitrate: Option<i64>,
    pub size_bytes: i64,
    pub codec: Option<String>,
    /// Scene has a stash-box identify apply (`stashdb_applied_at IS NOT NULL`).
    pub identified: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicateGroup {
    pub scenes: Vec<DuplicateSceneEntry>,
    pub max_distance: u32,
}

#[tauri::command]
pub async fn find_duplicate_groups(
    state: State<'_, AppState>,
    threshold: Option<u8>,
) -> Result<Vec<DuplicateGroup>, String> {
    let threshold = threshold.unwrap_or(DEFAULT_THRESHOLD) as u32;
    let pool = &state.pool;

    let rows: Vec<(
        String,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        i64,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<i64>,
        i64,
        Option<String>,
        bool,
    )> = sqlx::query_as(
        r#"
            SELECT s.id, s.title, ph.value, f.path, f.thumb_path, s.favorite,
                   f.width, f.height, f.duration, f.fps, f.bitrate, f.size_bytes, f.codec,
                   s.stashdb_applied_at IS NOT NULL AS identified
            FROM scenes s
            JOIN files f ON f.scene_id = s.id
            JOIN fingerprints ph ON ph.file_id = f.id AND ph.hash_type = 'phash'
            WHERE f.id = (
                SELECT f2.id FROM files f2
                WHERE f2.scene_id = s.id
                ORDER BY f2.duration DESC NULLS LAST, f2.scanned_at ASC
                LIMIT 1
            )
            ORDER BY s.created_at DESC
            "#,
    )
    .fetch_all(pool)
    .await
    .map_err(err)?;

    if rows.len() < 2 {
        return Ok(vec![]);
    }

    fn find(parent: &mut [usize], i: usize) -> usize {
        if parent[i] != i {
            let root = find(parent, parent[i]);
            parent[i] = root;
        }
        parent[i]
    }

    fn union(parent: &mut [usize], a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[rb] = ra;
        }
    }

    // The pairwise scan is O(n²) over all pHashed scenes. Parse each hash to u64
    // exactly once, then compare with xor + popcount, and run the whole thing on
    // a blocking thread so the async runtime stays responsive (SQL stays above).
    let groups = tokio::task::spawn_blocking(move || {
        let hashes: Vec<Option<u64>> = rows.iter().map(|row| phash::parse_hex(&row.2)).collect();
        let n = rows.len();
        let mut parent: Vec<usize> = (0..n).collect();

        for i in 0..n {
            for j in (i + 1)..n {
                if let (Some(a), Some(b)) = (hashes[i], hashes[j]) {
                    if (a ^ b).count_ones() <= threshold {
                        union(&mut parent, i, j);
                    }
                }
            }
        }

        let mut buckets: HashMap<usize, Vec<usize>> = HashMap::new();
        for i in 0..n {
            buckets.entry(find(&mut parent, i)).or_default().push(i);
        }

        let mut groups = Vec::new();
        for indices in buckets.values() {
            if indices.len() < 2 {
                continue;
            }

            let mut max_distance = 0u32;
            for a in 0..indices.len() {
                for b in (a + 1)..indices.len() {
                    let i = indices[a];
                    let j = indices[b];
                    if let (Some(x), Some(y)) = (hashes[i], hashes[j]) {
                        max_distance = max_distance.max((x ^ y).count_ones());
                    }
                }
            }

            let scenes: Vec<DuplicateSceneEntry> = indices
                .iter()
                .map(|&i| DuplicateSceneEntry {
                    scene_id: rows[i].0.clone(),
                    title: rows[i].1.clone(),
                    file_path: rows[i].3.clone(),
                    phash: rows[i].2.clone(),
                    thumb_path: rows[i].4.clone(),
                    favorite: rows[i].5,
                    width: rows[i].6,
                    height: rows[i].7,
                    duration: rows[i].8,
                    fps: rows[i].9,
                    bitrate: rows[i].10,
                    size_bytes: rows[i].11,
                    codec: rows[i].12.clone(),
                    identified: rows[i].13,
                })
                .collect();

            groups.push(DuplicateGroup {
                scenes,
                max_distance,
            });
        }

        groups.sort_by(|a, b| b.scenes.len().cmp(&a.scenes.len()));
        groups
    })
    .await
    .map_err(|e| e.to_string())?;

    Ok(groups)
}

/// Delete duplicate scenes, keeping `keeper_scene_id`. Merges the highest favorite
/// level from deleted scenes onto the keeper.
#[tauri::command]
pub async fn resolve_duplicate_group(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    keeper_scene_id: String,
    delete_scene_ids: Vec<String>,
) -> Result<u32, String> {
    let pool = &state.pool;

    let keeper_fav: (i64,) = sqlx::query_as("SELECT favorite FROM scenes WHERE id = ?")
        .bind(&keeper_scene_id)
        .fetch_one(pool)
        .await
        .map_err(err)?;

    let mut max_fav = keeper_fav.0;
    for id in &delete_scene_ids {
        if id == &keeper_scene_id {
            continue;
        }
        if let Some((fav,)) = sqlx::query_as("SELECT favorite FROM scenes WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(err)?
        {
            max_fav = max_fav.max(fav);
        }
    }

    if max_fav > keeper_fav.0 {
        sqlx::query("UPDATE scenes SET favorite = ?, updated_at = ? WHERE id = ?")
            .bind(max_fav)
            .bind(crate::models::now().to_rfc3339())
            .bind(&keeper_scene_id)
            .execute(pool)
            .await
            .map_err(err)?;
    }

    let mut deleted = 0u32;
    for id in delete_scene_ids {
        if id == keeper_scene_id {
            continue;
        }
        crate::commands::scenes::delete_scene_inner(&app, pool, &id).await?;
        deleted += 1;
    }

    Ok(deleted)
}
