-- Named snapshots of library search + filter state (Stash/Cove-style reuse).
CREATE TABLE IF NOT EXISTS saved_filters (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    payload     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_saved_filters_updated ON saved_filters(updated_at DESC);
