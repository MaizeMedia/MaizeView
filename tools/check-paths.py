import os
import sqlite3
import re

db = os.path.join(os.environ["APPDATA"], "MaizeView", "maizeview.db")
con = sqlite3.connect(db)
cur = con.cursor()
rows = cur.execute("select path from files limit 25").fetchall()
print("samples:")
for (p,) in rows:
    print(repr(p[:100]))

total = cur.execute("select count(*) from files").fetchone()[0]
# Missing \ or / after drive letter: X:foo (not X:\foo or X:/foo)
bad_re = re.compile(r"^[A-Za-z]:(?![\\/])")
ok_re = re.compile(r"^[A-Za-z]:[\\/]")
all_paths = [p for (p,) in cur.execute("select path from files")]
bad = [p for p in all_paths if bad_re.match(p)]
ok = [p for p in all_paths if ok_re.match(p)]
print("---")
print("total", total, "ok_root", len(ok), "missing_slash_after_drive", len(bad))
for p in bad[:8]:
    print("BAD", repr(p))
