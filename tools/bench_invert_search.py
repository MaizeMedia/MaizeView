"""Quick bench: invert-search SQL shapes against a MaizeView DB."""

from __future__ import annotations

import sqlite3
import sys
import time
from pathlib import Path

DB = Path(sys.argv[1]) if len(sys.argv) > 1 else Path.home() / "AppData/Roaming/MaizeView/maizeview.db"


def timed(cur: sqlite3.Cursor, label: str, sql: str, binds: tuple[str, ...]) -> None:
    t0 = time.perf_counter()
    row = cur.execute(sql, binds).fetchone()
    n = 0 if row is None else row[0]
    ms = (time.perf_counter() - t0) * 1000
    print(f"{label}: {n} rows in {ms:.1f} ms")


def main() -> None:
    print(f"db={DB} exists={DB.exists()}")
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    cur = con.cursor()
    print("scenes", cur.execute("SELECT COUNT(*) FROM scenes").fetchone()[0])
    print("tags", cur.execute("SELECT COUNT(*) FROM tags").fetchone()[0])
    print("scene_tags", cur.execute("SELECT COUNT(*) FROM scene_tags").fetchone()[0])

    old = """
    SELECT COUNT(*) FROM scenes s
    LEFT JOIN files f ON f.id = (
        SELECT id FROM files WHERE scene_id = s.id ORDER BY scanned_at DESC LIMIT 1
    )
    WHERE s.favorite >= 0 AND (0 = 0 OR s.favorite > 0)
    AND NOT (
        s.title LIKE ? COLLATE NOCASE
        OR s.details LIKE ? COLLATE NOCASE
        OR f.path LIKE ? COLLATE NOCASE
        OR EXISTS (
            SELECT 1 FROM scene_tags st JOIN tags t ON t.id = st.tag_id
            WHERE st.scene_id = s.id AND t.name LIKE ? COLLATE NOCASE
        )
            OR EXISTS (
            SELECT 1 FROM scene_performers sp JOIN performers p ON p.id = sp.performer_id
            WHERE sp.scene_id = s.id AND p.name LIKE ? COLLATE NOCASE
        )
    )
    """
    new = """
    SELECT COUNT(*) FROM scenes s
    LEFT JOIN files f ON f.id = (
        SELECT id FROM files WHERE scene_id = s.id ORDER BY scanned_at DESC LIMIT 1
    )
    WHERE s.favorite >= 0 AND (0 = 0 OR s.favorite > 0)
    AND NOT (
        COALESCE(s.title, '') LIKE ? COLLATE NOCASE
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
    )
    """
    cte = """
    WITH matched AS (
        SELECT s.id
        FROM scenes s
        LEFT JOIN files f ON f.id = (
            SELECT id FROM files WHERE scene_id = s.id ORDER BY scanned_at DESC LIMIT 1
        )
        WHERE s.favorite >= 0 AND (0 = 0 OR s.favorite > 0)
        AND NOT (
            COALESCE(s.title, '') LIKE ? COLLATE NOCASE
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
        )
    )
    SELECT COALESCE(COUNT(*) OVER(), 0) AS total FROM matched LIMIT 1
    """

    for label, term in [("rare", "%zzzinvertperf%"), ("common", "%a%")]:
        binds = (term,) * 5
        print(f"--- term {term!r} ---")
        timed(cur, "OLD", old, binds)
        timed(cur, "NEW", new, binds)
        timed(cur, "CTE", cte, binds)

    plain = """
    SELECT COUNT(*) FROM scenes s
    LEFT JOIN files f ON f.id = (
        SELECT id FROM files WHERE scene_id = s.id ORDER BY scanned_at DESC LIMIT 1
    )
    WHERE s.favorite >= 0 AND (0 = 0 OR s.favorite > 0)
    """
    timed(cur, "PLAIN", plain, ())


if __name__ == "__main__":
    main()
