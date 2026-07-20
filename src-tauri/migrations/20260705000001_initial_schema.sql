-- MaizeView initial schema.
--
-- Design notes:
--   * A `scene` is the catalogable entity (one per video). A scene can have
--     multiple `files` (e.g. different encodings of the same content), each
--     with its own path/size/hashes — mirrors Stash/Cove.
--   * IDs are TEXT (UUIDv7, sortable) generated in Rust.
--   * Timestamps are TEXT (RFC3339) via chrono.
--   * Provenance is first-class from day one (ADR-006): every field that can
--     come from an external source has its origin recorded.
--   * FTS5 virtual table mirrors scene titles/details for fast free-text search.
--
-- NOTE: connection-level PRAGMAs (WAL, foreign_keys, busy_timeout) are set in
-- db.rs, not here — sqlx runs each migration inside a transaction, and SQLite
-- refuses PRAGMA journal_mode / foreign_keys changes mid-transaction.

-- ─── scan_paths: library folders to scan ──────────────────────────────────
CREATE TABLE scan_paths (
    id          TEXT PRIMARY KEY,
    path        TEXT NOT NULL UNIQUE,
    label       TEXT,
    created_at  TEXT NOT NULL
);

-- ─── studios ──────────────────────────────────────────────────────────────
CREATE TABLE studios (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    aliases     TEXT,             -- JSON array
    image_path  TEXT,
    created_at  TEXT NOT NULL
);

-- ─── performers ───────────────────────────────────────────────────────────
CREATE TABLE performers (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    aliases     TEXT,             -- JSON array
    image_path  TEXT,
    created_at  TEXT NOT NULL
);

-- ─── tags ─────────────────────────────────────────────────────────────────
CREATE TABLE tags (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    -- hierarchical tags (Cove/Stash support this). parent_id → tags.id.
    parent_id   TEXT REFERENCES tags(id) ON DELETE SET NULL,
    color       TEXT,             -- optional hex color for chip display
    created_at  TEXT NOT NULL
);
CREATE INDEX idx_tags_parent ON tags(parent_id);

-- ─── scenes: the central entity ───────────────────────────────────────────
CREATE TABLE scenes (
    id              TEXT PRIMARY KEY,
    title           TEXT,
    details         TEXT,
    -- provenance for title/details: 'manual' | 'stashdb' | 'fansdb' | 'thePornDB' | 'javstash' | 'iafd' | 'scraper'
    title_source    TEXT NOT NULL DEFAULT 'manual',
    details_source  TEXT NOT NULL DEFAULT 'manual',
    studio_id       TEXT REFERENCES studios(id) ON DELETE SET NULL,
    cover_path      TEXT,
    cover_source    TEXT NOT NULL DEFAULT 'manual',
    rating          INTEGER,           -- 1..5 user rating (dormant; quality score)
    -- favorite: 0 (not favorited) .. 5 (most favorited). Doubles as the
    -- weighted-shuffle weight when playing playlists: higher = more frequent.
    favorite        INTEGER NOT NULL DEFAULT 0 CHECK (favorite BETWEEN 0 AND 5),
    play_count      INTEGER NOT NULL DEFAULT 0,
    last_played_at  TEXT,
    last_position   REAL,              -- seconds, for resume
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);
CREATE INDEX idx_scenes_studio   ON scenes(studio_id);
CREATE INDEX idx_scenes_favorite ON scenes(favorite);
CREATE INDEX idx_scenes_created  ON scenes(created_at);

-- ─── files: physical video files backing a scene ──────────────────────────
CREATE TABLE files (
    id           TEXT PRIMARY KEY,
    scene_id     TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    -- identity:
    path         TEXT NOT NULL UNIQUE,
    size_bytes   INTEGER NOT NULL,
    modified_at  TEXT NOT NULL,          -- file mtime at scan time
    -- ffprobe-extracted container/codec info:
    format_name  TEXT,
    duration     REAL,                   -- seconds
    width        INTEGER,
    height       INTEGER,
    codec        TEXT,
    fps          REAL,
    bitrate      INTEGER,
    -- preview artifacts:
    --   thumb_path        : single 16:9 representative thumbnail (for grid cards)
    --   thumb_sprite_path : multi-cell contact sheet (for scrubber previews, Phase 3)
    --   vtt_path          : WebVTT mapping time ranges → sprite cells
    thumb_path        TEXT,
    thumb_sprite_path TEXT,
    vtt_path          TEXT,
    scanned_at   TEXT NOT NULL
);
CREATE INDEX idx_files_scene  ON files(scene_id);
CREATE INDEX idx_files_format ON files(format_name);

-- ─── fingerprints: per-file hashes for identity + StashDB matching ────────
-- One row per (file, hash_type). Allows incremental computation (oshash at
-- scan time, phash later via a generate task — exactly Stash's split).
CREATE TABLE fingerprints (
    id         TEXT PRIMARY KEY,
    file_id    TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    hash_type  TEXT NOT NULL CHECK (hash_type IN ('oshash','md5','phash')),
    value      TEXT NOT NULL,            -- phash stored as base16 uint64
    created_at TEXT NOT NULL,
    UNIQUE (file_id, hash_type)
);
CREATE INDEX idx_fp_type_value ON fingerprints(hash_type, value);
CREATE INDEX idx_fp_file        ON fingerprints(file_id);

-- ─── junction tables ──────────────────────────────────────────────────────
CREATE TABLE scene_performers (
    scene_id     TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    performer_id TEXT NOT NULL REFERENCES performers(id) ON DELETE CASCADE,
    PRIMARY KEY (scene_id, performer_id)
);
CREATE INDEX idx_scene_performers_perf ON scene_performers(performer_id);

CREATE TABLE scene_tags (
    scene_id TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    tag_id   TEXT NOT NULL REFERENCES tags(id)   ON DELETE CASCADE,
    PRIMARY KEY (scene_id, tag_id)
);
CREATE INDEX idx_scene_tags_tag ON scene_tags(tag_id);

-- ─── playlists ────────────────────────────────────────────────────────────
CREATE TABLE playlists (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    -- When playback starts from this playlist, new windows inherit this as
    -- their initial shuffle state. Per-window toggle can flip it live.
    -- (ADR-011: shuffle is owned per playback window; this flag is the default.)
    shuffle_by_default  INTEGER NOT NULL DEFAULT 0 CHECK (shuffle_by_default IN (0,1)),
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE playlist_items (
    id           TEXT PRIMARY KEY,
    playlist_id  TEXT NOT NULL REFERENCES playlists(id) ON DELETE CASCADE,
    scene_id     TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    position     INTEGER NOT NULL,        -- 0-based order in the playlist
    added_at     TEXT NOT NULL,
    UNIQUE (playlist_id, position)
);
CREATE INDEX idx_playlist_items_pl ON playlist_items(playlist_id, position);

-- ─── scan_runs: log of scan executions ────────────────────────────────────
CREATE TABLE scan_runs (
    id              TEXT PRIMARY KEY,
    started_at      TEXT NOT NULL,
    finished_at     TEXT,
    status          TEXT NOT NULL CHECK (status IN ('running','completed','failed','cancelled')),
    paths_scanned   INTEGER NOT NULL DEFAULT 0,
    files_found     INTEGER NOT NULL DEFAULT 0,
    files_added     INTEGER NOT NULL DEFAULT 0,
    files_updated   INTEGER NOT NULL DEFAULT 0,
    files_removed   INTEGER NOT NULL DEFAULT 0,
    error_message   TEXT
);

-- ─── schema version marker ────────────────────────────────────────────────
CREATE TABLE schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT INTO schema_meta (key, value) VALUES ('schema_version', '1');
INSERT INTO schema_meta (key, value) VALUES ('created_at', datetime('now'));

-- ─── FTS5: fast free-text search over scene title/details ─────────────────
CREATE VIRTUAL TABLE scenes_fts USING fts5(
    title,
    details,
    content='scenes',
    content_rowid='rowid',
    tokenize='porter unicode61'
);
-- Keep FTS in sync with scenes via triggers.
CREATE TRIGGER scenes_ai AFTER INSERT ON scenes BEGIN
    INSERT INTO scenes_fts (rowid, title, details)
    VALUES (new.rowid, new.title, new.details);
END;
CREATE TRIGGER scenes_ad AFTER DELETE ON scenes BEGIN
    INSERT INTO scenes_fts (scenes_fts, rowid, title, details)
    VALUES ('delete', old.rowid, old.title, old.details);
END;
CREATE TRIGGER scenes_au AFTER UPDATE ON scenes BEGIN
    INSERT INTO scenes_fts (scenes_fts, rowid, title, details)
    VALUES ('delete', old.rowid, old.title, old.details);
    INSERT INTO scenes_fts (rowid, title, details)
    VALUES (new.rowid, new.title, new.details);
END;
