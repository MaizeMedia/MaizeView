-- Dedup playlist_items and enforce one row per (playlist_id, scene_id), plus
-- indexes backing the canonical representative-file picking
-- (ORDER BY duration DESC NULLS LAST, scanned_at ASC) used across queries.

-- Keep the oldest row for each (playlist_id, scene_id) pair.
DELETE FROM playlist_items
WHERE rowid NOT IN (
    SELECT MIN(rowid) FROM playlist_items GROUP BY playlist_id, scene_id
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_playlist_items_unique_scene
    ON playlist_items(playlist_id, scene_id);

CREATE INDEX IF NOT EXISTS idx_files_scene_scanned
    ON files(scene_id, scanned_at DESC);

CREATE INDEX IF NOT EXISTS idx_files_scene_duration
    ON files(scene_id, duration DESC);
