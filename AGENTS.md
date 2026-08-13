# AGENTS.md

Instructions for AI agents working in this repo. This is the canonical file —
`CLAUDE.md` points here.

## Agent skills

### Issue tracker

Issues live as GitHub issues in the private repo `johnnylibretexts/libretexts-reader`;
always pass `--repo` explicitly rather than relying on remote inference. See
`docs/agents/issue-tracker.md`.

### Triage labels

The five canonical roles, each label string equal to its name: `needs-triage`,
`needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See
`docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` and one `docs/adr/` at the repo root, covering both the
React frontend and the Rust/Tauri backend. See `docs/agents/domain.md`.
