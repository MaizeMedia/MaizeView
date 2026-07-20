-- Rejected stash-box matches + ignore flag so false positives don't re-apply.
ALTER TABLE scenes ADD COLUMN stashdb_ignored_at TEXT;
ALTER TABLE scenes ADD COLUMN stashdb_remote_id TEXT;

CREATE TABLE IF NOT EXISTS stashdb_rejected_matches (
    scene_id    TEXT NOT NULL REFERENCES scenes(id) ON DELETE CASCADE,
    provider_id TEXT NOT NULL,
    remote_id   TEXT NOT NULL,
    rejected_at TEXT NOT NULL,
    PRIMARY KEY (scene_id, provider_id, remote_id)
);

CREATE INDEX IF NOT EXISTS idx_stashdb_rejected_scene
    ON stashdb_rejected_matches(scene_id);
