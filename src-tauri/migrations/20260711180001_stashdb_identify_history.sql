-- StashDB identify history per scene (when checked, match count, when applied).

ALTER TABLE scenes ADD COLUMN stashdb_checked_at TEXT;
ALTER TABLE scenes ADD COLUMN stashdb_match_count INTEGER;
ALTER TABLE scenes ADD COLUMN stashdb_applied_at TEXT;

CREATE INDEX idx_scenes_stashdb_checked ON scenes(stashdb_checked_at);
