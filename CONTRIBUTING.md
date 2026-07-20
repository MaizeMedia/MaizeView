# Contributing

Thanks for considering a contribution! MaizeView is a personal project that's
public for anyone to use — there's no SLA on reviews, but PRs and issues are
read and answered.

## Ground rules

- **Windows-only.** The app is deeply Win32-integrated (libmpv wid-embedding,
  drive-aware jobs). Cross-platform PRs are out of scope for now.
- **Product rules apply.** See `docs/decisions.md` — notably: no automatic
  on-disk file organizing, no accounts/cloud/sync, no busywork manual-tagging
  UX, no explicit content in the repo itself (keep screenshots/samples clean).
- **Big changes start with an issue.** A feature PR that shows up unannounced
  risks a polite decline if it conflicts with where the project is going.

## Before you submit

- `cargo test --lib` passes
- `cargo fmt --check` is clean (and keep your own diff clippy-clean; CI runs
  clippy report-only while the codebase ratchets toward zero warnings)
- `npm run build` passes
- UI changes: run `npm run test:e2e:smoke` when practical (see `docs/e2e.md`)

CI runs all of these on your PR — green CI isn't optional, but feel free to
ask for help getting there.

## Code notes

- Rust lives in `src-tauri/src/` (commands, jobs, scanner); Svelte in `src/`.
- Player/4Play code has hard-won platform gotchas documented in
  `docs/progress.md` — read them before touching anything mpv-related.
- Match the surrounding style; keep diffs focused (one thing per PR).
