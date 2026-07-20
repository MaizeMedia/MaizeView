// Repo hygiene gate — runs in CI and via `npm run check:hygiene`.
//
// Fails when tracked files contain absolute user-home paths, which is the
// shape personal identifiers leak into a public repo (drive-letter path into
// an account's profile folder). Use env vars, os.tmpdir(), or placeholders
// (`C:\path\to\...`) instead.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const PATTERNS = [
  // Absolute Windows user-home path. `C:\Users\you\...` is an allowed placeholder.
  { re: /[A-Za-z]:[\\/]Users[\\/](?!you[\\/])/i, label: "absolute user-home path" },
];

// Binary-ish extensions not worth scanning.
const SKIP = /\.(png|jpe?g|gif|ico|icns|dll|exe|db|wasm|zip|tar|gz)$/i;

const files = execFileSync("git", ["ls-files"], { encoding: "utf8" })
  .split("\n")
  .map((f) => f.trim())
  .filter((f) => f && !SKIP.test(f));

const hits = [];
for (const f of files) {
  let text;
  try {
    text = readFileSync(f, "utf8");
  } catch {
    continue; // unreadable/binary — skip
  }
  text.split("\n").forEach((line, i) => {
    for (const p of PATTERNS) {
      if (p.re.test(line)) hits.push(`${f}:${i + 1}  ${p.label}: ${line.trim()}`);
    }
  });
}

if (hits.length) {
  console.error(`Repo hygiene check FAILED (${hits.length} hit(s)):\n${hits.join("\n")}`);
  process.exit(1);
}
console.log(`Repo hygiene check passed (${files.length} tracked files scanned).`);
