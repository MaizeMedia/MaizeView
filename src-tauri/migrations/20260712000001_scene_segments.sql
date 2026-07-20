-- Timed segments / bookmarks on a scene timeline (Cove-inspired spike).
-- start_sec required; end_sec NULL = point marker. Optional tag/performer links.

CREATE TABLE scene_segments (
    id            TEXT PRIMARY KEY,
    scene_id      TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    start_sec     REAL NOT NULL CHECK (start_sec >= 0),
    end_sec       REAL,
    label         TEXT NOT NULL DEFAULT '',
    tag_id        TEXT REFERENCES tags(id) ON DELETE SET NULL,
    performer_id  TEXT REFERENCES performers(id) ON DELETE SET NULL,
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    CHECK (end_sec IS NULL OR end_sec >= start_sec)
);

CREATE INDEX idx_segments_scene ON scene_segments(scene_id, start_sec);
