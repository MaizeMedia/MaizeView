# Agent orientation — MaizeView

**Product:** Windows Tauri adult video library (catalog + multi-window libmpv playback). Not an art/LoRA pipeline.

**Do not load** inn-roads art skills, Comfy/RunPod checklists, `art/state.json`, or laptop-remote rules here.

## Session start (every chat)

1. Read **`docs/progress.local.md`** — the real development log (hotline, gotchas, ops notes). **Gitignored; never commit it or quote its contents into tracked files.** If it doesn't exist, create it from the public repo's state and tell the owner.
2. **Staleness gate:** compare the *Dev-log revision* line in `docs/progress.local.md` with the marker in the public stub **`docs/progress.md`**. If they differ, tell the owner before doing work — the local log is stale (or the marker is) and gets synced off-GitHub.
3. If building or changing architecture: read **`docs/plan.md`** (and `docs/decisions.md` when relevant).
4. Tooling / PATH / libmpv / kill-locks: **`docs/setup.md`** + **`.cursor/rules/capabilities.mdc`**.
5. UI / filter / player changes: prefer **`npm run test:e2e:smoke`** when the E2E app is practical (`docs/e2e.md`).

## Source of truth

| Need | Where |
|------|--------|
| Resume / next | `docs/progress.local.md` (private) |
| Dev-log revision marker | `docs/progress.md` (public stub) |
| Phased plan | `docs/plan.md` |
| ADRs | `docs/decisions.md` |
| Dev env | `docs/setup.md` |
| E2E | `docs/e2e.md` |
| Tools & secrets shape | `.cursor/rules/capabilities.mdc` |
| Git / durability | `.cursor/rules/storage-and-git.mdc` |

**“Update the docs”** here means: update `docs/progress.local.md` (and plan/ADR if the decision changed), and bump the dev-log revision in both the local file and the public stub in the same PR. There is **no** `state.json` ceremony.

## Repo hygiene (public, open-source repo)

- **Never write absolute user paths, machine names, account names, or personal identifiers into tracked files** — use env vars, `os.tmpdir()`, or placeholders (`C:\path\to\...`).
- **Features yes, strategy no:** monetization/support *features* may live in code and README, but operational details (application statuses, pending approvals, promo plans, account notes) belong only in `docs/progress.local.md`.
- Keep public-facing text PG: "adult/NSFW video library" is fine; specific performers, studios, or scene titles are not (use clearly fictional fixtures like `ExampleStudio`, `Robin Monroe`).
- **Before opening a PR:** run `npm run check:hygiene` (the CI gate runs it too) and the private identifier sweep listed in `docs/progress.local.md`.

## Pull requests (all changes, human or agent)

`main` is protected: `enforce_admins` + required `build-test` CI. **Nothing lands without a PR the owner explicitly merges.**
- Own work: branch → commit → `gh pr create`. Never push to `main` directly (it's rejected).
- **Granularity (owner's call, 2026-07-20):** coherent-batch PRs, not micro-PRs — one PR per theme or work session (e.g. an audit-fix batch lands as one PR with several commits). Single-maintainer repo: the PR exists as the pre-public checkpoint (hygiene gate, CI, owner review), not for external review throughput. Split only when a change mixes unrelated concerns.
- **Independent review:** required for PRs touching schema/migrations, Tauri commands/IPC, release/CI config, or dependency upgrades — run a review subagent with **fresh context** (it has not seen the implementation; it reviews the raw diff cold) covering correctness, security, secrets, repo hygiene (above), ADR compliance (`docs/decisions.md`), and test coverage. Post its findings as the PR review. UI-only and docs-only PRs may skip it.
- Merge is the owner's call, always.

## Handoff phrase

*Read progress.local.md hotline, check the revision marker, and get ready.*
